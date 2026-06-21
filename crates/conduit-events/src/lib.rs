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
    compile_from_config, parse_connect_retry, parse_destination, parse_extra_field,
    parse_extra_fields, parse_extra_tags, parse_sample_percent, parse_sink_filters,
    resolve_sink_identity, validate_sink_identity_uniqueness, CompiledEvents, CompiledSinkFilters,
    CompiledSinkInstance, Destination, ExtraField, SinkIdentity, TagExportMode, EXTRA_FIELD_NAMES,
};
pub use connect_retry::{BackoffState, ConnectRetryConfig};
pub use event::{EventKind, ExportEvent};
pub use hub::EventHub;
pub use metrics::{SinkMetrics, SinkMetricsSnapshot};
pub use queue::DropPolicy;
pub use selectors::{
    compile_rule_selectors, compile_sample_key_fields, compile_selectors,
    compile_selectors_with_ctx, compile_sink_selectors, hash_sample, hash_sample_keyed,
    matches_every_nth_global, matches_every_nth_worker, parse_every_nth,
    parse_sample_percent as parse_selector_sample_percent, resolve_sample_key,
    validate_non_rule_selector_type, validate_sample_key_from, validate_selector_sample_key_fields,
    validate_selector_type, validate_top_level_sample_key_fields, CompiledSelector, PercentKey,
    SampleKey, SelectorCompileCtx, SelectorMatchCtx, NON_RULE_SELECTOR_TYPES,
    SAMPLE_KEY_FROM_QNAME, SAMPLE_KEY_FROM_RULE_NAME, SAMPLE_KEY_FROM_SINK_NAME, SELECTOR_TYPES,
};
pub use sink::EventSink;
pub use view::{TxnExtraSource, TxnView};
