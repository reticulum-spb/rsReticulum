//! Embedded REST API server for rnsd-rs.
//!
//! Activated via `--features api` during build and the `api_listen` key in the
//! `[reticulum]` section of the config, for example:
//!
//! ```ini
//! [reticulum]
//! api_listen = 0.0.0.0:8080
//! ```
//!
//! The server runs inside the rnsd process and accesses the transport
//! directly via `transport_tx`, bypassing the pickle/HMAC protocol.
//!
//! # Routes
//!
//! ```text
//! GET /health                     — liveness probe
//! GET /api/v1/status              — summary: counters + interface list
//! GET /api/v1/interfaces          — list of interfaces (?filter=…&all=true)
//! GET /api/v1/interfaces/{id}     — one interface by numeric id
//! GET /api/v1/config/interfaces   — configured interfaces, including disabled
//! GET /api/v1/paths               — path table (?max_hops=N)
//! GET /api/v1/links               — number of active links
//! POST /api/v1/interfaces         — add an interface
//! PUT /api/v1/interfaces/{id}     — replace the interface configuration
//! DELETE /api/v1/interfaces/{id}  — delete an interface
//! PUT /api/v1/config/interfaces/{name}    — replace by stable config name
//! DELETE /api/v1/config/interfaces/{name} — delete without a runtime ID
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, Request, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::{get, put};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc};

use rns_transport::messages::{
    InterfaceStatRpcEntry, PathTableRpcEntry, TransportMessage, TransportQuery,
    TransportQueryResponse,
};

use crate::config::{Config, ConfigSection, atomic_write};
use crate::interface_factory::{InterfaceConfig, synthesize_interface};
use crate::lifecycle::ShutdownSignal;
use crate::reticulum::{ReticulumHandle, teardown_interface};

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const STYLE_CSS: &str = include_str!("../web/style.css");

// ─────────────────────────────────────────────────────────────────────────────
// Starting the server
// ─────────────────────────────────────────────────────────────────────────────

pub async fn run_api_server(
    listen: SocketAddr,
    transport_tx: mpsc::Sender<TransportMessage>,
    handle: ReticulumHandle,
    shutdown: ShutdownSignal,
) {
    let state = AppState {
        transport_tx,
        handle,
        config_write_lock: Arc::new(Mutex::new(())),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style_css))
        .route("/health", get(health))
        .route("/api/v1/status", get(status))
        .route("/api/v1/interfaces", get(interfaces).post(create_interface))
        .route("/api/v1/config/interfaces", get(config_interfaces))
        .route(
            "/api/v1/config/interfaces/{name}",
            put(update_config_interface).delete(delete_config_interface),
        )
        .route(
            "/api/v1/interfaces/{id}",
            get(interface_by_id)
                .put(update_interface)
                .delete(delete_interface),
        )
        .route("/api/v1/paths", get(paths))
        .route("/api/v1/links", get(links))
        .layer(middleware::from_fn(security_headers))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind(listen).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(addr = %listen, error = %e, "REST API: failed to bind");
            return;
        }
    };

    tracing::info!(addr = %listen, "REST API listening");

    tokio::select! {
        result = axum::serve(listener, app) => {
            if let Err(e) = result {
                tracing::warn!(error = %e, "REST API server error");
            }
        }
        () = shutdown.wait() => {
            tracing::debug!("REST API shutting down");
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// State
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    transport_tx: mpsc::Sender<TransportMessage>,
    handle: ReticulumHandle,
    config_write_lock: Arc<Mutex<()>>,
}

struct LoadedConfig {
    config: Config,
    source: Vec<u8>,
}

fn save_config_snapshot(
    path: &std::path::Path,
    config: &Config,
    expected: &[u8],
) -> Result<Vec<u8>, ApiError> {
    let current = std::fs::read(path)
        .map_err(|e| ApiError::internal(format!("failed to re-read config: {e}")))?;
    if current != expected {
        return Err(ApiError::Conflict(
            "config changed outside the Web UI; reload and retry".to_string(),
        ));
    }

    let backup_path = path.with_file_name("config.web-ui.bak");
    atomic_write(&backup_path, expected)
        .map_err(|e| ApiError::internal(format!("failed to write config backup: {e}")))?;

    let updated = config.to_ini().into_bytes();
    atomic_write(path, &updated)
        .map_err(|e| ApiError::internal(format!("failed to write config: {e}")))?;
    Ok(updated)
}

fn rollback_config_snapshot(
    path: &std::path::Path,
    applied: &[u8],
    original: &[u8],
) -> Result<(), ApiError> {
    let current = std::fs::read(path)
        .map_err(|e| ApiError::internal(format!("failed to re-read config for rollback: {e}")))?;
    if current != applied {
        return Err(ApiError::Conflict(
            "config changed externally after the interface restart failed; rollback skipped"
                .to_string(),
        ));
    }
    atomic_write(path, original)
        .map_err(|e| ApiError::internal(format!("failed to roll back config: {e}")))
}

impl AppState {
    async fn query(&self, query: TransportQuery) -> Result<TransportQueryResponse, ApiError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.transport_tx
            .send(TransportMessage::Rpc {
                query,
                response_tx: tx,
            })
            .await
            .map_err(|_| ApiError::transport("transport actor is gone"))?;
        rx.await
            .map_err(|_| ApiError::transport("transport actor dropped response channel"))
    }

    fn config_path(&self) -> PathBuf {
        self.handle.config_dir.join("config")
    }

    fn load_config(&self) -> Result<Config, ApiError> {
        Ok(self.load_config_snapshot()?.config)
    }

    fn load_config_snapshot(&self) -> Result<LoadedConfig, ApiError> {
        let path = self.config_path();
        let source = std::fs::read(&path)
            .map_err(|e| ApiError::internal(format!("failed to read config: {e}")))?;
        let text = std::str::from_utf8(&source)
            .map_err(|e| ApiError::internal(format!("config is not valid UTF-8: {e}")))?;
        let config = Config::from_loaded_str(text, &path)
            .map_err(|e| ApiError::internal(format!("failed to parse config: {e}")))?;
        Ok(LoadedConfig { config, source })
    }

    fn save_config(&self, config: &Config, expected: &[u8]) -> Result<Vec<u8>, ApiError> {
        save_config_snapshot(&self.config_path(), config, expected)
    }

    fn rollback_config(&self, applied: &[u8], original: &[u8]) -> Result<(), ApiError> {
        rollback_config_snapshot(&self.config_path(), applied, original)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Errors
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum ApiError {
    NotFound,
    BadRequest(String),
    Conflict(String),
    Transport(String),
    Internal(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::NotFound => f.write_str("not found"),
            ApiError::BadRequest(message)
            | ApiError::Conflict(message)
            | ApiError::Transport(message)
            | ApiError::Internal(message) => f.write_str(message),
        }
    }
}

impl ApiError {
    fn transport(msg: impl Into<String>) -> Self {
        ApiError::Transport(msg.into())
    }
    fn internal(msg: impl Into<String>) -> Self {
        ApiError::Internal(msg.into())
    }
    fn bad(msg: impl Into<String>) -> Self {
        ApiError::BadRequest(msg.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found".to_string()),
            ApiError::BadRequest(m) => (StatusCode::UNPROCESSABLE_ENTITY, m),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, m),
            ApiError::Transport(m) => (StatusCode::SERVICE_UNAVAILABLE, m),
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

// ─────────────────────────────────────────────────────────────────────────────
// Embedded Web UI
// ─────────────────────────────────────────────────────────────────────────────

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn app_js() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        APP_JS,
    )
        .into_response()
}

async fn style_css() -> Response {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        STYLE_CSS,
    )
        .into_response()
}

async fn security_headers(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; connect-src 'self'; img-src 'self' data:; \
             style-src 'self'; script-src 'self'; base-uri 'none'; \
             frame-ancestors 'none'; form-action 'self'",
        ),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    response
}

// ─────────────────────────────────────────────────────────────────────────────
// Input JSON for creating/updating the interface
// ─────────────────────────────────────────────────────────────────────────────

/// Single request body for POST and PUT.
/// `type` defines the variant; the remaining fields are by type.
#[derive(Debug, Deserialize)]
struct InterfaceRequest {
    /// The interface name is the key in the `[interfaces]` section.
    name: String,

    #[serde(rename = "type")]
    iface_type: String,
    enabled: Option<bool>,

    // TCPClientInterface
    target_host: Option<String>,
    target_port: Option<u16>,
    connect_timeout: Option<u64>,
    max_reconnect_tries: Option<u64>,
    fixed_mtu: Option<u32>,

    // TCPServerInterface
    listen_ip: Option<String>,
    listen_port: Option<u16>,
    forward_ip: Option<String>,
    forward_port: Option<u16>,
    prefer_ipv6: Option<bool>,
    device: Option<String>,

    // Shared
    kiss_framing: Option<bool>,
    interface_mode: Option<String>,

    // SerialInterface / KISSInterface
    port: Option<String>,
    speed: Option<u32>,
    databits: Option<u8>,
    parity: Option<String>,
    stopbits: Option<u8>,
    preamble: Option<u32>,
    txtail: Option<u32>,
    persistence: Option<u8>,
    slottime: Option<u32>,
    flow_control: Option<bool>,

    // RNodeInterface
    frequency: Option<u32>,
    bandwidth: Option<u32>,
    spreadingfactor: Option<u8>,
    codingrate: Option<u8>,
    txpower: Option<i8>,
    airtime_limit_short: Option<f32>,
    airtime_limit_long: Option<f32>,
}

impl InterfaceRequest {
    /// Build `ConfigSection` from the request body to run through
    /// `synthesize_interface` with the same validation as during regular parsing.
    fn to_config_section(&self) -> ConfigSection {
        let mut s = ConfigSection::new();
        s.set("type", &self.iface_type);
        s.set(
            "enabled",
            if self.enabled.unwrap_or(true) {
                "Yes"
            } else {
                "No"
            },
        );

        if let Some(ref v) = self.target_host {
            s.set("target_host", v);
        }
        if let Some(v) = self.target_port {
            s.set("target_port", &v.to_string());
        }
        if let Some(v) = self.connect_timeout {
            s.set("connect_timeout", &v.to_string());
        }
        if let Some(v) = self.max_reconnect_tries {
            s.set("max_reconnect_tries", &v.to_string());
        }
        if let Some(v) = self.fixed_mtu {
            s.set("fixed_mtu", &v.to_string());
        }
        if let Some(ref v) = self.listen_ip {
            s.set("listen_ip", v);
        }
        if let Some(v) = self.listen_port {
            s.set("listen_port", &v.to_string());
        }
        if let Some(ref v) = self.forward_ip {
            s.set("forward_ip", v);
        }
        if let Some(v) = self.forward_port {
            s.set("forward_port", &v.to_string());
        }
        if let Some(v) = self.prefer_ipv6 {
            s.set("prefer_ipv6", if v { "Yes" } else { "No" });
        }
        if let Some(ref v) = self.device {
            s.set("device", v);
        }
        if let Some(v) = self.kiss_framing {
            s.set("kiss_framing", if v { "Yes" } else { "No" });
        }
        if let Some(ref v) = self.interface_mode {
            s.set("interface_mode", v);
        }
        if let Some(ref v) = self.port {
            s.set("port", v);
        }
        if let Some(v) = self.speed {
            s.set("speed", &v.to_string());
        }
        if let Some(v) = self.databits {
            s.set("databits", &v.to_string());
        }
        if let Some(ref v) = self.parity {
            s.set("parity", v);
        }
        if let Some(v) = self.stopbits {
            s.set("stopbits", &v.to_string());
        }
        if let Some(v) = self.preamble {
            s.set("preamble", &v.to_string());
        }
        if let Some(v) = self.txtail {
            s.set("txtail", &v.to_string());
        }
        if let Some(v) = self.persistence {
            s.set("persistence", &v.to_string());
        }
        if let Some(v) = self.slottime {
            s.set("slottime", &v.to_string());
        }
        if let Some(v) = self.flow_control {
            s.set("flow_control", if v { "Yes" } else { "No" });
        }
        if let Some(v) = self.frequency {
            s.set("frequency", &v.to_string());
        }
        if let Some(v) = self.bandwidth {
            s.set("bandwidth", &v.to_string());
        }
        if let Some(v) = self.spreadingfactor {
            s.set("spreadingfactor", &v.to_string());
        }
        if let Some(v) = self.codingrate {
            s.set("codingrate", &v.to_string());
        }
        if let Some(v) = self.txpower {
            s.set("txpower", &v.to_string());
        }
        if let Some(v) = self.airtime_limit_short {
            s.set("airtime_limit_short", &v.to_string());
        }
        if let Some(v) = self.airtime_limit_long {
            s.set("airtime_limit_long", &v.to_string());
        }
        s
    }

    /// Validate and build `InterfaceConfig` via `synthesize_interface`.
    fn synthesize(&self) -> Result<InterfaceConfig, ApiError> {
        let section = self.to_config_section();
        synthesize_interface(&self.name, &section).map_err(|e| ApiError::bad(format!("{e}")))
    }

    fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Config change + interface restart
// ─────────────────────────────────────────────────────────────────────────────

fn set_interface_config(config: &mut Config, name: &str, section: ConfigSection) {
    let interfaces = config.ensure_section("interfaces");
    interfaces.remove_subsection(name);
    *interfaces.add_subsection(name.to_string()) = section;
}

fn remove_interface_config(config: &mut Config, name: &str) -> bool {
    config.ensure_section("interfaces").remove_subsection(name)
}

/// Apply interface change:
/// 1. Write the new section to the config file (or delete it).
/// 2. Stop the old interface at runtime (if `old_id` is specified).
/// 3. Start the new interface at runtime (if `new_config` is specified).
///
/// Returns the ID of the new interface (or 0 if deleting).
async fn apply_interface_change(
    s: &AppState,
    iface_name: &str,
    new_section: Option<ConfigSection>, // None = remove from config
    old_id: Option<u64>,                // None = there was no new interface
    new_config: Option<&InterfaceConfig>, // None = deletion only
    renamed_from: Option<&str>,
    rollback_interface: Option<&InterfaceConfig>,
) -> ApiResult<u64> {
    // ── 1. Конфиг ──────────────────────────────────────────────────────────
    let LoadedConfig {
        mut config,
        source: original_source,
    } = s.load_config_snapshot()?;

    if let Some(old_name) = renamed_from {
        remove_interface_config(&mut config, old_name);
    }
    match new_section {
        Some(section) => {
            // Replace or add the [[iface_name]] subsection.
            set_interface_config(&mut config, iface_name, section);
        }
        None => {
            // Remove
            if !remove_interface_config(&mut config, iface_name) {
                return Err(ApiError::NotFound);
            }
        }
    }
    let applied_source = s.save_config(&config, &original_source)?;

    // ── 2. Teardown ─────────────────────────────────────────────────────────
    if let Some(id) = old_id {
        teardown_interface(&s.handle, id).await;
    }

    // ── 3. Spawn ─────────────────────────────────────────────────────────────
    let new_id = match new_config {
        Some(iface_config) => match spawn_from_config(s, iface_config).await {
            Ok(id) => id,
            Err(spawn_error) => {
                let config_rollback = s.rollback_config(&applied_source, &original_source);
                let runtime_rollback = match rollback_interface {
                    Some(old_config) => spawn_from_config(s, old_config).await.map(|_| ()),
                    None => Ok(()),
                };

                if let Err(error) = config_rollback {
                    tracing::error!(error = %error, "failed to roll back config");
                    return Err(ApiError::internal(format!(
                        "{spawn_error}; config rollback failed: {error}"
                    )));
                }
                if let Err(error) = runtime_rollback {
                    tracing::error!(error = %error, "failed to restore previous interface");
                    return Err(ApiError::internal(format!(
                        "{spawn_error}; runtime rollback failed: {error}"
                    )));
                }
                return Err(spawn_error);
            }
        },
        None => 0,
    };

    Ok(new_id)
}

/// Start the interface from `InterfaceConfig`.
/// Uses `spawn_interface_from_config` — the same path as when starting the daemon:
/// post_init, announce rates, and ingress defaults are read from the config on disk.
async fn spawn_from_config(s: &AppState, iface_config: &InterfaceConfig) -> ApiResult<u64> {
    crate::reticulum::spawn_interface_from_config(&s.handle, iface_config)
        .await
        .map_err(|e| ApiError::internal(format!("failed to spawn interface: {e}")))
}

fn interface_type_name(c: &InterfaceConfig) -> &'static str {
    match c {
        InterfaceConfig::TcpClient(_) => "TCPClientInterface",
        InterfaceConfig::TcpServer(_) => "TCPServerInterface",
        InterfaceConfig::Udp(_) => "UDPInterface",
        InterfaceConfig::Auto(_) => "AutoInterface",
        InterfaceConfig::Local(_) => "LocalInterface",
        InterfaceConfig::I2P(_) => "I2PInterface",
        InterfaceConfig::Pipe(_) => "PipeInterface",
        InterfaceConfig::Backbone(_) => "BackboneInterface",
        #[cfg(feature = "serial")]
        InterfaceConfig::Serial(_) => "SerialInterface",
        #[cfg(feature = "serial")]
        InterfaceConfig::KissSerial(_) => "KISSInterface",
        #[cfg(feature = "serial")]
        InterfaceConfig::RNodeMulti(_) => "RNodeMultiInterface",
        #[cfg(feature = "serial")]
        InterfaceConfig::AX25KISS(_) => "AX25KISSInterface",
        #[cfg(any(feature = "serial", feature = "rnode-tcp"))]
        InterfaceConfig::RNode(_) => "RNodeInterface",
        #[cfg(feature = "ble")]
        InterfaceConfig::BleRNode(_) => "BleRNodeInterface",
    }
}

/// Find the id of a running interface by name using transport stats.
async fn find_running_id(s: &AppState, name: &str) -> ApiResult<Option<u64>> {
    let stats = fetch_interfaces(s).await?;
    Ok(stats.into_iter().find(|e| e.name == name).map(|e| e.id))
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers — READ
// ─────────────────────────────────────────────────────────────────────────────

async fn health() -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(json!({ "ok": true })))
}

async fn status(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let stats = fetch_interfaces(&s).await?;
    let sections = load_interface_sections(&s)?;
    let configs = load_interface_configs_from_sections(&sections);
    let total_rx: u64 = stats.iter().map(|e| e.rx_bytes).sum();
    let total_tx: u64 = stats.iter().map(|e| e.tx_bytes).sum();
    let online = stats.iter().filter(|e| e.online).count();
    Ok(Json(json!({
        "interfaces_total":  stats.len(),
        "interfaces_online": online,
        "rx_bytes_total":    total_rx,
        "tx_bytes_total":    total_tx,
        "interfaces":        stats.iter().map(|e| merge_iface_json(
            e,
            configs.get(e.name.as_str()),
            sections.get(e.name.as_str()),
        )).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct InterfacesQuery {
    filter: Option<String>,
    all: Option<bool>,
}

async fn interfaces(
    State(s): State<AppState>,
    Query(q): Query<InterfacesQuery>,
) -> ApiResult<Json<Value>> {
    let stats = fetch_interfaces(&s).await?;
    let config_sections = load_interface_sections(&s)?;
    let configs = load_interface_configs_from_sections(&config_sections);
    let show_all = q.all.unwrap_or(false);
    let mut entries: Vec<_> = stats
        .iter()
        .filter(|e| show_all || visible_by_default(&e.name))
        .filter(|e| {
            q.filter
                .as_deref()
                .is_none_or(|f| e.name.to_lowercase().contains(&f.to_lowercase()))
        })
        .map(|e| {
            merge_iface_json(
                e,
                configs.get(e.name.as_str()),
                config_sections.get(e.name.as_str()),
            )
        })
        .collect();
    for (name, section) in &config_sections {
        if stats.iter().any(|entry| entry.name == *name) {
            continue;
        }
        if q.filter
            .as_deref()
            .is_some_and(|filter| !name.to_lowercase().contains(&filter.to_lowercase()))
        {
            continue;
        }
        entries.push(config_only_iface_json(name, section));
    }
    entries.sort_by(|left, right| {
        let left_name = left["name"].as_str().unwrap_or_default();
        let right_name = right["name"].as_str().unwrap_or_default();
        left_name
            .to_lowercase()
            .cmp(&right_name.to_lowercase())
            .then_with(|| left_name.cmp(right_name))
    });
    Ok(Json(json!({ "interfaces": entries })))
}

async fn interface_by_id(State(s): State<AppState>, Path(id): Path<u64>) -> ApiResult<Json<Value>> {
    let stats = fetch_interfaces(&s).await?;
    let configs = load_interface_configs(&s)?;
    stats
        .iter()
        .find(|e| e.id == id)
        .map(|e| Json(merge_iface_json(e, configs.get(e.name.as_str()), None)))
        .ok_or(ApiError::NotFound)
}

async fn config_interfaces(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let sections = load_interface_sections(&s)?;
    let mut entries: Vec<_> = sections
        .iter()
        .map(|(name, section)| config_only_iface_json(name, section))
        .collect();
    entries.sort_by(|left, right| {
        left["name"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase()
            .cmp(&right["name"].as_str().unwrap_or_default().to_lowercase())
    });
    Ok(Json(json!({ "interfaces": entries })))
}

#[derive(Deserialize)]
struct PathsQuery {
    max_hops: Option<u8>,
}

async fn paths(State(s): State<AppState>, Query(q): Query<PathsQuery>) -> ApiResult<Json<Value>> {
    let entries = match s.query(TransportQuery::GetPathTable).await? {
        TransportQueryResponse::PathTable(v) => v,
        TransportQueryResponse::Error(e) => return Err(ApiError::internal(e)),
        other => {
            return Err(ApiError::internal(format!(
                "unexpected response: {other:?}"
            )));
        }
    };
    let rows: Vec<_> = entries
        .iter()
        .filter(|e| q.max_hops.is_none_or(|max| e.hops <= max))
        .map(path_json)
        .collect();
    Ok(Json(json!({ "paths": rows, "count": rows.len() })))
}

async fn links(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    match s.query(TransportQuery::GetLinkCount).await? {
        TransportQueryResponse::IntResult(n) => Ok(Json(json!({ "link_count": n }))),
        TransportQueryResponse::Error(e) => Err(ApiError::internal(e)),
        other => Err(ApiError::internal(format!("unexpected: {other:?}"))),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers — WRITE
// ─────────────────────────────────────────────────────────────────────────────

/// `POST /api/v1/interfaces` — add a new interface.
///
/// Returns `201 Created` with `{ "id": N, "name": "..." }`.
/// Name conflict → `409 Conflict`.
async fn create_interface(
    State(s): State<AppState>,
    Json(req): Json<InterfaceRequest>,
) -> ApiResult<Response> {
    let _config_guard = s.config_write_lock.lock().await;

    // Reject both running and config-only duplicates.
    if load_interface_sections(&s)?.contains_key(&req.name)
        || find_running_id(&s, &req.name).await?.is_some()
    {
        return Err(ApiError::Conflict(format!(
            "interface '{}' already exists",
            req.name
        )));
    }

    let iface_config = if req.is_enabled() {
        Some(req.synthesize()?)
    } else {
        None
    };

    // Section for writing to the config — we build from req, not from InterfaceConfig,
    // to save only what was received (without defaults).
    let section = req.to_config_section();

    let id = apply_interface_change(
        &s,
        &req.name,
        Some(section),
        None,
        iface_config.as_ref(),
        None,
        None,
    )
    .await?;

    let body = Json(json!({
        "id": iface_config.as_ref().map(|_| id),
        "name": req.name,
        "enabled": req.is_enabled(),
    }));
    Ok((StatusCode::CREATED, body).into_response())
}

/// `PUT /api/v1/interfaces/{id}` — replace the interface configuration.
///
/// Teardowns the old interface and spawns a new one with new parameters.
/// The interface name is taken from the request body; the id is used only to find
/// the currently running interface.
async fn update_interface(
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<InterfaceRequest>,
) -> ApiResult<Json<Value>> {
    let _config_guard = s.config_write_lock.lock().await;

    // Find the old interface by id to know its name for deleting from the config.
    let ifaces = fetch_interfaces(&s).await?;
    let old_entry = ifaces
        .iter()
        .find(|e| e.id == id)
        .ok_or(ApiError::NotFound)?;
    let old_name = old_entry.name.clone();
    let old_configs = load_interface_configs(&s)?;
    let rollback_interface = old_configs.get(&old_name).cloned();

    let iface_config = req.synthesize()?;
    let section = req.to_config_section();

    let renamed_from = (old_name != req.name).then_some(old_name.as_str());
    let new_id = apply_interface_change(
        &s,
        &req.name,
        Some(section),
        Some(id),
        Some(&iface_config),
        renamed_from,
        rollback_interface.as_ref(),
    )
    .await?;

    Ok(Json(
        json!({ "id": new_id, "name": req.name, "old_id": id }),
    ))
}

/// `DELETE /api/v1/interfaces/{id}` — stop and delete the interface.
async fn delete_interface(
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Json<Value>> {
    let _config_guard = s.config_write_lock.lock().await;

    // find a name by id
    let ifaces = fetch_interfaces(&s).await?;
    let entry = ifaces
        .iter()
        .find(|e| e.id == id)
        .ok_or(ApiError::NotFound)?;
    let name = entry.name.clone();

    apply_interface_change(&s, &name, None, Some(id), None, None, None).await?;

    Ok(Json(json!({ "deleted": true, "id": id, "name": name })))
}

/// Update an interface by its stable config name. Unlike the numeric runtime
/// route, this also works for disabled or failed interfaces with no runtime ID.
async fn update_config_interface(
    State(s): State<AppState>,
    Path(old_name): Path<String>,
    Json(req): Json<InterfaceRequest>,
) -> ApiResult<Json<Value>> {
    let _config_guard = s.config_write_lock.lock().await;
    let sections = load_interface_sections(&s)?;
    let old_section = sections.get(&old_name).ok_or(ApiError::NotFound)?;
    if old_name != req.name && sections.contains_key(&req.name) {
        return Err(ApiError::Conflict(format!(
            "interface '{}' already exists",
            req.name
        )));
    }
    let rollback_interface = synthesize_interface(&old_name, old_section).ok();
    let old_id = find_running_id(&s, &old_name).await?;
    let new_config = if req.is_enabled() {
        Some(req.synthesize()?)
    } else {
        None
    };
    let section = req.to_config_section();
    let renamed_from = (old_name != req.name).then_some(old_name.as_str());

    let new_id = apply_interface_change(
        &s,
        &req.name,
        Some(section),
        old_id,
        new_config.as_ref(),
        renamed_from,
        rollback_interface.as_ref(),
    )
    .await?;

    Ok(Json(json!({
        "id": new_config.as_ref().map(|_| new_id),
        "name": req.name,
        "old_name": old_name,
        "enabled": req.is_enabled(),
    })))
}

/// Remove a configured interface by name, whether or not it is still running.
async fn delete_config_interface(
    State(s): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<Value>> {
    let _config_guard = s.config_write_lock.lock().await;
    let sections = load_interface_sections(&s)?;
    if !sections.contains_key(&name) {
        return Err(ApiError::NotFound);
    }
    let old_id = find_running_id(&s, &name).await?;
    apply_interface_change(&s, &name, None, old_id, None, None, None).await?;
    Ok(Json(json!({ "deleted": true, "name": name })))
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

async fn fetch_interfaces(s: &AppState) -> ApiResult<Vec<InterfaceStatRpcEntry>> {
    match s.query(TransportQuery::GetInterfaceStats).await? {
        TransportQueryResponse::InterfaceStats(v) => Ok(v),
        TransportQueryResponse::Error(e) => Err(ApiError::internal(e)),
        other => Err(ApiError::internal(format!("unexpected: {other:?}"))),
    }
}

/// Runtime stats + configuration, combined into a single object.
///
/// `config` will be `None` for interfaces that exist at runtime but are missing
/// in the config (LocalInterface, child TCP clients of servers, etc.).
fn merge_iface_json(
    e: &InterfaceStatRpcEntry,
    config: Option<&InterfaceConfig>,
    section: Option<&ConfigSection>,
) -> Value {
    let mut v = json!({
        // ── identity ──────────────────────────────────────────────────
        "id":                           e.id,
        "name":                         e.name,
        // ── runtime status ────────────────────────────────────────────
        "online":                       e.online,
        "mode":                         e.mode,
        "role":                         e.role,
        "bitrate":                      e.bitrate,
        "mtu":                          e.mtu,
        "ifac_size":                    e.ifac_size,
        "clients":                      e.clients,
        // ── traffic counters ──────────────────────────────────────────
        "rx_bytes":                     e.rx_bytes,
        "tx_bytes":                     e.tx_bytes,
        "rx_rate":                      e.rx_rate,
        "tx_rate":                      e.tx_rate,
        "tx_drops":                     e.tx_drops,
        // ── announce / ingress ────────────────────────────────────────
        "announce_queue":               e.announce_queue,
        "held_announces":               e.held_announces,
        "incoming_announce_frequency":  e.incoming_announce_frequency,
        "outgoing_announce_frequency":  e.outgoing_announce_frequency,
        "announce_rate_target":         e.announce_rate_target,
        "announce_rate_grace":          e.announce_rate_grace,
        "announce_rate_penalty":        e.announce_rate_penalty,
        "announce_cap":                 e.announce_cap,
        "incoming_pr_frequency":        e.incoming_pr_frequency,
        "outgoing_pr_frequency":        e.outgoing_pr_frequency,
        "burst_active":                 e.burst_active,
        "burst_activated":              e.burst_activated,
        "pr_burst_active":              e.pr_burst_active,
        "pr_burst_activated":           e.pr_burst_activated,
        // ── config (null when not in config file) ─────────────────────
        "config":                       Value::Null,
    });

    if let Some(section) = section {
        v["config"] = iface_section_json(section);
        v["configured"] = json!(true);
        v["enabled"] = json!(section_enabled(section));
    } else if let Some(cfg) = config {
        v["config"] = iface_config_json(cfg);
        v["configured"] = json!(true);
        v["enabled"] = json!(true);
    } else {
        v["configured"] = json!(false);
        v["enabled"] = json!(true);
    }
    v
}

fn config_only_iface_json(name: &str, section: &ConfigSection) -> Value {
    json!({
        "id": Value::Null,
        "name": name,
        "online": false,
        "mode": section.get("interface_mode"),
        "role": "configured",
        "bitrate": Value::Null,
        "mtu": Value::Null,
        "ifac_size": Value::Null,
        "clients": Value::Null,
        "rx_bytes": 0,
        "tx_bytes": 0,
        "rx_rate": 0,
        "tx_rate": 0,
        "tx_drops": 0,
        "announce_queue": 0,
        "held_announces": 0,
        "incoming_announce_frequency": 0,
        "outgoing_announce_frequency": 0,
        "configured": true,
        "enabled": section_enabled(section),
        "config": iface_section_json(section),
    })
}

fn section_enabled(section: &ConfigSection) -> bool {
    section
        .get_bool("enabled")
        .or_else(|| section.get_bool("interface_enabled"))
        .unwrap_or(true)
}

fn iface_section_json(section: &ConfigSection) -> Value {
    json!({
        "type": section.get("type"),
        "enabled": section_enabled(section),
        "target_host": section.get("target_host"),
        "target_port": section.get_uint("target_port"),
        "connect_timeout": section.get_uint("connect_timeout"),
        "max_reconnect_tries": section.get_uint("max_reconnect_tries"),
        "fixed_mtu": section.get_uint("fixed_mtu"),
        "listen_ip": section.get("listen_ip"),
        "listen_port": section.get_uint("listen_port"),
        "forward_ip": section.get("forward_ip"),
        "forward_port": section.get_uint("forward_port"),
        "prefer_ipv6": section.get_bool("prefer_ipv6"),
        "device": section.get("device"),
        "kiss_framing": section.get_bool("kiss_framing"),
        "interface_mode": section.get("interface_mode").unwrap_or("Full"),
        "port": section.get("port"),
        "speed": section.get_uint("speed").or_else(|| section.get_uint("baud_rate")),
        "databits": section.get_uint("databits").or_else(|| section.get_uint("data_bits")),
        "parity": section.get("parity"),
        "stopbits": section.get_uint("stopbits").or_else(|| section.get_uint("stop_bits")),
        "preamble": section.get_uint("preamble"),
        "txtail": section.get_uint("txtail"),
        "persistence": section.get_uint("persistence"),
        "slottime": section.get_uint("slottime"),
        "flow_control": section.get_bool("flow_control"),
        "frequency": section.get_uint("frequency"),
        "bandwidth": section.get_uint("bandwidth"),
        "spreadingfactor": section.get_uint("spreadingfactor")
            .or_else(|| section.get_uint("spreading_factor")),
        "codingrate": section.get_uint("codingrate").or_else(|| section.get_uint("coding_rate")),
        "txpower": section.get_int("txpower").or_else(|| section.get_int("tx_power")),
        "airtime_limit_short": section.get_float("airtime_limit_short")
            .or_else(|| section.get_float("st_alock")),
        "airtime_limit_long": section.get_float("airtime_limit_long")
            .or_else(|| section.get_float("lt_alock")),
    })
}

/// Serialize `InterfaceConfig` to JSON with full settings.
/// Only fields specific to this type.
fn iface_config_json(cfg: &InterfaceConfig) -> Value {
    match cfg {
        InterfaceConfig::TcpClient(c) => json!({
            "type":                "TCPClientInterface",
            "target_host":         c.target_host,
            "target_port":         c.target_port,
            "interface_mode":      mode_to_str(c.mode),
            "kiss_framing":        c.kiss_framing,
            "connect_timeout":     c.connect_timeout_secs,
            "max_reconnect_tries": c.max_reconnect_tries,
            "fixed_mtu":           c.fixed_mtu,
        }),
        InterfaceConfig::TcpServer(c) => json!({
            "type":           "TCPServerInterface",
            "listen_ip":      c.listen_ip,
            "listen_port":    c.listen_port,
            "interface_mode": mode_to_str(c.mode),
            "kiss_framing":   c.kiss_framing,
            "prefer_ipv6":    c.prefer_ipv6,
            "device":         c.device,
        }),
        InterfaceConfig::Udp(c) => json!({
            "type":           "UDPInterface",
            "listen_ip":      c.listen_ip,
            "listen_port":    c.listen_port,
            "forward_ip":     c.forward_ip,
            "forward_port":   c.forward_port,
            "device":         c.device,
            "interface_mode": mode_to_str(c.mode),
        }),
        #[cfg(feature = "serial")]
        InterfaceConfig::Serial(c) => json!({
            "type":           "SerialInterface",
            "port":           c.port,
            "speed":          c.baud_rate,
            "databits":       c.data_bits,
            "parity":         c.parity,
            "stopbits":       c.stop_bits,
            "interface_mode": mode_to_str(c.mode),
        }),
        #[cfg(feature = "serial")]
        InterfaceConfig::KissSerial(c) => json!({
            "type":           "KISSInterface",
            "port":           c.port,
            "speed":          c.baud_rate,
            "databits":       c.data_bits,
            "parity":         c.parity,
            "stopbits":       c.stop_bits,
            "preamble":       c.preamble_ms,
            "txtail":         c.txtail_ms,
            "persistence":    c.persistence,
            "slottime":       c.slottime_ms,
            "flow_control":   c.flow_control,
            "interface_mode": mode_to_str(c.mode),
        }),
        #[cfg(any(feature = "serial", feature = "rnode-tcp"))]
        InterfaceConfig::RNode(c) => json!({
            "type":                "RNodeInterface",
            "port":                c.port,
            "frequency":           c.frequency,
            "bandwidth":           c.bandwidth,
            "spreadingfactor":     c.spreading_factor,
            "codingrate":          c.coding_rate,
            "txpower":             c.tx_power,
            "flow_control":        c.flow_control,
            "airtime_limit_short": c.st_alock,
            "airtime_limit_long":  c.lt_alock,
            "interface_mode":      mode_to_str(c.mode),
        }),
        // The remaining types return only the type - it is expanded by analogy.
        other => json!({ "type": interface_type_name(other) }),
    }
}

fn mode_to_str(mode: rns_interface::traits::InterfaceMode) -> &'static str {
    use rns_interface::traits::InterfaceMode::*;
    match mode {
        Full => "Full",
        PointToPoint => "PointToPoint",
        AccessPoint => "AccessPoint",
        Roaming => "Roaming",
        Boundary => "Boundary",
        Gateway => "Gateway",
        Internal => "Internal",
    }
}

/// Load all interfaces from the config and return the HashMap name → InterfaceConfig.
fn load_interface_configs(
    s: &AppState,
) -> ApiResult<std::collections::HashMap<String, InterfaceConfig>> {
    Ok(load_interface_configs_from_sections(
        &load_interface_sections(s)?,
    ))
}

fn load_interface_sections(
    s: &AppState,
) -> ApiResult<std::collections::HashMap<String, ConfigSection>> {
    let config = s.load_config()?;
    Ok(config
        .subsections("interfaces")
        .into_iter()
        .map(|(name, section)| (name.to_string(), section.clone()))
        .collect())
}

fn load_interface_configs_from_sections(
    sections: &std::collections::HashMap<String, ConfigSection>,
) -> std::collections::HashMap<String, InterfaceConfig> {
    let mut map = std::collections::HashMap::new();
    for (name, section) in sections {
        // Disabled and unknown interface types are not present at runtime.
        if let Ok(cfg) = synthesize_interface(name, section) {
            map.insert(name.clone(), cfg);
        }
    }
    map
}

fn path_json(e: &PathTableRpcEntry) -> Value {
    json!({
        "hash":      hex::encode(e.hash),
        "hops":      e.hops,
        "interface": e.interface,
        "via":       e.via.map(hex::encode),
        "timestamp": e.timestamp,
        "expires":   e.expires,
    })
}

fn visible_by_default(name: &str) -> bool {
    !(name.starts_with("LocalInterface[")
        || name.starts_with("TCPInterface[Client")
        || name.starts_with("BackboneInterface[Client on")
        || name.starts_with("AutoInterfacePeer[")
        || name.starts_with("WeaveInterfacePeer[")
        || name.starts_with("I2PInterfacePeer[Connected peer"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, header};
    use tower::ServiceExt;

    fn web_router() -> Router {
        Router::new()
            .route("/", get(index))
            .route("/app.js", get(app_js))
            .route("/style.css", get(style_css))
            .layer(middleware::from_fn(security_headers))
    }

    #[tokio::test]
    async fn embedded_web_assets_have_expected_content_types() {
        let app = web_router();
        for (path, expected) in [
            ("/", "text/html; charset=utf-8"),
            ("/app.js", "text/javascript; charset=utf-8"),
            ("/style.css", "text/css; charset=utf-8"),
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers().get(header::CONTENT_TYPE).unwrap(),
                expected
            );
        }
    }

    #[tokio::test]
    async fn web_router_returns_404_for_unknown_route_with_security_headers() {
        let response = web_router()
            .oneshot(
                Request::builder()
                    .uri("/does-not-exist")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .unwrap(),
            "nosniff"
        );
        assert_eq!(
            response.headers().get(header::X_FRAME_OPTIONS).unwrap(),
            "DENY"
        );
        assert_eq!(
            response.headers().get(header::REFERRER_POLICY).unwrap(),
            "no-referrer"
        );
        assert!(
            response
                .headers()
                .get(header::CONTENT_SECURITY_POLICY)
                .unwrap()
                .to_str()
                .unwrap()
                .contains("frame-ancestors 'none'")
        );
    }

    #[tokio::test]
    async fn api_errors_are_json_with_status() {
        let response = ApiError::bad("invalid interface").into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({"error": "invalid interface"})
        );
    }

    #[test]
    fn tcp_client_request_validates_and_serializes() {
        let request: InterfaceRequest = serde_json::from_value(json!({
            "name": "Test client",
            "type": "TCPClientInterface",
            "target_host": "example.test",
            "target_port": 4242,
            "connect_timeout": 7,
            "max_reconnect_tries": 3,
            "fixed_mtu": 800,
            "interface_mode": "PointToPoint",
            "kiss_framing": true
        }))
        .unwrap();

        let config = request.synthesize().unwrap();
        let value = iface_config_json(&config);
        assert_eq!(value["type"], "TCPClientInterface");
        assert_eq!(value["target_host"], "example.test");
        assert_eq!(value["target_port"], 4242);
        assert_eq!(value["connect_timeout"], 7);
        assert_eq!(value["max_reconnect_tries"], 3);
        assert_eq!(value["fixed_mtu"], 800);
        assert_eq!(value["interface_mode"], "PointToPoint");
        assert_eq!(value["kiss_framing"], true);
    }

    #[test]
    fn tcp_server_request_validates_and_serializes() {
        let request: InterfaceRequest = serde_json::from_value(json!({
            "name": "Test server",
            "type": "TCPServerInterface",
            "listen_ip": "127.0.0.1",
            "listen_port": 4242,
            "prefer_ipv6": true,
            "device": "lo",
            "interface_mode": "Full",
            "kiss_framing": false
        }))
        .unwrap();

        let config = request.synthesize().unwrap();
        let value = iface_config_json(&config);
        assert_eq!(value["type"], "TCPServerInterface");
        assert_eq!(value["listen_ip"], "127.0.0.1");
        assert_eq!(value["listen_port"], 4242);
        assert_eq!(value["prefer_ipv6"], true);
        assert_eq!(value["device"], "lo");
        assert_eq!(value["interface_mode"], "Full");
        assert_eq!(value["kiss_framing"], false);
    }

    #[test]
    fn udp_request_validates_and_serializes() {
        let request: InterfaceRequest = serde_json::from_value(json!({
            "name": "Test UDP",
            "type": "UDPInterface",
            "listen_ip": "127.0.0.1",
            "listen_port": 4242,
            "forward_ip": "127.0.0.1",
            "forward_port": 4243,
            "device": "lo",
            "interface_mode": "PointToPoint"
        }))
        .unwrap();

        let config = request.synthesize().unwrap();
        let value = iface_config_json(&config);
        assert_eq!(value["type"], "UDPInterface");
        assert_eq!(value["listen_ip"], "127.0.0.1");
        assert_eq!(value["listen_port"], 4242);
        assert_eq!(value["forward_ip"], "127.0.0.1");
        assert_eq!(value["forward_port"], 4243);
        assert_eq!(value["device"], "lo");
        assert_eq!(value["interface_mode"], "PointToPoint");
    }

    #[cfg(feature = "serial")]
    #[test]
    fn serial_request_validates_and_serializes() {
        let request: InterfaceRequest = serde_json::from_value(json!({
            "name": "Test serial",
            "type": "SerialInterface",
            "port": "/dev/ttyUSB0",
            "speed": 115200,
            "databits": 8,
            "parity": "N",
            "stopbits": 1,
            "interface_mode": "Full"
        }))
        .unwrap();

        let value = iface_config_json(&request.synthesize().unwrap());
        assert_eq!(value["type"], "SerialInterface");
        assert_eq!(value["port"], "/dev/ttyUSB0");
        assert_eq!(value["speed"], 115200);
        assert_eq!(value["databits"], 8);
        assert_eq!(value["parity"], "N");
        assert_eq!(value["stopbits"], 1);
    }

    #[cfg(feature = "serial")]
    #[test]
    fn kiss_request_validates_and_serializes() {
        let request: InterfaceRequest = serde_json::from_value(json!({
            "name": "Test KISS",
            "type": "KISSInterface",
            "port": "tcp://127.0.0.1:8001",
            "speed": 57600,
            "databits": 8,
            "parity": "N",
            "stopbits": 1,
            "preamble": 150,
            "txtail": 10,
            "persistence": 200,
            "slottime": 30,
            "flow_control": true
        }))
        .unwrap();

        let value = iface_config_json(&request.synthesize().unwrap());
        assert_eq!(value["type"], "KISSInterface");
        assert_eq!(value["port"], "tcp://127.0.0.1:8001");
        assert_eq!(value["speed"], 57600);
        assert_eq!(value["preamble"], 150);
        assert_eq!(value["txtail"], 10);
        assert_eq!(value["persistence"], 200);
        assert_eq!(value["slottime"], 30);
        assert_eq!(value["flow_control"], true);
        assert!(value.get("id_interval").is_none());
        assert!(value.get("id_callsign").is_none());
    }

    #[cfg(feature = "serial")]
    #[test]
    fn rnode_request_validates_and_serializes() {
        let request: InterfaceRequest = serde_json::from_value(json!({
            "name": "Test RNode",
            "type": "RNodeInterface",
            "port": "/dev/ttyUSB0",
            "frequency": 867200000,
            "bandwidth": 125000,
            "spreadingfactor": 8,
            "codingrate": 5,
            "txpower": 7,
            "flow_control": true,
            "airtime_limit_short": 25.0,
            "airtime_limit_long": 2.5
        }))
        .unwrap();

        let value = iface_config_json(&request.synthesize().unwrap());
        assert_eq!(value["type"], "RNodeInterface");
        assert_eq!(value["port"], "/dev/ttyUSB0");
        assert_eq!(value["frequency"], 867200000);
        assert_eq!(value["bandwidth"], 125000);
        assert_eq!(value["spreadingfactor"], 8);
        assert_eq!(value["codingrate"], 5);
        assert_eq!(value["txpower"], 7);
        assert_eq!(value["flow_control"], true);
        assert_eq!(value["airtime_limit_short"], 25.0);
        assert_eq!(value["airtime_limit_long"], 2.5);
        assert!(value.get("id_interval").is_none());
        assert!(value.get("id_callsign").is_none());
    }

    #[test]
    fn invalid_tcp_request_returns_validation_error() {
        let request: InterfaceRequest = serde_json::from_value(json!({
            "name": "Broken client",
            "type": "TCPClientInterface",
            "target_port": 4242
        }))
        .unwrap();

        assert!(matches!(request.synthesize(), Err(ApiError::BadRequest(_))));
    }

    #[test]
    fn disabled_interface_is_serialized_without_runtime_config() {
        let request: InterfaceRequest = serde_json::from_value(json!({
            "name": "Standby server",
            "type": "TCPServerInterface",
            "enabled": false,
            "listen_ip": "127.0.0.1",
            "listen_port": 4242
        }))
        .unwrap();
        let section = request.to_config_section();

        assert!(!request.is_enabled());
        assert!(!section_enabled(&section));
        assert!(request.synthesize().is_err());

        let value = config_only_iface_json(&request.name, &section);
        assert_eq!(value["id"], Value::Null);
        assert_eq!(value["online"], false);
        assert_eq!(value["enabled"], false);
        assert_eq!(value["config"]["type"], "TCPServerInterface");
        assert_eq!(value["config"]["listen_port"], 4242);
    }

    #[test]
    fn interface_config_sections_can_be_created_renamed_and_deleted() {
        let mut config = Config::parse("[interfaces]\n").unwrap();
        let request: InterfaceRequest = serde_json::from_value(json!({
            "name": "First name",
            "type": "TCPServerInterface",
            "listen_ip": "127.0.0.1",
            "listen_port": 4242
        }))
        .unwrap();

        set_interface_config(&mut config, &request.name, request.to_config_section());
        assert!(
            config
                .subsections("interfaces")
                .iter()
                .any(|(name, _)| *name == "First name")
        );

        assert!(remove_interface_config(&mut config, "First name"));
        set_interface_config(&mut config, "Renamed", request.to_config_section());
        let serialized = config.to_ini();
        assert!(serialized.contains("[[Renamed]]"));
        assert!(!serialized.contains("[[First name]]"));

        assert!(remove_interface_config(&mut config, "Renamed"));
        assert!(config.subsections("interfaces").is_empty());
        assert!(!remove_interface_config(&mut config, "Missing"));
    }

    #[test]
    fn config_snapshot_creates_backup_rolls_back_and_detects_external_edits() {
        let dir = std::env::temp_dir().join(format!(
            "rns_api_config_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config");
        let original = b"[interfaces]\n# original comment\n".to_vec();
        std::fs::write(&path, &original).unwrap();

        let mut config = Config::parse(std::str::from_utf8(&original).unwrap()).unwrap();
        let mut section = ConfigSection::new();
        section.set("type", "TCPServerInterface");
        section.set("listen_ip", "127.0.0.1");
        section.set("listen_port", "4242");
        set_interface_config(&mut config, "Test", section);

        let applied = save_config_snapshot(&path, &config, &original).unwrap();
        assert_eq!(
            std::fs::read(dir.join("config.web-ui.bak")).unwrap(),
            original
        );
        assert_eq!(std::fs::read(&path).unwrap(), applied);

        rollback_config_snapshot(&path, &applied, &original).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original);

        std::fs::write(&path, b"[interfaces]\n# external edit\n").unwrap();
        assert!(matches!(
            save_config_snapshot(&path, &config, &original),
            Err(ApiError::Conflict(_))
        ));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"[interfaces]\n# external edit\n"
        );

        let _ = std::fs::remove_dir_all(dir);
    }
}
