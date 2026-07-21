//! Start listener worker threads for the active snapshot.

use crate::drain::{drain_slots, DrainFilter, DrainOutcome};
use crate::forward::{
    ForwardMode, ForwardTransport, TxnTable, WaitResponseStage, WorkerForwardEgress,
};
use crate::listener::{shutdown::DataplaneShutdown, startup_log, tcp, udp};
use conduit_config::resolve_listener_ingress;
use conduit_core::health::HealthRegistry;
use conduit_core::lookup::LookupStage;
use conduit_core::orchestrator::Orchestrator;
use conduit_core::phase::Phase;
use conduit_core::snapshot::SnapshotStore;
use conduit_core::stages::RouteStage;
use conduit_core::txn_store::{SharedTxnStore, DEFAULT_SLOT_CHUNK_SIZE};
use conduit_events::EventHub;
use conduit_metrics::{MetricsHub, TracingHub};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub struct DataplaneHandle {
    shutdown: DataplaneShutdown,
    threads: Vec<thread::JoinHandle<()>>,
    pub events: Arc<EventHub>,
    pub txn_table: Arc<TxnTable>,
    pub txn_store: SharedTxnStore,
    /// Per-backend health state written by the probe loop. Phase A exposes it
    /// for observation/tests only; routing does not read it yet.
    pub health: Arc<HealthRegistry>,
}

impl DataplaneHandle {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        shutdown: DataplaneShutdown,
        threads: Vec<thread::JoinHandle<()>>,
        events: Arc<EventHub>,
        txn_table: Arc<TxnTable>,
        txn_store: SharedTxnStore,
        health: Arc<HealthRegistry>,
    ) -> Self {
        Self {
            shutdown,
            threads,
            events,
            txn_table,
            txn_store,
            health,
        }
    }

    /// Wait for in-flight transaction slots to drain, or until `timeout`, or
    /// until `cancel` is set.
    ///
    /// `filter` selects which slots to wait on; `None` drains all slots. This
    /// blocks on the [`SharedTxnStore`] slot lifecycle (including parked
    /// `IoWait` legs); it does not stop listeners or implement process handoff.
    /// A `cancel` flag that becomes `true` mid-wait yields
    /// [`DrainOutcome::Aborted`] so a second shutdown signal can exit promptly.
    pub fn drain(
        &self,
        timeout: Duration,
        filter: Option<DrainFilter>,
        cancel: Option<&AtomicBool>,
    ) -> DrainOutcome {
        drain_slots(&self.txn_store, timeout, filter, cancel)
    }

    /// Signal workers to stop and join worker threads.
    ///
    /// Listener workers poll the cooperative [`DataplaneShutdown`] flag and use
    /// socket read timeouts (UDP) or non-blocking accept (TCP) so they exit
    /// without calling `shutdown()` on UDP sockets — that races with an in-flight
    /// `recv_from` and can panic in `std::net::UdpSocket` on Linux.
    pub fn shutdown(self) {
        self.shutdown.signal();
        for handle in self.threads {
            let _ = handle.join();
        }
        if let Ok(events) = Arc::try_unwrap(self.events) {
            events.shutdown();
        } else {
            tracing::warn!("observation hub still referenced after dataplane listener shutdown");
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

    let slot_capacity = orch_cfg.map(|o| o.txn_table_capacity).unwrap_or(1024);
    let slot_chunk = cfg
        .dataplane
        .as_ref()
        .and_then(|d| d.slot_chunk_size)
        .unwrap_or(DEFAULT_SLOT_CHUNK_SIZE);
    let txn_store = SharedTxnStore::new(slot_capacity, slot_chunk);

    let shutdown = DataplaneShutdown::new();

    // Runtime backend-health side-table (phase 1c). Owned by the snapshot store
    // so it survives reloads (reconciled, not rebuilt); the probe loop writes it
    // and Route reads it lock-free.
    let health_registry = store.health();
    let cache_registry = store.cache();
    cache_registry.set_metrics(metrics.clone());
    let probe_handle = crate::probe::spawn_probe_loop(
        &snap,
        health_registry.clone(),
        metrics.clone(),
        shutdown.clone(),
    );
    let reaper_handle =
        crate::cache_reaper::spawn_cache_reaper(cache_registry.clone(), shutdown.clone());

    let Some(listeners) = listeners else {
        return Ok(DataplaneHandle::new(
            shutdown,
            probe_handle.into_iter().chain(reaper_handle).collect(),
            events_hub,
            table,
            txn_store,
            health_registry,
        ));
    };

    let timeout_ms = snap.forward.timeout_ms;
    let global_query_counter = Arc::new(AtomicU64::new(0));

    let mut thread_handles = Vec::new();
    if let Some(h) = probe_handle {
        thread_handles.push(h);
    }
    if let Some(h) = reaper_handle {
        thread_handles.push(h);
    }
    for ln in &listeners.listeners {
        let ingress = resolve_listener_ingress(listeners, ln);
        let proto = ln.protocol.to_lowercase();
        for _ in 0..ingress.threads {
            let ln = ln.clone();
            let store = store.clone();
            let table = table.clone();
            let forward_compiled = snap.forward.clone();
            let bind_addresses_v4 = snap.egress_bind_addresses_v4();
            let bind_addresses_v6 = snap.egress_bind_addresses_v6();
            let parse_wire_meta = snap.scripting.needs_response_wire_meta;
            let obs = events_hub.clone();
            let metrics = metrics.clone();
            let tracing = tracing.clone();
            let reuse = ingress.reuse_port;
            let rcvbuf = ingress.rcvbuf;
            let worker_shutdown = shutdown.clone();
            let global_query_counter = global_query_counter.clone();
            let txn_store = txn_store.clone();
            let health_registry = health_registry.clone();

            let worker = if proto == "tcp" {
                let (socket, addr) = tcp::bind_socket(&ln)?;
                startup_log::log_listener_bound(addr, &ln.protocol);
                WorkerKind::Tcp(socket.into())
            } else {
                let (socket, addr) = udp::bind_socket(&ln, reuse, rcvbuf)?;
                startup_log::log_listener_bound(addr, &ln.protocol);
                WorkerKind::Udp(socket.into())
            };

            let cache = cache_registry.clone();
            thread_handles.push(thread::spawn(move || {
                let forward = match WorkerForwardEgress::new(
                    &forward_compiled,
                    &bind_addresses_v4,
                    &bind_addresses_v6,
                    timeout_ms,
                ) {
                    Ok(egress) => match ForwardTransport::new_with_mode_and_egress(
                        egress,
                        table.clone(),
                        &forward_compiled,
                        timeout_ms,
                        Some(metrics.clone()),
                        ForwardMode::Sync,
                        None,
                        None,
                        Some(health_registry.clone()),
                    ) {
                        Ok(f) => Arc::new(f),
                        Err(e) => {
                            tracing::error!(error = %e, "failed to build forward transport");
                            return;
                        }
                    },
                    Err(e) => {
                        tracing::error!(error = %e, "failed to build forward egress");
                        return;
                    }
                };
                let wait = Arc::new(WaitResponseStage::new(
                    parse_wire_meta,
                    Some(metrics.clone()),
                    Some(health_registry.clone()),
                ));
                let lookup = Arc::new(
                    LookupStage::new(
                        Arc::new(RouteStage::with_health(health_registry.clone())),
                        forward,
                        wait,
                        Some(metrics.clone()),
                    )
                    .with_cache(cache),
                );
                let mut orchestrator = Orchestrator::with_default_stages();
                orchestrator.metrics = Some(metrics.clone());
                orchestrator.tracing = Some(tracing.clone());
                let outstanding: conduit_core::stages::OutstandingPerBackendFn = {
                    let table = table.clone();
                    Arc::new(move || table.outstanding_per_backend().into_iter().collect())
                };
                orchestrator.registry.register(
                    Phase::RequestRules,
                    Arc::new(conduit_core::stages::RequestRulesStage {
                        metrics: Some(metrics.clone()),
                        health: Some(health_registry.clone()),
                        outstanding: Some(outstanding.clone()),
                    }),
                );
                orchestrator.registry.register(
                    Phase::ResponseRules,
                    Arc::new(conduit_core::stages::ResponseRulesStage {
                        metrics: Some(metrics.clone()),
                        health: Some(health_registry.clone()),
                        outstanding: Some(outstanding),
                    }),
                );
                orchestrator.registry.register(Phase::Lookup, lookup);
                let orchestrator = Arc::new(orchestrator);

                let result = match worker {
                    WorkerKind::Tcp(listener) => tcp::run_worker(
                        listener,
                        ln,
                        store,
                        txn_store,
                        orchestrator,
                        obs,
                        metrics,
                        worker_shutdown,
                        global_query_counter,
                    ),
                    WorkerKind::Udp(udp) => udp::run_worker(
                        udp,
                        ln,
                        store,
                        txn_store,
                        orchestrator,
                        obs,
                        metrics,
                        worker_shutdown,
                        global_query_counter,
                    ),
                };
                if let Err(e) = result {
                    tracing::error!(error = %e, "listener worker exited");
                }
            }));
        }
    }

    Ok(DataplaneHandle::new(
        shutdown,
        thread_handles,
        events_hub,
        table,
        txn_store,
        health_registry,
    ))
}

enum WorkerKind {
    Tcp(std::net::TcpListener),
    Udp(std::net::UdpSocket),
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::{load_yaml, validate};
    use conduit_core::RuntimeSnapshot;
    use std::sync::atomic::Ordering;

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

    /// Regression: shutting down while a UDP worker is blocked in `recv_from` must
    /// not call `socket.shutdown()` on the listener — that races and panics in
    /// `std::net::UdpSocket::recv_from` on Linux.
    #[test]
    fn shutdown_under_udp_recv_does_not_panic() {
        let yaml = r#"
schema_version: 1
listeners:
  threads: 1
  reuse_port: false
  listeners:
    - address: "127.0.0.1:25353"
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
        let stop = Arc::new(AtomicBool::new(false));
        let client_stop = stop.clone();
        let client = thread::spawn(move || {
            let sock = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            let target: std::net::SocketAddr = "127.0.0.1:25353".parse().unwrap();
            let payload = [0u8; 32];
            while !client_stop.load(Ordering::Relaxed) {
                let _ = sock.send_to(&payload, target);
            }
        });

        thread::sleep(Duration::from_millis(50));
        handle.shutdown();
        stop.store(true, Ordering::Relaxed);
        client.join().unwrap();
    }
}
