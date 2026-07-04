//! Per-backend runtime health state (the side-table, design §D2/§D9).
//!
//! This state is **not** part of the immutable [`crate::snapshot::RuntimeSnapshot`].
//! Probe *configuration* lives in the snapshot; probe *state* lives here and is
//! reconciled across snapshot swaps. Each backend carries `observed_health`
//! (probe truth) and `applied_health` (what Route reads). When not frozen,
//! `applied` tracks `observed`; while frozen, `applied` holds and `observed`
//! keeps updating; unfreeze snaps `applied := observed`.
//!
//! ## Concurrency (for readers new to Rust)
//!
//! Each backend's state is stored in atomics (`AtomicU8`, `AtomicU32`,
//! `AtomicU64`, `AtomicBool`). Workers read it **lock-free** — no mutex, just
//! atomic loads — which is the same "read cheaply, never block the hot path"
//! goal as the `arc-swap` config snapshot. Probe-driven mutations come from the
//! single probe loop (one writer); operator controls (a later phase) serialize
//! through the control plane. The whole backend table is published via
//! [`arc_swap::ArcSwap`] so a reload can swap in a reconciled table while
//! readers keep using the old one until they reload.

use conduit_config::health::{CompiledBackendHealth, CompiledHealth, InitialHealthState};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, RwLock};

/// Liveness of a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// No definite probe verdict yet.
    Unknown,
    /// Probes (or a manual set) consider the backend alive.
    Up,
    /// Probes (or a manual set) consider the backend dead.
    Down,
}

impl Health {
    fn to_u8(self) -> u8 {
        match self {
            Health::Unknown => 0,
            Health::Up => 1,
            Health::Down => 2,
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            1 => Health::Up,
            2 => Health::Down,
            _ => Health::Unknown,
        }
    }

    /// Prometheus gauge encoding: 0 = unknown, 1 = up, 2 = down.
    pub fn as_metric_value(self) -> f64 {
        self.to_u8() as f64
    }
}

/// Outcome of a passive (live-traffic) failure recorded against a backend.
/// Returned by [`BackendHealthState::record_passive_failure`] so callers can
/// emit appropriately-tiered logs with full query context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PassiveFailureResult {
    /// Resulting `applied` health after this failure.
    pub applied: Health,
    /// Passive failure count *after* this failure (1-based).
    pub consecutive_failures: u32,
    /// Configured threshold (`passive_fall`).
    pub passive_fall: u32,
    /// True when this failure crossed the threshold and moved `observed` to Down.
    pub transitioned: bool,
    /// True when the backend was already `observed == Down` before this failure
    /// (post-trip in-flight query completing).
    pub already_down: bool,
}

/// Identity of a backend within the running config. Includes the pool so the
/// same address in two pools is two independent health entries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BackendKey {
    pub pool: String,
    pub address: SocketAddr,
}

impl BackendKey {
    pub fn new(pool: impl Into<String>, address: SocketAddr) -> Self {
        Self {
            pool: pool.into(),
            address,
        }
    }
}

const EWMA_UNSET: u64 = u64::MAX; // sentinel: no latency sample yet

/// Neutral effective-weight factor: latency weighting has not reduced this
/// backend's share (it routes at its configured weight).
const WEIGHT_FACTOR_NEUTRAL: f64 = 1.0;

/// Atomic per-backend health state. Cheap, lock-free reads for routing.
#[derive(Debug)]
pub struct BackendHealthState {
    observed: AtomicU8,
    applied: AtomicU8,
    frozen: AtomicBool,
    last_transition_unix_ms: AtomicU64,
    consecutive_successes: AtomicU32,
    consecutive_failures: AtomicU32,
    passive_consecutive_failures: AtomicU32,
    latency_ewma_ms_bits: AtomicU64,
    /// Damped latency effective-weight factor in `[floor, 1.0]` (design §D3).
    /// Maintained by the probe loop and read lock-free at Route; `1.0` means no
    /// latency reduction.
    weight_factor_bits: AtomicU64,
    /// Cumulative observed/applied transitions (for Prometheus export).
    transitions_total: AtomicU64,
}

impl BackendHealthState {
    /// Build state from the pool's `initial_state` policy (design §D10).
    ///
    /// `observed` always starts `Unknown` (no probe has run yet). `applied`
    /// starts eligible (`Up`) only under the optimistic policy; the
    /// require-* policies start ineligible (`Down`) until probes earn it.
    pub fn from_initial_policy(initial: InitialHealthState) -> Self {
        let applied = match initial {
            InitialHealthState::Optimistic => Health::Up,
            InitialHealthState::Require1Good | InitialHealthState::RequireFullRise => Health::Down,
        };
        Self {
            observed: AtomicU8::new(Health::Unknown.to_u8()),
            applied: AtomicU8::new(applied.to_u8()),
            frozen: AtomicBool::new(false),
            last_transition_unix_ms: AtomicU64::new(0),
            consecutive_successes: AtomicU32::new(0),
            consecutive_failures: AtomicU32::new(0),
            passive_consecutive_failures: AtomicU32::new(0),
            latency_ewma_ms_bits: AtomicU64::new(EWMA_UNSET),
            weight_factor_bits: AtomicU64::new(WEIGHT_FACTOR_NEUTRAL.to_bits()),
            transitions_total: AtomicU64::new(0),
        }
    }

    pub fn observed(&self) -> Health {
        Health::from_u8(self.observed.load(Ordering::Relaxed))
    }

    pub fn applied(&self) -> Health {
        Health::from_u8(self.applied.load(Ordering::Relaxed))
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }

    /// Update the frozen flag from resolved scope (control plane / registry).
    pub(crate) fn set_frozen_flag(&self, frozen: bool) {
        self.frozen.store(frozen, Ordering::Relaxed);
    }

    /// Unix epoch milliseconds of the last observed/applied transition, if any.
    pub fn last_transition_unix_ms(&self) -> Option<u64> {
        let ms = self.last_transition_unix_ms.load(Ordering::Relaxed);
        if ms == 0 {
            None
        } else {
            Some(ms)
        }
    }

    fn note_transition(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.last_transition_unix_ms.store(now, Ordering::Relaxed);
        self.transitions_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Cumulative health transitions (observed or applied changes).
    pub fn transitions_total(&self) -> u64 {
        self.transitions_total.load(Ordering::Relaxed)
    }

    pub fn consecutive_successes(&self) -> u32 {
        self.consecutive_successes.load(Ordering::Relaxed)
    }

    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures.load(Ordering::Relaxed)
    }

    pub fn passive_consecutive_failures(&self) -> u32 {
        self.passive_consecutive_failures.load(Ordering::Relaxed)
    }

    /// Current latency EWMA in milliseconds, or `None` before the first
    /// successful probe.
    pub fn latency_ewma_ms(&self) -> Option<f64> {
        let bits = self.latency_ewma_ms_bits.load(Ordering::Relaxed);
        if bits == EWMA_UNSET {
            None
        } else {
            Some(f64::from_bits(bits))
        }
    }

    /// Current damped latency effective-weight factor (design §D3). `1.0` means
    /// the backend routes at its configured weight; lower values shrink its
    /// share, never below the pool's latency floor.
    pub fn weight_factor(&self) -> f64 {
        f64::from_bits(self.weight_factor_bits.load(Ordering::Relaxed))
    }

    /// Move the stored effective-weight factor part-way toward `target` by
    /// `damping` (in `(0, 1]`) and return the new value. Damping keeps the
    /// factor from jumping in one step so latency-driven shares do not oscillate
    /// (design §D3). `damping = 1.0` snaps straight to `target`.
    pub fn damp_weight_factor(&self, target: f64, damping: f64) -> f64 {
        let current = self.weight_factor();
        let next = current + damping * (target - current);
        self.weight_factor_bits
            .store(next.to_bits(), Ordering::Relaxed);
        next
    }

    fn set_applied(&self, value: Health) {
        let prev = self.applied();
        self.applied.store(value.to_u8(), Ordering::Relaxed);
        if prev != value {
            self.note_transition();
        }
    }

    /// Set `applied` without changing the frozen flag (pool/global manual set).
    pub(crate) fn set_applied_only(&self, value: Health) {
        self.set_applied(value);
    }

    /// Resume automatic: snap `applied := observed` (design §D2).
    pub(crate) fn snap_applied_to_observed(&self) {
        self.set_applied(self.observed());
    }

    fn update_ewma(&self, rtt_ms: f64, alpha: f64) {
        let next = match self.latency_ewma_ms() {
            None => rtt_ms,
            Some(prev) => alpha * rtt_ms + (1.0 - alpha) * prev,
        };
        self.latency_ewma_ms_bits
            .store(next.to_bits(), Ordering::Relaxed);
    }

    /// Record a successful probe with its round-trip time.
    ///
    /// Increments the success run (resetting failures), folds `rtt_ms` into the
    /// latency EWMA, and — once `rise` consecutive successes are reached — marks
    /// `observed` up. When not frozen, `applied` tracks the new `observed`.
    /// Returns the resulting `applied` health.
    pub fn record_success(&self, rise: u32, alpha: f64, rtt_ms: f64) -> Health {
        self.update_ewma(rtt_ms, alpha);
        self.consecutive_failures.store(0, Ordering::Relaxed);
        let successes = self
            .consecutive_successes
            .load(Ordering::Relaxed)
            .saturating_add(1);
        self.consecutive_successes
            .store(successes, Ordering::Relaxed);
        if successes >= rise.max(1) {
            let prev = self.observed();
            self.observed.store(Health::Up.to_u8(), Ordering::Relaxed);
            if prev != Health::Up {
                self.note_transition();
            }
            if !self.is_frozen() {
                self.set_applied(Health::Up);
            }
        }
        self.applied()
    }

    /// Record a failed probe (timeout / unacceptable rcode).
    ///
    /// Increments the failure run (resetting successes) and — once `fall`
    /// consecutive failures are reached — marks `observed` down. When not
    /// frozen, `applied` tracks the new `observed`. Returns the resulting
    /// `applied` health.
    pub fn record_failure(&self, fall: u32) -> Health {
        self.consecutive_successes.store(0, Ordering::Relaxed);
        let failures = self
            .consecutive_failures
            .load(Ordering::Relaxed)
            .saturating_add(1);
        self.consecutive_failures.store(failures, Ordering::Relaxed);
        if failures >= fall.max(1) {
            let prev = self.observed();
            self.observed.store(Health::Down.to_u8(), Ordering::Relaxed);
            if prev != Health::Down {
                self.note_transition();
            }
            if !self.is_frozen() {
                self.set_applied(Health::Down);
            }
        }
        self.applied()
    }

    /// Record a live forward failure (timeout / hard error). Passive may open
    /// the circuit at `passive_fall`; only probe rise may close (design §D1).
    ///
    /// Returns a [`PassiveFailureResult`] so callers can log with full query
    /// context (the state machine itself has no query knowledge).
    pub fn record_passive_failure(&self, passive_fall: u32) -> PassiveFailureResult {
        let was_already_down = self.observed() == Health::Down;
        let failures = self
            .passive_consecutive_failures
            .load(Ordering::Relaxed)
            .saturating_add(1);
        self.passive_consecutive_failures
            .store(failures, Ordering::Relaxed);
        let mut transitioned = false;
        if failures >= passive_fall.max(1) {
            let prev = self.observed();
            self.observed.store(Health::Down.to_u8(), Ordering::Relaxed);
            if prev != Health::Down {
                self.note_transition();
                transitioned = true;
            }
            if !self.is_frozen() {
                self.set_applied(Health::Down);
            }
        }
        PassiveFailureResult {
            applied: self.applied(),
            consecutive_failures: failures,
            passive_fall,
            transitioned,
            already_down: was_already_down,
        }
    }

    /// Reset the passive failure run on a successful forward. Does not mark up.
    pub fn record_passive_success(&self) {
        self.passive_consecutive_failures
            .store(0, Ordering::Relaxed);
    }

    /// Freeze probe-driven transitions: `applied` holds while `observed` keeps
    /// updating (design §D2).
    pub fn freeze(&self) {
        self.frozen.store(true, Ordering::Relaxed);
    }

    /// Resume automatic operation and snap `applied := observed` (design §D2).
    pub fn unfreeze(&self) -> Health {
        self.frozen.store(false, Ordering::Relaxed);
        let observed = self.observed();
        self.set_applied(observed);
        observed
    }

    /// Manually set `applied` up and freeze (a manual set implies freeze, §D2).
    pub fn set_up(&self) {
        self.set_applied(Health::Up);
        self.freeze();
    }

    /// Manually set `applied` down and freeze (drain; §D2).
    pub fn set_down(&self) {
        self.set_applied(Health::Down);
        self.freeze();
    }
}

/// Whether two compiled backends probe with the same semantics. A change here
/// means the old health verdict no longer describes the new probe, so the side
/// table resets the entry (design §D9). Probe transport follows
/// `forward.upstream_transport` (global, restart-required) so it is not compared
/// per backend here.
fn probe_semantics_eq(a: &CompiledBackendHealth, b: &CompiledBackendHealth) -> bool {
    a.probe_qname == b.probe_qname
        && a.probe_qtype == b.probe_qtype
        && a.probe_source == b.probe_source
}

/// Backend identity → health state map published behind one `Arc` so it can be
/// swapped atomically on reload.
pub type HealthTable = HashMap<BackendKey, Arc<BackendHealthState>>;

const SCOPE_AUTOMATIC: u8 = 0;

/// Tri-state scope overrides for operator controls (design §D8).
#[derive(Debug)]
pub(crate) struct HealthControlScopes {
    pub(crate) global: AtomicU8,
    pub(crate) pools: RwLock<HashMap<String, u8>>,
    pub(crate) backends: RwLock<HashMap<BackendKey, u8>>,
}

impl Default for HealthControlScopes {
    fn default() -> Self {
        Self {
            global: AtomicU8::new(SCOPE_AUTOMATIC),
            pools: RwLock::new(HashMap::new()),
            backends: RwLock::new(HashMap::new()),
        }
    }
}

/// Lock-free read handle to per-backend health state for workers and the probe
/// loop. Mirrors the `arc-swap` mechanism used for the config snapshot.
#[derive(Debug)]
pub struct HealthRegistry {
    pub(crate) table: arc_swap::ArcSwap<HealthTable>,
    pub(crate) scopes: HealthControlScopes,
}

impl HealthRegistry {
    /// Build a registry from compiled probe config, seeding each backend with
    /// its pool's initial-state policy.
    pub fn from_compiled(health: &CompiledHealth) -> Self {
        let mut table: HealthTable = HashMap::new();
        for (pool_name, pool) in &health.pools {
            for backend in &pool.backends {
                let key = BackendKey::new(pool_name.clone(), backend.address);
                table.insert(
                    key,
                    Arc::new(BackendHealthState::from_initial_policy(pool.initial_state)),
                );
            }
        }
        Self {
            table: arc_swap::ArcSwap::from_pointee(table),
            scopes: HealthControlScopes::default(),
        }
    }

    /// Empty registry (no health configured).
    pub fn empty() -> Self {
        Self {
            table: arc_swap::ArcSwap::from_pointee(HealthTable::new()),
            scopes: HealthControlScopes::default(),
        }
    }

    /// Reconcile the side-table against a new compiled health config on a
    /// snapshot swap (design §D9). Health is runtime state: a reload rebuilds the
    /// config snapshot wholesale but MUST NOT blanket-reset health, or a
    /// weight-only overlay apply would briefly treat a known-dead backend as
    /// alive. By backend identity `(pool, address)`:
    ///
    /// - **unchanged** (same address and probe semantics) → preserve the
    ///   existing state object (so the probe loop, which holds the same `Arc`,
    ///   stays coherent and `observed`/`applied` survive the reload);
    /// - **new backend** → fresh state from the pool's initial-state policy;
    /// - **address changed** (a repoint produces a new key) → fresh state;
    /// - **probe semantics changed** (qname/qtype/source) → fresh state.
    pub fn reconcile(&self, prev: &CompiledHealth, new: &CompiledHealth) {
        let old_table = self.table.load();
        let mut next: HealthTable = HashMap::new();
        for (pool_name, pool) in &new.pools {
            for backend in &pool.backends {
                let key = BackendKey::new(pool_name.clone(), backend.address);
                let preserved = old_table.get(&key).filter(|_| {
                    prev.pool(pool_name)
                        .and_then(|pp| pp.backends.iter().find(|b| b.address == backend.address))
                        .map(|prev_backend| probe_semantics_eq(prev_backend, backend))
                        .unwrap_or(false)
                });
                let state = match preserved {
                    Some(existing) => existing.clone(),
                    None => Arc::new(BackendHealthState::from_initial_policy(pool.initial_state)),
                };
                next.insert(key, state);
            }
        }
        self.table.store(Arc::new(next));
    }

    /// Snapshot the current table for lock-free reads.
    pub fn load(&self) -> Arc<HealthTable> {
        self.table.load_full()
    }

    /// Look up one backend's state.
    pub fn get(&self, pool: &str, address: SocketAddr) -> Option<Arc<BackendHealthState>> {
        self.table
            .load()
            .get(&BackendKey {
                pool: pool.to_string(),
                address,
            })
            .cloned()
    }

    pub fn is_empty(&self) -> bool {
        self.table.load().is_empty()
    }

    pub fn len(&self) -> usize {
        self.table.load().len()
    }

    /// Fold a live forward outcome into passive health (design §D1/D11).
    ///
    /// Returns `Some(PassiveFailureResult)` when a failure was recorded (callers
    /// use it to log with full query context). Returns `None` for successes and
    /// when passive is disabled/inapplicable.
    pub fn record_passive_forward_outcome(
        &self,
        compiled: &CompiledHealth,
        pool: &str,
        backend: SocketAddr,
        is_failure: bool,
    ) -> Option<PassiveFailureResult> {
        let Some(pool_cfg) = compiled.pool(pool) else {
            return None;
        };
        if !pool_cfg.passive_fast_trip {
            return None;
        };
        let Some(state) = self.get(pool, backend) else {
            return None;
        };
        if is_failure {
            Some(state.record_passive_failure(pool_cfg.passive_fall))
        } else {
            state.record_passive_success();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::health::{DEFAULT_LATENCY_EWMA_ALPHA, DEFAULT_LATENCY_FLOOR};

    const ALPHA: f64 = DEFAULT_LATENCY_EWMA_ALPHA;

    fn optimistic() -> BackendHealthState {
        BackendHealthState::from_initial_policy(InitialHealthState::Optimistic)
    }

    #[test]
    fn optimistic_starts_applied_up_observed_unknown() {
        let s = optimistic();
        assert_eq!(s.applied(), Health::Up);
        assert_eq!(s.observed(), Health::Unknown);
        assert!(s.latency_ewma_ms().is_none());
    }

    #[test]
    fn require_policies_start_applied_down() {
        let s = BackendHealthState::from_initial_policy(InitialHealthState::Require1Good);
        assert_eq!(s.applied(), Health::Down);
        let s2 = BackendHealthState::from_initial_policy(InitialHealthState::RequireFullRise);
        assert_eq!(s2.applied(), Health::Down);
    }

    #[test]
    fn fall_threshold_marks_down() {
        let s = optimistic();
        assert_eq!(s.record_failure(2), Health::Up); // 1 of 2, no transition
        assert_eq!(s.record_failure(2), Health::Down); // 2 of 2 -> down
        assert_eq!(s.observed(), Health::Down);
    }

    #[test]
    fn rise_threshold_marks_up() {
        let s = BackendHealthState::from_initial_policy(InitialHealthState::Require1Good);
        assert_eq!(s.applied(), Health::Down);
        assert_eq!(s.record_success(3, ALPHA, 5.0), Health::Down); // 1/3
        assert_eq!(s.record_success(3, ALPHA, 5.0), Health::Down); // 2/3
        assert_eq!(s.record_success(3, ALPHA, 5.0), Health::Up); // 3/3
        assert_eq!(s.observed(), Health::Up);
    }

    #[test]
    fn success_resets_failure_run_and_vice_versa() {
        let s = optimistic();
        s.record_failure(2); // failures=1
        s.record_success(3, ALPHA, 5.0); // resets failures, successes=1
        assert_eq!(s.consecutive_failures(), 0);
        assert_eq!(s.consecutive_successes(), 1);
        s.record_failure(2); // resets successes, failures=1
        assert_eq!(s.consecutive_successes(), 0);
    }

    #[test]
    fn freeze_holds_applied_while_observed_updates() {
        let s = optimistic();
        s.freeze();
        s.record_failure(2);
        s.record_failure(2); // observed -> Down, but frozen
        assert_eq!(s.observed(), Health::Down);
        assert_eq!(s.applied(), Health::Up, "frozen applied must not move");
    }

    #[test]
    fn unfreeze_snaps_applied_to_observed() {
        let s = optimistic();
        s.freeze();
        s.record_failure(2);
        s.record_failure(2);
        assert_eq!(s.applied(), Health::Up);
        assert_eq!(s.unfreeze(), Health::Down);
        assert_eq!(s.applied(), Health::Down);
    }

    #[test]
    fn manual_set_implies_freeze() {
        let s = optimistic();
        s.set_down();
        assert!(s.is_frozen());
        assert_eq!(s.applied(), Health::Down);
        // Probes keep observing up, but applied holds down until resumed.
        s.record_success(1, ALPHA, 5.0);
        assert_eq!(s.observed(), Health::Up);
        assert_eq!(s.applied(), Health::Down);
        // Resume automatic snaps to observed.
        assert_eq!(s.unfreeze(), Health::Up);
        assert_eq!(s.applied(), Health::Up);
    }

    #[test]
    fn passive_fall_threshold_marks_down() {
        let s = optimistic();
        let r1 = s.record_passive_failure(2);
        assert_eq!(r1.applied, Health::Up);
        assert_eq!(r1.consecutive_failures, 1);
        assert!(!r1.transitioned);
        assert!(!r1.already_down);
        let r2 = s.record_passive_failure(2);
        assert_eq!(r2.applied, Health::Down);
        assert_eq!(r2.consecutive_failures, 2);
        assert!(r2.transitioned);
        assert!(!r2.already_down);
        assert_eq!(s.observed(), Health::Down);
        assert_eq!(s.applied(), Health::Down);
    }

    #[test]
    fn passive_failure_already_down_flag() {
        let s = optimistic();
        let r1 = s.record_passive_failure(1);
        assert!(r1.transitioned, "first failure trips");
        let r2 = s.record_passive_failure(1);
        assert!(!r2.transitioned, "no new transition");
        assert!(r2.already_down, "backend was already down");
    }

    #[test]
    fn passive_success_does_not_close_circuit() {
        let s = optimistic();
        s.record_passive_failure(1);
        assert_eq!(s.applied(), Health::Down);
        s.record_passive_success();
        assert_eq!(s.passive_consecutive_failures(), 0);
        assert_eq!(
            s.applied(),
            Health::Down,
            "forward success must not mark up"
        );
        assert_eq!(s.record_success(3, ALPHA, 5.0), Health::Down);
        assert_eq!(s.record_success(3, ALPHA, 5.0), Health::Down);
        assert_eq!(s.record_success(3, ALPHA, 5.0), Health::Up);
    }

    #[test]
    fn frozen_backend_ignores_passive_on_applied() {
        let s = optimistic();
        s.freeze();
        let r = s.record_passive_failure(1);
        assert_eq!(s.observed(), Health::Down);
        assert_eq!(s.applied(), Health::Up, "frozen applied must not move");
        assert!(r.transitioned, "observed still transitions");
        assert_eq!(r.applied, Health::Up, "frozen applied stays up");
    }

    #[test]
    fn latency_ewma_smooths_toward_samples() {
        let s = optimistic();
        s.record_success(1, ALPHA, 10.0);
        assert_eq!(s.latency_ewma_ms(), Some(10.0)); // first sample seeds
        s.record_success(1, ALPHA, 20.0);
        let ewma = s.latency_ewma_ms().unwrap();
        // 0.2*20 + 0.8*10 = 12.0
        assert!((ewma - 12.0).abs() < 1e-9, "ewma was {ewma}");
        let _ = DEFAULT_LATENCY_FLOOR; // floor consumed by routing (Phase B)
    }

    // ---- Reload reconciliation (design §D9) ----

    use conduit_config::health::{CompiledBackendHealth, CompiledPoolHealth};

    fn compiled_backend(addr: &str, qname: &str, qtype: u16) -> CompiledBackendHealth {
        CompiledBackendHealth {
            address: addr.parse().unwrap(),
            name: None,
            label: addr.to_string(),
            probe_qname: qname.to_string(),
            probe_qtype: qtype,
            probe_source: None,
        }
    }

    fn compiled_pool(backends: Vec<CompiledBackendHealth>) -> CompiledHealth {
        let pool = CompiledPoolHealth {
            interval_ms: 1000,
            timeout_ms: 1000,
            rise: 3,
            fall: 2,
            acceptable_rcodes: None,
            initial_state: InitialHealthState::Optimistic,
            latency_weighting: false,
            latency_ewma_alpha: 0.2,
            latency_floor: 0.25,
            min_eligible: 0,
            passive_fast_trip: true,
            passive_fall: 2,
            backends,
        };
        let mut pools = HashMap::new();
        pools.insert("default".to_string(), pool);
        CompiledHealth { pools }
    }

    #[test]
    fn reconcile_preserves_unchanged_backend_state() {
        let prev = compiled_pool(vec![compiled_backend("127.0.0.1:5300", "health.", 1)]);
        let reg = HealthRegistry::from_compiled(&prev);
        // Mark it down via probes.
        reg.get("default", "127.0.0.1:5300".parse().unwrap())
            .unwrap()
            .set_down();
        // A weight-only reload (identical compiled health) must NOT wipe state.
        let new = prev.clone();
        reg.reconcile(&prev, &new);
        assert_eq!(
            reg.get("default", "127.0.0.1:5300".parse().unwrap())
                .unwrap()
                .applied(),
            Health::Down,
            "unchanged backend keeps its down state across reload"
        );
    }

    #[test]
    fn reconcile_resets_on_address_change() {
        let prev = compiled_pool(vec![compiled_backend("127.0.0.1:5300", "health.", 1)]);
        let reg = HealthRegistry::from_compiled(&prev);
        reg.get("default", "127.0.0.1:5300".parse().unwrap())
            .unwrap()
            .set_down();
        // Repoint to a new address: old key disappears, new key is fresh.
        let new = compiled_pool(vec![compiled_backend("127.0.0.1:5399", "health.", 1)]);
        reg.reconcile(&prev, &new);
        assert!(
            reg.get("default", "127.0.0.1:5300".parse().unwrap())
                .is_none(),
            "old address dropped"
        );
        assert_eq!(
            reg.get("default", "127.0.0.1:5399".parse().unwrap())
                .unwrap()
                .applied(),
            Health::Up,
            "repointed backend resets to initial-state policy"
        );
    }

    #[test]
    fn reconcile_resets_on_probe_semantics_change() {
        let prev = compiled_pool(vec![compiled_backend("127.0.0.1:5300", "health.", 1)]);
        let reg = HealthRegistry::from_compiled(&prev);
        reg.get("default", "127.0.0.1:5300".parse().unwrap())
            .unwrap()
            .set_down();
        // Same address, different probe qtype → reset.
        let new = compiled_pool(vec![compiled_backend("127.0.0.1:5300", "health.", 6)]);
        reg.reconcile(&prev, &new);
        assert_eq!(
            reg.get("default", "127.0.0.1:5300".parse().unwrap())
                .unwrap()
                .applied(),
            Health::Up,
            "changed probe semantics resets health"
        );
    }

    #[test]
    fn reconcile_seeds_new_backend() {
        let prev = compiled_pool(vec![compiled_backend("127.0.0.1:5300", "health.", 1)]);
        let reg = HealthRegistry::from_compiled(&prev);
        let new = compiled_pool(vec![
            compiled_backend("127.0.0.1:5300", "health.", 1),
            compiled_backend("127.0.0.1:5301", "health.", 1),
        ]);
        reg.reconcile(&prev, &new);
        assert_eq!(reg.len(), 2);
        assert_eq!(
            reg.get("default", "127.0.0.1:5301".parse().unwrap())
                .unwrap()
                .applied(),
            Health::Up,
            "new backend seeded from initial-state policy"
        );
    }

    #[test]
    fn registry_seeds_states_per_backend() {
        let cfg = conduit_config::load_yaml(include_str!(
            "../../../../tests/fixtures/config/with-health.yaml"
        ))
        .unwrap();
        let compiled = conduit_config::health::compile_health_from_config(&cfg).unwrap();
        let reg = HealthRegistry::from_compiled(&compiled);
        assert_eq!(reg.len(), 2);
        let b0 = reg
            .get("default", "127.0.0.1:5300".parse().unwrap())
            .expect("backend present");
        assert_eq!(b0.applied(), Health::Up); // optimistic default
        assert!(reg
            .get("default", "127.0.0.1:9999".parse().unwrap())
            .is_none());
    }

    #[test]
    fn registry_passive_disabled_has_no_effect() {
        let mut pool = compiled_pool(vec![compiled_backend("127.0.0.1:5300", "health.", 1)]);
        pool.pools.get_mut("default").unwrap().passive_fast_trip = false;
        let reg = HealthRegistry::from_compiled(&pool);
        let state = reg
            .get("default", "127.0.0.1:5300".parse().unwrap())
            .unwrap();
        let result = reg.record_passive_forward_outcome(
            &pool,
            "default",
            "127.0.0.1:5300".parse().unwrap(),
            true,
        );
        assert!(result.is_none(), "disabled passive returns None");
        assert_eq!(state.applied(), Health::Up);
        assert_eq!(state.passive_consecutive_failures(), 0);
    }
}
