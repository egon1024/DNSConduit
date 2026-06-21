//! Upstream forward transport (UDP + TCP, phase 1b).

use crate::forward::egress::{EgressSourceSelection, WorkerForwardEgress};
use crate::forward::tcp::forward_tcp;
use crate::forward::{ForwardKey, TxnTable};
use conduit_config::forward::UpstreamTransport;
use conduit_core::record_upstream_response;
use conduit_core::phase::Phase;
use conduit_core::pipeline::{PipelineStage, StageOutcome};
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_core::transaction::{ClientProtocol, Transaction};
use conduit_metrics::MetricsHub;
use hickory_proto::op::Message;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct ForwardTransport {
    egress: WorkerForwardEgress,
    table: Arc<TxnTable>,
    upstream_transport: UpstreamTransport,
    client_tcp_uses_upstream_tcp: bool,
    timeout_ms: u32,
    metrics: Option<Arc<MetricsHub>>,
}

impl ForwardTransport {
    pub fn new(
        table: Arc<TxnTable>,
        compiled: &conduit_config::forward::CompiledForward,
        bind_addresses_v4: &[std::net::Ipv4Addr],
        bind_addresses_v6: &[std::net::Ipv6Addr],
        timeout_ms: u32,
        metrics: Option<Arc<MetricsHub>>,
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
            metrics,
        })
    }

    fn record_forward(
        &self,
        txn: &mut Transaction,
        backend: Option<std::net::SocketAddr>,
        outcome: &str,
        error_reason: Option<&str>,
        started: Instant,
    ) {
        txn.complete_forward_rtt(started);
        let Some(hub) = self.metrics.as_ref() else {
            return;
        };
        if !hub.metrics_enabled() {
            return;
        }
        let pool = txn.selected_pool.as_deref().unwrap_or("unknown");
        let backend_label = backend
            .map(|a| a.to_string())
            .unwrap_or_else(|| "unknown".into());
        hub.builtin
            .record_forward_attempt(pool, &backend_label, outcome);
        if let Some(reason) = error_reason {
            hub.builtin.record_forward_error(pool, reason);
        }
        hub.builtin
            .record_forward_duration(pool, &backend_label, started.elapsed().as_secs_f64());
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
        started: Instant,
        parse_wire_meta: bool,
    ) -> StageOutcome {
        self.record_forward(txn, Some(key.backend), "success", None, started);
        self.table.remove(key);
        record_upstream_response(txn, &wire, parse_wire_meta);
        txn.response_wire = Some(wire);
        StageOutcome::Continue(Phase::ResponseRules)
    }

    fn servfail(
        &self,
        txn: &mut Transaction,
        key: Option<ForwardKey>,
        reason: &str,
        started: Instant,
    ) -> StageOutcome {
        let backend = key.map(|k| k.backend);
        self.record_forward(txn, backend, "error", Some(reason), started);
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
        let started = Instant::now();
        let parse_wire_meta = snapshot.scripting.needs_response_wire_meta;
        txn.mark_forward_started(started);
        let Some(backend) = txn.selected_backend else {
            self.record_forward(txn, None, "error", Some("no_backend"), started);
            txn.set_rcode_name("SERVFAIL");
            return StageOutcome::Continue(Phase::Send);
        };

        let key = ForwardKey {
            backend,
            dns_id: txn.dns_id,
        };
        if !self.table.register(key, txn.id) {
            self.record_forward(txn, Some(backend), "error", Some("table_full"), started);
            txn.set_rcode_name("SERVFAIL");
            return StageOutcome::Continue(Phase::Send);
        }

        let pool = txn.selected_pool.as_deref();
        let sources_v4 = snapshot.sources_v4_for_pool(pool);
        let sources_v6 = snapshot.sources_v6_for_pool(pool);
        let allowed_v4 = snapshot.allowed_sources_v4_for_pool(pool);
        let allowed_v6 = snapshot.allowed_sources_v6_for_pool(pool);
        let effective_v4 = txn.take_effective_source_override_v4();
        let effective_v6 = txn.take_effective_source_override_v6();
        let upstream_wire = &txn.query_wire;
        let bind_v4 = if backend.is_ipv4() {
            Some(
                self.egress
                    .select_source_v4(sources_v4, effective_v4, &allowed_v4),
            )
        } else {
            None
        };
        let bind_v6 = if backend.is_ipv6() {
            Some(
                self.egress
                    .select_source_v6(sources_v6, effective_v6, &allowed_v6),
            )
        } else {
            None
        };

        let try_tcp = self.use_tcp_for_attempt(txn, false);

        if try_tcp {
            match forward_tcp(backend, upstream_wire, self.timeout(), bind_v4, bind_v6) {
                Ok(wire) => return self.finish_response(txn, key, wire, started, parse_wire_meta),
                Err(e) => {
                    tracing::warn!(txn_id = txn.id, %backend, error = %e, "tcp forward failed");
                    return self.servfail(txn, Some(key), "tcp_error", started);
                }
            }
        }

        let sel = EgressSourceSelection {
            pool_sources_v4: sources_v4,
            pool_sources_v6: sources_v6,
            backend,
            override_v4: effective_v4,
            allowed_v4: &allowed_v4,
            override_v6: effective_v6,
            allowed_v6: &allowed_v6,
        };
        let socket = self.egress.udp_socket_for(&sel);

        tracing::debug!(
            txn_id = txn.id,
            dns_id = txn.dns_id,
            %backend,
            transport = "udp",
            "forwarding query"
        );

        if socket.send_to(upstream_wire, backend).is_err() {
            tracing::warn!(txn_id = txn.id, dns_id = txn.dns_id, %backend, "forward send failed");
            return self.servfail(txn, Some(key), "send_error", started);
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
                                upstream_wire,
                                self.timeout(),
                                bind_v4,
                                bind_v6,
                            ) {
                                Ok(tcp_wire) => {
                                    return self.finish_response(txn, key, tcp_wire, started, parse_wire_meta);
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
                self.finish_response(txn, key, wire, started, parse_wire_meta)
            }
            Err(_) => {
                tracing::warn!(
                    txn_id = txn.id,
                    dns_id = txn.dns_id,
                    %backend,
                    "forward recv timeout"
                );
                self.record_forward(txn, Some(backend), "error", Some("timeout"), started);
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
