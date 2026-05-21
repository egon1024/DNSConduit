//! Start listener worker threads for the active snapshot.

use crate::forward::TxnTable;
use crate::stages::UdpForwardStage;
use conduit_core::orchestrator::Orchestrator;
use conduit_core::phase::Phase;
use conduit_core::snapshot::SnapshotStore;
use std::sync::Arc;
use std::thread;

pub struct DataplaneHandle {
    _threads: Vec<thread::JoinHandle<()>>,
}

pub fn start(store: Arc<SnapshotStore>) -> std::io::Result<DataplaneHandle> {
    let snap = store.load();
    let cfg = &snap.config;
    let listeners = cfg.listeners.as_ref();
    let Some(listeners) = listeners else {
        return Ok(DataplaneHandle {
            _threads: Vec::new(),
        });
    };

    let forward_cfg = cfg.forward.as_ref();
    let orch_cfg = cfg.orchestrator.as_ref();
    let table = Arc::new(TxnTable::new(
        orch_cfg
            .map(|o| o.txn_table_capacity as usize)
            .unwrap_or(1024),
        forward_cfg
            .map(|f| f.outstanding_per_backend)
            .unwrap_or(100),
    ));
    let timeout_ms = forward_cfg.map(|f| f.timeout_ms).unwrap_or(2000);
    let forward = Arc::new(UdpForwardStage::new(table.clone(), timeout_ms)?);

    let mut orchestrator = Orchestrator::with_default_stages();
    orchestrator.registry.register(Phase::Forward, forward);
    orchestrator
        .registry
        .register(Phase::WaitResponse, Arc::new(NoopWaitStage));
    let orchestrator = Arc::new(orchestrator);

    let mut handles = Vec::new();
    let threads = listeners.threads.max(1);
    for ln in &listeners.listeners {
        for _ in 0..threads {
            let ln = ln.clone();
            let store = store.clone();
            let orch = orchestrator.clone();
            let reuse = listeners.reuse_port;
            let rcvbuf = listeners.rcvbuf;
            let proto = ln.protocol.to_lowercase();
            handles.push(thread::spawn(move || {
                let result = if proto == "tcp" {
                    crate::listener::tcp::run_worker(ln, store, orch)
                } else {
                    crate::listener::udp::run_worker(ln, store, orch, reuse, rcvbuf)
                };
                if let Err(e) = result {
                    tracing::error!(error = %e, "listener worker exited");
                }
            }));
        }
    }

    Ok(DataplaneHandle { _threads: handles })
}

struct NoopWaitStage;

impl conduit_core::PipelineStage for NoopWaitStage {
    fn name(&self) -> &'static str {
        "wait_response"
    }

    fn handle(
        &self,
        _txn: &mut conduit_core::Transaction,
        _snapshot: &Arc<conduit_core::RuntimeSnapshot>,
    ) -> conduit_core::StageOutcome {
        conduit_core::StageOutcome::Continue(Phase::ResponseRules)
    }
}
