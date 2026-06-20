//! Rhai script error reporting: built-in metrics and throttled logging.

use conduit_metrics::BuiltinRegistry;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const WARN_INTERVAL: Duration = Duration::from_secs(60);
const MILESTONES: [u64; 6] = [1, 10, 100, 1_000, 10_000, 100_000];

static SCRIPT_ERRORS: AtomicU64 = AtomicU64::new(0);

struct LookupWarnEntry {
    total: u64,
    since_last_log: u64,
    last_log_at: Option<Instant>,
    next_milestone_idx: usize,
}

struct LookupWarnState {
    generation: u64,
    entries: HashMap<(String, String), LookupWarnEntry>,
}

static LOOKUP_WARN: OnceLock<Mutex<LookupWarnState>> = OnceLock::new();

fn lookup_warn_state() -> &'static Mutex<LookupWarnState> {
    LOOKUP_WARN.get_or_init(|| {
        Mutex::new(LookupWarnState {
            generation: 0,
            entries: HashMap::new(),
        })
    })
}

/// Process-wide script error count (all reasons); mirrors exported `conduit_script_errors_total`.
pub fn rhai_script_errors_total() -> u64 {
    SCRIPT_ERRORS.load(Ordering::Relaxed)
}

/// Coarse error category for `conduit_script_errors_total{reason=…}`.
pub fn classify_script_error(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if lower.contains("not available in response phase")
        || lower.contains("not available in request phase")
    {
        conduit_metrics::SCRIPT_ERROR_PHASE_GUARD
    } else if lower.contains("script hook timeout") {
        conduit_metrics::SCRIPT_ERROR_TIMEOUT
    } else if lower.contains("operations limit") || lower.contains("too many operations") {
        conduit_metrics::SCRIPT_ERROR_OPERATION_LIMIT
    } else {
        conduit_metrics::SCRIPT_ERROR_EVAL
    }
}

/// Sanitize `table` for metric labels — bounded cardinality for dynamic names.
pub fn table_label_for_metric(table: &str) -> String {
    if table.len() > 64 {
        return "other".into();
    }
    if table.is_empty() {
        return String::new();
    }
    if table
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        table.to_string()
    } else {
        "other".into()
    }
}

pub fn record_script_error(
    builtin: Option<&BuiltinRegistry>,
    reason: &str,
    script: &str,
    table: &str,
) {
    SCRIPT_ERRORS.fetch_add(1, Ordering::Relaxed);
    if let Some(b) = builtin {
        b.record_script_error(reason, script, table);
    }
}

pub fn report_script_eval_error(
    builtin: Option<&BuiltinRegistry>,
    script: &str,
    rule: &str,
    message: &str,
) {
    let reason = classify_script_error(message);
    record_script_error(builtin, reason, script, "");
    tracing::warn!(
        script = %script,
        rule = %rule,
        reason = %reason,
        error = %message,
        "rhai script error"
    );
}

pub fn report_lookup_unknown_table(
    builtin: Option<&BuiltinRegistry>,
    snapshot_generation: u64,
    script: &str,
    rule: &str,
    table: &str,
) {
    let table_label = table_label_for_metric(table);
    record_script_error(
        builtin,
        conduit_metrics::SCRIPT_ERROR_LOOKUP_UNKNOWN_TABLE,
        script,
        &table_label,
    );

    let key = (script.to_string(), table_label.clone());
    let mut guard = lookup_warn_state().lock().expect("lookup warn lock");
    if guard.generation != snapshot_generation {
        guard.entries.clear();
        guard.generation = snapshot_generation;
    }
    let entry = guard.entries.entry(key).or_insert(LookupWarnEntry {
        total: 0,
        since_last_log: 0,
        last_log_at: None,
        next_milestone_idx: 0,
    });
    entry.total += 1;
    entry.since_last_log += 1;

    let milestone = next_milestone_hit(entry.total, &mut entry.next_milestone_idx);
    let periodic = entry
        .last_log_at
        .map(|t| t.elapsed() >= WARN_INTERVAL)
        .unwrap_or(false);

    if milestone || periodic {
        tracing::warn!(
            script = %script,
            rule = %rule,
            table = %table,
            table_label = %table_label,
            total = entry.total,
            since_last_log = entry.since_last_log,
            periodic,
            "unknown data source in table_lookup; returning empty string"
        );
        entry.since_last_log = 0;
        entry.last_log_at = Some(Instant::now());
    }
}

fn next_milestone_hit(total: u64, next_idx: &mut usize) -> bool {
    if *next_idx < MILESTONES.len() && total >= MILESTONES[*next_idx] {
        *next_idx += 1;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_phase_guard() {
        assert_eq!(
            classify_script_error("set_source_v4() is not available in response phase"),
            conduit_metrics::SCRIPT_ERROR_PHASE_GUARD
        );
    }

    #[test]
    fn classify_timeout() {
        assert_eq!(
            classify_script_error("script hook timeout"),
            conduit_metrics::SCRIPT_ERROR_TIMEOUT
        );
    }

    #[test]
    fn table_label_sanitizes_unsafe() {
        assert_eq!(table_label_for_metric("blocklist"), "blocklist");
        assert_eq!(table_label_for_metric("bad name"), "other");
    }

    #[test]
    fn milestone_fires_at_one_ten_hundred() {
        let mut idx = 0;
        assert!(next_milestone_hit(1, &mut idx));
        assert!(!next_milestone_hit(2, &mut idx));
        assert!(next_milestone_hit(10, &mut idx));
        assert!(next_milestone_hit(100, &mut idx));
    }
}
