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
        let compact = compact_value(self)?;
        serde_saphyr::to_string(&compact)
            .map_err(|error| YamlConfigError::Serialize(error.to_string()))
    }

    pub fn validate(&self) -> Result<(), YamlConfigError> {
        if self.reticulum.shared_instance_port == 0 || self.reticulum.instance_control_port == 0 {
            return Err(YamlConfigError::Validation(
                "reticulum shared_instance_port and instance_control_port must be in 1..=65535"
                    .into(),
            ));
        }
        if let Some(key) = &self.reticulum.rpc_key
            && (key.is_empty() || key.len() % 2 != 0 || hex::decode(key).is_err())
        {
            return Err(YamlConfigError::Validation(
                "reticulum.rpc_key must be a non-empty, even-length hexadecimal string".into(),
            ));
        }
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

    /// Normalize the validated typed configuration for runtime consumers.
    #[doc(hidden)]
    pub fn to_runtime_config(
        &self,
    ) -> Result<crate::normalized_config::NormalizedConfig, YamlConfigError> {
        let mut output = crate::normalized_config::NormalizedConfig::new();
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
                let mut section = crate::normalized_config::NormalizedSection::new();
                interface.write_normalized_section(&mut section)?;
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
        if self.port == Some(0) {
            return Err(YamlConfigError::Validation(
                "api.port must be in 1..=65535".into(),
            ));
        }
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
            Self::Kiss(v) => &v.common,
            Self::Rnode(v) => &v.common,
            Self::RnodeMulti(v) => &v.common,
            Self::Ax25Kiss(v) => &v.common,
            Self::Plugin(v) => &v.common,
        }
    }

    fn validate(&self) -> Result<(), YamlConfigError> {
        if let Some(cap) = self.common().announce_cap
            && (!cap.is_finite() || !(0.0 < cap && cap <= 100.0))
        {
            return Err(YamlConfigError::Validation(format!(
                "interface {:?}: announce_cap must be in (0, 100]",
                self.common().name
            )));
        }
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
            Self::TcpClient(v) if v.fixed_mtu.is_some_and(|mtu| mtu < 500) => {
                Err(YamlConfigError::Validation(format!(
                    "interface {:?}: fixed_mtu must be at least 500",
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
            Self::Serial(v) if v.port.trim().is_empty() => Err(YamlConfigError::Validation(
                format!("interface {:?}: port must not be empty", v.common.name),
            )),
            Self::Kiss(v) if v.port.trim().is_empty() => Err(YamlConfigError::Validation(format!(
                "interface {:?}: port must not be empty",
                v.common.name
            ))),
            Self::Rnode(v) if v.port.trim().is_empty() => Err(YamlConfigError::Validation(
                format!("interface {:?}: port must not be empty", v.common.name),
            )),
            Self::Rnode(v) => validate_radio(&v.common.name, &v.radio),
            Self::RnodeMulti(v) if v.port.trim().is_empty() || v.subinterfaces.is_empty() => {
                Err(YamlConfigError::Validation(format!(
                    "interface {:?}: port and at least one subinterface are required",
                    v.common.name
                )))
            }
            Self::RnodeMulti(v) => {
                let mut ports = HashSet::new();
                for sub in &v.subinterfaces {
                    if sub.name.trim().is_empty() {
                        return Err(YamlConfigError::Validation(format!(
                            "interface {:?}: RNode subinterface name must not be empty",
                            v.common.name
                        )));
                    }
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
                v.common.name
            ))),
            Self::Ax25Kiss(v) if v.port.trim().is_empty() || v.callsign.trim().is_empty() => {
                Err(YamlConfigError::Validation(format!(
                    "interface {:?}: port and callsign are required",
                    v.common.name
                )))
            }
            Self::Plugin(v) if v.plugin.trim().is_empty() => Err(YamlConfigError::Validation(
                format!("interface {:?}: plugin must not be empty", v.common.name),
            )),
            _ => Ok(()),
        }
    }

    fn write_normalized_section(
        &self,
        section: &mut crate::normalized_config::NormalizedSection,
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
                write_serial_fields(
                    section,
                    &v.port,
                    v.baud_rate,
                    v.data_bits,
                    &v.parity,
                    v.stop_bits,
                );
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
                write_serial_fields(
                    section,
                    &v.port,
                    v.baud_rate,
                    v.data_bits,
                    &v.parity,
                    v.stop_bits,
                );
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
            Self::Plugin(v) if !v.common.enabled => section.set("type", "PluginInterface"),
            Self::Plugin(v) => {
                return Err(YamlConfigError::Validation(format!(
                    "interface {:?}: enabled plugin interfaces require the future plugin ABI",
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
    pub common: InterfaceCommonConfig,
    pub port: String,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub parity: String,
    pub stop_bits: u8,
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
            common: Default::default(),
            port: String::new(),
            baud_rate: 9600,
            data_bits: 8,
            parity: "N".into(),
            stop_bits: 1,
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
    pub common: InterfaceCommonConfig,
    pub port: String,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub parity: String,
    pub stop_bits: u8,
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
            common: Default::default(),
            port: String::new(),
            baud_rate: 9600,
            data_bits: 8,
            parity: "N".into(),
            stop_bits: 1,
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

fn compact_value(config: &Config) -> Result<serde_json::Value, YamlConfigError> {
    let mut value = serde_json::to_value(config)
        .map_err(|error| YamlConfigError::Serialize(error.to_string()))?;
    let defaults = Config {
        interfaces: Vec::new(),
        ..Config::default()
    };
    let default_value = serde_json::to_value(defaults)
        .map_err(|error| YamlConfigError::Serialize(error.to_string()))?;

    let serde_json::Value::Object(root) = &mut value else {
        return Ok(value);
    };
    if let serde_json::Value::Object(default_root) = default_value {
        prune_object(root, &default_root, &[]);
    }

    if let Some(serde_json::Value::Array(interfaces)) = root.get_mut("interfaces") {
        for (item, interface) in interfaces.iter_mut().zip(&config.interfaces) {
            let baseline = serde_json::to_value(default_interface(interface))
                .map_err(|error| YamlConfigError::Serialize(error.to_string()))?;
            if let (serde_json::Value::Object(item), serde_json::Value::Object(baseline)) =
                (item, baseline)
            {
                prune_object(item, &baseline, &["type", "name"]);
                if matches!(interface, InterfaceConfig::RnodeMulti(_))
                    && let Some(serde_json::Value::Array(subinterfaces)) =
                        item.get_mut("subinterfaces")
                {
                    let sub_default = serde_json::to_value(RnodeSubInterfaceConfig::default())
                        .map_err(|error| YamlConfigError::Serialize(error.to_string()))?;
                    if let serde_json::Value::Object(sub_default) = sub_default {
                        for subinterface in subinterfaces {
                            if let serde_json::Value::Object(subinterface) = subinterface {
                                prune_object(subinterface, &sub_default, &["name"]);
                            }
                        }
                    }
                }
            }
        }
    }
    if root
        .get("interfaces")
        .is_some_and(|value| matches!(value, serde_json::Value::Array(items) if items.is_empty()))
    {
        root.remove("interfaces");
    }
    Ok(value)
}

fn prune_object(
    value: &mut serde_json::Map<String, serde_json::Value>,
    defaults: &serde_json::Map<String, serde_json::Value>,
    preserve: &[&str],
) {
    let keys: Vec<String> = value.keys().cloned().collect();
    for key in keys {
        if preserve.contains(&key.as_str()) {
            continue;
        }
        let Some(default) = defaults.get(&key) else {
            continue;
        };
        let remove = match (value.get_mut(&key), default) {
            (Some(serde_json::Value::Object(current)), serde_json::Value::Object(default)) => {
                prune_object(current, default, &[]);
                current.is_empty()
            }
            (Some(current), default) => current == default,
            (None, _) => false,
        };
        if remove {
            value.remove(&key);
        }
    }
}

fn default_interface(interface: &InterfaceConfig) -> InterfaceConfig {
    match interface {
        InterfaceConfig::Auto(_) => InterfaceConfig::Auto(AutoInterfaceConfig::default()),
        InterfaceConfig::TcpClient(_) => {
            InterfaceConfig::TcpClient(TcpClientInterfaceConfig::default())
        }
        InterfaceConfig::TcpServer(_) => {
            InterfaceConfig::TcpServer(TcpServerInterfaceConfig::default())
        }
        InterfaceConfig::Udp(_) => InterfaceConfig::Udp(UdpInterfaceConfig::default()),
        InterfaceConfig::Local(_) => InterfaceConfig::Local(LocalInterfaceConfig::default()),
        InterfaceConfig::I2p(_) => InterfaceConfig::I2p(I2pInterfaceConfig::default()),
        InterfaceConfig::Pipe(_) => InterfaceConfig::Pipe(PipeInterfaceConfig::default()),
        InterfaceConfig::Backbone(_) => {
            InterfaceConfig::Backbone(BackboneInterfaceConfig::default())
        }
        InterfaceConfig::Serial(_) => InterfaceConfig::Serial(SerialInterfaceConfig::default()),
        InterfaceConfig::Kiss(_) => InterfaceConfig::Kiss(KissInterfaceConfig::default()),
        InterfaceConfig::Rnode(_) => InterfaceConfig::Rnode(RnodeInterfaceConfig::default()),
        InterfaceConfig::RnodeMulti(_) => {
            InterfaceConfig::RnodeMulti(RnodeMultiInterfaceConfig::default())
        }
        InterfaceConfig::Ax25Kiss(_) => {
            InterfaceConfig::Ax25Kiss(Ax25KissInterfaceConfig::default())
        }
        InterfaceConfig::Plugin(_) => InterfaceConfig::Plugin(PluginInterfaceConfig::default()),
    }
}

fn set_bool(section: &mut crate::normalized_config::NormalizedSection, key: &str, value: bool) {
    section.set(key, if value { "Yes" } else { "No" });
}

fn set_num(
    section: &mut crate::normalized_config::NormalizedSection,
    key: &str,
    value: impl ToString,
) {
    section.set(key, &value.to_string());
}

fn set_opt(
    section: &mut crate::normalized_config::NormalizedSection,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value {
        section.set(key, value);
    }
}

fn set_opt_num<T: ToString + Copy>(
    section: &mut crate::normalized_config::NormalizedSection,
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

fn write_common(
    section: &mut crate::normalized_config::NormalizedSection,
    common: &InterfaceCommonConfig,
) {
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

fn write_ingress(
    section: &mut crate::normalized_config::NormalizedSection,
    ingress: &IngressConfig,
) {
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

fn write_serial(
    section: &mut crate::normalized_config::NormalizedSection,
    serial: &SerialInterfaceConfig,
) {
    write_serial_fields(
        section,
        &serial.port,
        serial.baud_rate,
        serial.data_bits,
        &serial.parity,
        serial.stop_bits,
    );
}

fn write_serial_fields(
    section: &mut crate::normalized_config::NormalizedSection,
    port: &str,
    baud_rate: u32,
    data_bits: u8,
    parity: &str,
    stop_bits: u8,
) {
    section.set("port", port);
    set_num(section, "baud_rate", baud_rate);
    set_num(section, "data_bits", data_bits);
    section.set("parity", parity);
    set_num(section, "stop_bits", stop_bits);
}

fn write_kiss(
    section: &mut crate::normalized_config::NormalizedSection,
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

fn write_radio(section: &mut crate::normalized_config::NormalizedSection, radio: &RadioConfig) {
    set_num(section, "frequency", radio.frequency);
    set_num(section, "bandwidth", radio.bandwidth);
    set_num(section, "spreading_factor", radio.spreading_factor);
    set_num(section, "coding_rate", radio.coding_rate);
    set_num(section, "tx_power", radio.tx_power);
    set_opt_num(section, "airtime_limit_short", radio.airtime_limit_short);
    set_opt_num(section, "airtime_limit_long", radio.airtime_limit_long);
}

/// Convert a runtime-normalized section into the typed YAML variant. This
/// is deliberately kept at the configuration boundary; runtime code never
/// receives parser values.
#[cfg(feature = "api")]
pub fn interface_from_normalized_section(
    name: &str,
    section: &crate::normalized_config::NormalizedSection,
) -> Result<InterfaceConfig, YamlConfigError> {
    let mut enabled_section = section.clone();
    enabled_section.set("enabled", "Yes");
    let runtime = crate::interface_factory::synthesize_interface(name, &enabled_section)
        .map_err(|error| YamlConfigError::Validation(error.to_string()))?;
    let common = common_from_normalized(name, section);
    use crate::interface_factory::InterfaceConfig as Runtime;
    Ok(match runtime {
        Runtime::Auto(v) => InterfaceConfig::Auto(AutoInterfaceConfig {
            common,
            group_id: v.group_id,
            discovery_scope: match v.discovery_scope {
                rns_interface::auto::DiscoveryScope::Link => DiscoveryScope::Link,
                rns_interface::auto::DiscoveryScope::Admin => DiscoveryScope::Admin,
                rns_interface::auto::DiscoveryScope::Site => DiscoveryScope::Site,
                rns_interface::auto::DiscoveryScope::Organisation => DiscoveryScope::Organisation,
                rns_interface::auto::DiscoveryScope::Global => DiscoveryScope::Global,
            },
            discovery_port: v.discovery_port,
            data_port: v.data_port,
            multicast_address_type: match v.multicast_address_type {
                rns_interface::auto::McastAddrType::Permanent => MulticastAddressType::Permanent,
                rns_interface::auto::McastAddrType::Temporary => MulticastAddressType::Temporary,
            },
            devices: v.devices,
            ignored_devices: v.ignored_devices,
            configured_bitrate: v.configured_bitrate,
        }),
        Runtime::TcpClient(v) => InterfaceConfig::TcpClient(TcpClientInterfaceConfig {
            common,
            target_host: v.target_host,
            target_port: v.target_port,
            kiss_framing: v.kiss_framing,
            connect_timeout: v.connect_timeout_secs,
            max_reconnect_tries: v.max_reconnect_tries,
            fixed_mtu: v.fixed_mtu,
        }),
        Runtime::TcpServer(v) => InterfaceConfig::TcpServer(TcpServerInterfaceConfig {
            common,
            listen_ip: v.listen_ip,
            listen_port: v.listen_port,
            kiss_framing: v.kiss_framing,
            prefer_ipv6: v.prefer_ipv6,
            device: v.device,
        }),
        Runtime::Udp(v) => InterfaceConfig::Udp(UdpInterfaceConfig {
            common,
            listen_ip: v.listen_ip,
            listen_port: v.listen_port,
            forward_ip: v.forward_ip,
            forward_port: v.forward_port,
            device: v.device,
        }),
        Runtime::Local(v) => InterfaceConfig::Local(LocalInterfaceConfig {
            common,
            port: v.port,
        }),
        Runtime::I2P(v) => InterfaceConfig::I2p(I2pInterfaceConfig {
            common,
            connectable: v.connectable,
            peers: v.peers,
            sam_host: v.i2p_sam_host,
            sam_port: v.i2p_sam_port,
        }),
        Runtime::Pipe(v) => InterfaceConfig::Pipe(PipeInterfaceConfig {
            common,
            command: v.command,
            respawn_delay: v.respawn_delay,
        }),
        Runtime::Backbone(v) => InterfaceConfig::Backbone(BackboneInterfaceConfig {
            common,
            listen_on: v.listen_on,
            target_host: v.target_host,
            port: v.port,
            device: v.device,
            prefer_ipv6: v.prefer_ipv6,
            connect_timeout: v.connect_timeout,
            max_reconnect_tries: v.max_reconnect_tries,
            i2p_tunneled: v.i2p_tunneled,
        }),
        #[cfg(feature = "serial")]
        Runtime::Serial(v) => InterfaceConfig::Serial(SerialInterfaceConfig {
            common,
            port: v.port,
            baud_rate: v.baud_rate,
            data_bits: v.data_bits,
            parity: v.parity,
            stop_bits: v.stop_bits,
        }),
        #[cfg(feature = "serial")]
        Runtime::KissSerial(v) => InterfaceConfig::Kiss(KissInterfaceConfig {
            common,
            port: v.port,
            baud_rate: v.baud_rate,
            data_bits: v.data_bits,
            parity: v.parity,
            stop_bits: v.stop_bits,
            preamble_ms: v.preamble_ms,
            tx_tail_ms: v.txtail_ms,
            persistence: v.persistence,
            slot_time_ms: v.slottime_ms,
            flow_control: v.flow_control,
            id_interval: v.id_interval,
            id_callsign: v.id_callsign,
        }),
        #[cfg(any(feature = "serial", feature = "rnode-tcp"))]
        Runtime::RNode(v) => InterfaceConfig::Rnode(RnodeInterfaceConfig {
            common,
            port: v.port,
            radio: RadioConfig {
                frequency: v.frequency,
                bandwidth: v.bandwidth,
                spreading_factor: v.spreading_factor,
                coding_rate: v.coding_rate,
                tx_power: v.tx_power,
                airtime_limit_short: v.st_alock,
                airtime_limit_long: v.lt_alock,
            },
            flow_control: v.flow_control,
            id_interval: v.id_interval,
            id_callsign: v.id_callsign,
        }),
        #[cfg(feature = "ble")]
        Runtime::BleRNode(v) => InterfaceConfig::Rnode(RnodeInterfaceConfig {
            common,
            port: v.port,
            radio: RadioConfig {
                frequency: v.frequency,
                bandwidth: v.bandwidth,
                spreading_factor: v.spreading_factor,
                coding_rate: v.coding_rate,
                tx_power: v.tx_power,
                airtime_limit_short: v.st_alock,
                airtime_limit_long: v.lt_alock,
            },
            flow_control: v.flow_control,
            id_interval: v.id_interval,
            id_callsign: v.id_callsign,
        }),
        #[cfg(feature = "serial")]
        Runtime::RNodeMulti(v) => InterfaceConfig::RnodeMulti(RnodeMultiInterfaceConfig {
            common,
            port: v.port,
            baud_rate: v.baud_rate,
            flow_control: v.flow_control,
            subinterfaces: v
                .subinterfaces
                .into_iter()
                .map(|sub| RnodeSubInterfaceConfig {
                    name: sub.name,
                    vport: sub.vport,
                    enabled: sub.enabled,
                    outgoing: sub.outgoing,
                    flow_control: Some(sub.flow_control),
                    mode: Some(interface_mode_from_runtime(sub.mode)),
                    radio: RadioConfig {
                        frequency: sub.frequency,
                        bandwidth: sub.bandwidth,
                        spreading_factor: sub.spreading_factor,
                        coding_rate: sub.coding_rate,
                        tx_power: sub.tx_power as i8,
                        airtime_limit_short: sub.st_alock,
                        airtime_limit_long: sub.lt_alock,
                    },
                })
                .collect(),
            id_interval: v.id_interval,
            id_callsign: v.id_callsign,
        }),
        #[cfg(feature = "serial")]
        Runtime::AX25KISS(v) => InterfaceConfig::Ax25Kiss(Ax25KissInterfaceConfig {
            common,
            port: v.port,
            baud_rate: v.baud_rate,
            data_bits: v.data_bits,
            parity: v.parity,
            stop_bits: v.stop_bits,
            callsign: v.callsign,
            ssid: v.ssid,
            preamble_ms: v.preamble,
            tx_tail_ms: v.txtail,
            persistence: v.persistence as u8,
            slot_time_ms: v.slottime,
            flow_control: v.flow_control,
        }),
    })
}

#[cfg(feature = "api")]
fn common_from_normalized(
    name: &str,
    section: &crate::normalized_config::NormalizedSection,
) -> InterfaceCommonConfig {
    InterfaceCommonConfig {
        name: name.to_string(),
        enabled: section.get_bool("enabled").unwrap_or(true),
        mode: section
            .get("interface_mode")
            .or_else(|| section.get("mode"))
            .and_then(interface_mode_from_name)
            .unwrap_or_default(),
        outgoing: section.get_bool("outgoing").unwrap_or(true),
        bitrate: section.get_uint("bitrate"),
        announce_cap: section.get_float("announce_cap"),
        announce_rate_target: section.get_uint("announce_rate_target"),
        announce_rate_grace: section.get_uint("announce_rate_grace").map(|v| v as u32),
        announce_rate_penalty: section.get_uint("announce_rate_penalty"),
        ifac_network_name: section
            .get("networkname")
            .or_else(|| section.get("network_name"))
            .map(str::to_string),
        ifac_passphrase: section
            .get("passphrase")
            .or_else(|| section.get("pass_phrase"))
            .map(str::to_string),
        ifac_size: section.get_uint("ifac_size").map(|v| v as usize),
        ingress_control: section.get_bool("ingress_control").unwrap_or(true),
        ingress: IngressConfig {
            burst_freq_new: section.get_float("ic_burst_freq_new"),
            burst_freq: section.get_float("ic_burst_freq"),
            path_request_burst_freq_new: section.get_float("ic_pr_burst_freq_new"),
            path_request_burst_freq: section.get_float("ic_pr_burst_freq"),
            new_time: section.get_float("ic_new_time"),
            burst_hold: section.get_float("ic_burst_hold"),
            burst_penalty: section.get_float("ic_burst_penalty"),
            max_held_announces: section
                .get_uint("ic_max_held_announces")
                .map(|v| v as usize),
            held_release_interval: section.get_float("ic_held_release_interval"),
            egress_path_request_freq: section.get_float("ec_pr_freq"),
            egress_control: section.get_bool("egress_control"),
        },
        recursive_path_requests: section.get_bool("recursive_prs").unwrap_or(false),
        announces_from_internal: section.get_bool("announces_from_internal").unwrap_or(true),
    }
}

#[cfg(feature = "api")]
fn interface_mode_from_name(name: &str) -> Option<InterfaceMode> {
    match name.to_ascii_lowercase().replace('-', "_").as_str() {
        "full" => Some(InterfaceMode::Full),
        "pointtopoint" | "point_to_point" => Some(InterfaceMode::PointToPoint),
        "accesspoint" | "access_point" | "ap" => Some(InterfaceMode::AccessPoint),
        "roaming" => Some(InterfaceMode::Roaming),
        "boundary" => Some(InterfaceMode::Boundary),
        "gateway" | "gw" => Some(InterfaceMode::Gateway),
        "internal" => Some(InterfaceMode::Internal),
        _ => None,
    }
}

#[cfg(all(feature = "api", feature = "serial"))]
fn interface_mode_from_runtime(mode: rns_interface::traits::InterfaceMode) -> InterfaceMode {
    match mode {
        rns_interface::traits::InterfaceMode::Full => InterfaceMode::Full,
        rns_interface::traits::InterfaceMode::PointToPoint => InterfaceMode::PointToPoint,
        rns_interface::traits::InterfaceMode::AccessPoint => InterfaceMode::AccessPoint,
        rns_interface::traits::InterfaceMode::Roaming => InterfaceMode::Roaming,
        rns_interface::traits::InterfaceMode::Boundary => InterfaceMode::Boundary,
        rns_interface::traits::InterfaceMode::Gateway => InterfaceMode::Gateway,
        rns_interface::traits::InterfaceMode::Internal => InterfaceMode::Internal,
    }
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
    fn rejects_unknown_interface_fields() {
        let yaml = "interfaces:\n  - type: auto\n    name: LAN\n    discovery_prot: 29716\n";
        let error = Config::parse(yaml, "config.yaml").unwrap_err();
        assert!(error.to_string().contains("discovery_prot"));
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
    fn rejects_out_of_range_ports_caps_and_invalid_rpc_keys() {
        for yaml in [
            "reticulum:\n  shared_instance_port: 0\n",
            "api:\n  port: 0\n",
            "reticulum:\n  rpc_key: xyz\n",
            "interfaces:\n  - type: auto\n    name: LAN\n    announce_cap: 0\n",
        ] {
            assert!(Config::parse(yaml, "config.yaml").is_err(), "{yaml}");
        }
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
    fn serialization_omits_default_values_but_preserves_required_fields() {
        let config = Config::default();
        let yaml = config.to_yaml().unwrap();
        assert!(yaml.contains("type: auto"));
        assert!(yaml.contains("name: Default Interface"));
        assert!(!yaml.contains("share_instance:"));
        assert!(!yaml.contains("enabled:"));
        assert!(!yaml.contains("discovery_port:"));
        assert_eq!(Config::parse(&yaml, "config.yaml").unwrap(), config);

        let mut changed = config;
        changed.reticulum.enable_transport = true;
        let InterfaceConfig::Auto(interface) = &mut changed.interfaces[0] else {
            unreachable!()
        };
        interface.common.outgoing = false;
        let yaml = changed.to_yaml().unwrap();
        assert!(yaml.contains("enable_transport: true"));
        assert!(yaml.contains("outgoing: false"));
        assert_eq!(Config::parse(&yaml, "config.yaml").unwrap(), changed);
    }

    #[test]
    fn example_config_is_valid() {
        Config::parse(EXAMPLE_CONFIG, "example config.yaml").unwrap();
    }

    #[test]
    fn every_builtin_interface_type_deserializes_and_validates() {
        let yaml = r#"
interfaces:
  - { type: auto, name: Auto }
  - { type: tcp_client, name: TCP client, target_host: example.org, target_port: 4242 }
  - { type: tcp_server, name: TCP server, listen_port: 4242 }
  - { type: udp, name: UDP, listen_port: 4242 }
  - { type: local, name: Local }
  - { type: i2p, name: I2P }
  - { type: pipe, name: Pipe, command: /usr/bin/example }
  - { type: backbone, name: Backbone, target_host: example.org, port: 4242 }
  - { type: serial, name: Serial, enabled: false, port: /dev/ttyUSB0 }
  - { type: kiss, name: KISS, enabled: false, port: /dev/ttyUSB1 }
  - type: rnode
    name: RNode
    enabled: false
    port: /dev/ttyUSB2
    frequency: 868000000
    bandwidth: 125000
    spreading_factor: 9
    coding_rate: 5
    tx_power: 17
  - type: rnode_multi
    name: RNode Multi
    enabled: false
    port: /dev/ttyACM0
    subinterfaces:
      - name: Primary
        vport: 0
        frequency: 868000000
        bandwidth: 125000
        spreading_factor: 9
        coding_rate: 5
        tx_power: 17
  - { type: ax25_kiss, name: AX25, enabled: false, port: /dev/ttyUSB3, callsign: NO1CLL, ssid: 0 }
  - { type: plugin, name: Future plugin, enabled: false, plugin: sx1262, config: { reset_pin: 12 } }
"#;
        let config = Config::parse(yaml, "all-interfaces.yaml").unwrap();
        assert_eq!(config.interfaces.len(), 14);
        config.to_runtime_config().unwrap();
    }

    #[test]
    fn missing_required_interface_field_is_rejected() {
        let yaml = "interfaces:\n  - type: tcp_client\n    name: Broken\n    target_port: 4242\n";
        assert!(Config::parse(yaml, "config.yaml").is_err());
    }

    #[test]
    fn invalid_enum_and_malformed_yaml_are_rejected() {
        let invalid_enum = "interfaces:\n  - type: auto\n    name: LAN\n    mode: impossible\n";
        assert!(Config::parse(invalid_enum, "config.yaml").is_err());
        assert!(Config::parse("reticulum: [", "config.yaml").is_err());
    }

    #[test]
    fn invalid_ports_and_radio_ranges_are_rejected() {
        let port = "interfaces:\n  - type: tcp_server\n    name: TCP\n    listen_port: 0\n";
        assert!(Config::parse(port, "config.yaml").is_err());
        let radio = "interfaces:\n  - type: rnode\n    name: Radio\n    port: /dev/null\n    frequency: 868000000\n    bandwidth: 125000\n    spreading_factor: 20\n    coding_rate: 5\n    tx_power: 17\n";
        assert!(Config::parse(radio, "config.yaml").is_err());
    }
}
