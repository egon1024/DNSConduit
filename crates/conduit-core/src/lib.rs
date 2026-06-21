//! Core runtime: snapshots, transactions, pipeline traits.

pub mod clock;
pub mod configurator;
pub mod event_emit;
pub mod orchestrator;
pub mod parse_reject;
pub mod phase;
pub mod pipeline;
pub mod routing;
pub mod rules;
pub mod script_host;
pub mod snapshot;
pub mod stages;
pub mod transaction;
pub mod upstream_response;

pub use clock::{Clock, SystemClock};
pub use conduit_config::forward::{
    CompiledForward, CompiledPoolForward, DEFAULT_SOURCE_SELECTION, MAX_SOURCES_V4,
};
pub use configurator::{
    spawn as spawn_configurator, ApplyResult, ConfiguratorHandle, ConfiguratorSpawn,
    ConfiguratorState, OverlayApplyMode, PolicyProposal, ProposalSource,
};
pub use orchestrator::{Orchestrator, RunOutcome, StageRegistry};
pub use parse_reject::ParseRejectReason;
pub use phase::Phase;
pub use pipeline::{PipelineStage, StageOutcome};
pub use routing::{select_backend, AttemptRecord};
pub use rules::CompiledRules;
pub use snapshot::{RuntimeSnapshot, SnapshotStore};
pub use transaction::{ClientProtocol, TagSet, Transaction};
pub use upstream_response::record_upstream_response;
