//! Metric consumer dependency graph (metrics-configurability Phase E / Gate G5).
//!
//! At snapshot compile, Rhai (and future WASM/sidecar) sites that reference user
//! metrics are recorded here. Validate/apply reject plans that stop collecting a
//! metric still referenced by any consumer.

use crate::plan::CompiledMetricsPlan;
use std::collections::BTreeMap;

/// Kind of consumer that references a metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConsumerKind {
    Rhai,
    /// Placeholder until WASM metric imports ship.
    Wasm,
    /// Placeholder until sidecar metric capabilities ship.
    Sidecar,
}

impl ConsumerKind {
    fn label(self) -> &'static str {
        match self {
            Self::Rhai => "rhai",
            Self::Wasm => "wasm",
            Self::Sidecar => "sidecar",
        }
    }
}

/// One reference site for a user metric (bare name, no `conduit_user_` prefix).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MetricConsumerRef {
    pub kind: ConsumerKind,
    /// Script/module path as known at compile time (config-relative when available).
    pub path: String,
    /// 1-based line number when known (Rhai static scan).
    pub line: Option<u32>,
    /// Call or import symbol, e.g. `metrics.inc` or a WASM import name.
    pub symbol: String,
}

/// Extensible map of user metric bare names → referencing consumers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetricConsumerGraph {
    /// Bare metric name → consumer sites (deduped, sorted for stable errors).
    refs: BTreeMap<String, Vec<MetricConsumerRef>>,
}

impl MetricConsumerGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a consumer site for `metric` (bare name).
    pub fn record(&mut self, metric: impl Into<String>, consumer: MetricConsumerRef) {
        let metric = metric.into();
        let entry = self.refs.entry(metric).or_default();
        if !entry.contains(&consumer) {
            entry.push(consumer);
            entry.sort();
        }
    }

    /// Merge another graph's sites into this one.
    pub fn extend(&mut self, other: &MetricConsumerGraph) {
        for (metric, sites) in &other.refs {
            for site in sites {
                self.record(metric.clone(), site.clone());
            }
        }
    }

    /// Hook for future WASM / sidecar scanners (no-op until those modules ship).
    pub fn extend_from_stub_registries(&mut self) {
        self.extend(&WasmConsumerRegistry::scan());
        self.extend(&SidecarConsumerRegistry::scan());
    }

    pub fn is_empty(&self) -> bool {
        self.refs.is_empty()
    }

    pub fn metrics(&self) -> impl Iterator<Item = &str> {
        self.refs.keys().map(|s| s.as_str())
    }

    pub fn consumers_for(&self, metric: &str) -> &[MetricConsumerRef] {
        self.refs.get(metric).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

/// Stub WASM consumer source — returns an empty graph until phase 7 WASM imports.
#[derive(Debug, Default)]
pub struct WasmConsumerRegistry;

impl WasmConsumerRegistry {
    pub fn scan() -> MetricConsumerGraph {
        MetricConsumerGraph::new()
    }
}

/// Stub sidecar consumer source — returns an empty graph until sidecar capabilities.
#[derive(Debug, Default)]
pub struct SidecarConsumerRegistry;

impl SidecarConsumerRegistry {
    pub fn scan() -> MetricConsumerGraph {
        MetricConsumerGraph::new()
    }
}

/// Canonical Prometheus/OTLP family name for a Rhai user metric bare name.
pub fn conduit_user_metric_name(bare: &str) -> String {
    format!("conduit_user_{bare}")
}

/// Reject proposed plans that stop collecting a user metric still referenced by
/// any consumer in `graph`. Returns formatted error strings (may be multi-line).
///
/// When metrics are disabled (`plan.enabled == false`), collection is off for
/// the whole subsystem and script `inc` sites are inert — no dependency error.
pub fn check_consumer_dependencies(
    graph: &MetricConsumerGraph,
    plan: &CompiledMetricsPlan,
) -> Vec<String> {
    if !plan.enabled {
        return Vec::new();
    }
    let mut errors = Vec::new();
    for (metric, consumers) in &graph.refs {
        if plan.user_collect_for(metric) {
            continue;
        }
        let family = conduit_user_metric_name(metric);
        let mut lines = Vec::with_capacity(4 + consumers.len());
        lines.push(format!(
            "metrics configuration rejected: cannot stop collecting metric \"{metric}\""
        ));
        lines.push(format!("  Metric: {family}"));
        lines.push("  Requested change: collect removed".to_string());
        lines.push("  Referenced by:".to_string());
        for c in consumers {
            let loc = match c.line {
                Some(line) => format!("{} (line {line}, {})", c.path, c.symbol),
                None => format!("{} ({})", c.path, c.symbol),
            };
            lines.push(format!("    - {}: {loc}", c.kind.label()));
        }
        let msg = lines.join("\n");
        tracing::warn!(
            metric = %metric,
            family = %family,
            consumers = consumers.len(),
            "metrics consumer dependency check failed"
        );
        errors.push(msg);
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{resolve_metrics_plan, UserMetricMode};
    use conduit_proto::config::{MetricsConfig, UserMetricExportConfig};

    fn rhai_ref(path: &str, line: u32, symbol: &str) -> MetricConsumerRef {
        MetricConsumerRef {
            kind: ConsumerKind::Rhai,
            path: path.into(),
            line: Some(line),
            symbol: symbol.into(),
        }
    }

    #[test]
    fn accepts_when_collect_enabled() {
        let mut graph = MetricConsumerGraph::new();
        graph.record("blat", rhai_ref("rules/a.rhai", 3, "metrics.inc"));
        let cfg = MetricsConfig {
            enabled: Some(true),
            base: "standard".into(),
            ..Default::default()
        };
        let plan = resolve_metrics_plan(Some(&cfg)).unwrap().plan;
        assert!(check_consumer_dependencies(&graph, &plan).is_empty());
    }

    #[test]
    fn rejects_collect_removed_with_script_path() {
        let mut graph = MetricConsumerGraph::new();
        graph.record("blat", rhai_ref("rules/blocklist.rhai", 42, "metrics.inc"));
        let cfg = MetricsConfig {
            enabled: Some(true),
            base: "standard".into(),
            user_metrics: vec![UserMetricExportConfig {
                name: "blat".into(),
                export: String::new(),
                collect: Some(false),
                emit: Some(false),
            }],
            ..Default::default()
        };
        let plan = resolve_metrics_plan(Some(&cfg)).unwrap().plan;
        assert!(!plan.user_collect_for("blat"));
        let errs = check_consumer_dependencies(&graph, &plan);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("cannot stop collecting metric \"blat\""));
        assert!(errs[0].contains("conduit_user_blat"));
        assert!(errs[0].contains("rules/blocklist.rhai"));
        assert!(errs[0].contains("line 42"));
        assert!(errs[0].contains("metrics.inc"));
    }

    #[test]
    fn stub_registries_are_empty() {
        let mut graph = MetricConsumerGraph::new();
        graph.extend_from_stub_registries();
        assert!(graph.is_empty());
    }

    #[test]
    fn user_metric_mode_roundtrip_in_plan_map() {
        let mut plan = resolve_metrics_plan(Some(&MetricsConfig {
            enabled: Some(true),
            base: "minimal".into(),
            ..Default::default()
        }))
        .unwrap()
        .plan;
        plan.user_metrics.insert(
            "x".into(),
            UserMetricMode {
                collect: true,
                emit: true,
            },
        );
        assert!(plan.user_collect_for("x"));
    }
}
