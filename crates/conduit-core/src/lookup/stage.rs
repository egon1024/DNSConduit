//! Lookup orchestrator stage — profile-ordered provider chain.

use crate::lookup::cache::{CacheKey, CacheLookupOutcome, LookupCacheRegistry};
use crate::lookup::{AnswerSource, LookupOutcome};
use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::{LookupCacheFill, LookupCacheWait, Transaction};
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
        self.cache = Some(cache);
        self
    }

    fn run_forward_chain(
        &self,
        txn: &mut Transaction,
        snapshot: &Arc<RuntimeSnapshot>,
    ) -> StageOutcome {
        txn.cache_lookup_eligible = false;

        let max_attempts = snapshot
            .config
            .orchestrator
            .as_ref()
            .map(|o| o.max_attempts)
            .unwrap_or(3);
        if txn.attempt_count >= max_attempts {
            txn.set_rcode_name("SERVFAIL");
            return StageOutcome::Continue(Phase::Send);
        }

        let route_out = self.route.handle(txn, snapshot);
        match route_out {
            StageOutcome::Continue(Phase::Forward) => {}
            StageOutcome::Continue(Phase::Send) => return StageOutcome::Continue(Phase::Send),
            StageOutcome::Drop => return StageOutcome::Drop,
            other => {
                tracing::warn!(?other, "unexpected route outcome inside lookup");
                return StageOutcome::Continue(Phase::Send);
            }
        }

        if let Some(hub) = self.metrics.as_ref() {
            if hub.metrics_enabled() {
                if let Some(ref pool) = txn.selected_pool {
                    hub.builtin.record_query_by_pool(pool);
                }
            }
        }

        let forward_out = self.forward.handle(txn, snapshot);
        match forward_out {
            StageOutcome::Suspend(Phase::WaitResponse) => {
                txn.lookup_forward_step = Some(LookupForwardStep::Wait);
                StageOutcome::Suspend(Phase::Lookup)
            }
            StageOutcome::Continue(Phase::WaitResponse) => {
                self.finish_forward_answer(txn, snapshot)
            }
            StageOutcome::Continue(Phase::ResponseRules) => {
                self.finish_forward_answer(txn, snapshot)
            }
            StageOutcome::Continue(Phase::Send) => StageOutcome::Continue(Phase::Send),
            StageOutcome::Drop => StageOutcome::Drop,
            other => {
                tracing::warn!(?other, "unexpected forward outcome inside lookup");
                StageOutcome::Continue(Phase::Send)
            }
        }
    }

    fn finish_forward_answer(
        &self,
        txn: &mut Transaction,
        snapshot: &Arc<RuntimeSnapshot>,
    ) -> StageOutcome {
        let wait_out = self.wait.handle(txn, snapshot);
        let continue_phase = match wait_out {
            StageOutcome::Continue(Phase::ResponseRules) => Phase::ResponseRules,
            StageOutcome::Continue(Phase::Send) => Phase::Send,
            StageOutcome::Drop => return StageOutcome::Drop,
            other => {
                tracing::warn!(?other, "unexpected wait outcome inside lookup");
                Phase::Send
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

        txn.lookup_outcome = Some(LookupOutcome::Answered);
        txn.answer_source = Some(AnswerSource::Forward);
        txn.cache_instance = None;
        StageOutcome::Continue(continue_phase)
    }

    fn apply_cache_hit(
        &self,
        txn: &mut Transaction,
        wire: Vec<u8>,
        cache_name: &str,
        skip_response_rules: bool,
    ) -> StageOutcome {
        txn.response_wire = Some(wire);
        txn.lookup_outcome = Some(LookupOutcome::Answered);
        // Pipeline state for selectors, Rhai, metrics, and dnstap — not operator tags.
        txn.answer_source = Some(AnswerSource::Cache);
        txn.cache_instance = Some(cache_name.to_string());
        txn.last_forward_ms = 0;
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
        _snapshot: &Arc<RuntimeSnapshot>,
    ) -> Option<StageOutcome> {
        let cache = self.cache.as_ref()?;
        if !snapshot_cache_enabled(_snapshot) {
            return None;
        }
        let now = Instant::now();
        match cache.lookup(cache_name, txn, now) {
            CacheLookupOutcome::Hit {
                wire,
                cache_name,
                skip_response_rules,
            } => Some(self.apply_cache_hit(txn, wire, &cache_name, skip_response_rules)),
            CacheLookupOutcome::Miss { key, gate: _ } => {
                txn.lookup_outcome = Some(LookupOutcome::Miss);
                txn.lookup_cache_fill = Some(LookupCacheFill {
                    cache_name: cache_name.to_string(),
                    key: key.as_bytes().to_vec(),
                });
                None
            }
            CacheLookupOutcome::WaitAsync { key } => {
                txn.lookup_cache_wait = Some(LookupCacheWait {
                    cache_name: cache_name.to_string(),
                    key: key.as_bytes().to_vec(),
                });
                Some(StageOutcome::Suspend(Phase::Lookup))
            }
            CacheLookupOutcome::Bypass => None,
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
            } => self.apply_cache_hit(txn, wire, &cache_name, skip_response_rules),
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
            return self.finish_forward_answer(txn, snapshot);
        }

        let profile_name = txn
            .lookup_profile
            .as_deref()
            .unwrap_or(DEFAULT_LOOKUP_PROFILE);
        let Some(profile) = snapshot.lookup.profiles.get(profile_name) else {
            tracing::error!(profile = profile_name, "unknown lookup profile");
            txn.set_rcode_name("SERVFAIL");
            return StageOutcome::Continue(Phase::Send);
        };
        txn.lookup_profile = Some(profile.name.clone());

        for provider in &profile.providers {
            match provider {
                CompiledLookupProvider::Cache { cache_name } => {
                    if !txn.cache_lookup_eligible {
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
