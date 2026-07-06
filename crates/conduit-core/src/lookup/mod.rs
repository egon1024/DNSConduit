//! Lookup phase: ordered providers (cache, forward, …) on one spine.

pub mod cache;
mod outcome;
mod stage;

pub use cache::LookupCacheRegistry;
pub use outcome::{AnswerSource, LookupOutcome};
pub use stage::{LookupForwardStep, LookupStage};
