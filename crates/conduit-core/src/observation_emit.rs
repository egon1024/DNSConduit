//! Dataplane hook helpers for observation enqueue.

use crate::snapshot::RuntimeSnapshot;
use crate::transaction::{ClientProtocol, Transaction};
use conduit_observation::{ObservationHub, TxnExtraSource, TxnView};
use std::sync::Arc;

fn extra_source(txn: &Transaction, include_tags: bool) -> TxnExtraSource {
    let (tag_bools, tag_strings) = if include_tags {
        txn.tags.export_all_tags()
    } else {
        (Vec::new(), Vec::new())
    };
    TxnExtraSource {
        pool: txn.selected_pool.clone(),
        backend: txn.selected_backend.map(|a| a.to_string()),
        attempt_count: txn.attempt_count,
        txn_id: txn.id,
        qname: txn.qname.clone(),
        rcode_label: txn.rcode_label(),
        client: txn.client_addr.to_string(),
        tag_bools,
        tag_strings,
    }
}

pub fn txn_view<'a>(txn: &'a Transaction, snapshot: &RuntimeSnapshot) -> TxnView<'a> {
    TxnView {
        txn_id: txn.id,
        client_addr: txn.client_addr,
        protocol_udp: txn.protocol == ClientProtocol::Udp,
        qname: txn.qname.as_deref(),
        query_wire: &txn.query_wire,
        response_wire: txn.response_wire.as_deref(),
        attempt_count: txn.attempt_count,
        extra: extra_source(txn, snapshot.observation.needs_tag_export()),
    }
}

pub fn emit_query(hub: &ObservationHub, txn: &Transaction, snapshot: &Arc<RuntimeSnapshot>) {
    let view = txn_view(txn, snapshot);
    hub.try_enqueue_query(view, &snapshot.observation, |k| txn.tags.has(k));
}

pub fn emit_response(hub: &ObservationHub, txn: &Transaction, snapshot: &Arc<RuntimeSnapshot>) {
    let view = txn_view(txn, snapshot);
    hub.try_enqueue_response(view, &snapshot.observation, |k| txn.tags.has(k));
}

pub fn emit_retry(hub: &ObservationHub, txn: &Transaction, snapshot: &Arc<RuntimeSnapshot>) {
    let view = txn_view(txn, snapshot);
    hub.try_enqueue_retry(view, &snapshot.observation, |k| txn.tags.has(k));
}
