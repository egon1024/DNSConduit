//! Probe scheduling core (design §D5): interval + jitter, skip-if-outstanding,
//! per-backend independence.
//!
//! This type holds **no sockets** — it only decides *when* to probe and how to
//! fold an outcome into health state. The real I/O loop ([`super::run`]) drives
//! it: it asks for [`ProbeScheduler::due_probes`], performs the I/O, then feeds
//! results back via [`ProbeScheduler::on_reply`] / [`ProbeScheduler::on_failure`].
//! Keeping the schedule pure makes the multiplex-isolation and skip-if-outstanding
//! behavior deterministically testable with a fake clock (no real timing).

use conduit_core::clock::Clock;
use conduit_core::health::{BackendHealthState, BackendKey, Health, ProbeOutcome, ProbeSpec};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Log a `(pool, backend)` health transition when a probe outcome moved
/// `observed` or `applied`. This is the only Phase A observation surface for
/// backend health (metrics and the control-plane RPC arrive in later phases),
/// so Gate A relies on these INFO lines.
fn log_transition(key: &BackendKey, before: (Health, Health), state: &BackendHealthState) {
    let after = (state.observed(), state.applied());
    if before != after {
        tracing::info!(
            pool = %key.pool,
            backend = %key.address,
            observed = ?after.0,
            applied = ?after.1,
            "backend health transition"
        );
    }
}

/// Positive jitter applied to each interval, as a percentage of the interval
/// (de-synchronizes a fleet and spreads backend load — design §D5).
const JITTER_PERCENT: u64 = 20;

/// Fraction of the gap to the target the latency weight factor moves on each
/// recompute. Below 1.0 the factor is damped so latency-driven shares change
/// gradually rather than jumping (design §D3).
const WEIGHT_FACTOR_DAMPING: f64 = 0.5;

/// A probe the I/O loop should send now.
#[derive(Debug, Clone)]
pub struct DueProbe {
    pub backend_idx: usize,
    pub address: SocketAddr,
    pub source: Option<IpAddr>,
    pub qid: u16,
    pub wire: Vec<u8>,
    /// Per-backend probe timeout (bounds the TCP probe thread's blocking I/O).
    pub timeout: Duration,
}

struct Outstanding {
    qid: u16,
    sent_at: Instant,
    deadline: Instant,
}

/// Per-backend scheduling + health wiring.
pub struct BackendProbe {
    pub key: BackendKey,
    pub address: SocketAddr,
    /// Metric/log label: configured backend `name` when set, else address.
    pub label: String,
    pub source: Option<IpAddr>,
    state: Arc<BackendHealthState>,
    spec: ProbeSpec,
    interval: Duration,
    timeout: Duration,
    rise: u32,
    fall: u32,
    alpha: f64,
    latency_weighting: bool,
    latency_floor: f64,
    next_due: Instant,
    outstanding: Option<Outstanding>,
}

impl BackendProbe {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        key: BackendKey,
        address: SocketAddr,
        label: String,
        source: Option<IpAddr>,
        state: Arc<BackendHealthState>,
        spec: ProbeSpec,
        interval: Duration,
        timeout: Duration,
        rise: u32,
        fall: u32,
        alpha: f64,
        latency_weighting: bool,
        latency_floor: f64,
        first_due: Instant,
    ) -> Self {
        Self {
            key,
            address,
            label,
            source,
            state,
            spec,
            interval,
            timeout,
            rise,
            fall,
            alpha,
            latency_weighting,
            latency_floor,
            next_due: first_due,
            outstanding: None,
        }
    }

    pub fn state(&self) -> &Arc<BackendHealthState> {
        &self.state
    }
}

/// Minimal seedable PRNG (SplitMix64) for probe ids and jitter. Probe ids only
/// need to vary unpredictably across probes; this avoids a crypto-rng dependency
/// and is seedable for deterministic tests.
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Drives per-backend probe timing and folds outcomes into health state.
pub struct ProbeScheduler {
    clock: Arc<dyn Clock>,
    rng: SplitMix64,
    backends: Vec<BackendProbe>,
}

impl ProbeScheduler {
    pub fn new(clock: Arc<dyn Clock>, seed: u64, backends: Vec<BackendProbe>) -> Self {
        Self {
            clock,
            rng: SplitMix64::new(seed),
            backends,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }

    pub fn len(&self) -> usize {
        self.backends.len()
    }

    /// Pool name and metric label for `backend_idx`, when in range.
    pub fn backend_labels(&self, backend_idx: usize) -> Option<(&str, &str)> {
        self.backends
            .get(backend_idx)
            .map(|b| (b.key.pool.as_str(), b.label.as_str()))
    }

    fn jitter(&mut self, interval: Duration) -> Duration {
        let max_ms = interval.as_millis() as u64 * JITTER_PERCENT / 100;
        if max_ms == 0 {
            return Duration::ZERO;
        }
        Duration::from_millis(self.rng.next_u64() % (max_ms + 1))
    }

    /// Bounds on the scheduled gap between consecutive sends for `interval`
    /// (inclusive): `[interval, interval + interval*JITTER_PERCENT/100]`.
    pub fn schedule_bounds(interval: Duration) -> (Duration, Duration) {
        let max_jitter = Duration::from_millis(interval.as_millis() as u64 * JITTER_PERCENT / 100);
        (interval, interval + max_jitter)
    }

    /// Probes that are due now and have no outstanding probe (skip-if-outstanding).
    ///
    /// Issuing a probe marks the backend outstanding (with a timeout deadline)
    /// and schedules its next send `interval + jitter` ahead. A backend with an
    /// in-flight probe is never re-issued, so at most one probe is outstanding
    /// per backend, and a slow/dead backend never delays the others.
    pub fn due_probes(&mut self) -> Vec<DueProbe> {
        let now = self.clock.now();
        let mut due = Vec::new();
        // Index loop so the borrow of `self.rng` (jitter) does not conflict with
        // the per-backend mutable borrow.
        for idx in 0..self.backends.len() {
            let (interval, timeout) = {
                let b = &self.backends[idx];
                if b.outstanding.is_some() || b.next_due > now {
                    continue;
                }
                (b.interval, b.timeout)
            };
            let qid = self.rng.next_u64() as u16;
            let jitter = self.jitter(interval);
            let b = &mut self.backends[idx];
            let wire = match b.spec.build_query(qid) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!(pool = %b.key.pool, backend = %b.address, error = %e, "probe build failed; skipping");
                    // Reschedule so we do not hot-spin on a bad spec.
                    b.next_due = now + interval;
                    continue;
                }
            };
            b.outstanding = Some(Outstanding {
                qid,
                sent_at: now,
                deadline: now + timeout,
            });
            b.next_due = now + interval + jitter;
            due.push(DueProbe {
                backend_idx: idx,
                address: b.address,
                source: b.source,
                qid,
                wire,
                timeout,
            });
        }
        due
    }

    /// Earliest instant the loop should wake: the soonest next-due (idle
    /// backend) or outstanding deadline. `None` when there are no backends.
    pub fn next_wakeup(&self) -> Option<Instant> {
        self.backends
            .iter()
            .map(|b| match &b.outstanding {
                Some(o) => o.deadline,
                None => b.next_due,
            })
            .min()
    }

    /// Feed a received datagram for `backend_idx` into the schedule.
    ///
    /// A matching success/failure clears the outstanding probe and records the
    /// outcome; an unmatched datagram is ignored (the probe stays outstanding
    /// until a matching reply or its timeout).
    pub fn on_reply(&mut self, backend_idx: usize, wire: &[u8]) -> Option<ProbeOutcome> {
        let now = self.clock.now();
        let b = self.backends.get_mut(backend_idx)?;
        let outstanding = b.outstanding.as_ref()?;
        let outcome = b.spec.classify_response(outstanding.qid, wire);
        let before = (b.state.observed(), b.state.applied());
        match outcome {
            ProbeOutcome::Success => {
                let rtt_ms = now
                    .saturating_duration_since(outstanding.sent_at)
                    .as_secs_f64()
                    * 1000.0;
                b.state.record_success(b.rise, b.alpha, rtt_ms);
                b.outstanding = None;
                log_transition(&b.key, before, &b.state);
            }
            ProbeOutcome::Failure => {
                b.state.record_failure(b.fall);
                b.outstanding = None;
                log_transition(&b.key, before, &b.state);
            }
            ProbeOutcome::Unmatched => {}
        }
        Some(outcome)
    }

    /// Mark the outstanding probe for `backend_idx` failed (timeout or a
    /// transport-level error such as connect/send failure). No-op if there is no
    /// outstanding probe.
    pub fn on_failure(&mut self, backend_idx: usize) -> bool {
        let Some(b) = self.backends.get_mut(backend_idx) else {
            return false;
        };
        if b.outstanding.is_none() {
            return false;
        }
        b.outstanding = None;
        let before = (b.state.observed(), b.state.applied());
        b.state.record_failure(b.fall);
        log_transition(&b.key, before, &b.state);
        true
    }

    /// Expire any outstanding probes whose deadline has passed (timeout =
    /// failure). Returns the indices that timed out (for logging/metrics later).
    pub fn expire_timeouts(&mut self) -> Vec<usize> {
        let now = self.clock.now();
        let mut expired = Vec::new();
        for idx in 0..self.backends.len() {
            let timed_out = matches!(&self.backends[idx].outstanding, Some(o) if o.deadline <= now);
            if timed_out {
                let b = &mut self.backends[idx];
                b.outstanding = None;
                let before = (b.state.observed(), b.state.applied());
                b.state.record_failure(b.fall);
                log_transition(&b.key, before, &b.state);
                expired.push(idx);
            }
        }
        expired
    }

    /// Recompute the damped latency effective-weight factor for every backend in
    /// a latency-weighted pool (design §D3). For each such pool the target factor
    /// is `clamp(ewma_min_in_pool / ewma_backend, floor, 1.0)` over the eligible
    /// (applied-up) backends with a latency sample; backends without a sample
    /// (or in a pool with no fastest reference yet) target `1.0` (no reduction).
    /// The stored factor moves only part-way toward the target so shares change
    /// gradually. Anchoring to the pool's fastest EWMA keeps a uniformly-slow
    /// pool from being penalized wholesale.
    pub fn recompute_weight_factors(&self) {
        use std::collections::HashMap;
        let mut pool_min: HashMap<&str, f64> = HashMap::new();
        for b in &self.backends {
            if !b.latency_weighting || b.state.applied() != Health::Up {
                continue;
            }
            if let Some(ewma) = b.state.latency_ewma_ms() {
                let entry = pool_min.entry(b.key.pool.as_str()).or_insert(ewma);
                if ewma < *entry {
                    *entry = ewma;
                }
            }
        }
        for b in &self.backends {
            if !b.latency_weighting {
                continue;
            }
            let target = match (pool_min.get(b.key.pool.as_str()), b.state.latency_ewma_ms()) {
                (Some(&min), Some(ewma)) if ewma > 0.0 => (min / ewma).clamp(b.latency_floor, 1.0),
                _ => 1.0,
            };
            b.state.damp_weight_factor(target, WEIGHT_FACTOR_DAMPING);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_core::health::{Health, ProbeSpec};
    use std::sync::Mutex;

    struct FakeClock {
        base: Instant,
        offset: Mutex<Duration>,
    }

    impl FakeClock {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                base: Instant::now(),
                offset: Mutex::new(Duration::ZERO),
            })
        }
        fn advance(&self, d: Duration) {
            *self.offset.lock().unwrap() += d;
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            self.base + *self.offset.lock().unwrap()
        }
    }

    fn spec() -> ProbeSpec {
        ProbeSpec::new("health.example.", 1, None).unwrap()
    }

    fn backend(idx: u8, clock: &Arc<FakeClock>, interval_ms: u64, timeout_ms: u64) -> BackendProbe {
        let addr: SocketAddr = format!("127.0.0.1:530{idx}").parse().unwrap();
        BackendProbe::new(
            BackendKey::new("default", addr),
            addr,
            addr.to_string(),
            None,
            Arc::new(BackendHealthState::from_initial_policy(
                conduit_config::health::InitialHealthState::Optimistic,
            )),
            spec(),
            Duration::from_millis(interval_ms),
            Duration::from_millis(timeout_ms),
            3,
            2,
            0.2,
            false,
            0.25,
            clock.now(),
        )
    }

    /// Synthesize a matching success response for a given qid.
    fn success_wire(qid: u16) -> Vec<u8> {
        use hickory_proto::op::{Message, MessageType, Query, ResponseCode};
        use hickory_proto::rr::{Name, RecordType};
        let mut msg = Message::new();
        msg.set_id(qid);
        msg.set_message_type(MessageType::Response);
        msg.set_response_code(ResponseCode::NoError);
        let mut name = Name::from_utf8("health.example.").unwrap();
        name.set_fqdn(true);
        msg.add_query(Query::query(name, RecordType::A));
        msg.to_vec().unwrap()
    }

    #[test]
    fn first_tick_issues_one_probe_per_backend() {
        let clock = FakeClock::new();
        let mut sched = ProbeScheduler::new(
            clock.clone(),
            1,
            vec![backend(0, &clock, 1000, 500), backend(1, &clock, 1000, 500)],
        );
        let due = sched.due_probes();
        assert_eq!(due.len(), 2);
        // Skip-if-outstanding: nothing new until replies/timeouts clear.
        assert!(sched.due_probes().is_empty());
    }

    #[test]
    fn skip_if_outstanding_holds_until_reply() {
        let clock = FakeClock::new();
        let mut sched = ProbeScheduler::new(clock.clone(), 1, vec![backend(0, &clock, 1000, 5000)]);
        let due = sched.due_probes();
        let qid = due[0].qid;
        // timeout(5s) > interval(1s): advancing past interval must NOT issue a
        // second probe while the first is outstanding.
        clock.advance(Duration::from_millis(1500));
        assert!(sched.due_probes().is_empty(), "must not double-probe");
        // Reply clears it; next tick (after next_due) issues again.
        sched.on_reply(0, &success_wire(qid));
        clock.advance(Duration::from_millis(1500));
        assert_eq!(sched.due_probes().len(), 1);
    }

    #[test]
    fn jitter_keeps_next_due_within_bounds() {
        let clock = FakeClock::new();
        let interval = Duration::from_millis(1000);
        let (lo, hi) = ProbeScheduler::schedule_bounds(interval);
        assert_eq!(lo, interval);
        assert_eq!(hi, Duration::from_millis(1200));
        // Across many seeds the scheduled next-send gap stays within
        // [interval, interval+20%]. We read `next_due` (the scheduled cadence)
        // rather than `next_wakeup`, which after issuing returns the sooner
        // outstanding-probe timeout deadline, not the next send.
        for seed in 0..50u64 {
            let mut sched =
                ProbeScheduler::new(clock.clone(), seed, vec![backend(0, &clock, 1000, 500)]);
            let now = clock.now();
            sched.due_probes();
            let gap = sched.backends[0].next_due - now;
            assert!(
                gap >= lo && gap <= hi,
                "seed {seed}: gap {gap:?} out of bounds"
            );
        }
    }

    #[test]
    fn timeout_records_failure_and_reprobes() {
        let clock = FakeClock::new();
        let mut sched = ProbeScheduler::new(clock.clone(), 1, vec![backend(0, &clock, 1000, 500)]);
        sched.due_probes();
        clock.advance(Duration::from_millis(600)); // past 500ms timeout
        let expired = sched.expire_timeouts();
        assert_eq!(expired, vec![0]);
        let state = sched.backends[0].state().clone();
        assert_eq!(state.consecutive_failures(), 1);
        // Second timeout (fall=2) marks observed down.
        clock.advance(Duration::from_millis(1000));
        sched.due_probes();
        clock.advance(Duration::from_millis(600));
        sched.expire_timeouts();
        assert_eq!(state.observed(), Health::Down);
    }

    #[test]
    fn dead_backend_does_not_delay_healthy_one() {
        // Backend 0 is "dead": it never replies and its timeout is huge.
        // Backend 1 is healthy: it replies each probe. The dead backend must not
        // stall the healthy backend's cadence (multiplex isolation, §D5).
        let clock = FakeClock::new();
        let mut sched = ProbeScheduler::new(
            clock.clone(),
            1,
            vec![
                backend(0, &clock, 1000, 60_000), // dead, 60s timeout
                backend(1, &clock, 1000, 500),    // healthy
            ],
        );
        let healthy_state = sched.backends[1].state().clone();

        let first = sched.due_probes();
        assert_eq!(first.len(), 2);
        // Healthy backend (idx 1) replies; dead backend (idx 0) stays outstanding.
        let qid1 = first.iter().find(|d| d.backend_idx == 1).unwrap().qid;
        sched.on_reply(1, &success_wire(qid1));

        // Drive several intervals: the healthy backend keeps probing while the
        // dead backend's single probe remains outstanding the whole time.
        for _ in 0..5 {
            clock.advance(Duration::from_millis(1300));
            let due = sched.due_probes();
            // Only the healthy backend should be due; the dead one is outstanding.
            assert!(
                due.iter().all(|d| d.backend_idx == 1),
                "dead backend must not be re-probed while outstanding"
            );
            assert_eq!(due.len(), 1, "healthy backend keeps its cadence");
            let qid = due[0].qid;
            sched.on_reply(1, &success_wire(qid));
        }
        // Healthy backend rose to Up; dead backend never timed out (huge timeout)
        // so it stays outstanding — but crucially it never blocked the other.
        assert_eq!(healthy_state.observed(), Health::Up);
    }

    fn backend_lw(idx: u8, clock: &Arc<FakeClock>) -> BackendProbe {
        let addr: SocketAddr = format!("127.0.0.1:540{idx}").parse().unwrap();
        BackendProbe::new(
            BackendKey::new("default", addr),
            addr,
            addr.to_string(),
            None,
            Arc::new(BackendHealthState::from_initial_policy(
                conduit_config::health::InitialHealthState::Optimistic,
            )),
            spec(),
            Duration::from_millis(1000),
            Duration::from_millis(500),
            3,
            2,
            0.2,
            true, // latency weighting on
            0.25, // floor
            clock.now(),
        )
    }

    #[test]
    fn recompute_weight_factors_shrinks_slower_backend() {
        let clock = FakeClock::new();
        let sched = ProbeScheduler::new(
            clock.clone(),
            1,
            vec![backend_lw(0, &clock), backend_lw(1, &clock)],
        );
        // Backend 0 fast (5ms), backend 1 slow (50ms) — both eligible (Up).
        let fast = sched.backends[0].state().clone();
        let slow = sched.backends[1].state().clone();
        // Seed EWMA and mark Up (rise=3 successes).
        for _ in 0..3 {
            fast.record_success(3, 0.2, 5.0);
            slow.record_success(3, 0.2, 50.0);
        }
        assert_eq!(fast.applied(), Health::Up);
        assert_eq!(slow.applied(), Health::Up);

        // Several recomputes drive the factors toward their damped targets.
        for _ in 0..20 {
            sched.recompute_weight_factors();
        }
        // Fast backend (pool minimum) keeps ~1.0; slow backend shrinks toward
        // 5/50 = 0.1, clamped to the 0.25 floor.
        assert!(
            (fast.weight_factor() - 1.0).abs() < 0.01,
            "fast {}",
            fast.weight_factor()
        );
        assert!(
            (slow.weight_factor() - 0.25).abs() < 0.01,
            "slow clamped to floor: {}",
            slow.weight_factor()
        );
    }

    #[test]
    fn unmatched_reply_keeps_probe_outstanding() {
        let clock = FakeClock::new();
        let mut sched = ProbeScheduler::new(clock.clone(), 1, vec![backend(0, &clock, 1000, 5000)]);
        sched.due_probes();
        // Reply with the wrong qid: ignored, probe stays outstanding.
        let outcome = sched.on_reply(0, &success_wire(0xFFFF));
        assert_eq!(outcome, Some(ProbeOutcome::Unmatched));
        clock.advance(Duration::from_millis(1500));
        assert!(
            sched.due_probes().is_empty(),
            "still outstanding after unmatched"
        );
    }
}
