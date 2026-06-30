//! Active backend health probing (phase 1c, design §D5/§D6).
//!
//! [`scheduler`] holds the pure, deterministically-testable scheduling core;
//! [`run`] wraps it with the real non-blocking, multiplexed socket I/O and the
//! startup spawn hook. Phase A writes health state only — routing does not read
//! it yet.

pub mod run;
pub mod scheduler;

pub use run::spawn_probe_loop;
