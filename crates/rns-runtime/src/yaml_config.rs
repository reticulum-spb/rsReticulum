//! Typed, format-independent configuration model used by `config.yaml`.
//!
//! Parsing and serialization live here during the migration. Runtime code must
//! consume these Rust types, never values from the YAML parser.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const CONFIG_FILE_NAME: &str = "config.yaml";

pub const EXAMPLE_CONFIG: &str = r#"# rsReticulum YAML configuration
reticulum:
  share_instance: true
  enable_transport: false

logging:
  level: 4
  timestamps: true

interfaces:
  - type: auto
    name: Default Interface
    enabled: true

  - type: udp
    name: UDP Interface
    enabled: false
    listen_ip: 0.0.0.0
    listen_port: 4242
    forward_ip: 255.255.255.255
    forward_port: 4242

  - type: tcp_server
    name: TCP Server Interface
    enabled: false
    listen_ip: 0.0.0.0
    listen_port: 4242

  - type: tcp_client
    name: TCP Client Interface
    enabled: false
    target_host: 127.0.0.1
    target_port: 4242
"#;

#[derive(Debug, Error)]
pub enum YamlConfigError {
    #[error("failed to read configuration {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("configuration error in {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("configuration validation error: {0}")]
    Validation(String),
    #[error("failed to serialize configuration: {0}")]
    Serialize(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub reticulum: ReticulumConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub api: ApiConfig,
    #[serde(default)]
    pub interfaces: Vec<InterfaceConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            reticulum: ReticulumConfig::default(),
            logging: LoggingConfig::default(),
            api: ApiConfig::default(),
            interfaces: vec![InterfaceConfig::Auto(AutoInterfaceConfig {
                common: InterfaceCommonConfig {
                    name: "Default Interface".into(),
                    ..Default::default()
                },
                ..Default::default()
            })],
        }
    }
}

impl Config {
    pub fn parse(input: &str, path: impl AsRef<Path>) -> Result<Self, YamlConfigError> {
        let path = path.as_ref();
        let config: Self =
            serde_saphyr::from_str(input).map_err(|error| YamlConfigError::Parse {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?;
        config.validate()?;
        Ok(config)
    }

    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, YamlConfigError> {
        let path = path.as_ref();
        let input = std::fs::read_to_string(path).map_err(|source| YamlConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&input, path)
    }

    pub fn to_yaml(&self) -> Result<String, YamlConfigError> {
        serde_saphyr::to_string(self).map_err(|error| YamlConfigError::Serialize(error.to_string()))
    }

    pub fn validate(&self) -> Result<(), YamlConfigError> {
        let mut names = HashSet::new();
        for interface in &self.interfaces {
            let name = interface.common().name.trim();
            if name.is_empty() {
                return Err(YamlConfigError::Validation(
                    "interface name must not be empty".into(),
                ));
            }
            if !names.insert(name) {
                return Err(YamlConfigError::Validation(format!(
                    "duplicate interface name: {name}"
                )));
            }
            interface.validate()?;
        }
        self.api.validate()?;
        validate_hashes(
            "remote_management_allowed",
            &self.reticulum.remote_management_allowed,
        )?;
        validate_hashes(
            "interface_discovery_sources",
            &self.reticulum.interface_discovery_sources,
        )?;
        validate_hashes("blackhole_sources", &self.reticulum.blackhole_sources)?;
        if !(0..=7).contains(&self.logging.level) {
            return Err(YamlConfigError::Validation(
                "logging.level must be in 0..=7".into(),
            ));
        }
        Ok(())
    }

    /// Transitional adapter for runtime components that have not yet been
    /// changed from `ConfigSection` to the typed model. Input is always typed
    /// YAML; this does not parse or accept the legacy file format.
    #[doc(hidden)]
    pub fn to_runtime_compat_config(&self) -> Result<crate::config::Config, YamlConfigError> {
        let mut output = crate::config::Config::new();
        {
            let section = output.ensure_section("reticulum");
            set_bool(section, "share_instance", self.reticulum.share_instance);
            section.set("instance_name", &self.reticulum.instance_name);
            let shared_type = match self.reticulum.shared_instance_type {
                SharedInstanceType::Tcp => "tcp",
                SharedInstanceType::Unix => "unix",
                SharedInstanceType::PlatformDefault => {
                    if cfg!(unix) {
                        "unix"
                    } else {
                        "tcp"
                    }
                }
            };
            section.set("shared_instance_type", shared_type);
            set_num(
                section,
                "shared_instance_port",
                self.reticulum.shared_instance_port,
            );
            set_num(
                section,
                "instance_control_port",
                self.reticulum.instance_control_port,
            );
            set_bool(section, "enable_transport", self.reticulum.enable_transport);
            set_bool(
                section,
                "static_transport_identity",
                self.reticulum.static_transport_identity,
            );
            set_bool(section, "local_hops_delta", self.reticulum.local_hops_delta);
            set_bool(
                section,
                "respond_to_probes",
                self.reticulum.respond_to_probes,
            );
            set_bool(
                section,
                "use_implicit_proof",
                self.reticulum.use_implicit_proof,
            );
            set_bool(
                section,
                "panic_on_interface_error",
                self.reticulum.panic_on_interface_error,
            );
            set_bool(
                section,
                "link_mtu_discovery",
                self.reticulum.link_mtu_discovery,
            );
            set_bool(
                section,
                "enable_remote_management",
                self.reticulum.enable_remote_management,
            );
            section.set_list(
                "remote_management_allowed",
                self.reticulum.remote_management_allowed.clone(),
            );
            set_opt(section, "rpc_key", self.reticulum.rpc_key.as_deref());
            set_opt_num(
                section,
                "force_shared_instance_bitrate",
                self.reticulum.force_shared_instance_bitrate,
            );
            set_opt_num(
                section,
                "default_ar_target",
                self.reticulum.default_ar_target,
            );
            set_opt_num(
                section,
                "default_ar_penalty",
                self.reticulum.default_ar_penalty,
            );
            set_opt_num(section, "default_ar_grace", self.reticulum.default_ar_grace);
            write_ingress(section, &self.reticulum.ingress);
            if let Some(path) = &self.reticulum.network_identity {
                section.set("network_identity", &path.to_string_lossy());
            }
            set_bool(
                section,
                "discover_interfaces",
                self.reticulum.discover_interfaces,
            );
            set_num(
                section,
                "autoconnect_discovered_interfaces",
                self.reticulum.autoconnect_discovered_interfaces,
            );
            set_num(
                section,
                "required_discovery_value",
                self.reticulum.required_discovery_value,
            );
            section.set_list(
                "interface_discovery_sources",
                self.reticulum.interface_discovery_sources.clone(),
            );
            section.set_list(
                "blackhole_sources",
                self.reticulum.blackhole_sources.clone(),
            );
            set_bool(
                section,
                "publish_blackhole",
                self.reticulum.publish_blackhole,
            );
            set_num(
                section,
                "blackhole_update_interval",
                self.reticulum.blackhole_update_interval_minutes,
            );
            section.set_list(
                "bootstrap_configs",
                self.reticulum
                    .bootstrap_configs
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect(),
            );
        }
        {
            let section = output.ensure_section("logging");
            set_num(section, "loglevel", self.logging.level);
            set_bool(section, "logtimestamps", self.logging.timestamps);
        }
        if self.api.port.is_some() || self.api.user.is_some() || self.api.password.is_some() {
            let section = output.ensure_section("api");
            set_opt_num(section, "port", self.api.port);
            set_opt(section, "user", self.api.user.as_deref());
            set_opt(section, "password", self.api.password.as_deref());
        }
        {
            let interfaces = output.ensure_section("interfaces");
            for interface in &self.interfaces {
                let mut section = crate::config::ConfigSection::new();
                interface.write_compat_section(&mut section)?;
                *interfaces.add_subsection(interface.common().name.clone()) = section;
            }
        }
        Ok(output)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReticulumConfig {
    pub share_instance: bool,
    pub instance_name: String,
    pub shared_instance_type: SharedInstanceType,
    pub shared_instance_port: u16,
    pub instance_control_port: u16,
    pub enable_transport: bool,
    pub static_transport_identity: bool,
    pub local_hops_delta: bool,
    pub respond_to_probes: bool,
    pub use_implicit_proof: bool,
    pub panic_on_interface_error: bool,
    pub link_mtu_discovery: bool,
    pub enable_remote_management: bool,
    pub remote_management_allowed: Vec<String>,
    pub rpc_key: Option<String>,
    pub force_shared_instance_bitrate: Option<u64>,
    pub default_ar_target: Option<u64>,
    pub default_ar_penalty: Option<u64>,
    pub default_ar_grace: Option<u32>,
    pub ingress: IngressConfig,
    pub network_identity: Option<PathBuf>,
    pub discover_interfaces: bool,
    pub autoconnect_discovered_interfaces: usize,
    pub required_discovery_value: u8,
    pub interface_discovery_sources: Vec<String>,
    pub blackhole_sources: Vec<String>,
    pub publish_blackhole: bool,
    pub blackhole_update_interval_minutes: f64,
    pub bootstrap_configs: Vec<PathBuf>,
}

impl Default for ReticulumConfig {
    fn default() -> Self {
        Self {
            share_instance: true,
            instance_name: "default".into(),
            shared_instance_type: SharedInstanceType::default(),
            shared_instance_port: 37428,
            instance_control_port: 37429,
            enable_transport: false,
            static_transport_identity: false,
            local_hops_delta: false,
            respond_to_probes: false,
            use_implicit_proof: true,
            panic_on_interface_error: false,
            link_mtu_discovery: true,
            enable_remote_management: false,
            remote_management_allowed: Vec::new(),
            rpc_key: None,
            force_shared_instance_bitrate: None,
            default_ar_target: None,
            default_ar_penalty: None,
            default_ar_grace: None,
            ingress: IngressConfig::default(),
            network_identity: None,
            discover_interfaces: false,
            autoconnect_discovered_interfaces: 0,
            required_discovery_value: 14,
            interface_discovery_sources: Vec::new(),
            blackhole_sources: Vec::new(),
            publish_blackhole: false,
            blackhole_update_interval_minutes: 60.0,
            bootstrap_configs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedInstanceType {
    Tcp,
    Unix,
    #[default]
    PlatformDefault,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    pub level: i32,
    pub timestamps: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: 4,
            timestamps: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ApiConfig {
    pub port: Option<u16>,
    pub user: Option<String>,
    pub password: Option<String>,
}

impl ApiConfig {
    fn validate(&self) -> Result<(), YamlConfigError> {
        if self.port.is_some()
            && (self.user.as_deref().is_none_or(str::is_empty)
                || self.password.as_deref().is_none_or(str::is_empty))
        {
            return Err(YamlConfigError::Validation(
                "api.user and api.password are required when api.port is set".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IngressConfig {
    pub burst_freq_new: Option<f64>,
    pub burst_freq: Option<f64>,
    pub path_request_burst_freq_new: Option<f64>,
    pub path_request_burst_freq: Option<f64>,
    pub new_time: Option<f64>,
    pub burst_hold: Option<f64>,
    pub burst_penalty: Option<f64>,
    pub max_held_announces: Option<usize>,
    pub held_release_interval: Option<f64>,
    pub egress_path_request_freq: Option<f64>,
    pub egress_control: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InterfaceConfig {
    Auto(AutoInterfaceConfig),
    TcpClient(TcpClientInterfaceConfig),
    TcpServer(TcpServerInterfaceConfig),
    Udp(UdpInterfaceConfig),
    Local(LocalInterfaceConfig),
    I2p(I2pInterfaceConfig),
    Pipe(PipeInterfaceConfig),
    Backbone(BackboneInterfaceConfig),
    Serial(SerialInterfaceConfig),
    Kiss(KissInterfaceConfig),
    Rnode(RnodeInterfaceConfig),
    RnodeMulti(RnodeMultiInterfaceConfig),
    Ax25Kiss(Ax25KissInterfaceConfig),
    Plugin(PluginInterfaceConfig),
}

impl InterfaceConfig {
    pub fn common(&self) -> &InterfaceCommonConfig {
        match self {
            Self::Auto(v) => &v.common,
            Self::TcpClient(v) => &v.common,
            Self::TcpServer(v) => &v.common,
            Self::Udp(v) => &v.common,
            Self::Local(v) => &v.common,
            Self::I2p(v) => &v.common,
            Self::Pipe(v) => &v.common,
            Self::Backbone(v) => &v.common,
            Self::Serial(v) => &v.common,
            Self::Kiss(v) => &v.serial.common,
            Self::Rnode(v) => &v.common,
            Self::RnodeMulti(v) => &v.common,
            Self::Ax25Kiss(v) => &v.serial.common,
            Self::Plugin(v) => &v.common,
        }
    }

    fn validate(&self) -> Result<(), YamlConfigError> {
        if let Some(size) = self.common().ifac_size
            && !(1..=64).contains(&size)
        {
            return Err(YamlConfigError::Validation(format!(
                "interface {:?}: ifac_size must be in 1..=64",
                self.common().name
            )));
        }
        match self {
            Self::Auto(v) if v.discovery_port == 0 || v.data_port == 0 => {
                Err(YamlConfigError::Validation(format!(
                    "interface {:?}: discovery_port and data_port must be in 1..=65535",
                    v.common.name
                )))
            }
            Self::TcpClient(v) if v.target_host.trim().is_empty() || v.target_port == 0 => {
                Err(YamlConfigError::Validation(format!(
                    "interface {:?}: target_host is required and target_port must be in 1..=65535",
                    v.common.name
                )))
            }
            Self::TcpServer(v) if v.listen_port == 0 => Err(YamlConfigError::Validation(format!(
                "interface {:?}: listen_port must be in 1..=65535",
                v.common.name
            ))),
            Self::Udp(v) if v.listen_port.is_none() && v.forward_port.is_none() => {
                Err(YamlConfigError::Validation(format!(
                    "interface {:?}: UDP requires listen_port or forward_port",
                    v.common.name
                )))
            }
            Self::Udp(v) if v.listen_port == Some(0) || v.forward_port == Some(0) => {
                Err(YamlConfigError::Validation(format!(
                    "interface {:?}: UDP ports must be in 1..=65535",
                    v.common.name
                )))
            }
            Self::Local(v) if v.port == 0 => Err(YamlConfigError::Validation(format!(
                "interface {:?}: port must be in 1..=65535",
                v.common.name
            ))),
            Self::I2p(v) if v.sam_port == 0 => Err(YamlConfigError::Validation(format!(
                "interface {:?}: sam_port must be in 1..=65535",
                v.common.name
            ))),
            Self::Pipe(v) if v.command.trim().is_empty() => Err(YamlConfigError::Validation(
                format!("interface {:?}: command must not be empty", v.common.name),
            )),
            Self::Backbone(v) if v.port == 0 => Err(YamlConfigError::Validation(format!(
                "interface {:?}: port must be in 1..=65535",
                v.common.name
            ))),
            Self::Rnode(v) => validate_radio(&v.common.name, &v.radio),
            Self::RnodeMulti(v) => {
                let mut ports = HashSet::new();
                for sub in &v.subinterfaces {
                    if !ports.insert(sub.vport) {
                        return Err(YamlConfigError::Validation(format!(
                            "interface {:?}: duplicate RNode vport {}",
                            v.common.name, sub.vport
                        )));
                    }
                    validate_radio(&format!("{}/{}", v.common.name, sub.name), &sub.radio)?;
                }
                Ok(())
            }
            Self::Ax25Kiss(v) if v.ssid > 15 => Err(YamlConfigError::Validation(format!(
                "interface {:?}: ssid must be in 0..=15",
                v.serial.common.name
            ))),
            Self::Plugin(v) if v.plugin.trim().is_empty() => Err(YamlConfigError::Validation(
                format!("interface {:?}: plugin must not be empty", v.common.name),
            )),
            _ => Ok(()),
        }
    }

    fn write_compat_section(
        &self,
        section: &mut crate::config::ConfigSection,
    ) -> Result<(), YamlConfigError> {
        write_common(section, self.common());
        match self {
            Self::Auto(v) => {
                section.set("type", "AutoInterface");
                section.set("group_id", &v.group_id);
                section.set(
                    "discovery_scope",
                    match v.discovery_scope {
                        DiscoveryScope::Link => "link",
                        DiscoveryScope::Admin => "admin",
                        DiscoveryScope::Site => "site",
                        DiscoveryScope::Organisation => "organisation",
                        DiscoveryScope::Global => "global",
                    },
                );
                set_num(section, "discovery_port", v.discovery_port);
                set_num(section, "data_port", v.data_port);
                section.set(
                    "multicast_address_type",
                    match v.multicast_address_type {
                        MulticastAddressType::Permanent => "permanent",
                        MulticastAddressType::Temporary => "temporary",
                    },
                );
                if let Some(devices) = &v.devices {
                    section.set("devices", &devices.join(","));
                }
                section.set("ignored_devices", &v.ignored_devices.join(","));
                set_opt_num(section, "configured_bitrate", v.configured_bitrate);
            }
            Self::TcpClient(v) => {
                section.set("type", "TCPClientInterface");
                section.set("target_host", &v.target_host);
                set_num(section, "target_port", v.target_port);
                set_bool(section, "kiss_framing", v.kiss_framing);
                set_num(section, "connect_timeout", v.connect_timeout);
                set_opt_num(section, "max_reconnect_tries", v.max_reconnect_tries);
                set_opt_num(section, "fixed_mtu", v.fixed_mtu);
            }
            Self::TcpServer(v) => {
                section.set("type", "TCPServerInterface");
                section.set("listen_ip", &v.listen_ip);
                set_num(section, "listen_port", v.listen_port);
                set_bool(section, "kiss_framing", v.kiss_framing);
                set_bool(section, "prefer_ipv6", v.prefer_ipv6);
                set_opt(section, "device", v.device.as_deref());
            }
            Self::Udp(v) => {
                section.set("type", "UDPInterface");
                set_opt(section, "listen_ip", v.listen_ip.as_deref());
                set_opt_num(section, "listen_port", v.listen_port);
                set_opt(section, "forward_ip", v.forward_ip.as_deref());
                set_opt_num(section, "forward_port", v.forward_port);
                set_opt(section, "device", v.device.as_deref());
            }
            Self::Local(v) => {
                section.set("type", "LocalInterface");
                set_num(section, "port", v.port);
            }
            Self::I2p(v) => {
                section.set("type", "I2PInterface");
                set_bool(section, "connectable", v.connectable);
                section.set("peers", &v.peers.join(","));
                section.set("i2p_sam_host", &v.sam_host);
                set_num(section, "i2p_sam_port", v.sam_port);
            }
            Self::Pipe(v) => {
                section.set("type", "PipeInterface");
                section.set("command", &v.command);
                set_num(section, "respawn_delay", v.respawn_delay);
            }
            Self::Backbone(v) => {
                section.set("type", "BackboneInterface");
                set_opt(section, "listen_on", v.listen_on.as_deref());
                set_opt(section, "target_host", v.target_host.as_deref());
                set_num(section, "port", v.port);
                set_opt(section, "device", v.device.as_deref());
                set_bool(section, "prefer_ipv6", v.prefer_ipv6);
                set_num(section, "connect_timeout", v.connect_timeout);
                set_opt_num(section, "max_reconnect_tries", v.max_reconnect_tries);
                set_bool(section, "i2p_tunneled", v.i2p_tunneled);
            }
            Self::Serial(v) => {
                section.set("type", "SerialInterface");
                write_serial(section, v);
            }
            Self::Kiss(v) => {
                section.set("type", "KISSInterface");
                write_serial(section, &v.serial);
                write_kiss(
                    section,
                    v.preamble_ms,
                    v.tx_tail_ms,
                    v.persistence,
                    v.slot_time_ms,
                    v.flow_control,
                );
                set_opt_num(section, "id_interval", v.id_interval);
                set_opt(section, "id_callsign", v.id_callsign.as_deref());
            }
            Self::Rnode(v) => {
                section.set("type", "RNodeInterface");
                section.set("port", &v.port);
                write_radio(section, &v.radio);
                set_bool(section, "flow_control", v.flow_control);
                set_opt_num(section, "id_interval", v.id_interval);
                set_opt(section, "id_callsign", v.id_callsign.as_deref());
            }
            Self::RnodeMulti(v) => {
                section.set("type", "RNodeMultiInterface");
                section.set("port", &v.port);
                set_num(section, "baud_rate", v.baud_rate);
                set_bool(section, "flow_control", v.flow_control);
                set_opt_num(section, "id_interval", v.id_interval);
                set_opt(section, "id_callsign", v.id_callsign.as_deref());
                for sub in &v.subinterfaces {
                    let child = section.add_subsection(sub.name.clone());
                    set_num(child, "vport", sub.vport);
                    set_bool(child, "enabled", sub.enabled);
                    set_bool(child, "outgoing", sub.outgoing);
                    if let Some(flow) = sub.flow_control {
                        set_bool(child, "flow_control", flow);
                    }
                    if let Some(mode) = sub.mode {
                        child.set("mode", mode_name(mode));
                    }
                    write_radio(child, &sub.radio);
                }
            }
            Self::Ax25Kiss(v) => {
                section.set("type", "AX25KISSInterface");
                write_serial(section, &v.serial);
                section.set("callsign", &v.callsign);
                set_num(section, "ssid", v.ssid);
                write_kiss(
                    section,
                    v.preamble_ms,
                    v.tx_tail_ms,
                    v.persistence,
                    v.slot_time_ms,
                    v.flow_control,
                );
            }
            Self::Plugin(v) => {
                return Err(YamlConfigError::Validation(format!(
                    "interface {:?}: plugin interfaces cannot run before the plugin ABI is implemented",
                    v.common.name
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InterfaceCommonConfig {
    pub name: String,
    pub enabled: bool,
    pub mode: InterfaceMode,
    pub outgoing: bool,
    pub bitrate: Option<u64>,
    pub announce_cap: Option<f64>,
    pub announce_rate_target: Option<u64>,
    pub announce_rate_grace: Option<u32>,
    pub announce_rate_penalty: Option<u64>,
    pub ifac_network_name: Option<String>,
    pub ifac_passphrase: Option<String>,
    pub ifac_size: Option<usize>,
    pub ingress_control: bool,
    pub ingress: IngressConfig,
    pub recursive_path_requests: bool,
    pub announces_from_internal: bool,
}

impl Default for InterfaceCommonConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            mode: InterfaceMode::Full,
            outgoing: true,
            bitrate: None,
            announce_cap: None,
            announce_rate_target: None,
            announce_rate_grace: None,
            announce_rate_penalty: None,
            ifac_network_name: None,
            ifac_passphrase: None,
            ifac_size: None,
            ingress_control: true,
            ingress: IngressConfig::default(),
            recursive_path_requests: false,
            announces_from_internal: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceMode {
    #[default]
    Full,
    PointToPoint,
    AccessPoint,
    Roaming,
    Boundary,
    Gateway,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalInterfaceConfig {
    #[serde(flatten)]
    pub common: InterfaceCommonConfig,
    pub port: u16,
}

impl Default for LocalInterfaceConfig {
    fn default() -> Self {
        Self {
            common: Default::default(),
            port: 37428,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AutoInterfaceConfig {
    #[serde(flatten)]
    pub common: InterfaceCommonConfig,
    pub group_id: String,
    pub discovery_scope: DiscoveryScope,
    pub discovery_port: u16,
    pub data_port: u16,
    pub multicast_address_type: MulticastAddressType,
    pub devices: Option<Vec<String>>,
    pub ignored_devices: Vec<String>,
    pub configured_bitrate: Option<u64>,
}
impl Default for AutoInterfaceConfig {
    fn default() -> Self {
        Self {
            common: Default::default(),
            group_id: "reticulum".into(),
            discovery_scope: Default::default(),
            discovery_port: 29716,
            data_port: 42671,
            multicast_address_type: Default::default(),
            devices: None,
            ignored_devices: Vec::new(),
            configured_bitrate: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryScope {
    Link,
    #[default]
    Admin,
    Site,
    Organisation,
    Global,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MulticastAddressType {
    Permanent,
    #[default]
    Temporary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TcpClientInterfaceConfig {
    #[serde(flatten)]
    pub common: InterfaceCommonConfig,
    pub target_host: String,
    pub target_port: u16,
    pub kiss_framing: bool,
    pub connect_timeout: u64,
    pub max_reconnect_tries: Option<usize>,
    pub fixed_mtu: Option<u32>,
}
impl Default for TcpClientInterfaceConfig {
    fn default() -> Self {
        Self {
            common: Default::default(),
            target_host: String::new(),
            target_port: 0,
            kiss_framing: false,
            connect_timeout: 5,
            max_reconnect_tries: None,
            fixed_mtu: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TcpServerInterfaceConfig {
    #[serde(flatten)]
    pub common: InterfaceCommonConfig,
    pub listen_ip: String,
    pub listen_port: u16,
    pub kiss_framing: bool,
    pub prefer_ipv6: bool,
    pub device: Option<String>,
}
impl Default for TcpServerInterfaceConfig {
    fn default() -> Self {
        Self {
            common: Default::default(),
            listen_ip: "0.0.0.0".into(),
            listen_port: 0,
            kiss_framing: false,
            prefer_ipv6: false,
            device: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UdpInterfaceConfig {
    #[serde(flatten)]
    pub common: InterfaceCommonConfig,
    pub listen_ip: Option<String>,
    pub listen_port: Option<u16>,
    pub forward_ip: Option<String>,
    pub forward_port: Option<u16>,
    pub device: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct I2pInterfaceConfig {
    #[serde(flatten)]
    pub common: InterfaceCommonConfig,
    pub connectable: bool,
    pub peers: Vec<String>,
    pub sam_host: String,
    pub sam_port: u16,
}
impl Default for I2pInterfaceConfig {
    fn default() -> Self {
        Self {
            common: Default::default(),
            connectable: false,
            peers: Vec::new(),
            sam_host: "127.0.0.1".into(),
            sam_port: 7656,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PipeInterfaceConfig {
    #[serde(flatten)]
    pub common: InterfaceCommonConfig,
    pub command: String,
    pub respawn_delay: u64,
}
impl Default for PipeInterfaceConfig {
    fn default() -> Self {
        Self {
            common: Default::default(),
            command: String::new(),
            respawn_delay: 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BackboneInterfaceConfig {
    #[serde(flatten)]
    pub common: InterfaceCommonConfig,
    pub listen_on: Option<String>,
    pub target_host: Option<String>,
    pub port: u16,
    pub device: Option<String>,
    pub prefer_ipv6: bool,
    pub connect_timeout: u64,
    pub max_reconnect_tries: Option<usize>,
    pub i2p_tunneled: bool,
}
impl Default for BackboneInterfaceConfig {
    fn default() -> Self {
        Self {
            common: Default::default(),
            listen_on: None,
            target_host: None,
            port: 0,
            device: None,
            prefer_ipv6: false,
            connect_timeout: 5,
            max_reconnect_tries: None,
            i2p_tunneled: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SerialInterfaceConfig {
    #[serde(flatten)]
    pub common: InterfaceCommonConfig,
    pub port: String,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub parity: String,
    pub stop_bits: u8,
}
impl Default for SerialInterfaceConfig {
    fn default() -> Self {
        Self {
            common: Default::default(),
            port: String::new(),
            baud_rate: 9600,
            data_bits: 8,
            parity: "N".into(),
            stop_bits: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct KissInterfaceConfig {
    #[serde(flatten)]
    pub serial: SerialInterfaceConfig,
    pub preamble_ms: u32,
    pub tx_tail_ms: u32,
    pub persistence: u8,
    pub slot_time_ms: u32,
    pub flow_control: bool,
    pub id_interval: Option<u64>,
    pub id_callsign: Option<String>,
}
impl Default for KissInterfaceConfig {
    fn default() -> Self {
        Self {
            serial: Default::default(),
            preamble_ms: 350,
            tx_tail_ms: 20,
            persistence: 64,
            slot_time_ms: 20,
            flow_control: false,
            id_interval: None,
            id_callsign: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RadioConfig {
    pub frequency: u32,
    pub bandwidth: u32,
    pub spreading_factor: u8,
    pub coding_rate: u8,
    pub tx_power: i8,
    pub airtime_limit_short: Option<f32>,
    pub airtime_limit_long: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RnodeInterfaceConfig {
    #[serde(flatten)]
    pub common: InterfaceCommonConfig,
    pub port: String,
    #[serde(flatten)]
    pub radio: RadioConfig,
    pub flow_control: bool,
    pub id_interval: Option<u64>,
    pub id_callsign: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RnodeMultiInterfaceConfig {
    #[serde(flatten)]
    pub common: InterfaceCommonConfig,
    pub port: String,
    pub baud_rate: u32,
    pub flow_control: bool,
    pub subinterfaces: Vec<RnodeSubInterfaceConfig>,
    pub id_interval: Option<u64>,
    pub id_callsign: Option<String>,
}
impl Default for RnodeMultiInterfaceConfig {
    fn default() -> Self {
        Self {
            common: Default::default(),
            port: String::new(),
            baud_rate: 115200,
            flow_control: false,
            subinterfaces: Vec::new(),
            id_interval: None,
            id_callsign: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RnodeSubInterfaceConfig {
    pub name: String,
    pub vport: u8,
    pub enabled: bool,
    pub outgoing: bool,
    pub flow_control: Option<bool>,
    pub mode: Option<InterfaceMode>,
    #[serde(flatten)]
    pub radio: RadioConfig,
}
impl Default for RnodeSubInterfaceConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            vport: 0,
            enabled: true,
            outgoing: true,
            flow_control: None,
            mode: None,
            radio: Default::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ax25KissInterfaceConfig {
    #[serde(flatten)]
    pub serial: SerialInterfaceConfig,
    pub callsign: String,
    pub ssid: u8,
    pub preamble_ms: u32,
    pub tx_tail_ms: u32,
    pub persistence: u8,
    pub slot_time_ms: u32,
    pub flow_control: bool,
}
impl Default for Ax25KissInterfaceConfig {
    fn default() -> Self {
        Self {
            serial: Default::default(),
            callsign: String::new(),
            ssid: 0,
            preamble_ms: 350,
            tx_tail_ms: 20,
            persistence: 64,
            slot_time_ms: 20,
            flow_control: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginInterfaceConfig {
    #[serde(flatten)]
    pub common: InterfaceCommonConfig,
    pub plugin: String,
    pub config: OpaqueValue,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OpaqueValue {
    #[default]
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Sequence(Vec<OpaqueValue>),
    Mapping(BTreeMap<String, OpaqueValue>),
}

fn set_bool(section: &mut crate::config::ConfigSection, key: &str, value: bool) {
    section.set(key, if value { "Yes" } else { "No" });
}

fn set_num(section: &mut crate::config::ConfigSection, key: &str, value: impl ToString) {
    section.set(key, &value.to_string());
}

fn set_opt(section: &mut crate::config::ConfigSection, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        section.set(key, value);
    }
}

fn set_opt_num<T: ToString + Copy>(
    section: &mut crate::config::ConfigSection,
    key: &str,
    value: Option<T>,
) {
    if let Some(value) = value {
        set_num(section, key, value);
    }
}

fn mode_name(mode: InterfaceMode) -> &'static str {
    match mode {
        InterfaceMode::Full => "full",
        InterfaceMode::PointToPoint => "point_to_point",
        InterfaceMode::AccessPoint => "access_point",
        InterfaceMode::Roaming => "roaming",
        InterfaceMode::Boundary => "boundary",
        InterfaceMode::Gateway => "gateway",
        InterfaceMode::Internal => "internal",
    }
}

fn write_common(section: &mut crate::config::ConfigSection, common: &InterfaceCommonConfig) {
    set_bool(section, "enabled", common.enabled);
    section.set("mode", mode_name(common.mode));
    set_bool(section, "outgoing", common.outgoing);
    set_opt_num(section, "bitrate", common.bitrate);
    set_opt_num(section, "announce_cap", common.announce_cap);
    set_opt_num(section, "announce_rate_target", common.announce_rate_target);
    set_opt_num(section, "announce_rate_grace", common.announce_rate_grace);
    set_opt_num(
        section,
        "announce_rate_penalty",
        common.announce_rate_penalty,
    );
    set_opt(section, "network_name", common.ifac_network_name.as_deref());
    set_opt(section, "passphrase", common.ifac_passphrase.as_deref());
    set_opt_num(section, "ifac_size", common.ifac_size);
    set_bool(section, "ingress_control", common.ingress_control);
    write_ingress(section, &common.ingress);
    set_bool(section, "recursive_prs", common.recursive_path_requests);
    set_bool(
        section,
        "announces_from_internal",
        common.announces_from_internal,
    );
}

fn write_ingress(section: &mut crate::config::ConfigSection, ingress: &IngressConfig) {
    set_opt_num(section, "ic_burst_freq_new", ingress.burst_freq_new);
    set_opt_num(section, "ic_burst_freq", ingress.burst_freq);
    set_opt_num(
        section,
        "ic_pr_burst_freq_new",
        ingress.path_request_burst_freq_new,
    );
    set_opt_num(section, "ic_pr_burst_freq", ingress.path_request_burst_freq);
    set_opt_num(section, "ic_new_time", ingress.new_time);
    set_opt_num(section, "ic_burst_hold", ingress.burst_hold);
    set_opt_num(section, "ic_burst_penalty", ingress.burst_penalty);
    set_opt_num(section, "ic_max_held_announces", ingress.max_held_announces);
    set_opt_num(
        section,
        "ic_held_release_interval",
        ingress.held_release_interval,
    );
    set_opt_num(section, "ec_pr_freq", ingress.egress_path_request_freq);
    if let Some(value) = ingress.egress_control {
        set_bool(section, "egress_control", value);
    }
}

fn write_serial(section: &mut crate::config::ConfigSection, serial: &SerialInterfaceConfig) {
    section.set("port", &serial.port);
    set_num(section, "baud_rate", serial.baud_rate);
    set_num(section, "data_bits", serial.data_bits);
    section.set("parity", &serial.parity);
    set_num(section, "stop_bits", serial.stop_bits);
}

fn write_kiss(
    section: &mut crate::config::ConfigSection,
    preamble_ms: u32,
    tx_tail_ms: u32,
    persistence: u8,
    slot_time_ms: u32,
    flow_control: bool,
) {
    set_num(section, "preamble", preamble_ms);
    set_num(section, "txtail", tx_tail_ms);
    set_num(section, "persistence", persistence);
    set_num(section, "slottime", slot_time_ms);
    set_bool(section, "flow_control", flow_control);
}

fn write_radio(section: &mut crate::config::ConfigSection, radio: &RadioConfig) {
    set_num(section, "frequency", radio.frequency);
    set_num(section, "bandwidth", radio.bandwidth);
    set_num(section, "spreading_factor", radio.spreading_factor);
    set_num(section, "coding_rate", radio.coding_rate);
    set_num(section, "tx_power", radio.tx_power);
    set_opt_num(section, "airtime_limit_short", radio.airtime_limit_short);
    set_opt_num(section, "airtime_limit_long", radio.airtime_limit_long);
}

fn validate_hashes(field: &str, hashes: &[String]) -> Result<(), YamlConfigError> {
    for hash in hashes {
        if hash.len() != 32 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(YamlConfigError::Validation(format!(
                "reticulum.{field}: {hash:?} must be a 32-character hexadecimal identity hash"
            )));
        }
    }
    Ok(())
}

fn validate_radio(name: &str, radio: &RadioConfig) -> Result<(), YamlConfigError> {
    let error = |field: &str, expected: &str| {
        YamlConfigError::Validation(format!("interface {name:?}: {field} must be {expected}"))
    };
    if radio.frequency == 0 {
        return Err(error("frequency", "non-zero"));
    }
    if !(7_800..=1_625_000).contains(&radio.bandwidth) {
        return Err(error("bandwidth", "in 7800..=1625000"));
    }
    if !(5..=12).contains(&radio.spreading_factor) {
        return Err(error("spreading_factor", "in 5..=12"));
    }
    if !(5..=8).contains(&radio.coding_rate) {
        return Err(error("coding_rate", "in 5..=8"));
    }
    if !(-128..=37).contains(&radio.tx_power) {
        return Err(error("tx_power", "at most 37 dBm"));
    }
    for (field, value) in [
        ("airtime_limit_short", radio.airtime_limit_short),
        ("airtime_limit_long", radio.airtime_limit_long),
    ] {
        if value.is_some_and(|v| !(0.0..=100.0).contains(&v)) {
            return Err(error(field, "in 0..=100 percent"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_config_applies_defaults_and_validates() {
        let config = Config::parse("reticulum: {}\ninterfaces: []\n", "config.yaml").unwrap();
        assert!(config.reticulum.share_instance);
        assert_eq!(config.logging.level, 4);
    }

    #[test]
    fn rejects_unknown_fields() {
        let error =
            Config::parse("reticulum:\n  enable_transprot: true\n", "config.yaml").unwrap_err();
        assert!(error.to_string().contains("enable_transprot"));
    }

    #[test]
    fn rejects_duplicate_interface_names() {
        let yaml = "interfaces:\n  - type: auto\n    name: LAN\n  - type: auto\n    name: LAN\n";
        assert!(
            Config::parse(yaml, "config.yaml")
                .unwrap_err()
                .to_string()
                .contains("duplicate")
        );
    }

    #[test]
    fn plugin_config_is_opaque_to_core() {
        let yaml = "interfaces:\n  - type: plugin\n    name: LoRa\n    plugin: sx1262\n    config:\n      reset_pin: 12\n      modulation:\n        spreading_factor: 9\n";
        Config::parse(yaml, "config.yaml").unwrap();
    }

    #[test]
    fn default_config_round_trips() {
        let config = Config::default();
        let yaml = config.to_yaml().unwrap();
        assert_eq!(Config::parse(&yaml, "config.yaml").unwrap(), config);
    }

    #[test]
    fn example_config_is_valid() {
        Config::parse(EXAMPLE_CONFIG, "example config.yaml").unwrap();
    }
}
