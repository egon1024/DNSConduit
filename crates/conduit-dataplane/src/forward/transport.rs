//! Upstream forward transport (UDP + TCP, phase 1b).

use crate::forward::egress::{EgressSourceSelection, WorkerForwardEgress};
use crate::forward::io_backend::IoBackend;
use crate::forward::mode::ForwardMode;
use crate::forward::pool_inflight::PoolInflight;
use crate::forward::tcp::forward_tcp;
use crate::forward::{ForwardKey, TxnTable};
use conduit_config::forward::UpstreamTransport;
use conduit_core::health::HealthRegistry;
use conduit_core::phase::Phase;
use conduit_core::pipeline::{PipelineStage, StageOutcome};
use conduit_core::record_upstream_response;
use conduit_core::routing::backend_metric_label_for_addr;
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_core::transaction::{ClientProtocol, Transaction};
use conduit_core::txn_store::SlotId;
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
    mode: ForwardMode,
    io_backend: Option<Arc<IoBackend>>,
    pool_inflight: Option<Arc<PoolInflight>>,
    health: Option<Arc<HealthRegistry>>,
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
        Self::new_with_mode(
            table,
            compiled,
            bind_addresses_v4,
            bind_addresses_v6,
            timeout_ms,
            metrics,
            ForwardMode::Sync,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_mode(
        table: Arc<TxnTable>,
        compiled: &conduit_config::forward::CompiledForward,
        bind_addresses_v4: &[std::net::Ipv4Addr],
        bind_addresses_v6: &[std::net::Ipv6Addr],
        timeout_ms: u32,
        metrics: Option<Arc<MetricsHub>>,
        mode: ForwardMode,
        io_backend: Option<Arc<IoBackend>>,
    ) -> std::io::Result<Self> {
        let egress =
            WorkerForwardEgress::new(compiled, bind_addresses_v4, bind_addresses_v6, timeout_ms)?;
        Self::new_with_mode_and_egress(
            egress, table, compiled, timeout_ms, metrics, mode, io_backend, None, None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_mode_and_egress(
        egress: WorkerForwardEgress,
        table: Arc<TxnTable>,
        compiled: &conduit_config::forward::CompiledForward,
        timeout_ms: u32,
        metrics: Option<Arc<MetricsHub>>,
        mode: ForwardMode,
        io_backend: Option<Arc<IoBackend>>,
        pool_inflight: Option<Arc<PoolInflight>>,
        health: Option<Arc<HealthRegistry>>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            egress,
            table,
            upstream_transport: compiled.upstream_transport,
            client_tcp_uses_upstream_tcp: compiled.client_tcp_uses_upstream_tcp,
            timeout_ms,
            metrics,
            mode,
            io_backend,
            pool_inflight,
            health,
        })
    }

    fn report_passive_outcome(
        &self,
        txn: &Transaction,
        snapshot: &RuntimeSnapshot,
        backend: std::net::SocketAddr,
        is_failure: bool,
        reason: Option<&str>,
    ) {
        let Some(registry) = self.health.as_ref() else {
            return;
        };
        let pool = txn.selected_pool.as_deref().unwrap_or("default");
        if let Some(result) =
            registry.record_passive_forward_outcome(&snapshot.health, pool, backend, is_failure)
        {
            let qname = txn.qname.as_deref().unwrap_or("?");
            let qtype = txn.qtype.unwrap_or(0);
            let reason = reason.unwrap_or("unknown");
            if result.transitioned {
                tracing::warn!(
                    %pool,
                    backend = %backend,
                    %reason,
                    %qname,
                    qtype,
                    client = %txn.client_addr,
                    passive_failures = result.consecutive_failures,
                    passive_fall = result.passive_fall,
                    "passive fast-trip: backend marked down"
                );
            } else if result.already_down {
                tracing::debug!(
                    %pool,
                    backend = %backend,
                    %reason,
                    %qname,
                    qtype,
                    client = %txn.client_addr,
                    "passive health: forward failure (backend already down)"
                );
            } else {
                tracing::warn!(
                    %pool,
                    backend = %backend,
                    %reason,
                    %qname,
                    qtype,
                    client = %txn.client_addr,
                    passive_failures = result.consecutive_failures,
                    passive_fall = result.passive_fall,
                    "passive health: forward failure"
                );
            }
        }
    }

    fn passive_failure_reason(reason: &str) -> bool {
        matches!(reason, "send_error" | "timeout" | "tcp_error")
    }

    fn release_pool_inflight(&self, txn: &Transaction) {
        if let Some(inflight) = self.pool_inflight.as_ref() {
            if let Some(pool) = txn.selected_pool.as_deref() {
                inflight.release(pool);
            }
        }
    }

    fn record_forward(
        &self,
        txn: &mut Transaction,
        snapshot: &RuntimeSnapshot,
        backend: Option<std::net::SocketAddr>,
        outcome: &str,
        error_reason: Option<&str>,
        started: Instant,
    ) {
        txn.complete_forward_rtt(started);
        txn.forward_metrics_recorded = true;
        let Some(hub) = self.metrics.as_ref() else {
            return;
        };
        if !hub.metrics_enabled() {
            return;
        }
        let pool = txn.selected_pool.as_deref().unwrap_or("unknown");
        let backend_label = backend
            .map(|addr| backend_metric_label_for_addr(&snapshot.config.pools, pool, addr))
            .unwrap_or_else(|| "unknown".into());
        hub.builtin
            .record_forward_attempt(pool, &backend_label, outcome);
        if let Some(reason) = error_reason {
            hub.builtin
                .record_forward_error(pool, &backend_label, reason);
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
        snapshot: &RuntimeSnapshot,
        key: ForwardKey,
        wire: Vec<u8>,
        started: Instant,
        parse_wire_meta: bool,
    ) -> StageOutcome {
        self.record_forward(txn, snapshot, Some(key.backend), "success", None, started);
        self.report_passive_outcome(txn, snapshot, key.backend, false, None);
        self.table.remove(key);
        record_upstream_response(txn, &wire, parse_wire_meta);
        txn.response_wire = Some(wire);
        StageOutcome::Continue(Phase::ResponseRules)
    }

    fn servfail(
        &self,
        txn: &mut Transaction,
        snapshot: &RuntimeSnapshot,
        key: Option<ForwardKey>,
        reason: &str,
        started: Instant,
    ) -> StageOutcome {
        let backend = key.map(|k| k.backend);
        self.record_forward(txn, snapshot, backend, "error", Some(reason), started);
        if let Some(b) = backend {
            if Self::passive_failure_reason(reason) {
                self.report_passive_outcome(txn, snapshot, b, true, Some(reason));
            }
        }
        if let Some(k) = key {
            self.table.remove(k);
            if let Some(io) = self.io_backend.as_ref() {
                io.cancel_pending(k);
            }
        }
        txn.set_rcode_name("SERVFAIL");
        StageOutcome::Continue(Phase::Send)
    }

    fn register_forward_key(
        &self,
        txn: &mut Transaction,
        snapshot: &RuntimeSnapshot,
        backend: std::net::SocketAddr,
    ) -> Result<ForwardKey, StageOutcome> {
        let key = ForwardKey {
            backend,
            dns_id: txn.dns_id,
        };
        if !self.table.register(key, txn.id) {
            return Err(self.servfail(txn, snapshot, None, "table_full", Instant::now()));
        }
        Ok(key)
    }

    fn handle_submit(
        &self,
        txn: &mut Transaction,
        snapshot: &Arc<RuntimeSnapshot>,
        started: Instant,
    ) -> StageOutcome {
        let parse_wire_meta = snapshot.scripting.needs_response_wire_meta;
        txn.mark_forward_started(started);
        let Some(backend) = txn.selected_backend else {
            return self.servfail(txn, snapshot, None, "no_backend", started);
        };

        let key = match self.register_forward_key(txn, snapshot, backend) {
            Ok(k) => k,
            Err(outcome) => return outcome,
        };

        if let Some(inflight) = self.pool_inflight.as_ref() {
            let pool = txn.selected_pool.as_deref().unwrap_or("default");
            if !inflight.try_acquire(pool) {
                self.table.remove(key);
                return self.servfail(txn, snapshot, None, "pool_inflight_exceeded", started);
            }
        }

        if self.use_tcp_for_attempt(txn, false) {
            self.table.remove(key);
            self.release_pool_inflight(txn);
            tracing::warn!(txn_id = txn.id, %backend, "tcp forward in submit mode not implemented");
            return self.servfail(txn, snapshot, None, "tcp_unsupported", started);
        }

        let pool = txn.selected_pool.as_deref();
        let sources_v4 = snapshot.sources_v4_for_pool(pool);
        let sources_v6 = snapshot.sources_v6_for_pool(pool);
        let allowed_v4 = snapshot.allowed_sources_v4_for_pool(pool);
        let allowed_v6 = snapshot.allowed_sources_v6_for_pool(pool);
        let effective_v4 = txn.take_effective_source_override_v4();
        let effective_v6 = txn.take_effective_source_override_v6();
        let upstream_wire = txn.query_wire.clone();
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

        if socket.send_to(&upstream_wire, backend).is_err() {
            self.release_pool_inflight(txn);
            return self.servfail(txn, snapshot, Some(key), "send_error", started);
        }

        let Some(io) = self.io_backend.as_ref() else {
            self.table.remove(key);
            self.release_pool_inflight(txn);
            return self.servfail(txn, snapshot, Some(key), "no_io_backend", started);
        };

        let slot_id = SlotId::from_index(txn.id as u32);
        io.track_pending(key, slot_id);
        let _ = parse_wire_meta;
        StageOutcome::Suspend(Phase::WaitResponse)
    }

    fn handle_sync(
        &self,
        txn: &mut Transaction,
        snapshot: &Arc<RuntimeSnapshot>,
        started: Instant,
    ) -> StageOutcome {
        let parse_wire_meta = snapshot.scripting.needs_response_wire_meta;
        txn.mark_forward_started(started);
        let Some(backend) = txn.selected_backend else {
            self.record_forward(txn, snapshot, None, "error", Some("no_backend"), started);
            txn.set_rcode_name("SERVFAIL");
            return StageOutcome::Continue(Phase::Send);
        };

        let key = match self.register_forward_key(txn, snapshot, backend) {
            Ok(k) => k,
            Err(outcome) => return outcome,
        };

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
                Ok(wire) => {
                    return self.finish_response(txn, snapshot, key, wire, started, parse_wire_meta)
                }
                Err(e) => {
                    tracing::warn!(txn_id = txn.id, %backend, error = %e, "tcp forward failed");
                    return self.servfail(txn, snapshot, Some(key), "tcp_error", started);
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
            return self.servfail(txn, snapshot, Some(key), "send_error", started);
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
                                    return self.finish_response(
                                        txn,
                                        snapshot,
                                        key,
                                        tcp_wire,
                                        started,
                                        parse_wire_meta,
                                    );
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
                self.finish_response(txn, snapshot, key, wire, started, parse_wire_meta)
            }
            Err(_) => {
                tracing::warn!(
                    txn_id = txn.id,
                    dns_id = txn.dns_id,
                    %backend,
                    "forward recv timeout"
                );
                self.record_forward(
                    txn,
                    snapshot,
                    Some(backend),
                    "error",
                    Some("timeout"),
                    started,
                );
                self.report_passive_outcome(txn, snapshot, backend, true, Some("timeout"));
                self.table.remove(key);
                txn.set_rcode_name("SERVFAIL");
                StageOutcome::Continue(Phase::ResponseRules)
            }
        }
    }
}

impl PipelineStage for ForwardTransport {
    fn name(&self) -> &'static str {
        "forward"
    }

    fn handle(&self, txn: &mut Transaction, snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        let started = Instant::now();
        match self.mode {
            ForwardMode::Sync => self.handle_sync(txn, snapshot, started),
            ForwardMode::Submit => self.handle_submit(txn, snapshot, started),
        }
    }
}

/// Backward-compatible alias used in tests (slice A name).
pub type UdpForwardTransport = ForwardTransport;
pub type UdpForwardStage = ForwardTransport;

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::forward::{CompiledForward, UpstreamTransport};
    use conduit_core::snapshot::RuntimeSnapshot;
    use std::net::{Ipv4Addr, UdpSocket};

    fn compiled_forward() -> CompiledForward {
        CompiledForward {
            sources_v4: vec![],
            sources_v6: vec![],
            source_selection: "round_robin".into(),
            upstream_transport: UpstreamTransport::UdpOnly,
            client_tcp_uses_upstream_tcp: false,
            timeout_ms: 500,
            outstanding_per_backend: 10,
        }
    }

    fn minimal_snapshot() -> Arc<RuntimeSnapshot> {
        let yaml = include_str!("../../../../tests/fixtures/config/minimal.yaml");
        Arc::new(RuntimeSnapshot::from_config(
            conduit_config::load_yaml(yaml).unwrap(),
        ))
    }

    #[test]
    fn submit_mode_suspends_at_wait_response() {
        let table = Arc::new(TxnTable::new(64, 16));
        let egress = UdpSocket::bind("127.0.0.1:0").unwrap();
        let (io, _resume_rx) = IoBackend::new(vec![egress], table.clone(), 1000).unwrap();
        let forward = ForwardTransport::new_with_mode(
            table.clone(),
            &compiled_forward(),
            &[Ipv4Addr::UNSPECIFIED],
            &[],
            500,
            None,
            ForwardMode::Submit,
            Some(Arc::new(io)),
        )
        .unwrap();

        let (port_tx, port_rx) = std::sync::mpsc::channel();
        let upstream = std::thread::spawn(move || {
            let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
            port_tx.send(sock.local_addr().unwrap().port()).unwrap();
            let mut buf = [0u8; 512];
            let (_, _) = sock.recv_from(&mut buf).unwrap();
        });

        let backend_port = port_rx.recv().unwrap();
        let backend: std::net::SocketAddr = format!("127.0.0.1:{backend_port}").parse().unwrap();

        let mut txn = Transaction::new(1, "127.0.0.1:53".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(vec![
                0x12, 0x34, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x77,
                0x77, 0x77, 0x07, 0x65, 0x78, 0x61, 0x6d, 0x70, 0x6c, 0x65, 0x03, 0x63, 0x6f, 0x6d,
                0x00, 0x00, 0x01, 0x00, 0x01,
            ]);
        txn.selected_backend = Some(backend);
        txn.dns_id = 0x1234;

        let snap = minimal_snapshot();
        let outcome = forward.handle(&mut txn, &snap);
        assert!(matches!(
            outcome,
            StageOutcome::Suspend(Phase::WaitResponse)
        ));
        assert!(txn.forward_started_at.is_some());
        assert!(table
            .lookup(ForwardKey {
                backend,
                dns_id: 0x1234
            })
            .is_some());
        let _ = upstream.join();
    }
}
