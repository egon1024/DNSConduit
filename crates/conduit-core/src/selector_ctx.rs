//! Build shared selector match context from a transaction.

use crate::transaction::Transaction;
use conduit_events::SelectorMatchCtx;

/// Selector inputs derived from pipeline transaction state (not operator tags).
pub fn selector_match_ctx<'a>(
    txn: &'a Transaction,
    tag_has: &'a dyn Fn(&str) -> bool,
) -> SelectorMatchCtx<'a> {
    SelectorMatchCtx {
        txn_id: txn.id,
        global_query_index: txn.global_query_index,
        qname: txn.qname.as_deref(),
        qtype: txn.qtype,
        rcode: txn.rcode(),
        qclass: txn.qclass,
        opcode: txn.opcode,
        edns_option_codes: &txn.edns_option_codes,
        answer_source: txn.answer_source.map(|s| s.as_str()),
        cache_instance: txn.cache_instance.as_deref(),
        tag_has,
    }
}
