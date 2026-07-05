//! Lookup phase: ordered providers (cache, forward, …) on one spine.

mod outcome;
mod stage;

pub use outcome::{AnswerSource, LookupOutcome};
pub use stage::{LookupForwardStep, LookupStage};
