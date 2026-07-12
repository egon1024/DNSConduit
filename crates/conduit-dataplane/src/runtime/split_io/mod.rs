//! Split I/O runtime: ingress, policy, and I/O worker pools.

mod ingress;
mod policy;
mod queue;

use super::orchestrator::{build_forward_egress, build_orchestrator};
use crate::forward::{ForwardMode, IoBackend, PoolInflight, TxnTable};
use crate::listener::supervisor::DataplaneHandle;
use crate::listener::{shutdown::DataplaneShutdown, startup_log};
use conduit_config::{effective_io_workers, effective_policy_workers, resolve_listener_ingress};
use conduit_core::snapshot::SnapshotStore;
use conduit_core::txn_store::{SharedTxnStore, SlotId, DEFAULT_SLOT_CHUNK_SIZE};
use conduit_events::EventHub;
use conduit_metrics::{MetricsHub, TracingHub};
use crossbeam_channel::RecvTimeoutError;
use ingress::{bind_tcp, bind_udp, run_tcp_ingress, run_udp_ingress};
use policy::run_policy_worker;
use queue::{PolicyQueue, PolicyWork};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub fn start_split_io(
    store: Arc<SnapshotStore>,
    metrics: Arc<MetricsHub>,
    tracing: Arc<TracingHub>,
) -> io::Result<DataplaneHandle> {
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
    let policy_queue = Arc::new(PolicyQueue::new());
    let reply_routes = Arc::new(queue::ReplyRoutes::new());
    let inflight = Arc::new(PoolInflight::from_config(cfg));
    let global_query_counter = Arc::new(AtomicU64::new(0));

    let timeout_ms = snap.forward.timeout_ms;
    let bind_addresses_v4 = snap.egress_bind_addresses_v4();
    let bind_addresses_v6 = snap.egress_bind_addresses_v6();
    let egress = build_forward_egress(
        &snap.forward,
        &bind_addresses_v4,
        &bind_addresses_v6,
        timeout_ms,
    )?;
    let egress_sockets = egress.all_udp_sockets();
    let (io_backend, io_resume_rx) = IoBackend::new(egress_sockets, table.clone(), timeout_ms)?;
    let io_shutdown = Arc::new(AtomicBool::new(false));
    let io_backend_for_orchestrator = Arc::new(io_backend.clone());

    // Runtime backend-health side-table (phase 1c). Owned by the snapshot store
    // so it survives reloads (reconciled, not rebuilt); the probe loop writes it
    // and Route reads it lock-free.
    let health_registry = store.health();
    let cache_registry = store.cache();
    cache_registry.set_metrics(metrics.clone());
    cache_registry.set_async_coalesce(true);
    let cache_wake_queue = policy_queue.clone();
    cache_registry.set_wake_handler(Arc::new(move |txn_id| {
        cache_wake_queue.push(PolicyWork::LookupResume(SlotId::from_index(txn_id as u32)));
    }));

    let orchestrator = build_orchestrator(
        &snap,
        table.clone(),
        &snap.forward,
        egress,
        timeout_ms,
        metrics.clone(),
        tracing.clone(),
        ForwardMode::Submit,
        Some(io_backend_for_orchestrator),
        Some(inflight.clone()),
        health_registry.clone(),
        Some(cache_registry.clone()),
    )?;

    let mut thread_handles = Vec::new();

    if let Some(h) = crate::probe::spawn_probe_loop(
        &snap,
        health_registry.clone(),
        metrics.clone(),
        shutdown.clone(),
    ) {
        thread_handles.push(h);
    }
    if let Some(h) = crate::cache_reaper::spawn_cache_reaper(cache_registry, shutdown.clone()) {
        thread_handles.push(h);
    }

    let policy_workers = effective_policy_workers(cfg);
    for _ in 0..policy_workers {
        let queue = policy_queue.clone();
        let txn_store = txn_store.clone();
        let orchestrator = orchestrator.clone();
        let store = store.clone();
        let events = events_hub.clone();
        let reply_routes = reply_routes.clone();
        let inflight = inflight.clone();
        let worker_shutdown = shutdown.clone();
        thread_handles.push(thread::spawn(move || {
            run_policy_worker(
                queue,
                txn_store,
                orchestrator,
                store,
                events,
                reply_routes,
                inflight,
                worker_shutdown,
            );
        }));
    }

    let io_workers = effective_io_workers(cfg);
    if io_workers > 1 {
        tracing::warn!(
            configured = io_workers,
            "dataplane.io_workers > 1 not yet scaled; using one I/O poll thread"
        );
    }
    let io_shutdown_poll = io_shutdown.clone();
    thread_handles.push(io_backend.spawn_poll_thread(io_shutdown_poll, shutdown.clone()));

    let policy_queue_io = policy_queue.clone();
    let io_resume_shutdown = shutdown.clone();
    let io_stop = io_shutdown.clone();
    thread_handles.push(thread::spawn(move || {
        while !io_resume_shutdown.is_shutdown() {
            match io_resume_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(resume) => policy_queue_io.push(PolicyWork::Resume(resume)),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        io_stop.store(true, Ordering::Relaxed);
    }));

    let Some(listeners) = listeners else {
        return Ok(DataplaneHandle::new(
            shutdown,
            thread_handles,
            events_hub,
            table,
            txn_store,
            health_registry,
        ));
    };

    for ln in &listeners.listeners {
        let ingress = resolve_listener_ingress(listeners, ln);
        let proto = ln.protocol.to_lowercase();
        for _ in 0..ingress.threads {
            let ln = ln.clone();
            let txn_store = txn_store.clone();
            let policy_queue = policy_queue.clone();
            let reply_routes = reply_routes.clone();
            let worker_shutdown = shutdown.clone();
            let global_query_counter = global_query_counter.clone();
            let metrics = metrics.clone();
            let reuse = ingress.reuse_port;
            let rcvbuf = ingress.rcvbuf;

            let worker = if proto == "tcp" {
                let (socket, addr) = bind_tcp(&ln)?;
                startup_log::log_listener_bound(addr, &ln.protocol);
                IngressKind::Tcp(socket.into())
            } else {
                let (socket, addr) = bind_udp(&ln, reuse, rcvbuf)?;
                startup_log::log_listener_bound(addr, &ln.protocol);
                IngressKind::Udp(Arc::new(socket.into()))
            };

            thread_handles.push(thread::spawn(move || {
                let result = match worker {
                    IngressKind::Tcp(tcp) => run_tcp_ingress(
                        tcp,
                        ln,
                        txn_store,
                        policy_queue,
                        reply_routes,
                        worker_shutdown,
                        global_query_counter,
                        metrics,
                    ),
                    IngressKind::Udp(udp) => run_udp_ingress(
                        udp,
                        ln,
                        txn_store,
                        policy_queue,
                        reply_routes,
                        worker_shutdown,
                        global_query_counter,
                        metrics,
                    ),
                };
                if let Err(e) = result {
                    tracing::error!(error = %e, "split_io ingress worker exited");
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

enum IngressKind {
    Tcp(std::net::TcpListener),
    Udp(Arc<std::net::UdpSocket>),
}
