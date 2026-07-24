//! Real probe I/O loop (design §D5/§D6): a single non-blocking, multiplexed
//! loop that fires probes for every backend and collects replies as they
//! arrive, so one dead backend never delays the others.
//!
//! The loop owns its own sockets (separate from listener/forward sockets) and
//! uses no transaction slot. UDP probes use one connected, non-blocking socket
//! per backend, multiplexed with the `polling` crate (the same epoll-based
//! mechanism as the forward I/O backend). TCP probes (when
//! `forward.upstream_transport` is TCP-only) use a fresh connection per probe,
//! run on a short-lived thread so a slow TCP handshake cannot stall the loop.
//! The loop **writes** per-backend health state (liveness + the damped latency
//! weight factor); Route reads that state lock-free for eligibility and
//! effective weight (Phase B).

use crate::forward::tcp::forward_tcp;
use crate::listener::DataplaneShutdown;
use crate::probe::scheduler::{BackendProbe, ProbeScheduler};
use conduit_config::forward::UpstreamTransport;
use conduit_config::health::CompiledHealth;
use conduit_core::clock::{Clock, SystemClock};
use conduit_core::health::{BackendKey, HealthRegistry, ProbeOutcome, ProbeSpec};
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_metrics::MetricsHub;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

/// Cap on how long the loop sleeps between iterations, so shutdown, probe
/// timeouts, and TCP completions are observed promptly (mirrors the forward I/O
/// backend's idle poll interval).
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(100);
const PROBE_RECV_BUF: usize = 4096;

/// Per-backend probe transport binding (built once at startup).
enum ProbeTransport {
    /// Connected, non-blocking UDP socket registered in the poller under
    /// `token == backend_idx`.
    Udp(UdpSocket),
    /// Fresh-connection-per-probe TCP, with optional bound source per family.
    Tcp {
        source_v4: Option<Ipv4Addr>,
        source_v6: Option<Ipv6Addr>,
    },
}

/// Outcome reported back from a TCP probe thread.
struct TcpOutcome {
    backend_idx: usize,
    wire: Option<Vec<u8>>, // Some = response received; None = connect/timeout/error
}

fn process_seed() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x1234_5678)
        ^ (std::process::id() as u64).rotate_left(17)
}

fn bind_probe_udp(
    source: Option<std::net::IpAddr>,
    backend: SocketAddr,
) -> std::io::Result<UdpSocket> {
    let bind_ip: std::net::IpAddr = match source {
        Some(ip) if ip.is_ipv4() == backend.is_ipv4() => ip,
        _ if backend.is_ipv4() => Ipv4Addr::UNSPECIFIED.into(),
        _ => Ipv6Addr::UNSPECIFIED.into(),
    };
    let socket = UdpSocket::bind(SocketAddr::new(bind_ip, 0))?;
    socket.connect(backend)?;
    socket.set_nonblocking(true)?;
    Ok(socket)
}

/// Build the per-backend scheduler entries and transport bindings from compiled
/// health config. Backends whose probe query cannot be built or whose socket
/// cannot be bound are skipped with a warning (the rest still probe).
fn build_backends(
    health: &CompiledHealth,
    registry: &HealthRegistry,
    upstream_transport: UpstreamTransport,
    seed: u64,
    now: Instant,
) -> (Vec<BackendProbe>, Vec<ProbeTransport>) {
    let use_tcp = matches!(upstream_transport, UpstreamTransport::TcpOnly);
    let mut spread = seed; // cheap LCG-ish spread for initial de-sync
    let mut backends = Vec::new();
    let mut transports = Vec::new();
    for (pool_name, pool) in &health.pools {
        let alpha = pool.latency_ewma_alpha;
        let interval = Duration::from_millis(pool.interval_ms as u64);
        let timeout = Duration::from_millis(pool.timeout_ms as u64);
        for backend in &pool.backends {
            let Some(state) = registry.get(pool_name, backend.address) else {
                continue;
            };
            let spec = match ProbeSpec::new(
                &backend.probe_qname,
                backend.probe_qtype,
                pool.acceptable_rcodes.clone(),
            ) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(pool = %pool_name, backend = %backend.address, error = %e, "skipping backend health probe");
                    continue;
                }
            };
            let transport = if use_tcp {
                let (source_v4, source_v6) = match backend.probe_source {
                    Some(std::net::IpAddr::V4(ip)) => (Some(ip), None),
                    Some(std::net::IpAddr::V6(ip)) => (None, Some(ip)),
                    None => (None, None),
                };
                ProbeTransport::Tcp {
                    source_v4,
                    source_v6,
                }
            } else {
                match bind_probe_udp(backend.probe_source, backend.address) {
                    Ok(sock) => ProbeTransport::Udp(sock),
                    Err(e) => {
                        tracing::warn!(pool = %pool_name, backend = %backend.address, error = %e, "failed to bind probe socket; skipping backend");
                        continue;
                    }
                }
            };
            // Spread initial sends across the interval so a fleet de-syncs.
            spread = spread.wrapping_mul(6364136223846793005).wrapping_add(1);
            let first_offset = if interval.as_millis() == 0 {
                Duration::ZERO
            } else {
                Duration::from_millis(spread % interval.as_millis() as u64)
            };
            backends.push(BackendProbe::new(
                BackendKey::new(pool_name.clone(), backend.address),
                backend.address,
                backend.label.clone(),
                backend.probe_source,
                state,
                spec,
                interval,
                timeout,
                pool.rise,
                pool.fall,
                alpha,
                pool.latency_weighting,
                pool.latency_floor,
                now + first_offset,
            ));
            transports.push(transport);
        }
    }
    (backends, transports)
}

/// Spawn the probe loop if any pool has health enabled. Returns `None` (no
/// thread) when health is unconfigured, preserving today's behavior.
pub fn spawn_probe_loop(
    snapshot: &Arc<RuntimeSnapshot>,
    registry: Arc<HealthRegistry>,
    metrics: Arc<MetricsHub>,
    shutdown: DataplaneShutdown,
) -> Option<thread::JoinHandle<()>> {
    if snapshot.health.is_empty() {
        return None;
    }
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let seed = process_seed();
    let now = clock.now();
    let (backends, transports) = build_backends(
        &snapshot.health,
        &registry,
        snapshot.forward.upstream_transport,
        seed,
        now,
    );
    if backends.is_empty() {
        return None;
    }
    let probe_count = backends.len();
    let scheduler = ProbeScheduler::new(clock.clone(), seed, backends);
    tracing::info!(backends = probe_count, "backend health probe loop starting");
    Some(thread::spawn(move || {
        if let Err(e) = run_loop(scheduler, transports, clock, metrics, shutdown) {
            tracing::error!(error = %e, "backend health probe loop exited");
        }
    }))
}

fn record_probe_result(
    metrics: &MetricsHub,
    scheduler: &ProbeScheduler,
    backend_idx: usize,
    outcome: &str,
) {
    if let Some((pool, backend)) = scheduler.backend_labels(backend_idx) {
        metrics
            .builtin()
            .record_probe_result(pool, backend, outcome);
    }
}

fn record_reply_outcome(
    metrics: &MetricsHub,
    scheduler: &ProbeScheduler,
    backend_idx: usize,
    outcome: Option<ProbeOutcome>,
) {
    match outcome {
        Some(ProbeOutcome::Success) => {
            record_probe_result(metrics, scheduler, backend_idx, "success");
        }
        Some(ProbeOutcome::Failure) => {
            record_probe_result(metrics, scheduler, backend_idx, "failure");
        }
        Some(ProbeOutcome::Unmatched) | None => {}
    }
}

fn run_loop(
    mut scheduler: ProbeScheduler,
    transports: Vec<ProbeTransport>,
    clock: Arc<dyn Clock>,
    metrics: Arc<MetricsHub>,
    shutdown: DataplaneShutdown,
) -> std::io::Result<()> {
    let poller = polling::Poller::new()?;
    for (idx, transport) in transports.iter().enumerate() {
        if let ProbeTransport::Udp(sock) = transport {
            // SAFETY: sockets are owned by `transports` for the loop's lifetime
            // and are not moved after registration.
            unsafe {
                poller.add_with_mode(
                    sock,
                    polling::Event::readable(idx),
                    polling::PollMode::Level,
                )?;
            }
        }
    }

    let (tcp_tx, tcp_rx) = crossbeam_channel::unbounded::<TcpOutcome>();
    let mut events = polling::Events::new();
    let mut buf = [0u8; PROBE_RECV_BUF];

    loop {
        if shutdown.is_shutdown() {
            break;
        }

        // 1) Issue probes that are due (skip-if-outstanding enforced by scheduler).
        for due in scheduler.due_probes() {
            match &transports[due.backend_idx] {
                ProbeTransport::Udp(sock) => {
                    if let Err(e) = sock.send(&due.wire) {
                        tracing::debug!(backend = %due.address, error = %e, "probe send failed");
                        if scheduler.on_failure(due.backend_idx) {
                            record_probe_result(
                                &metrics,
                                &scheduler,
                                due.backend_idx,
                                "send_error",
                            );
                        }
                    }
                }
                ProbeTransport::Tcp {
                    source_v4,
                    source_v6,
                } => {
                    let tx = tcp_tx.clone();
                    let backend = due.address;
                    let wire = due.wire;
                    let idx = due.backend_idx;
                    let timeout = due.timeout;
                    let (v4, v6) = (*source_v4, *source_v6);
                    thread::spawn(move || {
                        let result = forward_tcp(backend, &wire, timeout, v4, v6).ok();
                        let _ = tx.send(TcpOutcome {
                            backend_idx: idx,
                            wire: result,
                        });
                    });
                }
            }
        }

        // 2) Drain any completed TCP probes.
        while let Ok(outcome) = tcp_rx.try_recv() {
            match outcome.wire {
                Some(wire) => {
                    let classified = scheduler.on_reply(outcome.backend_idx, &wire);
                    record_reply_outcome(&metrics, &scheduler, outcome.backend_idx, classified);
                }
                None => {
                    if scheduler.on_failure(outcome.backend_idx) {
                        record_probe_result(
                            &metrics,
                            &scheduler,
                            outcome.backend_idx,
                            "send_error",
                        );
                    }
                }
            }
        }

        // 3) Expire UDP/TCP probes past their deadline (timeout = failure).
        for idx in scheduler.expire_timeouts() {
            record_probe_result(&metrics, &scheduler, idx, "timeout");
        }

        // 3b) Refresh damped latency effective-weight factors (design §D3).
        scheduler.recompute_weight_factors();

        // 4) Sleep until the next scheduled event (bounded so shutdown / TCP
        //    completions are observed promptly).
        let now = clock.now();
        let wait = scheduler
            .next_wakeup()
            .map(|w| w.saturating_duration_since(now))
            .unwrap_or(IDLE_POLL_INTERVAL)
            .min(IDLE_POLL_INTERVAL);
        events.clear();
        poller.wait(&mut events, Some(wait))?;

        // 5) Read replies on readable UDP sockets and feed them to the scheduler.
        for ev in events.iter() {
            if !ev.readable {
                continue;
            }
            if let Some(ProbeTransport::Udp(sock)) = transports.get(ev.key) {
                loop {
                    match sock.recv(&mut buf) {
                        Ok(len) => {
                            let classified = scheduler.on_reply(ev.key, &buf[..len]);
                            record_reply_outcome(&metrics, &scheduler, ev.key, classified);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(_) => break,
                    }
                }
            }
        }
    }
    Ok(())
}
