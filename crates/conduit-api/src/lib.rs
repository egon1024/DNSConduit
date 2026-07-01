//! gRPC control plane server.

pub mod access_log;
pub mod auth;
pub mod health;
pub mod server;
pub mod tls;

pub use health::BackendHealthService;
pub use server::{serve, serve_on_listener, spawn_control_plane, ControlHandle, ControlService};
