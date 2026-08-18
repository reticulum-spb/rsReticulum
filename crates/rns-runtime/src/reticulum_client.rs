//! Strict shared-instance client runtime.
//!
//! This module intentionally implements the stable application-facing subset
//! of `reticulum`: it starts a local transport actor and connects it to an
//! already-running shared instance over LocalInterface or loopback TCP. It
//! never binds a shared server socket and never synthesises hardware/network
//! interfaces from the Reticulum configuration.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::sync::mpsc;

use crate::constants::{
    ANNOUNCE_CAP, DEFAULT_INSTANCE_NAME, LOCAL_CONTROL_PORT, LOCAL_INTERFACE_PORT,
};
use crate::lifecycle::ShutdownSignal;
use crate::normalized_config::{ConfigError, NormalizedConfig as Config};
use crate::platform::{StoragePaths, resolve_config_dir};
use rns_transport::await_path::{AwaitPathError, await_path};
use rns_transport::discovery::{DiscoveredInterface, DiscoveryStamper};
use rns_transport::messages::{TransportMessage, TransportQuery, TransportQueryResponse};

static INSTANCE: OnceLock<ReticulumHandle> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceMode {
    Shared,
    Client,
    Standalone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedInstanceType {
    Tcp,
    Unix,
}

impl SharedInstanceType {
    fn platform_default() -> Self {
        if cfg!(any(target_os = "linux", target_os = "android")) {
            Self::Unix
        } else {
            Self::Tcp
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharedInstanceRpcEndpoint {
    Tcp(u16),
    Unix(String),
}

impl SharedInstanceRpcEndpoint {
    pub fn display(&self) -> String {
        match self {
            Self::Tcp(port) => format!("127.0.0.1:{port}"),
            Self::Unix(path) if path.as_bytes().first() == Some(&0) => {
                format!("\\0{}", &path[1..])
            }
            Self::Unix(path) => path.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReticulumConfig {
    pub share_instance: bool,
    pub instance_name: String,
    pub shared_instance_type: SharedInstanceType,
    pub shared_instance_port: u16,
    pub control_port: u16,
    pub rpc_key: Option<Vec<u8>>,
    pub use_implicit_proof: bool,
    pub link_mtu_discovery: bool,
}

impl Default for ReticulumConfig {
    fn default() -> Self {
        Self {
            share_instance: true,
            instance_name: DEFAULT_INSTANCE_NAME.to_string(),
            shared_instance_type: SharedInstanceType::platform_default(),
            shared_instance_port: LOCAL_INTERFACE_PORT,
            control_port: LOCAL_CONTROL_PORT,
            rpc_key: None,
            use_implicit_proof: true,
            link_mtu_discovery: true,
        }
    }
}

impl ReticulumConfig {
    pub fn try_from_config(config: &Config) -> Result<Self, ConfigError> {
        let mut result = Self::default();
        let Some(section) = config.section("reticulum") else {
            return Ok(result);
        };
        if let Some(value) = section.get_bool("share_instance") {
            result.share_instance = value;
        }
        if let Some(value) = section.get("instance_name") {
            result.instance_name = value.to_string();
        }
        if let Some(value) = section.get("shared_instance_type") {
            result.shared_instance_type = match value.to_ascii_lowercase().as_str() {
                "tcp" => SharedInstanceType::Tcp,
                "unix" => SharedInstanceType::Unix,
                _ => result.shared_instance_type,
            };
        }
        result.shared_instance_port =
            parse_u16(section.get("shared_instance_port"), "shared_instance_port")?
                .unwrap_or(result.shared_instance_port);
        result.control_port = parse_u16(
            section.get("instance_control_port"),
            "instance_control_port",
        )?
        .unwrap_or(result.control_port);
        if let Some(value) = section.get_bool("use_implicit_proof") {
            result.use_implicit_proof = value;
        }
        if let Some(value) = section.get_bool("link_mtu_discovery") {
            result.link_mtu_discovery = value;
        }
        if let Some(value) = section.get("rpc_key") {
            result.rpc_key = decode_hex(value);
        }
        Ok(result)
    }

    pub fn from_config(config: &Config) -> Self {
        Self::try_from_config(config).expect("valid Reticulum configuration")
    }

    pub fn shared_rpc_endpoint(&self, socket_base: &Path) -> SharedInstanceRpcEndpoint {
        match self.shared_instance_type {
            SharedInstanceType::Tcp => SharedInstanceRpcEndpoint::Tcp(self.control_port),
            SharedInstanceType::Unix => SharedInstanceRpcEndpoint::Unix(
                shared_instance_rpc_socket_path(&self.instance_name, socket_base),
            ),
        }
    }
}

fn parse_u16(value: Option<&str>, key: &str) -> Result<Option<u16>, ConfigError> {
    value
        .map(|value| {
            value.parse::<u16>().map_err(|_| ConfigError::InvalidValue {
                section: "reticulum".to_string(),
                key: key.to_string(),
                message: "expected a TCP port in 0..=65535".to_string(),
            })
        })
        .transpose()
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect()
}

/// Placeholder element type retained so downstream diagnostics using
/// `handle.interface_configs.len()` remain source-compatible in client mode.
#[derive(Debug, Clone)]
pub enum ClientInterfaceConfig {}

#[derive(Clone)]
pub struct ReticulumHandle {
    pub transport_tx: mpsc::Sender<TransportMessage>,
    pub config_dir: PathBuf,
    pub instance_mode: InstanceMode,
    pub interface_configs: Vec<ClientInterfaceConfig>,
    pub id_gen: Arc<AtomicU64>,
    pub handle_tx: mpsc::Sender<rns_interface::traits::InterfaceHandle>,
    pub socket_base: PathBuf,
    pub config: ReticulumConfig,
    pub is_foreground: Arc<AtomicBool>,
    pub shutdown: ShutdownSignal,
    pub transport_identity: Arc<rns_identity::identity::Identity>,
    pub network_identity: Option<Arc<rns_identity::identity::Identity>>,
}

impl ReticulumHandle {
    pub fn transport_enabled(&self) -> bool {
        false
    }
    pub fn should_use_implicit_proof(&self) -> bool {
        self.config.use_implicit_proof
    }
    pub fn remote_management_enabled(&self) -> bool {
        false
    }
    pub fn link_mtu_discovery(&self) -> bool {
        self.config.link_mtu_discovery
    }

    pub async fn await_path(
        &self,
        destination_hash: [u8; 16],
        timeout: Duration,
    ) -> Result<(), AwaitPathError> {
        await_path(&self.transport_tx, destination_hash, timeout).await
    }

    pub async fn query_transport(&self, query: TransportQuery) -> Option<TransportQueryResponse> {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        self.transport_tx
            .send(TransportMessage::Rpc { query, response_tx })
            .await
            .ok()?;
        tokio::time::timeout(Duration::from_secs(5), response_rx)
            .await
            .ok()?
            .ok()
    }

    pub async fn query_control(&self, query: TransportQuery) -> Option<TransportQueryResponse> {
        if matches!(query, TransportQuery::GetInterfaceStats)
            && let Some(rpc_key) = self.config.rpc_key.as_deref()
        {
            let request = crate::rpc::RpcRequest::GetInterfaceStats;
            let rpc_result = match self.config.shared_rpc_endpoint(&self.socket_base) {
                SharedInstanceRpcEndpoint::Tcp(port) => {
                    crate::rpc::connect_and_request(port, rpc_key, &request, Duration::from_secs(5))
                        .await
                }
                SharedInstanceRpcEndpoint::Unix(socket_path) => {
                    crate::rpc::connect_unix_and_request(
                        &socket_path,
                        rpc_key,
                        &request,
                        Duration::from_secs(5),
                    )
                    .await
                }
            };
            match rpc_result {
                Ok(crate::rpc::RpcResponse::InterfaceStats(entries)) => {
                    return Some(interface_stats_to_transport_response(entries));
                }
                Ok(response) => {
                    tracing::debug!(
                        ?response,
                        "unexpected shared instance interface stats response"
                    );
                }
                Err(error) => {
                    tracing::debug!(%error, "shared instance interface stats RPC failed; falling back to local actor");
                }
            }
        }

        self.query_transport(query).await
    }

    /// Discovery is owned by the shared instance in a client-only build.
    /// Keep the application-facing hook source-compatible while deliberately
    /// declining to start a second local discovery subsystem.
    pub async fn enable_on_network_discovery(
        &self,
        _stamper: Arc<dyn DiscoveryStamper + Send + Sync>,
    ) {
        tracing::debug!("client-only runtime leaves network discovery to the shared instance");
    }

    pub async fn discovery_enabled(&self) -> bool {
        false
    }

    pub async fn discovered_interfaces(&self) -> Vec<DiscoveredInterface> {
        Vec::new()
    }

    pub fn blackhole_sources(&self) -> &[[u8; 16]] {
        &[]
    }

    pub fn publish_blackhole_enabled(&self) -> bool {
        false
    }
}

pub fn shared_instance_rpc_socket_path(instance_name: &str, socket_base: &Path) -> String {
    if cfg!(any(target_os = "linux", target_os = "android")) {
        format!("\0rns/{instance_name}/rpc")
    } else {
        socket_base
            .join(format!("reticulum_rs_{instance_name}.rpc.sock"))
            .to_string_lossy()
            .to_string()
    }
}

fn shared_instance_socket_path(instance_name: &str, socket_base: &Path) -> String {
    if cfg!(any(target_os = "linux", target_os = "android")) {
        rns_interface::local::python_shared_socket_name(instance_name)
    } else {
        socket_base
            .join(format!("reticulum_rs_{instance_name}.sock"))
            .to_string_lossy()
            .to_string()
    }
}

/// The stable `init` signature is retained for downstream crates. In a
/// client-only build its policy is strict: connect to an existing shared
/// instance or return `SharedInstanceUnavailable`.
pub async fn init(
    configdir: Option<&str>,
    socket_dir: Option<PathBuf>,
    shutdown: ShutdownSignal,
    is_foreground: Arc<AtomicBool>,
) -> Result<ReticulumHandle, ReticulumError> {
    connect_shared(configdir, socket_dir, shutdown, is_foreground).await
}

pub async fn connect_shared(
    configdir: Option<&str>,
    socket_dir: Option<PathBuf>,
    shutdown: ShutdownSignal,
    is_foreground: Arc<AtomicBool>,
) -> Result<ReticulumHandle, ReticulumError> {
    if INSTANCE.get().is_some() {
        return Err(ReticulumError::AlreadyInitialized);
    }

    let config_dir = resolve_config_dir(configdir);
    let paths = StoragePaths::from_config_dir(&config_dir);
    paths.ensure_dirs().map_err(ReticulumError::Io)?;
    let config_path = config_dir.join(crate::config::CONFIG_FILE_NAME);
    let config = if config_path.exists() {
        crate::config::Config::from_file(&config_path)?.to_runtime_config()?
    } else {
        let typed = crate::config::Config::default();
        std::fs::write(&config_path, typed.to_yaml()?).map_err(ReticulumError::Io)?;
        typed.to_runtime_config()?
    };
    let mut config = ReticulumConfig::try_from_config(&config)?;
    if !config.share_instance {
        return Err(ReticulumError::ClientModeRequired);
    }
    let socket_base = socket_dir.unwrap_or_else(std::env::temp_dir);
    ensure_shared_available(&config, &socket_base).await?;

    let (mut actor, transport_tx) = rns_transport::actor::TransportActor::new();
    actor.is_foreground = is_foreground.clone();
    actor.initialize_storage(paths.storage_dir.clone());
    tokio::spawn(async move { actor.run().await });

    let identity_path = paths.storage_dir.join("transport_identity");
    let identity =
        rns_identity::identity::Identity::from_file(&identity_path).unwrap_or_else(|_| {
            let identity = rns_identity::identity::Identity::new();
            let _ = identity.to_file(&identity_path);
            identity
        });
    let _ = transport_tx.try_send(TransportMessage::SetTransportIdentity {
        identity_hash: identity.hash,
    });
    if config.rpc_key.is_none()
        && let Some(private_key) = identity.get_private_key()
    {
        config.rpc_key = Some(crate::rpc::derive_rpc_key(&*private_key).to_vec());
    }

    let id_gen = Arc::new(AtomicU64::new(1));
    let interface_id = id_gen.fetch_add(1, Ordering::Relaxed);
    let interface = match config.shared_instance_type {
        SharedInstanceType::Tcp => {
            let interface_config = rns_interface::tcp::TcpClientConfig::new(
                "SharedInstanceClient",
                "127.0.0.1",
                config.shared_instance_port,
            );
            rns_interface::tcp::spawn_tcp_client(
                interface_config,
                interface_id,
                transport_tx.clone(),
            )
            .await
            .map_err(|error| ReticulumError::SharedInstanceUnavailable(error.to_string()))?
        }
        SharedInstanceType::Unix => {
            let interface_config = rns_interface::local::LocalClientConfig {
                socket_path: shared_instance_socket_path(&config.instance_name, &socket_base),
                name: "SharedInstanceClient".to_string(),
            };
            rns_interface::local::spawn_local_client(
                interface_config,
                interface_id,
                transport_tx.clone(),
            )
            .await
            .map_err(|error| ReticulumError::SharedInstanceUnavailable(error.to_string()))?
        }
    };

    register_shared_interface(&transport_tx, interface).await?;
    let (handle_tx, _handle_rx) = mpsc::channel(1);
    let shutdown_tx = transport_tx.clone();
    let shutdown_wait = shutdown.clone();
    tokio::spawn(async move {
        shutdown_wait.wait().await;
        let _ = shutdown_tx.send(TransportMessage::Shutdown).await;
    });

    let handle = ReticulumHandle {
        transport_tx,
        config_dir,
        instance_mode: InstanceMode::Client,
        interface_configs: Vec::new(),
        id_gen,
        handle_tx,
        socket_base,
        config,
        is_foreground,
        shutdown,
        transport_identity: Arc::new(identity),
        network_identity: None,
    };
    let _ = INSTANCE.set(handle.clone());
    Ok(handle)
}

async fn ensure_shared_available(
    config: &ReticulumConfig,
    socket_base: &Path,
) -> Result<(), ReticulumError> {
    let probe = match config.shared_instance_type {
        SharedInstanceType::Tcp => {
            let address = format!("127.0.0.1:{}", config.shared_instance_port);
            match tokio::time::timeout(
                Duration::from_millis(500),
                tokio::net::TcpStream::connect(address),
            )
            .await
            {
                Ok(result) => result.map(|_| ()).map_err(|error| error.to_string()),
                Err(_) => Err("connection timed out".to_string()),
            }
        }
        SharedInstanceType::Unix => {
            #[cfg(unix)]
            {
                tokio::net::UnixStream::connect(shared_instance_socket_path(
                    &config.instance_name,
                    socket_base,
                ))
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
            }
            #[cfg(not(unix))]
            {
                Err("Unix shared instances are unsupported on this platform".to_string())
            }
        }
    };
    probe.map_err(ReticulumError::SharedInstanceUnavailable)
}

async fn register_shared_interface(
    transport_tx: &mpsc::Sender<TransportMessage>,
    interface: rns_interface::traits::InterfaceHandle,
) -> Result<(), ReticulumError> {
    let id = interface.id;
    let entry = rns_transport::messages::InterfaceEntry {
        name: interface.name,
        mode: convert_mode(interface.mode),
        role: rns_transport::messages::InterfaceRole::SharedInstancePeer,
        direction: rns_transport::constants::InterfaceDirection {
            inbound: interface.direction.inbound,
            outbound: interface.direction.outbound,
        },
        bitrate: interface.bitrate,
        mtu: interface.mtu,
        tx: interface.tx,
        ifac_key: None,
        ifac_size: 0,
        announce_cap: ANNOUNCE_CAP,
        announce_allowed_at: 0.0,
        announce_rate_target: None,
        announce_rate_grace: None,
        announce_rate_penalty: None,
        online: Some(interface.online),
        rxb: interface.rxb,
        txb: interface.txb,
        tx_drops: Arc::new(AtomicU64::new(0)),
        ingress: rns_transport::ingress::IngressController::disabled(),
        announce_queue: Vec::new(),
        multipoint: false,
        recursive_prs: false,
        announces_from_internal: true,
    };
    transport_tx
        .send(TransportMessage::RegisterInterface { id, entry })
        .await
        .map_err(|error| ReticulumError::Transport(error.to_string()))?;
    // Dropping a JoinHandle detaches the driver task, which is what the
    // process-lifetime client connection needs here.
    drop(interface.read_task);
    Ok(())
}

fn convert_mode(
    mode: rns_interface::traits::InterfaceMode,
) -> rns_transport::constants::InterfaceMode {
    use rns_interface::traits::InterfaceMode as Source;
    use rns_transport::constants::InterfaceMode as Target;
    match mode {
        Source::Full => Target::Full,
        Source::PointToPoint => Target::PointToPoint,
        Source::AccessPoint => Target::AccessPoint,
        Source::Roaming => Target::Roaming,
        Source::Boundary => Target::Boundary,
        Source::Gateway => Target::Gateway,
        Source::Internal => Target::Internal,
    }
}

fn interface_stats_to_transport_response(
    entries: Vec<crate::rpc::InterfaceStatEntry>,
) -> TransportQueryResponse {
    use rns_transport::messages::InterfaceStatRpcEntry;

    TransportQueryResponse::InterfaceStats(
        entries
            .into_iter()
            .map(|entry| InterfaceStatRpcEntry {
                id: entry.id,
                name: entry.name,
                rx_bytes: entry.rx_bytes,
                tx_bytes: entry.tx_bytes,
                rx_rate: entry.rx_rate,
                tx_rate: entry.tx_rate,
                online: entry.online,
                bitrate: entry.bitrate,
                mtu: entry.mtu,
                mode: entry.mode,
                role: entry.role,
                announce_queue: entry.announce_queue,
                held_announces: entry.held_announces,
                incoming_announce_frequency: entry.incoming_announce_frequency,
                outgoing_announce_frequency: entry.outgoing_announce_frequency,
                incoming_pr_frequency: entry.incoming_pr_frequency,
                outgoing_pr_frequency: entry.outgoing_pr_frequency,
                burst_active: entry.burst_active,
                burst_activated: entry.burst_activated,
                pr_burst_active: entry.pr_burst_active,
                pr_burst_activated: entry.pr_burst_activated,
                clients: entry.clients,
                announce_rate_target: entry.announce_rate_target,
                announce_rate_grace: entry.announce_rate_grace,
                announce_rate_penalty: entry.announce_rate_penalty,
                announce_cap: entry.announce_cap,
                ifac_size: entry.ifac_size,
                tx_drops: entry.tx_drops,
            })
            .collect(),
    )
}

pub fn get_instance() -> Option<&'static ReticulumHandle> {
    INSTANCE.get()
}

#[derive(Debug, thiserror::Error)]
pub enum ReticulumError {
    #[error("config error: {0}")]
    Config(#[from] ConfigError),
    #[error("{0}")]
    YamlConfig(#[from] crate::config::YamlConfigError),
    #[error("I/O error: {0}")]
    Io(std::io::Error),
    #[error("already initialized")]
    AlreadyInitialized,
    #[error("client-only runtime requires share_instance = Yes")]
    ClientModeRequired,
    #[error("shared Reticulum instance is unavailable: {0}")]
    SharedInstanceUnavailable(String),
    #[error("transport error: {0}")]
    Transport(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_config_reads_shared_endpoint_without_full_runtime() {
        let config = crate::config::Config::parse(
            "reticulum:\n  share_instance: true\n  instance_name: lxmf\n  shared_instance_type: tcp\n  shared_instance_port: 41234\n  instance_control_port: 41235\n",
            "config.yaml",
        )
        .unwrap()
        .to_runtime_config()
        .unwrap();
        let client = ReticulumConfig::try_from_config(&config).unwrap();
        assert!(client.share_instance);
        assert_eq!(client.instance_name, "lxmf");
        assert_eq!(client.shared_instance_type, SharedInstanceType::Tcp);
        assert_eq!(client.shared_instance_port, 41234);
        assert_eq!(client.control_port, 41235);
    }

    #[test]
    fn invalid_shared_port_is_rejected() {
        assert!(
            crate::config::Config::parse(
                "reticulum:\n  share_instance: true\n  shared_instance_port: invalid\n",
                "config.yaml"
            )
            .is_err()
        );
    }
}
