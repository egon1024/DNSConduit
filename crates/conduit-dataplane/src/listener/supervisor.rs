//! Start listener worker threads for the active snapshot.

use crate::forward::{TxnTable, UdpForwardStage};
use conduit_core::orchestrator::Orchestrator;
use conduit_core::phase::Phase;
use conduit_core::snapshot::SnapshotStore;
use conduit_observation::ObservationHub;
use std::sync::Arc;
use std::thread;

pub struct DataplaneHandle {
    _threads: Vec<thread::JoinHandle<()>>,
    pub observation: Arc<ObservationHub>,
}

pub fn start(store: Arc<SnapshotStore>) -> std::io::Result<DataplaneHandle> {
    let snap = store.load();
    let observation = Arc::new(ObservationHub::from_compiled(&snap.observation));
    if observation.enabled() {
        tracing::info!(
            sinks = observation.consumer_count(),
            queue_depth = snap.observation.queue_depth,
            drop_policy = ?snap.observation.drop_policy,
            "observation enabled"
        );
    }
    let cfg = &snap.config;
    let listeners = cfg.listeners.as_ref();
    let Some(listeners) = listeners else {
        return Ok(DataplaneHandle {
            _threads: Vec::new(),
            observation,
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
    let timeout_ms = snap.forward.timeout_ms;

    let mut handles = Vec::new();
    let threads = listeners.threads.max(1);
    for ln in &listeners.listeners {
        for _ in 0..threads {
            let ln = ln.clone();
            let store = store.clone();
            let table = table.clone();
            let forward_compiled = snap.forward.clone();
            let bind_addresses_v4 = snap.egress_bind_addresses_v4();
            let obs = observation.clone();
            let reuse = listeners.reuse_port;
            let rcvbuf = listeners.rcvbuf;
            let proto = ln.protocol.to_lowercase();
            handles.push(thread::spawn(move || {
                let forward = match UdpForwardStage::new(
                    table.clone(),
                    &forward_compiled,
                    &bind_addresses_v4,
                    timeout_ms,
                ) {
                    Ok(f) => Arc::new(f),
                    Err(e) => {
                        tracing::error!(error = %e, "failed to build forward egress");
                        return;
                    }
                };
                let mut orchestrator = Orchestrator::with_default_stages();
                orchestrator.registry.register(Phase::Forward, forward);
                orchestrator
                    .registry
                    .register(Phase::WaitResponse, Arc::new(NoopWaitStage));
                let orchestrator = Arc::new(orchestrator);

                let result = if proto == "tcp" {
                    crate::listener::tcp::run_worker(ln, store, orchestrator, obs)
                } else {
                    crate::listener::udp::run_worker(ln, store, orchestrator, obs, reuse, rcvbuf)
                };
                if let Err(e) = result {
                    tracing::error!(error = %e, "listener worker exited");
                }
            }));
        }
    }

    Ok(DataplaneHandle {
        _threads: handles,
        observation,
    })
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
