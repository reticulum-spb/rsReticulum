#![no_std]
//! Stable C ABI shared by rsReticulum and in-process interface plugins.
//!
//! This crate intentionally contains definitions only. It has no allocator,
//! async runtime, configuration parser, loader, or Reticulum dependencies.

use core::ffi::{c_char, c_void};
use core::mem::{offset_of, size_of};

pub const ABI_MAJOR: u32 = 1;
pub const ABI_MINOR: u32 = 0;

pub type PluginResult = i32;
pub const OK: PluginResult = 0;
pub const ERROR: PluginResult = -1;

pub type LogLevel = i32;
pub const LOG_ERROR: LogLevel = 1;
pub const LOG_WARN: LogLevel = 2;
pub const LOG_INFO: LogLevel = 3;
pub const LOG_DEBUG: LogLevel = 4;
pub const LOG_TRACE: LogLevel = 5;

pub const RX_METADATA_RSSI: u32 = 1 << 0;
pub const RX_METADATA_SNR: u32 = 1 << 1;

pub const PLUGIN_INFO_NAME_MAX_SIZE: usize = 128;
pub const PLUGIN_INFO_VERSION_MAX_SIZE: usize = 64;
pub const PLUGIN_INFO_DESCRIPTION_MAX_SIZE: usize = 4096;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RxMetadata {
    pub valid_fields: u32,
    pub rssi_dbm: i16,
    pub snr_db: i16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RnsString {
    pub data: *const u8,
    pub len: usize,
}

#[repr(C)]
pub struct PluginInfo {
    pub name: RnsString,
    pub version: RnsString,
    pub description: RnsString,
}

pub const PLUGIN_INFO_V1_0_SIZE: usize =
    offset_of!(PluginInfo, description) + size_of::<RnsString>();

pub type LogFn = unsafe extern "C" fn(
    host_context: *mut c_void,
    level: LogLevel,
    message: *const u8,
    message_len: usize,
);
pub type SetBitrateFn = unsafe extern "C" fn(host_context: *mut c_void, bitrate_bps: u64);
pub type SetOnlineFn = unsafe extern "C" fn(host_context: *mut c_void, online: u8);
pub type RxPacketFn = unsafe extern "C" fn(
    host_context: *mut c_void,
    data: *const u8,
    data_len: usize,
    metadata: *const RxMetadata,
    metadata_size: usize,
);

#[repr(C)]
pub struct HostApi {
    pub abi_major: u32,
    pub abi_minor: u32,
    pub struct_size: u32,
    pub reserved0: u32,
    pub host_context: *mut c_void,
    pub log: Option<LogFn>,
    pub set_bitrate: Option<SetBitrateFn>,
    pub set_online: Option<SetOnlineFn>,
    pub rx_packet: Option<RxPacketFn>,
}

pub const HOST_API_V1_0_SIZE: usize =
    offset_of!(HostApi, rx_packet) + size_of::<Option<RxPacketFn>>();

#[repr(C)]
pub struct PluginInstance {
    _private: [u8; 0],
}

pub type CreateFn = unsafe extern "C" fn(
    host: *const HostApi,
    config_yaml: *const u8,
    config_len: usize,
    out_plugin: *mut *mut PluginInstance,
) -> PluginResult;
pub type SendFn = unsafe extern "C" fn(
    plugin: *mut PluginInstance,
    data: *const u8,
    data_len: usize,
) -> PluginResult;
pub type DestroyFn = unsafe extern "C" fn(plugin: *mut PluginInstance);

#[repr(C)]
pub struct PluginApi {
    pub abi_major: u32,
    pub abi_minor: u32,
    pub struct_size: u32,
    pub reserved0: u32,
    pub info: *const PluginInfo,
    pub info_size: usize,
    pub create: Option<CreateFn>,
    pub send: Option<SendFn>,
    pub destroy: Option<DestroyFn>,
}

pub const PLUGIN_API_V1_0_SIZE: usize =
    offset_of!(PluginApi, destroy) + size_of::<Option<DestroyFn>>();

pub type GetApiFn = unsafe extern "C" fn() -> *const PluginApi;
pub const GET_API_SYMBOL: &[u8] = b"rns_plugin_get_api\0";

// Kept public for bindings which declare the exported symbol themselves.
pub type CChar = c_char;

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rx_metadata_has_fixed_layout() {
        assert_eq!(size_of::<RxMetadata>(), 8);
        assert_eq!(offset_of!(RxMetadata, valid_fields), 0);
        assert_eq!(offset_of!(RxMetadata, rssi_dbm), 4);
        assert_eq!(offset_of!(RxMetadata, snr_db), 6);
    }

    #[test]
    fn v1_minimum_sizes_end_at_last_required_field() {
        assert_eq!(PLUGIN_INFO_V1_0_SIZE, size_of::<PluginInfo>());
        assert_eq!(HOST_API_V1_0_SIZE, size_of::<HostApi>());
        assert_eq!(PLUGIN_API_V1_0_SIZE, size_of::<PluginApi>());
    }

    #[test]
    fn optional_function_pointer_uses_pointer_layout() {
        assert_eq!(size_of::<Option<LogFn>>(), size_of::<*const c_void>());
        assert_eq!(size_of::<Option<CreateFn>>(), size_of::<*const c_void>());
    }
}
