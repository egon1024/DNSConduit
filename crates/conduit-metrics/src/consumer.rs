//! Metric consumer dependency graph (metrics-configurability Phase E / Gate G5).
//!
//! At snapshot compile, Rhai (and future WASM/sidecar) sites that reference user
//! metrics are recorded here. **Write** sites (`metrics.inc*`) with collect or
//! emit off produce warnings (increments no-op / series stay out of export —
//! same model as built-in categories). **Read** sites (future window/rate APIs)
//! still reject plans that stop collecting a referenced metric.

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

/// Result of checking consumer dependencies against a proposed plan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConsumerDependencyReport {
    /// Hard failures (e.g. read APIs still reference a metric with collect off).
    pub errors: Vec<String>,
    /// Soft notices (write sites with collect and/or emit off).
    pub warnings: Vec<String>,
}

impl ConsumerDependencyReport {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// True for Rhai (and legacy) write APIs — collect off is a no-op, not a reject.
pub fn is_write_consumer_symbol(symbol: &str) -> bool {
    matches!(
        symbol,
        "metrics.inc" | "metrics.inc_labels" | "metric_inc" | "metric_inc_labels"
    )
}

fn format_consumer_list(consumers: &[&MetricConsumerRef]) -> Vec<String> {
    let mut lines = vec!["  Referenced by:".to_string()];
    for c in consumers {
        let loc = match c.line {
            Some(line) => format!("{} (line {line}, {})", c.path, c.symbol),
            None => format!("{} ({})", c.path, c.symbol),
        };
        lines.push(format!("    - {}: {loc}", c.kind.label()));
    }
    lines
}

/// Check proposed plans against the consumer graph.
///
/// - **Write** sites (`metrics.inc*`) with collect off → **warning** (increments
///   no-op, like built-in categories). Collect on / emit off → **warning**.
/// - **Read** sites (future window/rate helpers, non-write symbols) with collect
///   off → **error**.
///
/// When metrics are disabled (`plan.enabled == false`), collection is off for
/// the whole subsystem and script sites are inert — no dependency findings.
pub fn check_consumer_dependencies(
    graph: &MetricConsumerGraph,
    plan: &CompiledMetricsPlan,
) -> ConsumerDependencyReport {
    if !plan.enabled {
        return ConsumerDependencyReport::default();
    }
    let mut report = ConsumerDependencyReport::default();
    for (metric, consumers) in &graph.refs {
        let writes: Vec<&MetricConsumerRef> = consumers
            .iter()
            .filter(|c| is_write_consumer_symbol(&c.symbol))
            .collect();
        let reads: Vec<&MetricConsumerRef> = consumers
            .iter()
            .filter(|c| !is_write_consumer_symbol(&c.symbol))
            .collect();

        let collect = plan.user_collect_for(metric);
        let emit = plan.user_emit_for(metric);
        let family = conduit_user_metric_name(metric);

        if !collect && !reads.is_empty() {
            let mut lines = Vec::with_capacity(4 + reads.len());
            lines.push(format!(
                "metrics configuration rejected: cannot stop collecting metric \"{metric}\""
            ));
            lines.push(format!("  Metric: {family}"));
            lines.push("  Requested change: collect removed".to_string());
            lines.extend(format_consumer_list(&reads));
            let msg = lines.join("\n");
            tracing::warn!(
                metric = %metric,
                family = %family,
                consumers = reads.len(),
                "metrics consumer dependency check failed (read sites require collect)"
            );
            report.errors.push(msg);
        }

        if !writes.is_empty() && (!collect || !emit) {
            let mut lines = Vec::with_capacity(4 + writes.len());
            if !collect {
                lines.push(format!(
                    "metrics: user metric \"{metric}\" ({family}) is referenced by scripts but collect is off — increments are not recorded (same as a built-in category with collect off)"
                ));
            } else {
                // collect on, emit off
                lines.push(format!(
                    "metrics: user metric \"{metric}\" ({family}) is referenced by scripts but emit is off — series stay out of Prometheus scrape and OTLP push"
                ));
            }
            lines.extend(format_consumer_list(&writes));
            let msg = lines.join("\n");
            tracing::warn!(
                metric = %metric,
                family = %family,
                collect,
                emit,
                consumers = writes.len(),
                "user metric referenced while collect and/or emit is off"
            );
            report.warnings.push(msg);
        }
    }
    report
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
    fn accepts_when_collect_and_emit_enabled() {
        let mut graph = MetricConsumerGraph::new();
        graph.record("blat", rhai_ref("rules/a.rhai", 3, "metrics.inc"));
        let cfg = MetricsConfig {
            enabled: Some(true),
            base: "standard".into(),
            ..Default::default()
        };
        let plan = resolve_metrics_plan(Some(&cfg)).unwrap().plan;
        let report = check_consumer_dependencies(&graph, &plan);
        assert!(report.errors.is_empty());
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn write_collect_off_warns_does_not_reject() {
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
                help: String::new(),
            }],
            ..Default::default()
        };
        let plan = resolve_metrics_plan(Some(&cfg)).unwrap().plan;
        assert!(!plan.user_collect_for("blat"));
        let report = check_consumer_dependencies(&graph, &plan);
        assert!(
            report.errors.is_empty(),
            "writes must not reject: {:?}",
            report.errors
        );
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("collect is off"));
        assert!(report.warnings[0].contains("conduit_user_blat"));
        assert!(report.warnings[0].contains("rules/blocklist.rhai"));
        assert!(report.warnings[0].contains("line 42"));
        assert!(report.warnings[0].contains("metrics.inc"));
    }

    #[test]
    fn write_emit_off_warns_when_collecting() {
        let mut graph = MetricConsumerGraph::new();
        graph.record("blat", rhai_ref("rules/a.rhai", 1, "metrics.inc"));
        let cfg = MetricsConfig {
            enabled: Some(true),
            base: "standard".into(),
            user_metrics: vec![UserMetricExportConfig {
                name: "blat".into(),
                export: String::new(),
                collect: Some(true),
                emit: Some(false),
                help: String::new(),
            }],
            ..Default::default()
        };
        let plan = resolve_metrics_plan(Some(&cfg)).unwrap().plan;
        let report = check_consumer_dependencies(&graph, &plan);
        assert!(report.errors.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("emit is off"));
    }

    #[test]
    fn read_collect_off_rejects() {
        let mut graph = MetricConsumerGraph::new();
        graph.record(
            "blat",
            rhai_ref("rules/blocklist.rhai", 42, "metrics.window_rate"),
        );
        let cfg = MetricsConfig {
            enabled: Some(true),
            base: "standard".into(),
            user_metrics: vec![UserMetricExportConfig {
                name: "blat".into(),
                export: String::new(),
                collect: Some(false),
                emit: Some(false),
                help: String::new(),
            }],
            ..Default::default()
        };
        let plan = resolve_metrics_plan(Some(&cfg)).unwrap().plan;
        let report = check_consumer_dependencies(&graph, &plan);
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].contains("cannot stop collecting metric \"blat\""));
        assert!(report.errors[0].contains("metrics.window_rate"));
        assert!(report.warnings.is_empty());
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
                help: String::new(),
            },
        );
        assert!(plan.user_collect_for("x"));
    }
}
