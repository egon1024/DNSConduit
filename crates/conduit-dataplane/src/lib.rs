//! DNS dataplane: listeners, forward, and I/O pipeline stages.

pub mod forward;
pub mod listener;
pub mod metrics_scrape;
pub mod query_slot;
pub mod runtime;
pub mod stages;

pub use listener::{supervisor, DataplaneShutdown};
pub use runtime::{start, DataplaneHandle};
