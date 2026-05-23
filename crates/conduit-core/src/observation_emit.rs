//! Dataplane hook helpers for observation enqueue.

use crate::snapshot::RuntimeSnapshot;
use crate::transaction::{ClientProtocol, Transaction};
use conduit_observation::{ObservationHub, TxnView};
use std::sync::Arc;

pub fn txn_view<'a>(txn: &'a Transaction) -> TxnView<'a> {
    TxnView {
        txn_id: txn.id,
        client_addr: txn.client_addr,
        protocol_udp: txn.protocol == ClientProtocol::Udp,
        qname: txn.qname.as_deref(),
        query_wire: &txn.query_wire,
        response_wire: txn.response_wire.as_deref(),
        attempt_count: txn.attempt_count,
    }
}

pub fn emit_query(hub: &ObservationHub, txn: &Transaction, snapshot: &Arc<RuntimeSnapshot>) {
    let view = txn_view(txn);
    hub.try_enqueue_query(view, &snapshot.observation, |k| txn.tags.has(k));
}

pub fn emit_response(hub: &ObservationHub, txn: &Transaction, snapshot: &Arc<RuntimeSnapshot>) {
    let view = txn_view(txn);
    hub.try_enqueue_response(view, &snapshot.observation, |k| txn.tags.has(k));
}

pub fn emit_retry(hub: &ObservationHub, txn: &Transaction, snapshot: &Arc<RuntimeSnapshot>) {
    let view = txn_view(txn);
    hub.try_enqueue_retry(view, &snapshot.observation, |k| txn.tags.has(k));
}
