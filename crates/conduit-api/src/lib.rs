//! gRPC control plane server.

pub mod server;

pub use server::{serve, serve_on_listener, ControlService};
