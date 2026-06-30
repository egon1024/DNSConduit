//! Shared orchestrator wiring for dataplane runtimes.

use crate::forward::pool_inflight::PoolInflight;
use crate::forward::{ForwardMode, ForwardTransport, IoBackend, WaitResponseStage};
use crate::forward::{TxnTable, WorkerForwardEgress};
use conduit_config::forward::CompiledForward;
use conduit_core::health::HealthRegistry;
use conduit_core::orchestrator::Orchestrator;
use conduit_core::phase::Phase;
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_core::stages::RouteStage;
use conduit_metrics::{MetricsHub, TracingHub};
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;

#[allow(clippy::too_many_arguments)]
pub fn build_orchestrator(
    snap: &RuntimeSnapshot,
    table: Arc<TxnTable>,
    forward_compiled: &CompiledForward,
    egress: WorkerForwardEgress,
    timeout_ms: u32,
    metrics: Arc<MetricsHub>,
    tracing: Arc<TracingHub>,
    forward_mode: ForwardMode,
    io_backend: Option<Arc<IoBackend>>,
    pool_inflight: Option<Arc<PoolInflight>>,
    health: Arc<HealthRegistry>,
) -> io::Result<Arc<Orchestrator>> {
    let forward = Arc::new(ForwardTransport::new_with_mode_and_egress(
        egress,
        table,
        forward_compiled,
        timeout_ms,
        Some(metrics.clone()),
        forward_mode,
        io_backend,
        pool_inflight,
    )?);
    let parse_wire_meta = snap.scripting.needs_response_wire_meta;
    let mut orchestrator = Orchestrator::with_default_stages();
    orchestrator.metrics = Some(metrics.clone());
    orchestrator.tracing = Some(tracing);
    orchestrator
        .registry
        .register(Phase::Route, Arc::new(RouteStage::with_health(health)));
    orchestrator.registry.register(
        Phase::RequestRules,
        Arc::new(conduit_core::stages::RequestRulesStage {
            metrics: Some(metrics.clone()),
        }),
    );
    orchestrator.registry.register(
        Phase::ResponseRules,
        Arc::new(conduit_core::stages::ResponseRulesStage {
            metrics: Some(metrics.clone()),
        }),
    );
    orchestrator.registry.register(Phase::Forward, forward);
    orchestrator.registry.register(
        Phase::WaitResponse,
        Arc::new(WaitResponseStage::new(
            parse_wire_meta,
            Some(metrics.clone()),
        )),
    );
    Ok(Arc::new(orchestrator))
}

/// Build shared upstream egress used by forward transport and I/O backend polling.
pub fn build_forward_egress(
    forward_compiled: &CompiledForward,
    bind_addresses_v4: &[Ipv4Addr],
    bind_addresses_v6: &[Ipv6Addr],
    timeout_ms: u32,
) -> io::Result<WorkerForwardEgress> {
    WorkerForwardEgress::new(
        forward_compiled,
        bind_addresses_v4,
        bind_addresses_v6,
        timeout_ms,
    )
}
