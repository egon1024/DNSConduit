//! Backend health runtime: per-backend state side-table and probe
//! construction/validation (phase 1c, design §D2/§D4/§D9).
//!
//! Probe *configuration* is compiled in `conduit-config` and stored in the
//! immutable snapshot; the mutable *state* defined here lives outside the
//! snapshot and is read lock-free by workers.

pub mod control;
pub mod probe;
pub mod scrape;
pub mod state;

pub use control::{
    BackendHealthFilter, BackendHealthView, EffectiveScope, HealthControlAction,
    HealthControlOutcome, HealthControlScope, ScopeMode,
};
pub use probe::{ProbeOutcome, ProbeSpec};
pub use scrape::{build_health_scrape, BackendHealthScrapeRow};
pub use state::{BackendHealthState, BackendKey, Health, HealthRegistry, HealthTable};
