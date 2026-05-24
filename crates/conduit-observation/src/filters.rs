//! Per-sink observation filter evaluation (phase 2.7).

use crate::compile::CompiledSinkFilters;
use crate::event::EventKind;
use crate::selectors::{hash_sample, SelectorMatchCtx};
use crate::view::TxnView;

pub fn sink_event_matches(
    filters: &CompiledSinkFilters,
    kind: EventKind,
    view: &TxnView<'_>,
    tag_has: &dyn Fn(&str) -> bool,
) -> bool {
    if let Some(ref key) = filters.tag_required {
        if !tag_has(key) {
            return false;
        }
    }

    if !filters.selectors.is_empty() {
        let ctx = SelectorMatchCtx {
            qname: view.qname,
            qtype_label: view.qtype_label.clone(),
            rcode_label: view.extra.rcode_label.clone(),
            tag_has,
        };
        if !filters.selectors.iter().all(|s| s.matches_ctx(&ctx)) {
            return false;
        }
    }

    if kind != EventKind::Query {
        if let Some(ref pool) = filters.pool {
            if view.extra.pool.as_deref() != Some(pool.as_str()) {
                return false;
            }
        }
        if let Some(ref backend) = filters.backend {
            if view.extra.backend.as_deref() != Some(backend.as_str()) {
                return false;
            }
        }
    }

    hash_sample(view.txn_id, filters.sample_rate)
}
