//! Core runtime: snapshots, transactions, pipeline traits.

pub mod clock;
pub mod phase;
pub mod pipeline;
pub mod snapshot;
pub mod transaction;

pub use clock::{Clock, SystemClock};
pub use phase::Phase;
pub use pipeline::{PipelineStage, StageOutcome};
pub use snapshot::{RuntimeSnapshot, SnapshotStore};
pub use transaction::{TagSet, Transaction};
