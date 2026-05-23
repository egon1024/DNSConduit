//! Observation queue, hub, and dnstap sink (phase 2).

pub mod compile;
pub mod dnstap;
pub mod event;
pub mod hub;
pub mod queue;
pub mod sink;
pub mod view;

pub use compile::{compile_from_config, CompiledObservation, CompiledSinkInstance, Destination};
pub use event::{EventKind, ObservationEvent};
pub use hub::ObservationHub;
pub use queue::DropPolicy;
pub use sink::ObservationSink;
pub use view::TxnView;
