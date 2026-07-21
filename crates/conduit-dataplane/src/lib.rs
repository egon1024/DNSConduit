//! DNS dataplane: listeners, forward, and I/O pipeline stages.

pub mod acl_gate;
mod cache_reaper;
pub mod drain;
pub mod forward;
pub mod listener;
pub mod metrics_scrape;
pub mod probe;
pub mod query_slot;
pub mod runtime;
pub mod stages;

pub use drain::{drain_slots, DrainFilter, DrainOutcome, DEFAULT_DRAIN_TIMEOUT};
pub use listener::{supervisor, DataplaneShutdown};
pub use runtime::{start, DataplaneHandle};
