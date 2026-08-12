//! Library surface for `conduitctl` (shared connect + client config).

pub mod client_config;
pub mod connect;

pub use client_config::{
    default_client_config_path, default_endpoint, load_client_config, resolve_connect,
    ClientFileConfig, ConnectCliOverrides, ResolvedConnect,
};
pub use connect::{auth_metadata, connect_channel, with_auth};
