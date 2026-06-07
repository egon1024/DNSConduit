//! gRPC control plane server.

pub mod access_log;
pub mod auth;
pub mod server;
pub mod tls;

pub use server::{serve, serve_on_listener, spawn_control_plane, ControlHandle, ControlService};
