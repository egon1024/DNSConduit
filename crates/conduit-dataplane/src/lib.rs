//! DNS dataplane: listeners, forward, and I/O pipeline stages.

pub mod forward;
pub mod listener;
pub mod metrics_scrape;
pub mod stages;

pub use listener::supervisor;
