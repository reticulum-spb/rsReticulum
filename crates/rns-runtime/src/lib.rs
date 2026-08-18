//! Reticulum runtime: config, lifecycle, RPC, and the [`reticulum::ReticulumHandle`]
//! that user code holds. Python reference: `RNS/Reticulum.py`.

#[cfg(not(any(feature = "client", feature = "full")))]
compile_error!("rns-runtime requires either the `client` or `full` feature");

#[cfg(feature = "api")]
pub mod api_server;
pub mod application;
#[path = "yaml_config.rs"]
pub mod config;
#[path = "config.rs"]
pub(crate) mod config_compat;
pub mod constants;
#[cfg(feature = "full")]
pub mod interface_factory;
#[cfg(feature = "full")]
pub mod jobs;
pub mod lifecycle;
pub mod link_client;
pub mod link_manager;
pub mod link_session;
pub mod platform;
#[cfg(feature = "full")]
pub mod probe;
#[cfg(feature = "full")]
pub mod remote_management;
#[cfg(feature = "full")]
pub mod remote_management_schema;
#[cfg(feature = "full")]
pub mod reticulum;
#[cfg(all(feature = "client", not(feature = "full")))]
#[path = "reticulum_client.rs"]
pub mod reticulum;
#[cfg(feature = "full")]
pub mod rncp;
#[cfg(feature = "full")]
pub mod rnsh;
pub mod rpc;
#[cfg(feature = "full")]
pub mod rpc_server;
#[cfg(feature = "api")]
pub mod web_logs;
