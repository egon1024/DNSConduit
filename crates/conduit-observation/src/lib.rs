//! Observation queue, hub, and dnstap sink (phase 2).

pub mod compile;
pub mod dnstap;
pub mod event;
pub mod extra;
pub mod fstrm;
pub mod hub;
pub mod queue;
pub mod sink;
pub mod view;

pub use compile::{
    compile_from_config, parse_extra_field, parse_extra_fields, parse_extra_tags,
    CompiledObservation, CompiledSinkInstance, Destination, ExtraField, TagExportMode,
    EXTRA_FIELD_NAMES,
};
pub use event::{EventKind, ObservationEvent};
pub use hub::ObservationHub;
pub use queue::DropPolicy;
pub use sink::ObservationSink;
pub use view::{TxnExtraSource, TxnView};
