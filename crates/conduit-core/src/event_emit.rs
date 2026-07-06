//! Dataplane hook helpers for event enqueue.

use crate::snapshot::RuntimeSnapshot;
use crate::transaction::{ClientProtocol, Transaction};
use conduit_events::{EventHub, TxnExtraSource, TxnView};
use std::sync::Arc;

fn extra_source(txn: &Transaction, include_tags: bool) -> TxnExtraSource {
    let (tag_bools, tag_strings) = if include_tags {
        txn.tags.export_all_tags()
    } else {
        (Vec::new(), Vec::new())
    };
    TxnExtraSource {
        pool: txn.selected_pool.clone(),
        backend: txn.selected_backend_display(),
        attempt_count: txn.attempt_count,
        txn_id: txn.id,
        qname: txn.qname.clone(),
        rcode_label: txn.rcode_label(),
        client: txn.client_addr.to_string(),
        answer_source: txn.answer_source.map(|s| s.as_str().to_string()),
        cache_instance: txn.cache_instance.clone(),
        tag_bools,
        tag_strings,
    }
}

pub fn txn_view<'a>(txn: &'a Transaction, snapshot: &RuntimeSnapshot) -> TxnView<'a> {
    TxnView {
        txn_id: txn.id,
        global_query_index: txn.global_query_index,
        client_addr: txn.client_addr,
        protocol_udp: txn.protocol == ClientProtocol::Udp,
        qname: txn.qname.as_deref(),
        qtype: txn.qtype,
        rcode: txn.rcode(),
        qclass: txn.qclass,
        opcode: txn.opcode,
        edns_option_codes: &txn.edns_option_codes,
        qtype_label: txn.qtype_label(),
        query_wire: &txn.query_wire,
        response_wire: txn.response_wire.as_deref(),
        attempt_count: txn.attempt_count,
        answer_source: txn.answer_source.map(|s| s.as_str()),
        cache_instance: txn.cache_instance.as_deref(),
        extra: extra_source(txn, snapshot.events.needs_tag_export()),
    }
}

pub fn emit_query(hub: &EventHub, txn: &Transaction, snapshot: &Arc<RuntimeSnapshot>) {
    if !hub.enabled() {
        return;
    }
    let view = txn_view(txn, snapshot);
    hub.try_enqueue_query(view, &snapshot.events, |k| txn.tags.has(k));
}

pub fn emit_response(hub: &EventHub, txn: &Transaction, snapshot: &Arc<RuntimeSnapshot>) {
    if !hub.enabled() {
        return;
    }
    let view = txn_view(txn, snapshot);
    hub.try_enqueue_response(view, &snapshot.events, |k| txn.tags.has(k));
}

pub fn emit_retry(hub: &EventHub, txn: &Transaction, snapshot: &Arc<RuntimeSnapshot>) {
    if !hub.enabled() {
        return;
    }
    let view = txn_view(txn, snapshot);
    hub.try_enqueue_retry(view, &snapshot.events, |k| txn.tags.has(k));
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_events::EventHub;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn emit_helpers_skip_when_hub_disabled() {
        let hub = EventHub::disabled();
        let snap = Arc::new(RuntimeSnapshot::from_config(
            conduit_config::load_yaml(include_str!("../../../tests/fixtures/config/minimal.yaml"))
                .unwrap(),
        ));
        let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 12345);
        let mut txn = Transaction::new(1, peer, ClientProtocol::Udp);
        txn.qname = Some("example.com.".into());
        txn.query_wire = vec![0x00, 0x01];

        emit_query(&hub, &txn, &snap);
        emit_response(&hub, &txn, &snap);
        emit_retry(&hub, &txn, &snap);

        assert_eq!(hub.dropped_total(), 0);
    }
}
