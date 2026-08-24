//! Lookup orchestrator stage — profile-ordered provider chain.

use crate::lookup::cache::{CacheKey, CacheLookupOutcome, LookupCacheRegistry};
use crate::lookup::{AnswerSource, LookupOutcome};
use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::record_upstream_response;
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::{ConvergenceReason, LookupCacheFill, LookupCacheWait, Transaction};
use conduit_config::lookup::{CompiledLookupProvider, DEFAULT_LOOKUP_PROFILE};
use conduit_metrics::MetricsHub;
use std::sync::Arc;
use std::time::Instant;

/// Resume point inside the forward provider after async upstream I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupForwardStep {
    Wait,
}

/// Runs lookup providers for the active profile. The forward provider internalizes
/// Route → Forward → WaitResponse without exposing those as top-level graph phases.
pub struct LookupStage {
    route: Arc<dyn PipelineStage>,
    forward: Arc<dyn PipelineStage>,
    wait: Arc<dyn PipelineStage>,
    cache: Option<Arc<LookupCacheRegistry>>,
    metrics: Option<Arc<MetricsHub>>,
}

impl LookupStage {
    pub fn new(
        route: Arc<dyn PipelineStage>,
        forward: Arc<dyn PipelineStage>,
        wait: Arc<dyn PipelineStage>,
        metrics: Option<Arc<MetricsHub>>,
    ) -> Self {
        Self {
            route,
            forward,
            wait,
            cache: None,
            metrics,
        }
    }

    pub fn with_cache(mut self, cache: Arc<LookupCacheRegistry>) -> Self {
        if let Some(metrics) = self.metrics.clone() {
            cache.set_metrics(metrics);
        }
        self.cache = Some(cache);
        self
    }

    fn profile_label(txn: &Transaction) -> String {
        txn.lookup_profile
            .clone()
            .unwrap_or_else(|| DEFAULT_LOOKUP_PROFILE.to_string())
    }

    fn record_provider_outcome(&self, profile: &str, provider: &str, outcome: &str) {
        if let Some(hub) = self.metrics.as_ref() {
            if hub.metrics_enabled() {
                hub.builtin()
                    .record_lookup_provider_outcome(profile, provider, outcome);
            }
        }
    }

    fn record_cache_lookup_metric(&self, cache: &str, profile: &str, result: &str) {
        if let Some(hub) = self.metrics.as_ref() {
            if hub.metrics_enabled() {
                hub.builtin().record_cache_lookup(cache, profile, result);
            }
        }
    }

    fn observe_cache_lookup_metric(&self, cache: &str, profile: &str, started: Instant) {
        if let Some(hub) = self.metrics.as_ref() {
            if hub.metrics_enabled() {
                hub.builtin().observe_cache_lookup_duration(
                    cache,
                    profile,
                    started.elapsed().as_secs_f64(),
                );
            }
        }
    }

    fn observe_lookup_provider_duration(&self, profile: &str, provider: &str, started: Instant) {
        if let Some(hub) = self.metrics.as_ref() {
            if hub.metrics_enabled() {
                hub.builtin().observe_lookup_duration(
                    profile,
                    provider,
                    started.elapsed().as_secs_f64(),
                );
            }
        }
    }

    fn trace_nested(
        txn: &mut Transaction,
        message: &str,
        pool: Option<&str>,
        backend: Option<&str>,
        cache: Option<&str>,
    ) {
        if txn.trace_log.is_some() {
            txn.trace_record_phase(
                "lookup",
                Some(message.to_string()),
                pool.map(str::to_string),
                backend.map(str::to_string),
                cache.map(str::to_string),
            );
        }
    }

    fn run_forward_chain(
        &self,
        txn: &mut Transaction,
        snapshot: &Arc<RuntimeSnapshot>,
    ) -> StageOutcome {
        let profile = Self::profile_label(txn);
        let forward_started = Instant::now();
        txn.cache_lookup_eligible = false;

        let max_attempts = snapshot
            .config
            .orchestrator
            .as_ref()
            .map(|o| o.max_attempts)
            .unwrap_or(3);
        if txn.attempt_count >= max_attempts {
            tracing::warn!(
                txn_id = txn.id,
                attempt_count = txn.attempt_count,
                max_attempts,
                "lookup forward max attempts exceeded; converging at no_answer"
            );
            txn.set_rcode_name("SERVFAIL");
            txn.set_convergence_reason(ConvergenceReason::AttemptsExhausted);
            return StageOutcome::Continue(Phase::NoAnswer);
        }

        let route_out = self.route.handle(txn, snapshot);
        match route_out {
            StageOutcome::Continue(Phase::Forward) => {
                let pool = txn.selected_pool.clone();
                let backend = txn.selected_backend_label.clone();
                Self::trace_nested(
                    txn,
                    "route selected backend",
                    pool.as_deref(),
                    backend.as_deref(),
                    None,
                );
            }
            StageOutcome::Continue(Phase::NoAnswer) => {
                return StageOutcome::Continue(Phase::NoAnswer);
            }
            StageOutcome::Continue(Phase::ResponseRules) => {
                return StageOutcome::Continue(Phase::ResponseRules);
            }
            StageOutcome::Drop => return StageOutcome::Drop,
            other => {
                tracing::warn!(?other, "unexpected route outcome inside lookup");
                txn.set_convergence_reason(ConvergenceReason::ForwardError);
                return StageOutcome::Continue(Phase::NoAnswer);
            }
        }

        if let Some(hub) = self.metrics.as_ref() {
            if hub.metrics_enabled() {
                if let Some(ref pool) = txn.selected_pool {
                    txn.builtin_registry(hub).record_query_by_pool(pool);
                }
            }
        }

        let forward_out = self.forward.handle(txn, snapshot);
        match forward_out {
            StageOutcome::Suspend(Phase::WaitResponse) => {
                txn.lookup_forward_step = Some(LookupForwardStep::Wait);
                Self::trace_nested(txn, "provider forward pending", None, None, None);
                self.record_provider_outcome(&profile, "forward", "pending");
                self.observe_lookup_provider_duration(&profile, "forward", forward_started);
                StageOutcome::Suspend(Phase::Lookup)
            }
            StageOutcome::Continue(Phase::WaitResponse) => {
                self.finish_forward_answer(txn, snapshot, &profile, forward_started)
            }
            StageOutcome::Continue(Phase::ResponseRules) => {
                self.finish_forward_answer(txn, snapshot, &profile, forward_started)
            }
            StageOutcome::Continue(Phase::NoAnswer) => StageOutcome::Continue(Phase::NoAnswer),
            StageOutcome::Continue(Phase::Send) => {
                // Hard forward failure with no wire → NoAnswer (compat for callers
                // that still return Send).
                if txn.response_wire.is_none() {
                    txn.set_convergence_reason(ConvergenceReason::ForwardError);
                    StageOutcome::Continue(Phase::NoAnswer)
                } else {
                    StageOutcome::Continue(Phase::Send)
                }
            }
            StageOutcome::Drop => StageOutcome::Drop,
            other => {
                tracing::warn!(?other, "unexpected forward outcome inside lookup");
                txn.set_convergence_reason(ConvergenceReason::ForwardError);
                StageOutcome::Continue(Phase::NoAnswer)
            }
        }
    }

    fn finish_forward_answer(
        &self,
        txn: &mut Transaction,
        snapshot: &Arc<RuntimeSnapshot>,
        profile: &str,
        forward_started: Instant,
    ) -> StageOutcome {
        let wait_out = self.wait.handle(txn, snapshot);
        let continue_phase = match wait_out {
            StageOutcome::Continue(Phase::ResponseRules) => Phase::ResponseRules,
            StageOutcome::Continue(Phase::NoAnswer) => Phase::NoAnswer,
            StageOutcome::Continue(Phase::Send) => {
                if txn.response_wire.is_none() {
                    txn.set_convergence_reason(ConvergenceReason::ForwardError);
                    Phase::NoAnswer
                } else {
                    Phase::Send
                }
            }
            StageOutcome::Drop => return StageOutcome::Drop,
            other => {
                tracing::warn!(?other, "unexpected wait outcome inside lookup");
                if txn.response_wire.is_none() {
                    txn.set_convergence_reason(ConvergenceReason::ForwardError);
                    Phase::NoAnswer
                } else {
                    Phase::Send
                }
            }
        };

        if let (Some(cache), Some(fill)) = (self.cache.as_ref(), txn.lookup_cache_fill.take()) {
            let key = CacheKey(fill.key);
            if let Some(wire) = txn.response_wire.as_ref() {
                if let Some(gate) = cache.instance_gate(&fill.cache_name, &key) {
                    cache.fill_from_forward(
                        &fill.cache_name,
                        &key,
                        &gate,
                        Arc::from(wire.clone().into_boxed_slice()),
                        txn,
                    );
                }
            } else if let Some(gate) = cache.instance_gate(&fill.cache_name, &key) {
                cache.complete_inflight_miss(&fill.cache_name, &key, &gate);
            }
        }

        if continue_phase == Phase::NoAnswer {
            self.observe_lookup_provider_duration(profile, "forward", forward_started);
            return StageOutcome::Continue(Phase::NoAnswer);
        }

        txn.lookup_outcome = Some(LookupOutcome::Answered);
        txn.answer_source = Some(AnswerSource::Forward);
        txn.cache_instance = None;
        Self::trace_nested(txn, "provider forward answered", None, None, None);
        self.record_provider_outcome(profile, "forward", "answered");
        self.observe_lookup_provider_duration(profile, "forward", forward_started);
        StageOutcome::Continue(continue_phase)
    }

    fn finish_forward_answer_from_wait(
        &self,
        txn: &mut Transaction,
        snapshot: &Arc<RuntimeSnapshot>,
    ) -> StageOutcome {
        let profile = Self::profile_label(txn);
        let started = txn.suspend_phase_started_at.unwrap_or_else(Instant::now);
        self.finish_forward_answer(txn, snapshot, &profile, started)
    }

    fn apply_cache_hit(
        &self,
        txn: &mut Transaction,
        wire: Vec<u8>,
        cache_name: &str,
        skip_response_rules: bool,
        snapshot: &Arc<RuntimeSnapshot>,
    ) -> StageOutcome {
        record_upstream_response(txn, &wire, snapshot.scripting.needs_response_wire_meta);
        txn.response_wire = Some(wire);
        txn.lookup_outcome = Some(LookupOutcome::Answered);
        // Pipeline state for selectors, Rhai, metrics, and dnstap — not operator tags.
        txn.answer_source = Some(AnswerSource::Cache);
        txn.cache_instance = Some(cache_name.to_string());
        txn.last_forward_ms = 0;
        let profile = Self::profile_label(txn);
        Self::trace_nested(txn, "provider cache answered", None, None, Some(cache_name));
        self.record_provider_outcome(&profile, "cache", "answered");
        if skip_response_rules {
            StageOutcome::Continue(Phase::Send)
        } else {
            StageOutcome::Continue(Phase::ResponseRules)
        }
    }

    fn try_cache_provider(
        &self,
        cache_name: &str,
        txn: &mut Transaction,
        snapshot: &Arc<RuntimeSnapshot>,
    ) -> Option<StageOutcome> {
        let cache = self.cache.as_ref()?;
        if !snapshot_cache_enabled(snapshot) {
            return None;
        }
        let now = Instant::now();
        let profile = Self::profile_label(txn);
        let cache_started = Instant::now();
        match cache.lookup(cache_name, txn, now) {
            CacheLookupOutcome::Hit {
                wire,
                cache_name,
                skip_response_rules,
            } => {
                self.record_cache_lookup_metric(cache_name.as_str(), &profile, "hit");
                self.observe_cache_lookup_metric(cache_name.as_str(), &profile, cache_started);
                Some(self.apply_cache_hit(txn, wire, &cache_name, skip_response_rules, snapshot))
            }
            CacheLookupOutcome::Miss { key, gate: _ } => {
                Self::trace_nested(txn, "provider cache miss", None, None, Some(cache_name));
                self.record_cache_lookup_metric(cache_name, &profile, "miss");
                self.observe_cache_lookup_metric(cache_name, &profile, cache_started);
                self.record_provider_outcome(&profile, "cache", "miss");
                txn.lookup_outcome = Some(LookupOutcome::Miss);
                txn.lookup_cache_fill = Some(LookupCacheFill {
                    cache_name: cache_name.to_string(),
                    key: key.as_bytes().to_vec(),
                });
                None
            }
            CacheLookupOutcome::WaitAsync { key } => {
                self.record_cache_lookup_metric(cache_name, &profile, "miss");
                self.observe_cache_lookup_metric(cache_name, &profile, cache_started);
                self.record_provider_outcome(&profile, "cache", "miss");
                txn.lookup_cache_wait = Some(LookupCacheWait {
                    cache_name: cache_name.to_string(),
                    key: key.as_bytes().to_vec(),
                });
                Some(StageOutcome::Suspend(Phase::Lookup))
            }
            CacheLookupOutcome::Bypass => {
                Self::trace_nested(txn, "provider cache bypass", None, None, Some(cache_name));
                self.record_cache_lookup_metric(cache_name, &profile, "bypass");
                self.observe_cache_lookup_metric(cache_name, &profile, cache_started);
                self.record_provider_outcome(&profile, "cache", "bypass");
                None
            }
        }
    }

    fn resume_cache_wait(
        &self,
        txn: &mut Transaction,
        wait: LookupCacheWait,
        snapshot: &Arc<RuntimeSnapshot>,
    ) -> StageOutcome {
        let Some(cache) = self.cache.as_ref() else {
            return self.run_forward_chain(txn, snapshot);
        };
        let key = CacheKey(wait.key);
        match cache.resume_after_wait(&wait.cache_name, &key, txn) {
            CacheLookupOutcome::Hit {
                wire,
                cache_name,
                skip_response_rules,
            } => self.apply_cache_hit(txn, wire, &cache_name, skip_response_rules, snapshot),
            _ => {
                txn.lookup_outcome = Some(LookupOutcome::Miss);
                txn.lookup_cache_fill = Some(LookupCacheFill {
                    cache_name: wait.cache_name,
                    key: key.as_bytes().to_vec(),
                });
                self.run_forward_chain(txn, snapshot)
            }
        }
    }
}

impl PipelineStage for LookupStage {
    fn name(&self) -> &'static str {
        "lookup"
    }

    fn handle(&self, txn: &mut Transaction, snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        if let Some(wait) = txn.lookup_cache_wait.take() {
            return self.resume_cache_wait(txn, wait, snapshot);
        }

        if txn.lookup_forward_step == Some(LookupForwardStep::Wait) {
            txn.lookup_forward_step = None;
            return self.finish_forward_answer_from_wait(txn, snapshot);
        }

        let profile_name = txn
            .lookup_profile
            .as_deref()
            .unwrap_or(DEFAULT_LOOKUP_PROFILE);
        let Some(profile) = snapshot.lookup.profiles.get(profile_name) else {
            tracing::error!(profile = profile_name, "unknown lookup profile");
            txn.set_rcode_name("SERVFAIL");
            txn.set_convergence_reason(ConvergenceReason::UnknownProfile);
            return StageOutcome::Continue(Phase::NoAnswer);
        };
        txn.lookup_profile = Some(profile.name.clone());

        for provider in &profile.providers {
            match provider {
                CompiledLookupProvider::Cache { cache_name } => {
                    if !txn.cache_lookup_eligible {
                        let profile = Self::profile_label(txn);
                        Self::trace_nested(
                            txn,
                            "provider cache bypass",
                            None,
                            None,
                            Some(cache_name.as_str()),
                        );
                        self.record_cache_lookup_metric(cache_name, &profile, "bypass");
                        self.record_provider_outcome(&profile, "cache", "bypass");
                        continue;
                    }
                    if let Some(out) = self.try_cache_provider(cache_name, txn, snapshot) {
                        return out;
                    }
                }
                CompiledLookupProvider::Forward => {
                    return self.run_forward_chain(txn, snapshot);
                }
            }
        }

        txn.lookup_outcome = Some(LookupOutcome::Miss);
        StageOutcome::Continue(Phase::ResponseRules)
    }
}

fn snapshot_cache_enabled(snapshot: &RuntimeSnapshot) -> bool {
    snapshot.cache_enabled()
}
