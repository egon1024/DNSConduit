//! Per-sink observation filter evaluation (phase 2.7).

use crate::compile::CompiledSinkFilters;
use crate::event::EventKind;
use crate::selectors::{hash_sample_keyed, resolve_sample_key, SelectorMatchCtx};
use crate::view::TxnView;

fn selector_ctx<'a>(
    view: &'a TxnView<'a>,
    tag_has: &'a dyn Fn(&str) -> bool,
) -> SelectorMatchCtx<'a> {
    SelectorMatchCtx {
        txn_id: view.txn_id,
        global_query_index: view.global_query_index,
        qname: view.qname,
        qtype: view.qtype,
        rcode: view.rcode,
        qclass: view.qclass,
        opcode: view.opcode,
        edns_option_codes: view.edns_option_codes,
        answer_source: view.answer_source,
        cache_instance: view.cache_instance,
        tag_has,
        // `client_cidr` is rule-only; sink filters never carry this selector.
        client_cidr_match: None,
    }
}

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
        let ctx = selector_ctx(view, tag_has);
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

    let ctx = selector_ctx(view, tag_has);
    let salt = resolve_sample_key(&filters.sample_key, &ctx);
    hash_sample_keyed(view.txn_id, filters.sample_percent / 100.0, salt.as_deref())
}
