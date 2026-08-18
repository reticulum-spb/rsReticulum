//! Linux shared-library loader for interface plugins.

use std::ffi::OsStr;
use std::fs;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};

use libloading::{Library, Symbol};
use rns_plugin::{
    ABI_MAJOR, GET_API_SYMBOL, GetApiFn, PLUGIN_API_V1_0_SIZE, PLUGIN_INFO_DESCRIPTION_MAX_SIZE,
    PLUGIN_INFO_NAME_MAX_SIZE, PLUGIN_INFO_V1_0_SIZE, PLUGIN_INFO_VERSION_MAX_SIZE, PluginApi,
    PluginInfo, RnsString,
};
use thiserror::Error;

pub const PLUGIN_DIRECTORY: &str = "/usr/lib/reticulum-rs";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedPluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
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
        let library = unsafe { Library::new(&path) }.map_err(|error| PluginLoadError::Open {
            path: path.clone(),
            message: error.to_string(),
        })?;

        // SAFETY: The symbol name and C signature are fixed by the public ABI.
        let get_api: Symbol<'_, GetApiFn> =
            unsafe { library.get(GET_API_SYMBOL) }.map_err(|error| {
                PluginLoadError::MissingEntryPoint {
                    path: path.clone(),
                    message: error.to_string(),
                }
            })?;
        // SAFETY: A plugin must return a static table or null. No reference is
        // formed until the pointer and mandatory prefix have been validated.
        let api_ptr = unsafe { get_api() };
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
            _library: ManuallyDrop::new(library),
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
}
