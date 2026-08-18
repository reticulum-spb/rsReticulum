//! Embedded REST API server for rnsd-rs.
//!
//! Activated via `--features api` and the `api` YAML mapping:
//!
//! ```yaml
//! api:
//!   port: 8080
//!   user: admin
//!   password: change-me
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

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderValue, Request, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::{get, post, put};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use rns_transport::messages::{
    InterfaceStatRpcEntry, PathTableRpcEntry, TransportMessage, TransportQuery,
    TransportQueryResponse,
};

use crate::config_compat::{Config, ConfigSection, atomic_write};
use crate::interface_factory::{InterfaceConfig, synthesize_interface};
use crate::lifecycle::ShutdownSignal;
use crate::reticulum::{ReticulumConfig, ReticulumHandle, SharedInstanceType, teardown_interface};

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const STYLE_CSS: &str = include_str!("../web/style.css");
pub const RESTART_EXIT_CODE: u8 = 100;
pub const REBOOT_EXIT_CODE: u8 = 101;

// ─────────────────────────────────────────────────────────────────────────────
// Starting the server
// ─────────────────────────────────────────────────────────────────────────────

pub async fn run_api_server(
    listen: SocketAddr,
    transport_tx: mpsc::Sender<TransportMessage>,
    handle: ReticulumHandle,
    shutdown: ShutdownSignal,
) {
    let auth = Arc::new(AuthState {
        user: handle.config.api_user.clone().unwrap_or_default(),
        password: handle.config.api_password.clone().unwrap_or_default(),
        sessions: Mutex::new(HashSet::new()),
        throttle: Mutex::new(LoginThrottle::default()),
    });
    let state = AppState {
        transport_tx,
        handle,
        shutdown: shutdown.clone(),
        config_write_lock: Arc::new(Mutex::new(())),
        auth,
    };

    let protected = Router::new()
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
        .route("/api/v1/logs", get(log_history))
        .route("/api/v1/logs/stream", get(log_stream))
        .route("/api/v1/settings", get(settings).put(update_settings))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/system/restart", post(restart_daemon))
        .route("/api/v1/system/reboot", post(reboot_system))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth));
    let app = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style_css))
        .route("/health", get(health))
        .route("/api/v1/auth/login", post(login))
        .merge(protected)
        .layer(DefaultBodyLimit::max(64 * 1024))
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
    shutdown: ShutdownSignal,
    auth: Arc<AuthState>,
}

struct AuthState {
    user: String,
    password: String,
    sessions: Mutex<HashSet<String>>,
    throttle: Mutex<LoginThrottle>,
}

#[derive(Default)]
struct LoginThrottle {
    failures: u8,
    blocked_until: Option<std::time::Instant>,
}

struct LoadedConfig {
    config: Config,
    typed: crate::config::Config,
    source: Vec<u8>,
}

#[derive(Deserialize)]
struct SettingsRequest {
    share_instance: bool,
    instance_name: String,
    shared_instance_type: String,
    shared_instance_port: u16,
    instance_control_port: u16,
    enable_transport: bool,
    static_transport_identity: bool,
    local_hops_delta: bool,
    respond_to_probes: bool,
    use_implicit_proof: bool,
    panic_on_interface_error: bool,
    link_mtu_discovery: bool,
    force_shared_instance_bitrate: Option<u64>,
    default_ar_target: Option<u64>,
    default_ar_grace: Option<u32>,
    default_ar_penalty: Option<u64>,
    discover_interfaces: bool,
    autoconnect_discovered_interfaces: usize,
    required_discovery_value: u8,
    api_port: u16,
    api_user: String,
    api_password: Option<String>,
    loglevel: i32,
    logtimestamps: bool,
}

fn save_config_snapshot(
    path: &std::path::Path,
    config: &crate::config::Config,
    expected: &[u8],
) -> Result<Vec<u8>, ApiError> {
    let current = std::fs::read(path)
        .map_err(|e| ApiError::internal(format!("failed to re-read config: {e}")))?;
    if current != expected {
        return Err(ApiError::Conflict(
            "config changed outside the Web UI; reload and retry".to_string(),
        ));
    }

    let backup_path = path.with_file_name("config.yaml.web-ui.bak");
    atomic_write(&backup_path, expected)
        .map_err(|e| ApiError::internal(format!("failed to write config backup: {e}")))?;

    config
        .validate()
        .map_err(|e| ApiError::bad(format!("invalid configuration: {e}")))?;
    let updated = config
        .to_yaml()
        .map_err(|e| ApiError::internal(format!("failed to serialize config: {e}")))?
        .into_bytes();
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
        self.handle.config_dir.join(crate::config::CONFIG_FILE_NAME)
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
        let typed = crate::config::Config::parse(text, &path)
            .map_err(|e| ApiError::internal(format!("failed to parse config: {e}")))?;
        let config = typed
            .to_runtime_compat_config()
            .map_err(|e| ApiError::internal(format!("failed to normalize config: {e}")))?;
        Ok(LoadedConfig {
            config,
            typed,
            source,
        })
    }

    fn save_config(
        &self,
        config: &crate::config::Config,
        expected: &[u8],
    ) -> Result<Vec<u8>, ApiError> {
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

    // BackboneInterface
    listen_on: Option<String>,
    i2p_tunneled: Option<bool>,

    // Shared
    kiss_framing: Option<bool>,
    interface_mode: Option<String>,

    // AutoInterface
    group_id: Option<String>,
    discovery_scope: Option<String>,
    discovery_port: Option<u16>,
    data_port: Option<u16>,
    multicast_address_type: Option<String>,
    devices: Option<String>,
    ignored_devices: Option<String>,
    configured_bitrate: Option<u64>,

    // Common advanced options
    outgoing: Option<bool>,
    bitrate: Option<u64>,
    announce_cap: Option<f64>,
    announce_rate_target: Option<u64>,
    announce_rate_grace: Option<u32>,
    announce_rate_penalty: Option<u64>,
    network_name: Option<String>,
    passphrase: Option<String>,
    ifac_size: Option<usize>,
    ingress_control: Option<bool>,
    ic_burst_freq_new: Option<f64>,
    ic_burst_freq: Option<f64>,
    ic_pr_burst_freq_new: Option<f64>,
    ic_pr_burst_freq: Option<f64>,
    ic_new_time: Option<f64>,
    ic_burst_hold: Option<f64>,
    ic_burst_penalty: Option<f64>,
    ic_max_held_announces: Option<u64>,
    ic_held_release_interval: Option<f64>,
    ec_pr_freq: Option<f64>,
    egress_control: Option<bool>,
    recursive_prs: Option<bool>,
    announces_from_internal: Option<bool>,

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

    // AX25KISSInterface
    callsign: Option<String>,
    ssid: Option<u8>,
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
        if let Some(ref v) = self.listen_on {
            s.set("listen_on", v);
        }
        if let Some(v) = self.i2p_tunneled {
            s.set("i2p_tunneled", if v { "Yes" } else { "No" });
        }
        if let Some(v) = self.kiss_framing {
            s.set("kiss_framing", if v { "Yes" } else { "No" });
        }
        if let Some(ref v) = self.interface_mode {
            s.set("interface_mode", v);
        }
        if let Some(ref v) = self.group_id {
            s.set("group_id", v);
        }
        if let Some(ref v) = self.discovery_scope {
            s.set("discovery_scope", v);
        }
        if let Some(v) = self.discovery_port {
            s.set("discovery_port", &v.to_string());
        }
        if let Some(v) = self.data_port {
            s.set("data_port", &v.to_string());
        }
        if let Some(ref v) = self.multicast_address_type {
            s.set("multicast_address_type", v);
        }
        if let Some(ref v) = self.devices {
            s.set("devices", v);
        }
        if let Some(ref v) = self.ignored_devices {
            s.set("ignored_devices", v);
        }
        if let Some(v) = self.configured_bitrate {
            s.set("configured_bitrate", &v.to_string());
        }
        for (key, value) in [
            ("outgoing", self.outgoing),
            ("ingress_control", self.ingress_control),
            ("egress_control", self.egress_control),
            ("recursive_prs", self.recursive_prs),
            ("announces_from_internal", self.announces_from_internal),
        ] {
            if let Some(value) = value {
                s.set(key, if value { "Yes" } else { "No" });
            }
        }
        for (key, value) in [
            ("bitrate", self.bitrate),
            ("announce_rate_target", self.announce_rate_target),
            ("announce_rate_penalty", self.announce_rate_penalty),
            ("ic_max_held_announces", self.ic_max_held_announces),
        ] {
            if let Some(value) = value {
                s.set(key, &value.to_string());
            }
        }
        if let Some(value) = self.announce_rate_grace {
            s.set("announce_rate_grace", &value.to_string());
        }
        for (key, value) in [
            ("announce_cap", self.announce_cap),
            ("ic_burst_freq_new", self.ic_burst_freq_new),
            ("ic_burst_freq", self.ic_burst_freq),
            ("ic_pr_burst_freq_new", self.ic_pr_burst_freq_new),
            ("ic_pr_burst_freq", self.ic_pr_burst_freq),
            ("ic_new_time", self.ic_new_time),
            ("ic_burst_hold", self.ic_burst_hold),
            ("ic_burst_penalty", self.ic_burst_penalty),
            ("ic_held_release_interval", self.ic_held_release_interval),
            ("ec_pr_freq", self.ec_pr_freq),
        ] {
            if let Some(value) = value {
                s.set(key, &value.to_string());
            }
        }
        if let Some(ref value) = self.network_name {
            s.set("networkname", value);
        }
        if let Some(ref value) = self.passphrase {
            s.set("passphrase", value);
        }
        if let Some(value) = self.ifac_size {
            s.set("ifac_size", &value.to_string());
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
        if self.iface_type == "AX25KISSInterface" {
            if let Some(ref v) = self.callsign {
                s.set("callsign", v);
            }
            if let Some(v) = self.ssid {
                s.set("ssid", &v.to_string());
            }
        }
        s
    }

    /// Validate and build `InterfaceConfig` via `synthesize_interface`.
    fn synthesize(&self) -> Result<InterfaceConfig, ApiError> {
        self.validate_fields()?;
        let section = self.to_config_section();
        synthesize_interface(&self.name, &section).map_err(|e| ApiError::bad(format!("{e}")))
    }

    fn to_yaml_config(&self) -> Result<crate::config::InterfaceConfig, ApiError> {
        self.validate_fields()?;
        crate::config::interface_from_compat_section(&self.name, &self.to_config_section())
            .map_err(|error| ApiError::bad(error.to_string()))
    }

    fn validate_fields(&self) -> Result<(), ApiError> {
        #[cfg(feature = "serial")]
        if self.iface_type == "AX25KISSInterface" {
            let callsign = self
                .callsign
                .as_deref()
                .ok_or_else(|| ApiError::bad("missing callsign"))?;
            let ssid = self.ssid.ok_or_else(|| ApiError::bad("missing ssid"))?;
            rns_interface::ax25kiss::parse_callsign_ssid(&format!("{callsign}-{ssid}"))
                .map_err(ApiError::bad)?;
        }
        Ok(())
    }

    fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Config change + interface restart
// ─────────────────────────────────────────────────────────────────────────────

fn set_interface_config(
    config: &mut crate::config::Config,
    interface: crate::config::InterfaceConfig,
) {
    let name = interface.common().name.as_str();
    config
        .interfaces
        .retain(|existing| existing.common().name != name);
    config.interfaces.push(interface);
}

fn remove_interface_config(config: &mut crate::config::Config, name: &str) -> bool {
    let before = config.interfaces.len();
    config
        .interfaces
        .retain(|entry| entry.common().name != name);
    config.interfaces.len() != before
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
    new_yaml_config: Option<crate::config::InterfaceConfig>, // None = remove
    old_id: Option<u64>,                                     // None = there was no new interface
    new_config: Option<&InterfaceConfig>,                    // None = deletion only
    renamed_from: Option<&str>,
    rollback_interface: Option<&InterfaceConfig>,
) -> ApiResult<u64> {
    // ── 1. Конфиг ──────────────────────────────────────────────────────────
    let LoadedConfig {
        mut typed,
        source: original_source,
        ..
    } = s.load_config_snapshot()?;

    if let Some(old_name) = renamed_from {
        remove_interface_config(&mut typed, old_name);
    }
    match new_yaml_config {
        Some(interface) => {
            set_interface_config(&mut typed, interface);
        }
        None => {
            if !remove_interface_config(&mut typed, iface_name) {
                return Err(ApiError::NotFound);
            }
        }
    }
    let applied_source = s.save_config(&typed, &original_source)?;

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

#[derive(Deserialize)]
struct LoginRequest {
    user: String,
    password: String,
}

async fn login(State(s): State<AppState>, Json(req): Json<LoginRequest>) -> Response {
    let mut throttle = s.auth.throttle.lock().await;
    if throttle
        .blocked_until
        .is_some_and(|until| until > std::time::Instant::now())
    {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, "30")],
            Json(json!({ "error": "too many failed login attempts" })),
        )
            .into_response();
    }
    if req.user != s.auth.user || req.password != s.auth.password {
        throttle.failures = throttle.failures.saturating_add(1);
        if throttle.failures >= 5 {
            throttle.failures = 0;
            throttle.blocked_until =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
        }
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid user or password" })),
        )
            .into_response();
    }
    throttle.failures = 0;
    throttle.blocked_until = None;
    drop(throttle);
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    s.auth.sessions.lock().await.insert(token.clone());
    let cookie = format!("rns_session={token}; Path=/; HttpOnly; SameSite=Strict");
    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(json!({ "ok": true })),
    )
        .into_response()
}

async fn logout(State(s): State<AppState>, request: Request<Body>) -> Response {
    if let Some(token) = session_token(request.headers()) {
        s.auth.sessions.lock().await.remove(token);
    }
    (
        StatusCode::OK,
        [(
            header::SET_COOKIE,
            "rns_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0",
        )],
        Json(json!({ "ok": true })),
    )
        .into_response()
}

async fn require_auth(State(s): State<AppState>, request: Request<Body>, next: Next) -> Response {
    if request.method() != axum::http::Method::GET
        && request.method() != axum::http::Method::HEAD
        && !same_origin(request.headers())
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "cross-origin request rejected" })),
        )
            .into_response();
    }
    let authorized = match session_token(request.headers()) {
        Some(token) => s.auth.sessions.lock().await.contains(token),
        None => false,
    };
    if authorized {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "authentication required" })),
        )
            .into_response()
    }
}

fn same_origin(headers: &axum::http::HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Some(host) = headers.get(header::HOST) else {
        return false;
    };
    let (Ok(origin), Ok(host)) = (origin.to_str(), host.to_str()) else {
        return false;
    };
    origin
        .split_once("://")
        .is_some_and(|(_, authority)| authority.trim_end_matches('/') == host)
}

fn session_token(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|cookie| cookie.strip_prefix("rns_session="))
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

async fn log_history() -> Json<Value> {
    Json(json!({ "entries": crate::web_logs::history() }))
}

async fn log_stream()
-> Sse<impl tokio_stream::Stream<Item = Result<SseEvent, std::convert::Infallible>>> {
    let stream = BroadcastStream::new(crate::web_logs::subscribe()).filter_map(|entry| {
        entry.ok().and_then(|entry| {
            serde_json::to_string(&entry).ok().map(|data| {
                Ok(SseEvent::default()
                    .id(entry.id.to_string())
                    .event("log")
                    .data(data))
            })
        })
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn settings(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let config = s.load_config()?;
    let parsed = ReticulumConfig::try_from_config(&config)
        .map_err(|e| ApiError::internal(format!("invalid settings: {e}")))?;
    let restart_required = settings_differ(&parsed, &s.handle.config);
    Ok(Json(json!({
        "share_instance": parsed.share_instance,
        "instance_name": parsed.instance_name,
        "shared_instance_type": match parsed.shared_instance_type {
            SharedInstanceType::Tcp => "tcp",
            SharedInstanceType::Unix => "unix",
        },
        "shared_instance_port": parsed.shared_instance_port,
        "instance_control_port": parsed.control_port,
        "enable_transport": parsed.enable_transport,
        "static_transport_identity": parsed.static_transport_identity,
        "local_hops_delta": parsed.local_hops_delta,
        "respond_to_probes": parsed.respond_to_probes,
        "use_implicit_proof": parsed.use_implicit_proof,
        "panic_on_interface_error": parsed.panic_on_interface_error,
        "link_mtu_discovery": parsed.link_mtu_discovery,
        "force_shared_instance_bitrate": parsed.force_shared_instance_bitrate,
        "default_ar_target": parsed.default_ar_target,
        "default_ar_grace": parsed.default_ar_grace,
        "default_ar_penalty": parsed.default_ar_penalty,
        "discover_interfaces": parsed.discover_interfaces,
        "autoconnect_discovered_interfaces": parsed.autoconnect_discovered_interfaces,
        "required_discovery_value": parsed.discover_interfaces_required_value,
        "api_port": parsed.api_port,
        "api_user": parsed.api_user,
        "password_configured": parsed.api_password.is_some(),
        "loglevel": parsed.loglevel,
        "logtimestamps": parsed.log_timestamps,
        "restart_required": restart_required,
        "apply_mode": "daemon_restart",
    })))
}

fn settings_differ(a: &ReticulumConfig, b: &ReticulumConfig) -> bool {
    a.share_instance != b.share_instance
        || a.instance_name != b.instance_name
        || a.shared_instance_type != b.shared_instance_type
        || a.shared_instance_port != b.shared_instance_port
        || a.control_port != b.control_port
        || a.enable_transport != b.enable_transport
        || a.static_transport_identity != b.static_transport_identity
        || a.local_hops_delta != b.local_hops_delta
        || a.respond_to_probes != b.respond_to_probes
        || a.use_implicit_proof != b.use_implicit_proof
        || a.panic_on_interface_error != b.panic_on_interface_error
        || a.link_mtu_discovery != b.link_mtu_discovery
        || a.force_shared_instance_bitrate != b.force_shared_instance_bitrate
        || a.default_ar_target != b.default_ar_target
        || a.default_ar_grace != b.default_ar_grace
        || a.default_ar_penalty != b.default_ar_penalty
        || a.discover_interfaces != b.discover_interfaces
        || a.autoconnect_discovered_interfaces != b.autoconnect_discovered_interfaces
        || a.discover_interfaces_required_value != b.discover_interfaces_required_value
        || a.api_port != b.api_port
        || a.api_user != b.api_user
        || a.api_password != b.api_password
        || a.loglevel != b.loglevel
        || a.log_timestamps != b.log_timestamps
}

async fn update_settings(
    State(s): State<AppState>,
    Json(req): Json<SettingsRequest>,
) -> ApiResult<Json<Value>> {
    let _guard = s.config_write_lock.lock().await;
    let mut loaded = s.load_config_snapshot()?;
    {
        let section = loaded.config.ensure_section("reticulum");
        for (key, value) in [
            ("share_instance", req.share_instance),
            ("enable_transport", req.enable_transport),
            ("static_transport_identity", req.static_transport_identity),
            ("local_hops_delta", req.local_hops_delta),
            ("respond_to_probes", req.respond_to_probes),
            ("use_implicit_proof", req.use_implicit_proof),
            ("panic_on_interface_error", req.panic_on_interface_error),
            ("link_mtu_discovery", req.link_mtu_discovery),
            ("discover_interfaces", req.discover_interfaces),
        ] {
            section.set(key, if value { "Yes" } else { "No" });
        }
        section.set("instance_name", req.instance_name.trim());
        section.set("shared_instance_type", req.shared_instance_type.trim());
        section.set(
            "shared_instance_port",
            &req.shared_instance_port.to_string(),
        );
        section.set(
            "instance_control_port",
            &req.instance_control_port.to_string(),
        );
        section.set(
            "autoconnect_discovered_interfaces",
            &req.autoconnect_discovered_interfaces.to_string(),
        );
        section.set(
            "required_discovery_value",
            &req.required_discovery_value.to_string(),
        );
        for (key, value) in [
            (
                "force_shared_instance_bitrate",
                req.force_shared_instance_bitrate,
            ),
            ("default_ar_target", req.default_ar_target),
            ("default_ar_penalty", req.default_ar_penalty),
        ] {
            if let Some(value) = value {
                section.set(key, &value.to_string());
            } else {
                section.remove(key);
            }
        }
        if let Some(value) = req.default_ar_grace {
            section.set("default_ar_grace", &value.to_string());
        } else {
            section.remove("default_ar_grace");
        }
    }
    {
        let section = loaded.config.ensure_section("logging");
        section.set("loglevel", &req.loglevel.to_string());
        section.set(
            "logtimestamps",
            if req.logtimestamps { "Yes" } else { "No" },
        );
    }
    {
        let section = loaded.config.ensure_section("api");
        section.set("port", &req.api_port.to_string());
        section.set("user", req.api_user.trim());
        if let Some(password) = req.api_password.as_deref().filter(|v| !v.is_empty()) {
            section.set("password", password);
        }
    }
    loaded.typed.reticulum.share_instance = req.share_instance;
    loaded.typed.reticulum.instance_name = req.instance_name.trim().to_string();
    loaded.typed.reticulum.shared_instance_type = match req
        .shared_instance_type
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "tcp" => crate::config::SharedInstanceType::Tcp,
        "unix" => crate::config::SharedInstanceType::Unix,
        _ => {
            return Err(ApiError::bad("shared_instance_type must be tcp or unix"));
        }
    };
    loaded.typed.reticulum.shared_instance_port = req.shared_instance_port;
    loaded.typed.reticulum.instance_control_port = req.instance_control_port;
    loaded.typed.reticulum.enable_transport = req.enable_transport;
    loaded.typed.reticulum.static_transport_identity = req.static_transport_identity;
    loaded.typed.reticulum.local_hops_delta = req.local_hops_delta;
    loaded.typed.reticulum.respond_to_probes = req.respond_to_probes;
    loaded.typed.reticulum.use_implicit_proof = req.use_implicit_proof;
    loaded.typed.reticulum.panic_on_interface_error = req.panic_on_interface_error;
    loaded.typed.reticulum.link_mtu_discovery = req.link_mtu_discovery;
    loaded.typed.reticulum.force_shared_instance_bitrate = req.force_shared_instance_bitrate;
    loaded.typed.reticulum.default_ar_target = req.default_ar_target;
    loaded.typed.reticulum.default_ar_grace = req.default_ar_grace;
    loaded.typed.reticulum.default_ar_penalty = req.default_ar_penalty;
    loaded.typed.reticulum.discover_interfaces = req.discover_interfaces;
    loaded.typed.reticulum.autoconnect_discovered_interfaces =
        req.autoconnect_discovered_interfaces;
    loaded.typed.reticulum.required_discovery_value = req.required_discovery_value;
    loaded.typed.logging.level = req.loglevel;
    loaded.typed.logging.timestamps = req.logtimestamps;
    loaded.typed.api.port = Some(req.api_port);
    loaded.typed.api.user = Some(req.api_user.trim().to_string());
    if let Some(password) = req.api_password.filter(|value| !value.is_empty()) {
        loaded.typed.api.password = Some(password);
    }
    ReticulumConfig::try_from_config(&loaded.config)
        .map_err(|e| ApiError::bad(format!("invalid settings: {e}")))?;
    s.save_config(&loaded.typed, &loaded.source)?;
    Ok(Json(json!({ "ok": true, "restart_required": true })))
}

async fn restart_daemon(State(s): State<AppState>) -> ApiResult<Response> {
    schedule_exit(s, RESTART_EXIT_CODE, "restart")
}

async fn reboot_system(State(s): State<AppState>) -> ApiResult<Response> {
    schedule_exit(s, REBOOT_EXIT_CODE, "reboot")
}

fn schedule_exit(s: AppState, code: u8, action: &'static str) -> ApiResult<Response> {
    if !s.shutdown.request_exit(code) {
        return Err(ApiError::Conflict(
            "a shutdown action is already scheduled".to_string(),
        ));
    }
    let shutdown = s.shutdown;
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(750)).await;
        tracing::info!(exit_code = code, action, "Web UI requested shutdown");
        shutdown.trigger();
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "accepted": true, "action": action, "exit_code": code })),
    )
        .into_response())
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
    req.validate_fields()?;

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
    let yaml_config = req.to_yaml_config()?;

    let id = apply_interface_change(
        &s,
        &req.name,
        Some(yaml_config),
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
    req.validate_fields()?;

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
    let yaml_config = req.to_yaml_config()?;

    let renamed_from = (old_name != req.name).then_some(old_name.as_str());
    let new_id = apply_interface_change(
        &s,
        &req.name,
        Some(yaml_config),
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
    req.validate_fields()?;
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
    let yaml_config = req.to_yaml_config()?;
    let renamed_from = (old_name != req.name).then_some(old_name.as_str());

    let new_id = apply_interface_change(
        &s,
        &req.name,
        Some(yaml_config),
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
    let mut value = json!({
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
        "listen_on": section.get("listen_on"),
        "i2p_tunneled": section.get_bool("i2p_tunneled"),
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
        "callsign": section.get("callsign"),
        "ssid": section.get_uint("ssid"),
    });
    value.as_object_mut().unwrap().extend([
        ("group_id".into(), json!(section.get("group_id"))),
        (
            "discovery_scope".into(),
            json!(section.get("discovery_scope")),
        ),
        (
            "discovery_port".into(),
            json!(section.get_uint("discovery_port")),
        ),
        ("data_port".into(), json!(section.get_uint("data_port"))),
        (
            "multicast_address_type".into(),
            json!(section.get("multicast_address_type")),
        ),
        ("devices".into(), json!(section.get("devices"))),
        (
            "ignored_devices".into(),
            json!(section.get("ignored_devices")),
        ),
        (
            "configured_bitrate".into(),
            json!(
                section
                    .get_uint("configured_bitrate")
                    .or_else(|| section.get_uint("bitrate"))
            ),
        ),
    ]);
    let object = value.as_object_mut().unwrap();
    for key in [
        "outgoing",
        "ingress_control",
        "egress_control",
        "recursive_prs",
        "announces_from_internal",
    ] {
        object.insert(key.into(), json!(section.get_bool(key)));
    }
    for key in [
        "bitrate",
        "announce_rate_target",
        "announce_rate_grace",
        "announce_rate_penalty",
        "ifac_size",
        "ic_max_held_announces",
    ] {
        object.insert(key.into(), json!(section.get_uint(key)));
    }
    for key in [
        "announce_cap",
        "ic_burst_freq_new",
        "ic_burst_freq",
        "ic_pr_burst_freq_new",
        "ic_pr_burst_freq",
        "ic_new_time",
        "ic_burst_hold",
        "ic_burst_penalty",
        "ic_held_release_interval",
        "ec_pr_freq",
    ] {
        object.insert(key.into(), json!(section.get_float(key)));
    }
    object.insert(
        "network_name".into(),
        json!(
            section
                .get("networkname")
                .or_else(|| section.get("network_name"))
        ),
    );
    object.insert("passphrase".into(), json!(section.get("passphrase")));
    value
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
        InterfaceConfig::Auto(c) => json!({
            "type":                   "AutoInterface",
            "group_id":               c.group_id,
            "discovery_scope":        c.discovery_scope.to_string(),
            "discovery_port":         c.discovery_port,
            "data_port":              c.data_port,
            "multicast_address_type": c.multicast_address_type.to_string(),
            "devices":                c.devices.as_ref().map(|v| v.join(", ")),
            "ignored_devices":        c.ignored_devices.join(", "),
            "configured_bitrate":     c.configured_bitrate,
            "interface_mode":         mode_to_str(c.mode),
        }),
        InterfaceConfig::Backbone(c) => json!({
            "type":                  "BackboneInterface",
            "listen_on":             c.listen_on,
            "target_host":           c.target_host,
            "port":                  c.port,
            "device":                c.device,
            "prefer_ipv6":           c.prefer_ipv6,
            "connect_timeout":       c.connect_timeout,
            "max_reconnect_tries":   c.max_reconnect_tries,
            "i2p_tunneled":          c.i2p_tunneled,
            "interface_mode":        mode_to_str(c.mode),
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
        #[cfg(feature = "serial")]
        InterfaceConfig::AX25KISS(c) => json!({
            "type":           "AX25KISSInterface",
            "port":           c.port,
            "speed":          c.baud_rate,
            "databits":       c.data_bits,
            "parity":         c.parity,
            "stopbits":       c.stop_bits,
            "callsign":       c.callsign,
            "ssid":           c.ssid,
            "preamble":       c.preamble,
            "txtail":         c.txtail,
            "persistence":    c.persistence,
            "slottime":       c.slottime,
            "flow_control":   c.flow_control,
            "interface_mode": mode_to_str(c.mode),
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

    #[test]
    fn auto_request_validates_and_serializes() {
        let request: InterfaceRequest = serde_json::from_value(json!({
            "name": "Test Auto",
            "type": "AutoInterface",
            "group_id": "field-network",
            "discovery_scope": "site",
            "discovery_port": 29717,
            "data_port": 42672,
            "multicast_address_type": "permanent",
            "devices": "eth0, wlan0",
            "ignored_devices": "docker0, veth0",
            "configured_bitrate": 20000000,
            "interface_mode": "Full"
        }))
        .unwrap();

        let value = iface_config_json(&request.synthesize().unwrap());
        assert_eq!(value["type"], "AutoInterface");
        assert_eq!(value["group_id"], "field-network");
        assert_eq!(value["discovery_scope"], "site");
        assert_eq!(value["discovery_port"], 29717);
        assert_eq!(value["data_port"], 42672);
        assert_eq!(value["multicast_address_type"], "permanent");
        assert_eq!(value["devices"], "eth0, wlan0");
        assert_eq!(value["ignored_devices"], "docker0, veth0");
        assert_eq!(value["configured_bitrate"], 20000000);
        assert_eq!(value["interface_mode"], "Full");
    }

    #[test]
    fn backbone_request_validates_and_serializes() {
        let request: InterfaceRequest = serde_json::from_value(json!({
            "name": "Test Backbone",
            "type": "BackboneInterface",
            "target_host": "backbone.example",
            "target_port": 4242,
            "prefer_ipv6": true,
            "connect_timeout": 7,
            "max_reconnect_tries": 4,
            "i2p_tunneled": true
        }))
        .unwrap();
        let value = iface_config_json(&request.synthesize().unwrap());
        assert_eq!(value["type"], "BackboneInterface");
        assert_eq!(value["target_host"], "backbone.example");
        assert_eq!(value["port"], 4242);
        assert_eq!(value["prefer_ipv6"], true);
        assert_eq!(value["connect_timeout"], 7);
        assert_eq!(value["max_reconnect_tries"], 4);
        assert_eq!(value["i2p_tunneled"], true);
    }

    #[test]
    fn advanced_interface_options_are_written_to_config() {
        let request: InterfaceRequest = serde_json::from_value(json!({
            "name": "Advanced",
            "type": "AutoInterface",
            "network_name": "private",
            "passphrase": "secret",
            "ifac_size": 16,
            "announce_cap": 2.5,
            "announce_rate_target": 10,
            "ingress_control": false,
            "ic_burst_freq": 3.5,
            "egress_control": true
        }))
        .unwrap();
        let value = iface_section_json(&request.to_config_section());
        assert_eq!(value["network_name"], "private");
        assert_eq!(value["passphrase"], "secret");
        assert_eq!(value["ifac_size"], 16);
        assert_eq!(value["announce_cap"], 2.5);
        assert_eq!(value["announce_rate_target"], 10);
        assert_eq!(value["ingress_control"], false);
        assert_eq!(value["ic_burst_freq"], 3.5);
        assert_eq!(value["egress_control"], true);
    }

    #[test]
    fn settings_restart_state_tracks_runtime_difference() {
        let running = ReticulumConfig::default();
        let mut stored = running.clone();
        assert!(!settings_differ(&stored, &running));
        stored.loglevel += 1;
        assert!(settings_differ(&stored, &running));
        stored = running.clone();
        stored.api_port = Some(8080);
        assert!(settings_differ(&stored, &running));
    }

    #[test]
    fn api_section_requires_credentials() {
        let config = Config::parse(
            "[reticulum]\nshare_instance = Yes\n[api]\nport = 8080\nuser = admin\npassword = secret\n",
        )
        .unwrap();
        let parsed = ReticulumConfig::try_from_config(&config).unwrap();
        assert_eq!(parsed.api_port, Some(8080));
        assert_eq!(parsed.api_user.as_deref(), Some("admin"));
        assert_eq!(parsed.api_password.as_deref(), Some("secret"));

        let invalid = Config::parse("[api]\nport = 8080\nuser = admin\n").unwrap();
        assert!(ReticulumConfig::try_from_config(&invalid).is_err());
    }

    #[test]
    fn auth_header_helpers_validate_cookie_and_origin() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "other=x; rns_session=abc123".parse().unwrap(),
        );
        assert_eq!(session_token(&headers), Some("abc123"));
        assert!(same_origin(&headers));
        headers.insert(header::HOST, "device.local:8080".parse().unwrap());
        headers.insert(header::ORIGIN, "http://device.local:8080".parse().unwrap());
        assert!(same_origin(&headers));
        headers.insert(header::ORIGIN, "http://attacker.local".parse().unwrap());
        assert!(!same_origin(&headers));
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

    #[cfg(feature = "serial")]
    #[test]
    fn ax25_request_validates_and_serializes() {
        let request: InterfaceRequest = serde_json::from_value(json!({
            "name": "Test AX.25",
            "type": "AX25KISSInterface",
            "port": "/dev/ttyUSB2",
            "callsign": "N0CALL",
            "ssid": 3,
            "speed": 115200,
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
        assert_eq!(value["type"], "AX25KISSInterface");
        assert_eq!(value["port"], "/dev/ttyUSB2");
        assert_eq!(value["callsign"], "N0CALL");
        assert_eq!(value["ssid"], 3);
        assert_eq!(value["speed"], 115200);
        assert_eq!(value["preamble"], 150);
        assert_eq!(value["txtail"], 10);
        assert_eq!(value["persistence"], 200);
        assert_eq!(value["slottime"], 30);
        assert_eq!(value["flow_control"], true);

        let invalid: InterfaceRequest = serde_json::from_value(json!({
            "name": "Invalid AX.25",
            "type": "AX25KISSInterface",
            "port": "/dev/ttyUSB2",
            "callsign": "NO@",
            "ssid": 0
        }))
        .unwrap();
        assert!(matches!(invalid.synthesize(), Err(ApiError::BadRequest(_))));
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
        let mut config = crate::config::Config {
            interfaces: Vec::new(),
            ..Default::default()
        };
        let request: InterfaceRequest = serde_json::from_value(json!({
            "name": "First name",
            "type": "TCPServerInterface",
            "listen_ip": "127.0.0.1",
            "listen_port": 4242
        }))
        .unwrap();

        set_interface_config(&mut config, request.to_yaml_config().unwrap());
        assert!(
            config
                .interfaces
                .iter()
                .any(|interface| interface.common().name == "First name")
        );

        assert!(remove_interface_config(&mut config, "First name"));
        let renamed: InterfaceRequest = serde_json::from_value(json!({
            "name": "Renamed",
            "type": "TCPServerInterface",
            "listen_ip": "127.0.0.1",
            "listen_port": 4242
        }))
        .unwrap();
        set_interface_config(&mut config, renamed.to_yaml_config().unwrap());
        let serialized = config.to_yaml().unwrap();
        assert!(serialized.contains("name: Renamed"));
        assert!(!serialized.contains("name: First name"));

        assert!(remove_interface_config(&mut config, "Renamed"));
        assert!(config.interfaces.is_empty());
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
        let path = dir.join("config.yaml");
        let original = b"# original comment\ninterfaces: []\n".to_vec();
        std::fs::write(&path, &original).unwrap();

        let mut config =
            crate::config::Config::parse(std::str::from_utf8(&original).unwrap(), &path).unwrap();
        let request: InterfaceRequest = serde_json::from_value(json!({
            "name": "Test",
            "type": "TCPServerInterface",
            "listen_ip": "127.0.0.1",
            "listen_port": 4242
        }))
        .unwrap();
        set_interface_config(&mut config, request.to_yaml_config().unwrap());

        let applied = save_config_snapshot(&path, &config, &original).unwrap();
        assert_eq!(
            std::fs::read(dir.join("config.yaml.web-ui.bak")).unwrap(),
            original
        );
        assert_eq!(std::fs::read(&path).unwrap(), applied);

        rollback_config_snapshot(&path, &applied, &original).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original);

        std::fs::write(&path, b"# external edit\ninterfaces: []\n").unwrap();
        assert!(matches!(
            save_config_snapshot(&path, &config, &original),
            Err(ApiError::Conflict(_))
        ));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"# external edit\ninterfaces: []\n"
        );

        let _ = std::fs::remove_dir_all(dir);
    }
}
