//! Start listener worker threads for the active snapshot.

use crate::forward::{ForwardTransport, TxnTable};
use crate::listener::{shutdown::DataplaneShutdown, startup_log, tcp, udp};
use conduit_core::orchestrator::Orchestrator;
use conduit_core::phase::Phase;
use conduit_core::snapshot::SnapshotStore;
use conduit_events::EventHub;
use conduit_metrics::{MetricsHub, TracingHub};
use socket2::Socket;
use std::net::Shutdown;
use std::sync::Arc;
use std::thread;

/// Holds a duplicate of a bound listener socket so `shutdown()` can unblock workers.
struct ListenerCloser {
    socket: Socket,
}

impl ListenerCloser {
    fn new(socket: Socket) -> std::io::Result<Self> {
        Ok(Self {
            socket: socket.try_clone()?,
        })
    }

    fn shutdown(&self) {
        let _ = self.socket.shutdown(Shutdown::Both);
    }
}

pub struct DataplaneHandle {
    shutdown: DataplaneShutdown,
    listener_closers: Vec<ListenerCloser>,
    threads: Vec<thread::JoinHandle<()>>,
    pub events: Arc<EventHub>,
    pub txn_table: Arc<TxnTable>,
}

impl DataplaneHandle {
    /// Signal workers to stop, shut down listener sockets, and join worker threads.
    pub fn shutdown(self) {
        self.shutdown.signal();
        for closer in &self.listener_closers {
            closer.shutdown();
        }
        for handle in self.threads {
            let _ = handle.join();
        }
    }
}

pub fn start(
    store: Arc<SnapshotStore>,
    metrics: Arc<MetricsHub>,
    tracing: Arc<TracingHub>,
) -> std::io::Result<DataplaneHandle> {
    let snap = store.load();
    let events_hub = Arc::new(EventHub::from_compiled(&snap.events));
    startup_log::log_startup_summary(&snap, &events_hub);
    let cfg = &snap.config;
    let listeners = cfg.listeners.as_ref();
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

    let shutdown = DataplaneShutdown::new();

    let Some(listeners) = listeners else {
        return Ok(DataplaneHandle {
            shutdown,
            listener_closers: Vec::new(),
            threads: Vec::new(),
            events: events_hub,
            txn_table: table,
        });
    };

    let timeout_ms = snap.forward.timeout_ms;

    let mut thread_handles = Vec::new();
    let mut listener_closers = Vec::new();
    let threads = listeners.threads.max(1);
    for ln in &listeners.listeners {
        let proto = ln.protocol.to_lowercase();
        for _ in 0..threads {
            let ln = ln.clone();
            let store = store.clone();
            let table = table.clone();
            let forward_compiled = snap.forward.clone();
            let bind_addresses_v4 = snap.egress_bind_addresses_v4();
            let bind_addresses_v6 = snap.egress_bind_addresses_v6();
            let obs = events_hub.clone();
            let metrics = metrics.clone();
            let tracing = tracing.clone();
            let reuse = listeners.reuse_port;
            let rcvbuf = listeners.rcvbuf;
            let worker_shutdown = shutdown.clone();

            let (closer, worker) = if proto == "tcp" {
                let (socket, addr) = tcp::bind_socket(&ln)?;
                startup_log::log_listener_bound(addr, &ln.protocol);
                let closer = ListenerCloser::new(socket.try_clone()?)?;
                let listener: std::net::TcpListener = socket.into();
                (closer, WorkerKind::Tcp(listener))
            } else {
                let (socket, addr) = udp::bind_socket(&ln, reuse, rcvbuf)?;
                startup_log::log_listener_bound(addr, &ln.protocol);
                let closer = ListenerCloser::new(socket.try_clone()?)?;
                let udp: std::net::UdpSocket = socket.into();
                (closer, WorkerKind::Udp(udp))
            };
            listener_closers.push(closer);

            thread_handles.push(thread::spawn(move || {
                let forward = match ForwardTransport::new(
                    table.clone(),
                    &forward_compiled,
                    &bind_addresses_v4,
                    &bind_addresses_v6,
                    timeout_ms,
                    Some(metrics.clone()),
                ) {
                    Ok(f) => Arc::new(f),
                    Err(e) => {
                        tracing::error!(error = %e, "failed to build forward egress");
                        return;
                    }
                };
                let mut orchestrator = Orchestrator::with_default_stages();
                orchestrator.metrics = Some(metrics.clone());
                orchestrator.tracing = Some(tracing.clone());
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
                orchestrator
                    .registry
                    .register(Phase::WaitResponse, Arc::new(NoopWaitStage));
                let orchestrator = Arc::new(orchestrator);

                let result = match worker {
                    WorkerKind::Tcp(listener) => {
                        tcp::run_worker(listener, ln, store, orchestrator, obs, worker_shutdown)
                    }
                    WorkerKind::Udp(udp) => {
                        udp::run_worker(udp, ln, store, orchestrator, obs, worker_shutdown)
                    }
                };
                if let Err(e) = result {
                    tracing::error!(error = %e, "listener worker exited");
                }
            }));
        }
    }

    Ok(DataplaneHandle {
        shutdown,
        listener_closers,
        threads: thread_handles,
        events: events_hub,
        txn_table: table,
    })
}

enum WorkerKind {
    Tcp(std::net::TcpListener),
    Udp(std::net::UdpSocket),
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

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::{load_yaml, validate};
    use conduit_core::RuntimeSnapshot;

    #[test]
    fn shutdown_joins_listener_workers() {
        let yaml = r#"
schema_version: 1
listeners:
  threads: 1
  reuse_port: false
  listeners:
    - address: "127.0.0.1:0"
      protocol: udp
forward:
  outstanding_per_backend: 10
  timeout_ms: 1000
orchestrator:
  max_attempts: 1
  max_txn_duration_ms: 1000
  txn_table_capacity: 64
events:
  queue_depth: 64
  drop_policy: drop_oldest
  sinks: []
rhai:
  max_operations: 1000
  max_call_depth: 8
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
control:
  listen_address: "127.0.0.1:0"
"#;
        let cfg = load_yaml(yaml).unwrap();
        assert!(validate(&cfg).ok);
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
            cfg.clone(),
        )));
        let metrics = Arc::new(MetricsHub::from_config(&cfg));
        let tracing = Arc::new(TracingHub::from_config(&cfg));

        let handle = start(store, metrics, tracing).unwrap();
        handle.shutdown();
    }
}
