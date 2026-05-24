//! Observation queue, hub, and dnstap sink (phase 2).

pub mod compile;
pub mod connect_retry;
pub mod dnstap;
pub mod event;
pub mod extra;
pub mod filters;
pub mod fstrm;
pub mod hub;
pub mod metrics;
pub mod queue;
pub mod selectors;
pub mod sink;
pub mod view;

pub use compile::{
    compile_from_config, parse_connect_retry, parse_extra_field, parse_extra_fields,
    parse_extra_tags, parse_sample_rate, parse_sink_filters, resolve_sink_identity,
    validate_sink_identity_uniqueness, CompiledObservation, CompiledSinkFilters,
    CompiledSinkInstance, Destination, ExtraField, SinkIdentity, TagExportMode, EXTRA_FIELD_NAMES,
};
pub use connect_retry::{BackoffState, ConnectRetryConfig};
pub use event::{EventKind, ObservationEvent};
pub use hub::ObservationHub;
pub use metrics::{SinkMetrics, SinkMetricsSnapshot};
pub use queue::DropPolicy;
pub use selectors::{
    compile_selectors, hash_sample, validate_selector_type, CompiledSelector, SelectorMatchCtx,
    SELECTOR_TYPES,
};
pub use sink::ObservationSink;
pub use view::{TxnExtraSource, TxnView};
