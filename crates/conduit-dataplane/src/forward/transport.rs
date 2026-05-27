//! Upstream forward transport (UDP + TCP, phase 1b).

use crate::forward::egress::{EgressSourceSelection, WorkerForwardEgress};
use crate::forward::rd::build_upstream_wire;
use crate::forward::tcp::forward_tcp;
use crate::forward::{ForwardKey, TxnTable};
use conduit_config::forward::UpstreamTransport;
use conduit_core::phase::Phase;
use conduit_core::pipeline::{PipelineStage, StageOutcome};
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_core::transaction::{ClientProtocol, Transaction};
use hickory_proto::op::Message;
use std::sync::Arc;
use std::time::Duration;

pub struct ForwardTransport {
    egress: WorkerForwardEgress,
    table: Arc<TxnTable>,
    upstream_transport: UpstreamTransport,
    client_tcp_uses_upstream_tcp: bool,
    timeout_ms: u32,
}

impl ForwardTransport {
    pub fn new(
        table: Arc<TxnTable>,
        compiled: &conduit_config::forward::CompiledForward,
        bind_addresses_v4: &[std::net::Ipv4Addr],
        bind_addresses_v6: &[std::net::Ipv6Addr],
        timeout_ms: u32,
    ) -> std::io::Result<Self> {
        Ok(Self {
            egress: WorkerForwardEgress::new(
                compiled,
                bind_addresses_v4,
                bind_addresses_v6,
                timeout_ms,
            )?,
            table,
            upstream_transport: compiled.upstream_transport,
            client_tcp_uses_upstream_tcp: compiled.client_tcp_uses_upstream_tcp,
            timeout_ms,
        })
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms as u64)
    }

    fn use_tcp_for_attempt(&self, txn: &Transaction, force_tcp: bool) -> bool {
        if force_tcp {
            return true;
        }
        if self.client_tcp_uses_upstream_tcp && txn.protocol == ClientProtocol::Tcp {
            return true;
        }
        matches!(self.upstream_transport, UpstreamTransport::TcpOnly)
    }

    fn finish_response(
        &self,
        txn: &mut Transaction,
        key: ForwardKey,
        wire: Vec<u8>,
    ) -> StageOutcome {
        self.table.remove(key);
        if let Ok(msg) = Message::from_vec(&wire) {
            txn.set_rcode(msg.response_code().low() as u16);
        }
        txn.response_wire = Some(wire);
        StageOutcome::Continue(Phase::ResponseRules)
    }

    fn servfail(&self, txn: &mut Transaction, key: Option<ForwardKey>) -> StageOutcome {
        if let Some(k) = key {
            self.table.remove(k);
        }
        txn.set_rcode_name("SERVFAIL");
        StageOutcome::Continue(Phase::Send)
    }
}

impl PipelineStage for ForwardTransport {
    fn name(&self) -> &'static str {
        "forward"
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
        let rd = txn.upstream_rd_policy();
        let upstream_wire = build_upstream_wire(&txn.query_wire, rd);
        let sources_v4 = snapshot.sources_v4_for_pool(pool);
        let sources_v6 = snapshot.sources_v6_for_pool(pool);
        let allowed_v4 = snapshot.allowed_sources_v4_for_pool(pool);
        let allowed_v6 = snapshot.allowed_sources_v6_for_pool(pool);
        let bind_v4 = if backend.is_ipv4() {
            Some(
                self.egress
                    .select_source_v4(sources_v4, txn.source_override_v4, &allowed_v4),
            )
        } else {
            None
        };
        let bind_v6 = if backend.is_ipv6() {
            Some(
                self.egress
                    .select_source_v6(sources_v6, txn.source_override_v6, &allowed_v6),
            )
        } else {
            None
        };

        let try_tcp = self.use_tcp_for_attempt(txn, false);

        if try_tcp {
            match forward_tcp(backend, &upstream_wire, self.timeout(), bind_v4, bind_v6) {
                Ok(wire) => return self.finish_response(txn, key, wire),
                Err(e) => {
                    tracing::warn!(txn_id = txn.id, %backend, error = %e, "tcp forward failed");
                    return self.servfail(txn, Some(key));
                }
            }
        }

        let sel = EgressSourceSelection {
            pool_sources_v4: sources_v4,
            pool_sources_v6: sources_v6,
            backend,
            override_v4: txn.source_override_v4,
            allowed_v4: &allowed_v4,
            override_v6: txn.source_override_v6,
            allowed_v6: &allowed_v6,
        };
        let socket = self.egress.udp_socket_for(&sel);

        tracing::debug!(
            txn_id = txn.id,
            dns_id = txn.dns_id,
            %backend,
            rd = %rd.as_str(),
            transport = "udp",
            "forwarding query"
        );

        if socket.send_to(&upstream_wire, backend).is_err() {
            tracing::warn!(txn_id = txn.id, dns_id = txn.dns_id, %backend, "forward send failed");
            return self.servfail(txn, Some(key));
        }

        let mut buf = [0u8; 4096];
        match socket.recv_from(&mut buf) {
            Ok((len, _from)) => {
                let wire = buf[..len].to_vec();
                if matches!(
                    self.upstream_transport,
                    UpstreamTransport::PreferUdpWithTcpFallback
                ) && backend.is_ipv4()
                {
                    if let Ok(msg) = Message::from_vec(&wire) {
                        if msg.header().truncated() {
                            tracing::debug!(
                                txn_id = txn.id,
                                %backend,
                                "udp response TC=1, retrying over tcp"
                            );
                            match forward_tcp(
                                backend,
                                &upstream_wire,
                                self.timeout(),
                                bind_v4,
                                bind_v6,
                            ) {
                                Ok(tcp_wire) => {
                                    return self.finish_response(txn, key, tcp_wire);
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        txn_id = txn.id,
                                        %backend,
                                        error = %e,
                                        "tcp fallback after TC failed"
                                    );
                                }
                            }
                        }
                    }
                }
                self.finish_response(txn, key, wire)
            }
            Err(_) => {
                tracing::warn!(
                    txn_id = txn.id,
                    dns_id = txn.dns_id,
                    %backend,
                    "forward recv timeout"
                );
                self.table.remove(key);
                txn.set_rcode_name("SERVFAIL");
                StageOutcome::Continue(Phase::ResponseRules)
            }
        }
    }
}

/// Backward-compatible alias used in tests (slice A name).
pub type UdpForwardTransport = ForwardTransport;
pub type UdpForwardStage = ForwardTransport;
