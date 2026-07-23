//! Process-lifetime `MetricStore` (design §Decisions 6, 13; §Runtime architecture).
//!
//! Stable series identity → handle. G1 scope: schema-level identity
//! (metric name + sorted label dimension names) with get-or-create/reuse.
//! Continuity across snapshot generations (drain/removal, §Decisions 6-7) is
//! Phase D / gate G4 — not implemented here.

use parking_lot::RwLock;
use prometheus::{GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, Opts};
use std::collections::HashMap;

/// Stable series identity: metric name + **sorted** label dimension names.
///
/// Per design §Decisions 6: "Series identity = metric name + sorted label
/// dimension schema (+ label values for handle lookup)." The label *values*
/// component is delegated to the underlying `prometheus` `*Vec` type once a
/// schema-level handle is obtained.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SeriesIdentity {
    name: String,
    labels: Vec<String>,
}

impl SeriesIdentity {
    pub fn new(name: impl Into<String>, labels: &[&str]) -> Self {
        let mut labels: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
        labels.sort();
        Self {
            name: name.into(),
            labels,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn labels(&self) -> &[String] {
        &self.labels
    }
}

enum Instrument {
    Counter(IntCounterVec),
    Histogram(HistogramVec),
    Gauge(GaugeVec),
}

/// Process-lifetime handle store (design §Runtime architecture "MetricStore").
pub struct MetricStore {
    inner: RwLock<HashMap<SeriesIdentity, Instrument>>,
}

impl Default for MetricStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricStore {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    /// Number of distinct series identities currently held (test/diagnostic use).
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get-or-create a counter vec for `name` with dimension schema `labels`.
    /// Reuses the existing handle when the schema (name + sorted label set)
    /// already exists — cumulative values are not reset (design §Decisions 6).
    pub fn get_or_create_counter(&self, name: &str, help: &str, labels: &[&str]) -> IntCounterVec {
        let identity = SeriesIdentity::new(name, labels);
        {
            let guard = self.inner.read();
            if let Some(Instrument::Counter(v)) = guard.get(&identity) {
                return v.clone();
            }
        }
        let mut guard = self.inner.write();
        if let Some(Instrument::Counter(v)) = guard.get(&identity) {
            return v.clone();
        }
        let v = IntCounterVec::new(Opts::new(name, help), labels).expect("metric");
        guard.insert(identity, Instrument::Counter(v.clone()));
        v
    }

    /// Get-or-create a histogram vec for `name` with dimension schema `labels`.
    /// A schema change (different label set for the same name) is a **new**
    /// identity — no faux continuity from old label combinations (design §Decisions 8).
    pub fn get_or_create_histogram(
        &self,
        name: &str,
        help: &str,
        labels: &[&str],
        buckets: Vec<f64>,
    ) -> HistogramVec {
        let identity = SeriesIdentity::new(name, labels);
        {
            let guard = self.inner.read();
            if let Some(Instrument::Histogram(v)) = guard.get(&identity) {
                return v.clone();
            }
        }
        let mut guard = self.inner.write();
        if let Some(Instrument::Histogram(v)) = guard.get(&identity) {
            return v.clone();
        }
        let v = HistogramVec::new(HistogramOpts::new(name, help).buckets(buckets), labels)
            .expect("metric");
        guard.insert(identity, Instrument::Histogram(v.clone()));
        v
    }

    /// Get-or-create a gauge vec for `name` with dimension schema `labels`.
    pub fn get_or_create_gauge(&self, name: &str, help: &str, labels: &[&str]) -> GaugeVec {
        let identity = SeriesIdentity::new(name, labels);
        {
            let guard = self.inner.read();
            if let Some(Instrument::Gauge(v)) = guard.get(&identity) {
                return v.clone();
            }
        }
        let mut guard = self.inner.write();
        if let Some(Instrument::Gauge(v)) = guard.get(&identity) {
            return v.clone();
        }
        let v = GaugeVec::new(Opts::new(name, help), labels).expect("metric");
        guard.insert(identity, Instrument::Gauge(v.clone()));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_ignores_label_declaration_order() {
        let a = SeriesIdentity::new("conduit_x_total", &["pool", "backend"]);
        let b = SeriesIdentity::new("conduit_x_total", &["backend", "pool"]);
        assert_eq!(a, b);
    }

    #[test]
    fn get_or_create_counter_reuses_handle_for_same_schema() {
        let store = MetricStore::new();
        let c1 = store.get_or_create_counter("conduit_queries_total", "help", &["pool"]);
        c1.with_label_values(&["default"]).inc_by(5);
        let c2 = store.get_or_create_counter("conduit_queries_total", "help", &["pool"]);
        // Same schema (even if we constructed it again) -> same handle -> value persists.
        assert_eq!(c2.with_label_values(&["default"]).get(), 5);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn different_label_schema_same_name_creates_distinct_identity() {
        let store = MetricStore::new();
        let coarse = store.get_or_create_counter("conduit_queries_total", "help", &["listener"]);
        coarse.with_label_values(&["ln"]).inc();
        let fine =
            store.get_or_create_counter("conduit_queries_total", "help", &["listener", "qtype"]);
        fine.with_label_values(&["ln", "A"]).inc();

        assert_eq!(
            store.len(),
            2,
            "schema change must create a distinct identity"
        );
        // The two handles are independent counters (no faux continuity).
        assert_eq!(coarse.with_label_values(&["ln"]).get(), 1);
        assert_eq!(fine.with_label_values(&["ln", "A"]).get(), 1);
    }

    #[test]
    fn histogram_get_or_create_reuses_by_schema() {
        let store = MetricStore::new();
        let h1 = store.get_or_create_histogram(
            "conduit_forward_duration_seconds",
            "help",
            &["pool"],
            vec![0.01, 0.1, 1.0],
        );
        h1.with_label_values(&["default"]).observe(0.05);
        let h2 = store.get_or_create_histogram(
            "conduit_forward_duration_seconds",
            "help",
            &["pool"],
            vec![0.01, 0.1, 1.0],
        );
        assert_eq!(h2.with_label_values(&["default"]).get_sample_count(), 1);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn gauge_get_or_create_reuses_by_schema() {
        let store = MetricStore::new();
        let g1 = store.get_or_create_gauge("conduit_slots_in_use", "help", &["pool"]);
        g1.with_label_values(&["default"]).set(3.0);
        let g2 = store.get_or_create_gauge("conduit_slots_in_use", "help", &["pool"]);
        assert_eq!(g2.with_label_values(&["default"]).get(), 3.0);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn distinct_names_are_distinct_identities_even_with_same_schema() {
        let store = MetricStore::new();
        store.get_or_create_counter("conduit_a_total", "help", &["pool"]);
        store.get_or_create_counter("conduit_b_total", "help", &["pool"]);
        assert_eq!(store.len(), 2);
    }
}
