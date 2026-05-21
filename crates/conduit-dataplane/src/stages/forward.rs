//! UDP forward to upstream backend (blocking recv in phase 1).

use crate::forward::{ForwardKey, TxnTable};
use conduit_core::phase::Phase;
use conduit_core::pipeline::{PipelineStage, StageOutcome};
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_core::transaction::Transaction;
use hickory_proto::op::Message;
use std::net::UdpSocket;
use std::sync::Arc;
use std::time::Duration;

pub struct UdpForwardStage {
    socket: UdpSocket,
    table: Arc<TxnTable>,
}

impl UdpForwardStage {
    pub fn new(table: Arc<TxnTable>, timeout_ms: u32) -> std::io::Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_read_timeout(Some(Duration::from_millis(timeout_ms as u64)))?;
        Ok(Self { socket, table })
    }
}

impl PipelineStage for UdpForwardStage {
    fn name(&self) -> &'static str {
        "udp_forward"
    }

    fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        let Some(backend) = txn.selected_backend else {
            txn.set_rcode_name("SERVFAIL");
            return StageOutcome::Continue(Phase::Send);
        };

        let key = ForwardKey {
            backend,
            dns_id: txn.dns_id,
        };
        if !self.table.register(key, txn.id) {
            txn.set_rcode_name("SERVFAIL");
            return StageOutcome::Continue(Phase::Send);
        }

        tracing::debug!(txn_id = txn.id, dns_id = txn.dns_id, %backend, "forwarding query");
        if self.socket.send_to(&txn.query_wire, backend).is_err() {
            self.table.remove(key);
            tracing::warn!(txn_id = txn.id, dns_id = txn.dns_id, %backend, "forward send failed");
            txn.set_rcode_name("SERVFAIL");
            return StageOutcome::Continue(Phase::Send);
        }

        let mut buf = [0u8; 4096];
        match self.socket.recv_from(&mut buf) {
            Ok((len, _from)) => {
                self.table.remove(key);
                if let Ok(msg) = Message::from_vec(&buf[..len]) {
                    txn.set_rcode(msg.response_code().low() as u16);
                }
                txn.response_wire = Some(buf[..len].to_vec());
                StageOutcome::Continue(Phase::ResponseRules)
            }
            Err(_) => {
                self.table.remove(key);
                tracing::warn!(txn_id = txn.id, dns_id = txn.dns_id, %backend, "forward recv timeout");
                txn.set_rcode_name("SERVFAIL");
                StageOutcome::Continue(Phase::ResponseRules)
            }
        }
    }
}
