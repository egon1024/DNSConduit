//! Rhai user-defined metrics runtime storage.

use prometheus::{IntCounter, Registry};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct UserMetricDelta {
    pub name: String,
    pub labels: HashMap<String, String>,
    pub delta: u64,
}

pub struct UserRegistry {
    enabled: bool,
    registry: Registry,
    counters: Mutex<HashMap<String, IntCounter>>,
}

impl UserRegistry {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            registry: Registry::new(),
            counters: Mutex::new(HashMap::new()),
        }
    }

    pub fn add_delta(&self, delta: UserMetricDelta) {
        if !self.enabled || delta.delta == 0 {
            return;
        }
        let key = label_key(&delta.name, &delta.labels);
        let mut map = self.counters.lock().unwrap();
        if !map.contains_key(&key) {
            let metric_name = sanitize_metric_name(&delta.name);
            let counter =
                IntCounter::new(metric_name, "Rhai user-defined metric").expect("counter");
            self.registry
                .register(Box::new(counter.clone()))
                .expect("register");
            map.insert(key.clone(), counter);
        }
        map.get(&key).unwrap().inc_by(delta.delta);
    }

    pub fn gather(&self) -> Vec<prometheus::proto::MetricFamily> {
        self.registry.gather()
    }
}

fn label_key(name: &str, labels: &HashMap<String, String>) -> String {
    let mut pairs: Vec<_> = labels.iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    let mut key = name.to_string();
    for (k, v) in pairs {
        key.push('|');
        key.push_str(k);
        key.push('=');
        key.push_str(v);
    }
    key
}

fn sanitize_metric_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 16);
    out.push_str("conduit_user_");
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cumulative_user_metric() {
        let reg = UserRegistry::new(true);
        reg.add_delta(UserMetricDelta {
            name: "block_hits".into(),
            labels: HashMap::new(),
            delta: 1,
        });
        reg.add_delta(UserMetricDelta {
            name: "block_hits".into(),
            labels: HashMap::new(),
            delta: 2,
        });
        let families = reg.gather();
        assert!(!families.is_empty());
    }
}
