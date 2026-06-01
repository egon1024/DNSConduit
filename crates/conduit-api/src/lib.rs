//! gRPC control plane server.

pub mod auth;
pub mod server;
pub mod tls;

pub use server::{serve, serve_on_listener, ControlService};
