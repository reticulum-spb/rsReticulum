//! Linux shared-library loader for interface plugins.

use std::collections::{HashMap, VecDeque};
use std::ffi::{OsStr, c_void};
use std::fs;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use bytes::Bytes;
use libloading::{Library, Symbol};
use rns_plugin::{
    ABI_MAJOR, ABI_MINOR, GET_API_SYMBOL, GetApiFn, HOST_API_V1_0_SIZE, HostApi, LOG_DEBUG,
    LOG_ERROR, LOG_INFO, LOG_TRACE, LOG_WARN, OK, PLUGIN_API_V1_0_SIZE,
    PLUGIN_INFO_DESCRIPTION_MAX_SIZE, PLUGIN_INFO_NAME_MAX_SIZE, PLUGIN_INFO_V1_0_SIZE,
    PLUGIN_INFO_VERSION_MAX_SIZE, PluginApi, PluginInfo, PluginInstance, RX_METADATA_RSSI,
    RX_METADATA_SNR, RnsString, RxMetadata,
};
use rns_transport::messages::{InboundPacket, TransportMessage};
use thiserror::Error;
use tokio::sync::mpsc;

use crate::traits::{InterfaceDirection, InterfaceHandle, InterfaceId, InterfaceMode};

pub const PLUGIN_DIRECTORY: &str = "/usr/lib/reticulum-rs";
const TX_QUEUE_SIZE: usize = 256;
const EARLY_RX_QUEUE_SIZE: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedPluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct PluginListEntry {
    pub filename: String,
    pub result: Result<LoadedPluginInfo, String>,
}

#[derive(Debug, Error)]
pub enum PluginLoadError {
    #[error("invalid plugin name '{0}' (expected 1-128 ASCII letters, digits, '_' or '-')")]
    InvalidName(String),
    #[error("cannot resolve plugin path '{path}': {source}")]
    Canonicalize {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("plugin path escapes {PLUGIN_DIRECTORY}: {0}")]
    OutsidePluginDirectory(PathBuf),
    #[error("cannot load plugin '{path}': {message}")]
    Open { path: PathBuf, message: String },
    #[error("plugin '{path}' does not export rns_plugin_get_api: {message}")]
    MissingEntryPoint { path: PathBuf, message: String },
    #[error("plugin '{0}' returned a null API table")]
    NullApi(PathBuf),
    #[error("plugin '{path}' ABI major {actual} is incompatible with host ABI major {expected}")]
    AbiMajor {
        path: PathBuf,
        expected: u32,
        actual: u32,
    },
    #[error("plugin '{path}' API table is too small: {actual} bytes, need at least {minimum}")]
    ApiTooSmall {
        path: PathBuf,
        actual: u32,
        minimum: usize,
    },
    #[error("plugin '{0}' is missing a mandatory API function")]
    MissingFunction(PathBuf),
    #[error("plugin '{0}' has missing or truncated info")]
    MissingInfo(PathBuf),
    #[error("plugin '{path}' info field '{field}' has invalid length {length}")]
    InvalidInfoLength {
        path: PathBuf,
        field: &'static str,
        length: usize,
    },
    #[error("plugin '{path}' info field '{field}' is not UTF-8")]
    InvalidInfoUtf8 { path: PathBuf, field: &'static str },
}

#[derive(Debug, Clone)]
pub struct PluginInterfaceConfig {
    pub name: String,
    pub plugin: String,
    pub config_yaml: Vec<u8>,
    pub mtu: u32,
    pub mode: InterfaceMode,
}

impl PluginInterfaceConfig {
    pub fn new(name: impl Into<String>, plugin: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            plugin: plugin.into(),
            config_yaml: Vec::new(),
            mtu: rns_wire::constants::MTU as u32,
            mode: InterfaceMode::Full,
        }
    }
}

#[derive(Clone, Copy)]
#[doc(hidden)]
pub struct PluginFunctions {
    pub create: rns_plugin::CreateFn,
    pub send: rns_plugin::SendFn,
    pub destroy: rns_plugin::DestroyFn,
}

/// A validated plugin library. The native handle intentionally remains loaded
/// until process exit, even when this Rust value is dropped.
pub struct PluginLibrary {
    path: PathBuf,
    info: LoadedPluginInfo,
    functions: PluginFunctions,
    _library: ManuallyDrop<Library>,
}

static LIBRARIES: OnceLock<Mutex<HashMap<PathBuf, Arc<PluginLibrary>>>> = OnceLock::new();
static HOST_CONTEXTS: OnceLock<Mutex<HashMap<InterfaceId, Weak<PluginHostContext>>>> =
    OnceLock::new();

impl std::fmt::Debug for PluginLibrary {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginLibrary")
            .field("path", &self.path)
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl PluginLibrary {
    /// Load an already-canonicalized library path and validate ABI v1.0.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, PluginLoadError> {
        let path = path.as_ref().to_path_buf();

        // SAFETY: Loading native code is the purpose of this module. The
        // resulting handle is retained for process lifetime, and all symbols
        // are validated and copied before the temporary Symbol is dropped.
        let library = ManuallyDrop::new(unsafe { Library::new(&path) }.map_err(|error| {
            PluginLoadError::Open {
                path: path.clone(),
                message: error.to_string(),
            }
        })?);

        // SAFETY: The symbol name and C signature are fixed by the public ABI.
        let api_ptr =
            {
                let get_api: Symbol<'_, GetApiFn> = unsafe { library.get(GET_API_SYMBOL) }
                    .map_err(|error| PluginLoadError::MissingEntryPoint {
                        path: path.clone(),
                        message: error.to_string(),
                    })?;
                // SAFETY: A plugin must return a static table or null. No reference is
                // formed until the pointer and mandatory prefix have been validated.
                unsafe { get_api() }
            };
        if api_ptr.is_null() {
            return Err(PluginLoadError::NullApi(path));
        }

        #[repr(C)]
        struct ApiHeader {
            abi_major: u32,
            _abi_minor: u32,
            struct_size: u32,
            _reserved0: u32,
        }

        // SAFETY: PluginApi begins with ApiHeader in every ABI version.
        let header = unsafe { &*api_ptr.cast::<ApiHeader>() };
        if header.abi_major != ABI_MAJOR {
            return Err(PluginLoadError::AbiMajor {
                path,
                expected: ABI_MAJOR,
                actual: header.abi_major,
            });
        }
        if (header.struct_size as usize) < PLUGIN_API_V1_0_SIZE {
            return Err(PluginLoadError::ApiTooSmall {
                path,
                actual: header.struct_size,
                minimum: PLUGIN_API_V1_0_SIZE,
            });
        }

        // SAFETY: The table contains the complete v1.0 prefix after the size
        // check, and the library remains loaded for the process lifetime.
        let api: &PluginApi = unsafe { &*api_ptr };
        let functions = PluginFunctions {
            create: api
                .create
                .ok_or_else(|| PluginLoadError::MissingFunction(path.clone()))?,
            send: api
                .send
                .ok_or_else(|| PluginLoadError::MissingFunction(path.clone()))?,
            destroy: api
                .destroy
                .ok_or_else(|| PluginLoadError::MissingFunction(path.clone()))?,
        };
        if api.info.is_null() || api.info_size < PLUGIN_INFO_V1_0_SIZE {
            return Err(PluginLoadError::MissingInfo(path));
        }
        // SAFETY: info_size proves that the mandatory v1.0 prefix is present;
        // the ABI requires the pointed-to structure and strings to be static.
        let raw_info: &PluginInfo = unsafe { &*api.info };
        let info = LoadedPluginInfo {
            name: copy_info_string(&path, "name", raw_info.name, PLUGIN_INFO_NAME_MAX_SIZE)?,
            version: copy_info_string(
                &path,
                "version",
                raw_info.version,
                PLUGIN_INFO_VERSION_MAX_SIZE,
            )?,
            description: copy_info_string(
                &path,
                "description",
                raw_info.description,
                PLUGIN_INFO_DESCRIPTION_MAX_SIZE,
            )?,
        };

        Ok(Self {
            path,
            info,
            functions,
            _library: library,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn info(&self) -> &LoadedPluginInfo {
        &self.info
    }

    #[doc(hidden)]
    pub fn functions(&self) -> PluginFunctions {
        self.functions
    }
}

pub fn load_configured_library(name: &str) -> Result<Arc<PluginLibrary>, PluginLoadError> {
    let configured = configured_plugin_path(name)?;
    let canonical = canonical_plugin_path(&configured)?;
    load_registered_library(&canonical)
}

fn load_registered_library(canonical: &Path) -> Result<Arc<PluginLibrary>, PluginLoadError> {
    let registry = LIBRARIES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut libraries = registry.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(library) = libraries.get(canonical) {
        return Ok(library.clone());
    }
    let library = Arc::new(PluginLibrary::load(canonical)?);
    libraries.insert(canonical.to_path_buf(), library.clone());
    Ok(library)
}

pub fn list_available_plugins() -> Result<Vec<PluginListEntry>, std::io::Error> {
    let directory = match fs::read_dir(PLUGIN_DIRECTORY) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut paths = directory
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| is_shared_library(path))
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| left.file_name().cmp(&right.file_name()));

    Ok(paths
        .into_iter()
        .map(|path| {
            let filename = path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            let result = canonical_plugin_path(&path)
                .and_then(|canonical| load_registered_library(&canonical))
                .map(|library| library.info().clone())
                .map_err(|error| error.to_string());
            PluginListEntry { filename, result }
        })
        .collect())
}

#[derive(Debug, Error)]
pub enum PluginSpawnError {
    #[error(transparent)]
    Load(#[from] PluginLoadError),
    #[error("plugin interface '{name}' failed to create: {message}")]
    Create { name: String, message: String },
}

struct PluginHostContext {
    id: InterfaceId,
    name: String,
    transport_tx: mpsc::Sender<TransportMessage>,
    online: Arc<AtomicBool>,
    bitrate: Arc<AtomicU64>,
    rxb: Arc<AtomicU64>,
    activated: AtomicBool,
    early_rx: Mutex<VecDeque<InboundPacket>>,
}

impl PluginHostContext {
    fn deliver_or_buffer(&self, packet: InboundPacket) {
        if self.activated.load(Ordering::Acquire) {
            self.deliver(packet);
            return;
        }
        let mut pending = self
            .early_rx
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if self.activated.load(Ordering::Acquire) {
            drop(pending);
            self.deliver(packet);
        } else if pending.len() < EARLY_RX_QUEUE_SIZE {
            pending.push_back(packet);
        } else {
            tracing::warn!(
                interface_id = self.id,
                interface_name = %self.name,
                "dropping early plugin RX packet: activation queue full"
            );
        }
    }

    fn deliver(&self, packet: InboundPacket) {
        let len = packet.raw.len() as u64;
        match self
            .transport_tx
            .try_send(TransportMessage::Inbound(packet))
        {
            Ok(()) => {
                self.rxb.fetch_add(len, Ordering::Relaxed);
            }
            Err(error) => {
                tracing::warn!(
                    interface_id = self.id,
                    interface_name = %self.name,
                    %error,
                    "dropping plugin RX packet: transport channel unavailable"
                );
            }
        }
    }

    fn activate(&self) {
        self.activated.store(true, Ordering::Release);
        let mut pending = self
            .early_rx
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let packets = pending.drain(..).collect::<Vec<_>>();
        drop(pending);
        for packet in packets {
            self.deliver(packet);
        }
    }
}

/// Release RX packets buffered between plugin create() and transport
/// registration. Calling this for a non-plugin interface is a no-op.
pub fn activate_plugin_interface(id: InterfaceId) {
    let Some(contexts) = HOST_CONTEXTS.get() else {
        return;
    };
    let context = contexts
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(&id)
        .and_then(Weak::upgrade);
    if let Some(context) = context {
        context.activate();
    }
}

unsafe extern "C" fn host_log(
    host_context: *mut c_void,
    level: rns_plugin::LogLevel,
    message: *const u8,
    message_len: usize,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: host_context is the stable Arc allocation supplied in HostApi.
        let Some(context) = (unsafe { (host_context as *const PluginHostContext).as_ref() }) else {
            return;
        };
        if message.is_null() || message_len == 0 {
            tracing::error!(interface_name = %context.name, "plugin supplied an empty log message");
            return;
        }
        // SAFETY: The plugin ABI promises a readable borrowed buffer for the
        // duration of this callback.
        let bytes = unsafe { std::slice::from_raw_parts(message, message_len) };
        let text = String::from_utf8_lossy(bytes);
        match level {
            LOG_ERROR => tracing::error!(interface_name = %context.name, "{text}"),
            LOG_WARN => tracing::warn!(interface_name = %context.name, "{text}"),
            LOG_INFO => tracing::info!(interface_name = %context.name, "{text}"),
            LOG_DEBUG => tracing::debug!(interface_name = %context.name, "{text}"),
            LOG_TRACE => tracing::trace!(interface_name = %context.name, "{text}"),
            _ => tracing::warn!(interface_name = %context.name, level, "{text}"),
        }
    }));
}

unsafe extern "C" fn host_set_bitrate(host_context: *mut c_void, bitrate_bps: u64) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: host_context is the stable Arc allocation supplied in HostApi.
        let Some(context) = (unsafe { (host_context as *const PluginHostContext).as_ref() }) else {
            return;
        };
        if bitrate_bps == 0 {
            tracing::error!(interface_name = %context.name, "plugin reported zero bitrate");
            return;
        }
        context.bitrate.store(bitrate_bps, Ordering::Release);
        let _ = context
            .transport_tx
            .try_send(TransportMessage::UpdateInterfaceBitrate {
                id: context.id,
                bitrate: bitrate_bps,
            });
    }));
}

unsafe extern "C" fn host_set_online(host_context: *mut c_void, online: u8) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: host_context is the stable Arc allocation supplied in HostApi.
        let Some(context) = (unsafe { (host_context as *const PluginHostContext).as_ref() }) else {
            return;
        };
        match online {
            0 => context.online.store(false, Ordering::Release),
            1 => context.online.store(true, Ordering::Release),
            _ => tracing::error!(
                interface_name = %context.name,
                online,
                "plugin reported invalid online value"
            ),
        }
    }));
}

unsafe extern "C" fn host_rx_packet(
    host_context: *mut c_void,
    data: *const u8,
    data_len: usize,
    metadata: *const RxMetadata,
    metadata_size: usize,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: host_context is the stable Arc allocation supplied in HostApi.
        let Some(context) = (unsafe { (host_context as *const PluginHostContext).as_ref() }) else {
            return;
        };
        if data.is_null() || data_len == 0 {
            tracing::error!(interface_name = %context.name, "plugin supplied an empty RX packet");
            return;
        }
        // SAFETY: The plugin owns a readable buffer for this callback. Bytes
        // copies it before the callback returns.
        let raw = Bytes::copy_from_slice(unsafe { std::slice::from_raw_parts(data, data_len) });
        let (rssi, snr) = read_rx_metadata(metadata, metadata_size);
        context.deliver_or_buffer(InboundPacket {
            raw,
            interface_id: context.id,
            rssi,
            snr,
            q: None,
        });
    }));
}

fn read_rx_metadata(
    metadata: *const RxMetadata,
    metadata_size: usize,
) -> (Option<f32>, Option<f32>) {
    if metadata.is_null() || metadata_size < size_of::<u32>() {
        return (None, None);
    }
    // SAFETY: Each field is read only after its byte range has been proven to
    // fit metadata_size. read_unaligned avoids relying on plugin alignment.
    let valid = unsafe { std::ptr::read_unaligned(metadata.cast::<u32>()) };
    let rssi = if valid & RX_METADATA_RSSI != 0
        && metadata_size >= std::mem::offset_of!(RxMetadata, rssi_dbm) + size_of::<i16>()
    {
        let pointer = metadata
            .cast::<u8>()
            .wrapping_add(std::mem::offset_of!(RxMetadata, rssi_dbm));
        Some(unsafe { std::ptr::read_unaligned(pointer.cast::<i16>()) } as f32)
    } else {
        None
    };
    let snr = if valid & RX_METADATA_SNR != 0
        && metadata_size >= std::mem::offset_of!(RxMetadata, snr_db) + size_of::<i16>()
    {
        let pointer = metadata
            .cast::<u8>()
            .wrapping_add(std::mem::offset_of!(RxMetadata, snr_db));
        Some(unsafe { std::ptr::read_unaligned(pointer.cast::<i16>()) } as f32)
    } else {
        None
    };
    (rssi, snr)
}

pub async fn spawn_plugin_interface(
    config: PluginInterfaceConfig,
    id: InterfaceId,
    transport_tx: mpsc::Sender<TransportMessage>,
) -> Result<InterfaceHandle, PluginSpawnError> {
    let library = load_configured_library(&config.plugin)?;
    spawn_plugin_interface_with_library(config, id, transport_tx, library).await
}

async fn spawn_plugin_interface_with_library(
    config: PluginInterfaceConfig,
    id: InterfaceId,
    transport_tx: mpsc::Sender<TransportMessage>,
    library: Arc<PluginLibrary>,
) -> Result<InterfaceHandle, PluginSpawnError> {
    let functions = library.functions();
    let online = Arc::new(AtomicBool::new(false));
    let bitrate = Arc::new(AtomicU64::new(0));
    let rxb = Arc::new(AtomicU64::new(0));
    let txb = Arc::new(AtomicU64::new(0));
    let context = Arc::new(PluginHostContext {
        id,
        name: config.name.clone(),
        transport_tx,
        online: online.clone(),
        bitrate: bitrate.clone(),
        rxb: rxb.clone(),
        activated: AtomicBool::new(false),
        early_rx: Mutex::new(VecDeque::new()),
    });

    HOST_CONTEXTS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .insert(id, Arc::downgrade(&context));

    let (tx, mut rx) = mpsc::channel::<Bytes>(TX_QUEUE_SIZE);
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
    let worker_name = config.name.clone();
    let config_yaml = config.config_yaml;
    let worker_mtu = config.mtu as usize;
    let worker_online = online.clone();
    let worker_txb = txb.clone();
    std::thread::Builder::new()
        .name(format!("rns-plugin-{id}"))
        .spawn(move || {
            let host_api = Box::new(HostApi {
                abi_major: ABI_MAJOR,
                abi_minor: ABI_MINOR,
                struct_size: HOST_API_V1_0_SIZE as u32,
                reserved0: 0,
                host_context: Arc::as_ptr(&context) as *mut c_void,
                log: Some(host_log),
                set_bitrate: Some(host_set_bitrate),
                set_online: Some(host_set_online),
                rx_packet: Some(host_rx_packet),
            });
            let mut raw: *mut PluginInstance = std::ptr::null_mut();
            let config_pointer = if config_yaml.is_empty() {
                std::ptr::null()
            } else {
                config_yaml.as_ptr()
            };
            // SAFETY: The library functions and mandatory ABI were validated;
            // host_api, context, and config stay valid for the required calls.
            let result = unsafe {
                (functions.create)(
                    host_api.as_ref(),
                    config_pointer,
                    config_yaml.len(),
                    &mut raw,
                )
            };
            let startup_error = if result != OK {
                Some("create returned ERROR".to_string())
            } else if raw.is_null() {
                Some("create returned OK with a null instance".to_string())
            } else if bitrate.load(Ordering::Acquire) == 0 {
                Some("create did not report a non-zero bitrate".to_string())
            } else if !worker_online.load(Ordering::Acquire) {
                Some("create did not report the instance online".to_string())
            } else {
                None
            };
            if let Some(message) = startup_error {
                if !raw.is_null() {
                    // SAFETY: A non-null instance returned by create belongs
                    // to this plugin even when the return contract was broken.
                    unsafe { (functions.destroy)(raw) };
                }
                worker_online.store(false, Ordering::Release);
                let _ = started_tx.send(Err(message));
                HOST_CONTEXTS
                    .get()
                    .expect("plugin host registry initialized")
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .remove(&id);
                let _ = finished_tx.send(());
                return;
            }

            let _ = started_tx.send(Ok(bitrate.load(Ordering::Acquire)));
            while let Some(data) = rx.blocking_recv() {
                if !worker_online.load(Ordering::Acquire) {
                    continue;
                }
                if data.len() > worker_mtu {
                    tracing::error!(
                        interface_id = id,
                        interface_name = %context.name,
                        packet_size = data.len(),
                        mtu = worker_mtu,
                        "dropping plugin TX packet larger than configured MTU"
                    );
                    continue;
                }
                // SAFETY: raw is a live instance; data remains borrowed until
                // synchronous send returns, and calls are serialized here.
                let result = unsafe { (functions.send)(raw, data.as_ptr(), data.len()) };
                if result == OK {
                    worker_txb.fetch_add(data.len() as u64, Ordering::Relaxed);
                }
            }
            // SAFETY: The TX channel is closed, no send is active, and destroy
            // is called exactly once on the worker that owns the instance.
            unsafe { (functions.destroy)(raw) };
            worker_online.store(false, Ordering::Release);
            HOST_CONTEXTS
                .get()
                .expect("plugin host registry initialized")
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .remove(&id);
            let _ = finished_tx.send(());
            drop(host_api);
            drop(library);
        })
        .map_err(|error| PluginSpawnError::Create {
            name: worker_name.clone(),
            message: format!("cannot start TX worker: {error}"),
        })?;

    let initial_bitrate = started_rx
        .recv()
        .map_err(|_| PluginSpawnError::Create {
            name: worker_name.clone(),
            message: "TX worker exited during create".to_string(),
        })?
        .map_err(|message| PluginSpawnError::Create {
            name: worker_name,
            message,
        })?;
    let read_task = tokio::spawn(async move {
        let _ = finished_rx.await;
    });

    Ok(InterfaceHandle {
        id,
        parent_id: None,
        name: config.name,
        mode: config.mode,
        direction: InterfaceDirection {
            inbound: true,
            outbound: true,
            forward: false,
            repeat: false,
        },
        bitrate: initial_bitrate,
        mtu: config.mtu,
        online,
        rxb: Some(rxb),
        txb: Some(txb),
        tx,
        read_task,
    })
}

fn copy_info_string(
    path: &Path,
    field: &'static str,
    value: RnsString,
    maximum: usize,
) -> Result<String, PluginLoadError> {
    if value.data.is_null() || value.len == 0 || value.len > maximum {
        return Err(PluginLoadError::InvalidInfoLength {
            path: path.to_path_buf(),
            field,
            length: value.len,
        });
    }
    // SAFETY: The validated ABI requires each info pointer to reference len
    // readable bytes for the lifetime of the library. A malicious native
    // plugin remains capable of violating this in an in-process architecture.
    let bytes = unsafe { std::slice::from_raw_parts(value.data, value.len) };
    let text = std::str::from_utf8(bytes).map_err(|_| PluginLoadError::InvalidInfoUtf8 {
        path: path.to_path_buf(),
        field,
    })?;
    Ok(text.to_owned())
}

pub fn validate_plugin_name(name: &str) -> Result<(), PluginLoadError> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(PluginLoadError::InvalidName(name.to_owned()));
    }
    Ok(())
}

pub fn configured_plugin_path(name: &str) -> Result<PathBuf, PluginLoadError> {
    validate_plugin_name(name)?;
    Ok(Path::new(PLUGIN_DIRECTORY).join(format!("{name}.so")))
}

pub fn canonical_plugin_path(path: &Path) -> Result<PathBuf, PluginLoadError> {
    let canonical = fs::canonicalize(path).map_err(|source| PluginLoadError::Canonicalize {
        path: path.to_path_buf(),
        source,
    })?;
    let directory =
        fs::canonicalize(PLUGIN_DIRECTORY).map_err(|source| PluginLoadError::Canonicalize {
            path: PathBuf::from(PLUGIN_DIRECTORY),
            source,
        })?;
    if !canonical.starts_with(&directory) {
        return Err(PluginLoadError::OutsidePluginDirectory(canonical));
    }
    Ok(canonical)
}

pub fn is_shared_library(path: &Path) -> bool {
    path.extension() == Some(OsStr::new("so"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_loopback_plugin() -> PathBuf {
        static PLUGIN: OnceLock<PathBuf> = OnceLock::new();
        PLUGIN
            .get_or_init(|| {
                let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../rns-plugin");
                let status = std::process::Command::new("make")
                    .arg("-C")
                    .arg(&crate_dir)
                    .arg("all")
                    .status()
                    .expect("run make for C loopback plugin");
                assert!(status.success());
                crate_dir.join("build/loopback.so")
            })
            .clone()
    }

    #[test]
    fn accepts_simple_plugin_names() {
        for name in ["sx126x", "lora-spi", "PLUGIN_2"] {
            assert!(validate_plugin_name(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn rejects_paths_suffixes_unicode_and_empty_names() {
        for name in ["", "../evil", "radio.so", "a/b", "радио", "."] {
            assert!(validate_plugin_name(name).is_err(), "{name}");
        }
    }

    #[test]
    fn configured_name_maps_directly_to_so_filename() {
        assert_eq!(
            configured_plugin_path("sx126x").unwrap(),
            PathBuf::from("/usr/lib/reticulum-rs/sx126x.so")
        );
    }

    #[test]
    fn detects_only_so_suffix() {
        assert!(is_shared_library(Path::new("loopback.so")));
        assert!(!is_shared_library(Path::new("loopback.so.1")));
        assert!(!is_shared_library(Path::new("README")));
    }

    #[tokio::test]
    async fn c_loopback_runs_through_adapter_lifecycle() {
        let library = Arc::new(PluginLibrary::load(build_loopback_plugin()).unwrap());
        let (transport_tx, mut transport_rx) = mpsc::channel(16);
        let mut config = PluginInterfaceConfig::new("C loopback", "loopback");
        config.config_yaml = b"{}\n".to_vec();
        let handle = spawn_plugin_interface_with_library(config, 42, transport_tx, library)
            .await
            .unwrap();
        assert_eq!(handle.bitrate, 1_000_000_000);
        assert!(handle.online.load(Ordering::Acquire));
        activate_plugin_interface(42);

        handle.tx.send(Bytes::from_static(b"packet")).await.unwrap();
        let inbound = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Some(TransportMessage::Inbound(packet)) = transport_rx.recv().await {
                    break packet;
                }
            }
        })
        .await
        .expect("loopback RX timeout");
        assert_eq!(inbound.interface_id, 42);
        assert_eq!(inbound.raw.as_ref(), b"packet");
        assert_eq!(inbound.rssi, None);
        assert_eq!(inbound.snr, None);

        let online = handle.online.clone();
        let read_task = handle.read_task;
        drop(handle.tx);
        tokio::time::timeout(std::time::Duration::from_secs(2), read_task)
            .await
            .expect("plugin worker shutdown timeout")
            .expect("plugin read task panicked");
        assert!(!online.load(Ordering::Acquire));
    }
}
