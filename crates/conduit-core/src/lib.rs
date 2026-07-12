//! Core runtime: snapshots, transactions, pipeline traits.

pub mod clock;
pub mod configurator;
pub mod event_emit;
pub mod health;
pub mod lookup;
pub mod orchestrator;
pub mod parse_reject;
pub mod phase;
pub mod pipeline;
pub mod routing;
pub mod rules;
pub mod runtime_view;
pub mod script_host;
pub mod selector_ctx;
pub mod snapshot;
pub mod stages;
pub mod structural_parse;
pub mod transaction;
pub mod txn_store;
pub mod upstream_response;

pub use clock::{Clock, SystemClock};
pub use conduit_config::forward::{
    CompiledForward, CompiledPoolForward, DEFAULT_SOURCE_SELECTION, MAX_SOURCES_V4,
};
pub use configurator::{
    spawn as spawn_configurator, ApplyResult, ConfiguratorHandle, ConfiguratorSpawn,
    ConfiguratorState, OverlayApplyMode, PolicyProposal, ProposalSource,
};
pub use health::{
    BackendHealthState, BackendKey, Health, HealthControlAction, HealthControlScope,
    HealthRegistry, HealthTable, ProbeOutcome, ProbeSpec,
};
pub use lookup::{
    AnswerSource, LookupCacheRegistry, LookupForwardStep, LookupOutcome, LookupStage, ReapBudget,
};
pub use orchestrator::{Orchestrator, OrchestratorRun, RunOutcome, StageRegistry};
pub use parse_reject::ParseRejectReason;
pub use phase::{Phase, PipelineAttachment};
pub use pipeline::{PipelineStage, StageOutcome};
pub use routing::{
    backend_metric_label, backend_metric_label_for_addr, select_backend, AttemptRecord,
    PoolHealthView,
};
pub use rules::CompiledRules;
pub use runtime_view::build_routing_runtime_snapshot;
pub use snapshot::{RuntimeSnapshot, SnapshotStore};
pub use structural_parse::{apply_parsed_query, structural_parse, ParsedQuery};
pub use transaction::{ClientProtocol, TagSet, Transaction};
pub use txn_store::{
    AcquireError, SharedTxnStore, SlotError, SlotId, SlotState, TxnSlot, TxnStore, WireBufferError,
    DEFAULT_SLOT_CHUNK_SIZE, DNS_WIRE_BUFFER_SIZE,
};
pub use upstream_response::record_upstream_response;
