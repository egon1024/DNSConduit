//! Upstream forward transport boundary (UDP slice A; TCP in slice C).

use crate::forward::egress::WorkerForwardEgress;
use crate::forward::rd::build_upstream_wire;
use crate::forward::{ForwardKey, TxnTable};
use conduit_core::phase::Phase;
use conduit_core::pipeline::{PipelineStage, StageOutcome};
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_core::transaction::Transaction;
use hickory_proto::op::Message;
use std::sync::Arc;

pub struct UdpForwardTransport {
    egress: WorkerForwardEgress,
    table: Arc<TxnTable>,
}

impl UdpForwardTransport {
    pub fn new(
        table: Arc<TxnTable>,
        compiled: &conduit_config::forward::CompiledForward,
        bind_addresses_v4: &[std::net::Ipv4Addr],
        timeout_ms: u32,
    ) -> std::io::Result<Self> {
        Ok(Self {
            egress: WorkerForwardEgress::new(compiled, bind_addresses_v4, timeout_ms)?,
            table,
        })
    }
}

impl PipelineStage for UdpForwardTransport {
    fn name(&self) -> &'static str {
        "udp_forward"
    }

    fn handle(&self, txn: &mut Transaction, snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
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

        let pool = txn.selected_pool.as_deref();
        let rd = snapshot.recursion_desired_for_pool(pool);
        let upstream_wire = build_upstream_wire(&txn.query_wire, rd);
        let sources = snapshot.sources_v4_for_pool(pool);
        let socket = self.egress.socket_for_pool(sources);

        tracing::debug!(
            txn_id = txn.id,
            dns_id = txn.dns_id,
            %backend,
            rd = %rd.as_str(),
            "forwarding query"
        );
        if socket.send_to(&upstream_wire, backend).is_err() {
            self.table.remove(key);
            tracing::warn!(txn_id = txn.id, dns_id = txn.dns_id, %backend, "forward send failed");
            txn.set_rcode_name("SERVFAIL");
            return StageOutcome::Continue(Phase::Send);
        }

        let mut buf = [0u8; 4096];
        match socket.recv_from(&mut buf) {
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
                tracing::warn!(
                    txn_id = txn.id,
                    dns_id = txn.dns_id,
                    %backend,
                    "forward recv timeout"
                );
                txn.set_rcode_name("SERVFAIL");
                StageOutcome::Continue(Phase::ResponseRules)
            }
        }
    }
}

/// Type alias for the slice A forward stage registered on the orchestrator.
pub type UdpForwardStage = UdpForwardTransport;
