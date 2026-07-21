//! Client IP ACL enforcement gate for dataplane ingress (client-acls §4, §6).
//!
//! One [`AclGate`] lives per ingress worker (each worker binds a single
//! listener). It holds the effective compiled policy for that listener and is
//! recompiled in place when the runtime snapshot generation changes, so ACL
//! edits apply on hot reload without a restart. The gate runs the two evaluation
//! tiers, records `conduit_acl_decisions_total`, and emits sampled denial logs.

use conduit_core::acl::{effective_acl, evaluate_full, evaluate_preadmission, AclDecision};
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_core::CompiledAclPolicy;
use conduit_metrics::MetricsHub;
use conduit_proto::config::AclsConfig;
use conduit_script::DataSourceStore;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};

const TIER_PREADMISSION: &str = "preadmission";
const TIER_LISTENER: &str = "listener";

const ACTION_DROP: &str = "drop";
const ACTION_REFUSE: &str = "refuse";
const ACTION_TAG: &str = "tag";
const ACTION_ADMIT: &str = "admit";

/// What ingress should do with a query after ACL evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AclGateOutcome {
    /// Proceed: acquire a slot and run the pipeline.
    Admit,
    /// Proceed, but set the named tag on the transaction first.
    AdmitTagged(String),
    /// Terminal deny: no DNS reply, no slot consumed.
    Drop,
    /// Terminal deny: send REFUSED, no slot consumed.
    Refuse,
}

/// Per-worker client IP ACL gate. Recompiled on snapshot generation change.
pub struct AclGate {
    listener_label: String,
    /// This listener's own `acls:` (replaces global when set); fixed for the worker.
    listener_acls: Option<AclsConfig>,
    /// Snapshot generation the compiled policy + logging config were built from.
    generation: Option<u64>,
    /// `None` when no `acls:` apply to this listener (admit-all fast path).
    compiled: Option<CompiledAclPolicy>,
    log: AclDeniedLog,
}

impl AclGate {
    pub fn new(listener_acls: Option<AclsConfig>, listener_label: String) -> Self {
        Self {
            listener_label,
            listener_acls,
            generation: None,
            compiled: None,
            log: AclDeniedLog::default(),
        }
    }

    /// Tier 0 only: evaluate explicit `drop` matches before structural parse.
    /// Call this first so known-bad clients never pay for parse or a slot.
    pub fn decide_preadmission(
        &mut self,
        snap: &RuntimeSnapshot,
        client_ip: IpAddr,
        metrics: &MetricsHub,
    ) -> AclGateOutcome {
        self.refresh(snap);
        if self.compiled.is_none() {
            return AclGateOutcome::Admit;
        }
        self.preadmission(client_ip, &snap.scripting.data_sources, metrics)
    }

    /// Tier 1: full first-match evaluation after a successful structural parse,
    /// still before slot acquire. Call after [`Self::decide_preadmission`].
    pub fn decide_full(
        &mut self,
        snap: &RuntimeSnapshot,
        client_ip: IpAddr,
        metrics: &MetricsHub,
    ) -> AclGateOutcome {
        self.refresh(snap);
        if self.compiled.is_none() {
            return AclGateOutcome::Admit;
        }
        self.full(client_ip, &snap.scripting.data_sources, metrics)
    }

    /// Evaluate both ACL tiers for `client_ip` (preadmission then full). Prefer
    /// the split methods at ingress so Tier 0 can run before structural parse.
    pub fn decide(
        &mut self,
        snap: &RuntimeSnapshot,
        client_ip: IpAddr,
        metrics: &MetricsHub,
    ) -> AclGateOutcome {
        match self.decide_preadmission(snap, client_ip, metrics) {
            AclGateOutcome::Admit => self.decide_full(snap, client_ip, metrics),
            terminal => terminal,
        }
    }

    fn refresh(&mut self, snap: &RuntimeSnapshot) {
        if self.generation == Some(snap.generation) {
            return;
        }
        self.generation = Some(snap.generation);
        let global = snap.config.acls.as_ref();
        let listener = self.listener_acls.as_ref();
        self.compiled = effective_acl(global, listener)
            .map(|_| CompiledAclPolicy::compile_effective(global, listener));
        self.log.configure(
            snap.config
                .logging
                .as_ref()
                .and_then(|l| l.query_access.as_ref()),
        );
    }

    fn preadmission(
        &self,
        client_ip: IpAddr,
        store: &DataSourceStore,
        metrics: &MetricsHub,
    ) -> AclGateOutcome {
        let Some(policy) = self.compiled.as_ref() else {
            return AclGateOutcome::Admit;
        };
        match evaluate_preadmission(policy, client_ip, store) {
            AclDecision::Drop => {
                metrics.builtin.record_acl_decision(
                    TIER_PREADMISSION,
                    ACTION_DROP,
                    &self.listener_label,
                    client_ip,
                );
                self.log.maybe_log(
                    policy,
                    client_ip,
                    store,
                    TIER_PREADMISSION,
                    ACTION_DROP,
                    &self.listener_label,
                );
                AclGateOutcome::Drop
            }
            _ => AclGateOutcome::Admit,
        }
    }

    fn full(
        &self,
        client_ip: IpAddr,
        store: &DataSourceStore,
        metrics: &MetricsHub,
    ) -> AclGateOutcome {
        let Some(policy) = self.compiled.as_ref() else {
            return AclGateOutcome::Admit;
        };
        let decision = evaluate_full(policy, client_ip, store);
        let action = match &decision {
            AclDecision::Admit => ACTION_ADMIT,
            AclDecision::Drop => ACTION_DROP,
            AclDecision::Refuse => ACTION_REFUSE,
            AclDecision::Tag(_) => ACTION_TAG,
        };
        metrics
            .builtin
            .record_acl_decision(TIER_LISTENER, action, &self.listener_label, client_ip);
        match decision {
            AclDecision::Admit => AclGateOutcome::Admit,
            AclDecision::Tag(tag) => AclGateOutcome::AdmitTagged(tag),
            AclDecision::Drop => {
                self.log.maybe_log(
                    policy,
                    client_ip,
                    store,
                    TIER_LISTENER,
                    ACTION_DROP,
                    &self.listener_label,
                );
                AclGateOutcome::Drop
            }
            AclDecision::Refuse => {
                self.log.maybe_log(
                    policy,
                    client_ip,
                    store,
                    TIER_LISTENER,
                    ACTION_REFUSE,
                    &self.listener_label,
                );
                AclGateOutcome::Refuse
            }
        }
    }
}

/// The `type: cidr` source (view) whose first match produced the decision, or
/// `"default"` when no rule matched (default-action fall-through).
fn matched_view<'p>(policy: &'p CompiledAclPolicy, ip: IpAddr, store: &DataSourceStore) -> &'p str {
    policy
        .rules
        .iter()
        .find(|r| store.lookup_ip(&r.view, ip).is_some())
        .map(|r| r.view.as_str())
        .unwrap_or("default")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum AclLogLevel {
    #[default]
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl AclLogLevel {
    fn parse(value: &str) -> Self {
        match value {
            "error" => Self::Error,
            "warn" => Self::Warn,
            "info" => Self::Info,
            "debug" => Self::Debug,
            "trace" => Self::Trace,
            _ => Self::Off,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
enum AclLogSample {
    #[default]
    All,
    /// Sample a fraction of distinct client IPs (0-100 percent).
    PerSource(f64),
    /// Emit one of every N denials on this worker.
    EveryNth(u32),
}

/// Resolved denial-logging config for one worker. Sampling here affects only log
/// emission — never metrics or enforcement.
#[derive(Default)]
struct AclDeniedLog {
    level: AclLogLevel,
    sample: AclLogSample,
    every_nth_counter: AtomicU64,
}

impl AclDeniedLog {
    fn configure(&mut self, qa: Option<&conduit_proto::config::QueryAccessLogging>) {
        let Some(qa) = qa else {
            self.level = AclLogLevel::Off;
            self.sample = AclLogSample::All;
            return;
        };
        self.level = AclLogLevel::parse(qa.acl_denied.as_str());
        self.sample = match qa.acl_denied_sample.as_ref() {
            Some(s) if s.mode == "per_source" => AclLogSample::PerSource(s.rate.unwrap_or(100.0)),
            Some(s) if s.mode == "every_nth" => AclLogSample::EveryNth(s.nth.unwrap_or(1).max(1)),
            _ => AclLogSample::All,
        };
    }

    fn should_emit(&self, client_ip: IpAddr) -> bool {
        match self.sample {
            AclLogSample::All => true,
            AclLogSample::PerSource(rate) => {
                if rate >= 100.0 {
                    return true;
                }
                if rate <= 0.0 {
                    return false;
                }
                let mut hasher = DefaultHasher::new();
                client_ip.hash(&mut hasher);
                (hasher.finish() % 10_000) < (rate * 100.0) as u64
            }
            AclLogSample::EveryNth(nth) => {
                let n = self.every_nth_counter.fetch_add(1, Ordering::Relaxed) + 1;
                n % u64::from(nth) == 0
            }
        }
    }

    fn maybe_log(
        &self,
        policy: &CompiledAclPolicy,
        client_ip: IpAddr,
        store: &DataSourceStore,
        stage: &str,
        action: &str,
        listener: &str,
    ) {
        if self.level == AclLogLevel::Off {
            return;
        }
        if !self.should_emit(client_ip) {
            return;
        }
        let view = matched_view(policy, client_ip, store);
        let ip_family = if client_ip.is_ipv6() { "v6" } else { "v4" };
        match self.level {
            AclLogLevel::Off => {}
            AclLogLevel::Error => tracing::error!(
                client_ip = %client_ip, listener, view, action, stage, ip_family,
                "client ACL denied query"
            ),
            AclLogLevel::Warn => tracing::warn!(
                client_ip = %client_ip, listener, view, action, stage, ip_family,
                "client ACL denied query"
            ),
            AclLogLevel::Info => tracing::info!(
                client_ip = %client_ip, listener, view, action, stage, ip_family,
                "client ACL denied query"
            ),
            AclLogLevel::Debug => tracing::debug!(
                client_ip = %client_ip, listener, view, action, stage, ip_family,
                "client ACL denied query"
            ),
            AclLogLevel::Trace => tracing::trace!(
                client_ip = %client_ip, listener, view, action, stage, ip_family,
                "client ACL denied query"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_proto::config::{AclDeniedSample, QueryAccessLogging};

    #[test]
    fn every_nth_emits_one_in_n_per_worker() {
        let mut log = AclDeniedLog::default();
        log.configure(Some(&QueryAccessLogging {
            acl_denied: "warn".into(),
            acl_denied_sample: Some(AclDeniedSample {
                mode: "every_nth".into(),
                rate: None,
                nth: Some(3),
            }),
        }));
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let emitted: usize = (0..9).filter(|_| log.should_emit(ip)).count();
        assert_eq!(emitted, 3);
    }

    #[test]
    fn per_source_is_stable_per_ip() {
        let mut log = AclDeniedLog::default();
        log.configure(Some(&QueryAccessLogging {
            acl_denied: "warn".into(),
            acl_denied_sample: Some(AclDeniedSample {
                mode: "per_source".into(),
                rate: Some(50.0),
                nth: None,
            }),
        }));
        let ip: IpAddr = "203.0.113.9".parse().unwrap();
        let first = log.should_emit(ip);
        for _ in 0..100 {
            assert_eq!(log.should_emit(ip), first);
        }
    }

    #[test]
    fn off_by_default_and_full_rate_always_emits() {
        let mut log = AclDeniedLog::default();
        assert_eq!(log.level, AclLogLevel::Off);
        log.configure(Some(&QueryAccessLogging {
            acl_denied: "info".into(),
            acl_denied_sample: None,
        }));
        assert_eq!(log.level, AclLogLevel::Info);
        assert!(log.should_emit("10.0.0.1".parse().unwrap()));
    }
}
