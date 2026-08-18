//! Runtime-ready normalized configuration.
//!
//! YAML parsing, defaults and validation belong to [`crate::config`]. This
//! module contains only the internal representation consumed by runtime
//! components and the atomic writer used by the configuration API.

use std::collections::HashMap;
#[cfg(feature = "api")]
use std::io::Write;
#[cfg(feature = "api")]
use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid value for [{section}] {key}: {message}")]
    InvalidValue {
        section: String,
        key: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedValue {
    Scalar(String),
    List(Vec<String>),
}

#[derive(Debug, Clone, Default)]
pub struct NormalizedSection {
    pub values: HashMap<String, NormalizedValue>,
    pub subsections: HashMap<String, NormalizedSection>,
    subsection_order: Vec<String>,
}

impl NormalizedSection {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        match self.values.get(key) {
            Some(NormalizedValue::Scalar(value)) => Some(value),
            _ => None,
        }
    }
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.get(key)?.to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Some(true),
            "false" | "no" | "off" | "0" => Some(false),
            _ => None,
        }
    }
    pub fn get_int(&self, key: &str) -> Option<i64> {
        self.get(key)?.parse().ok()
    }
    pub fn get_uint(&self, key: &str) -> Option<u64> {
        self.get(key)?.parse().ok()
    }
    pub fn get_float(&self, key: &str) -> Option<f64> {
        self.get(key)?.parse().ok()
    }
    pub fn get_list(&self, key: &str) -> Option<Vec<String>> {
        match self.values.get(key)? {
            NormalizedValue::Scalar(value) => Some(vec![value.clone()]),
            NormalizedValue::List(values) => Some(values.clone()),
        }
    }
    pub fn get_hex(&self, key: &str) -> Option<Vec<u8>> {
        hex::decode(self.get(key)?).ok()
    }
    pub fn has(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }
    pub fn set(&mut self, key: &str, value: &str) {
        self.values
            .insert(key.to_string(), NormalizedValue::Scalar(value.to_string()));
    }
    pub fn set_list(&mut self, key: &str, value: Vec<String>) {
        self.values
            .insert(key.to_string(), NormalizedValue::List(value));
    }
    pub fn remove(&mut self, key: &str) -> bool {
        self.values.remove(key).is_some()
    }
    pub fn add_subsection(&mut self, name: String) -> &mut NormalizedSection {
        if !self.subsections.contains_key(&name) {
            self.subsection_order.push(name.clone());
        }
        self.subsections.entry(name).or_default()
    }
}

#[derive(Debug, Clone, Default)]
pub struct NormalizedConfig {
    pub sections: HashMap<String, NormalizedSection>,
}

impl NormalizedConfig {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn section(&self, name: &str) -> Option<&NormalizedSection> {
        self.sections.get(name)
    }
    pub fn subsection(&self, section: &str, subsection: &str) -> Option<&NormalizedSection> {
        self.section(section)?.subsections.get(subsection)
    }
    pub fn subsections(&self, section: &str) -> Vec<(&str, &NormalizedSection)> {
        let Some(section) = self.section(section) else {
            return Vec::new();
        };
        section
            .subsection_order
            .iter()
            .filter_map(|name| {
                section
                    .subsections
                    .get(name)
                    .map(|value| (name.as_str(), value))
            })
            .collect()
    }
    pub fn ensure_section(&mut self, name: &str) -> &mut NormalizedSection {
        self.sections.entry(name.to_string()).or_default()
    }
}

#[cfg(feature = "api")]
pub(crate) fn atomic_write(path: &Path, content: &[u8]) -> Result<(), ConfigError> {
    let target = match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::canonicalize(path)?,
        _ => path.to_path_buf(),
    };
    let parent = target.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "config path has no parent directory",
        )
    })?;
    let file_name = target.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "config path has no file name",
        )
    })?;
    let permissions = std::fs::metadata(&target)
        .ok()
        .map(|value| value.permissions());
    for attempt in 0..32_u8 {
        let temporary = parent.join(format!(
            ".{}.tmp.{}.{}",
            file_name.to_string_lossy(),
            std::process::id(),
            attempt
        ));
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        };
        let result = (|| -> Result<(), std::io::Error> {
            if let Some(permissions) = permissions.clone() {
                file.set_permissions(permissions)?;
            }
            file.write_all(content)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, &target)?;
            #[cfg(unix)]
            std::fs::File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        return result.map_err(Into::into);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "temporary file collision",
    )
    .into())
}
