//! Lab OTLP HTTP metrics receiver for Conduit `metrics.otel` push.
//!
//! **Not for production.** Accepts OTLP HTTP POST `/v1/metrics` (protobuf body as
//! sent by Conduit), counts accepts/failures, optional artificial delay, and
//! streams lab-oriented debug lines to stdout. Exposes `GET /stats` for harnesses.

use anyhow::{bail, Context, Result};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::Serialize;
use std::convert::Infallible;
use std::env;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::Notify;

const DEFAULT_LISTEN: &str = "127.0.0.1:4318";
const DEFAULT_PATH: &str = "/v1/metrics";
const STATS_PATH: &str = "/stats";

fn usage() -> &'static str {
    "conduit-otlp-metrics-tracer — lab OTLP HTTP metrics receiver (not for production)\n\
     \n\
     Accepts Conduit metrics.otel OTLP HTTP pushes (POST /v1/metrics), counts accepts\n\
     and failures, optionally delays responses, and writes debug lines to stdout.\n\
     GET /stats returns {\"accepts\":N,\"failures\":N} for harnesses.\n\
     \n\
     Usage:\n\
       conduit-otlp-metrics-tracer [-a host:port] [-p path] [-f log|json] [--delay-ms N]\n\
     \n\
     Options:\n\
       -a, --listen ADDR     Bind address (default: 127.0.0.1:4318)\n\
       -p, --path PATH       Metrics POST path (default: /v1/metrics)\n\
       -f, --format FMT      Debug output: log (default) or json\n\
       --delay-ms N          Sleep N ms before responding to a metrics POST (default: 0)\n\
       --stats-interval-s N  Print accept/failure counts every N seconds (0 = quiet; default: 0)\n\
       --once                Exit after the first successful metrics accept\n\
       -h, --help            Show this help\n"
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputFormat {
    Log,
    Json,
}

impl OutputFormat {
    fn parse(s: &str) -> Result<Self> {
        match s {
            "log" => Ok(Self::Log),
            "json" => Ok(Self::Json),
            other => bail!("unknown format '{other}' (use log or json)"),
        }
    }
}

struct Args {
    listen: SocketAddr,
    path: String,
    format: OutputFormat,
    delay_ms: u64,
    stats_interval_s: u64,
    once: bool,
}

fn parse_args() -> Result<Args> {
    let mut listen: SocketAddr = DEFAULT_LISTEN
        .parse()
        .expect("default listen address is valid");
    let mut path = DEFAULT_PATH.to_string();
    let mut format = OutputFormat::Log;
    let mut delay_ms = 0_u64;
    let mut stats_interval_s = 0_u64;
    let mut once = false;

    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{}", usage());
                std::process::exit(0);
            }
            "-a" | "--listen" => {
                let s = it.next().context("-a requires host:port")?;
                listen = s.parse().context("invalid listen address")?;
            }
            "-p" | "--path" => {
                path = it.next().context("-p requires path")?;
                if !path.starts_with('/') {
                    bail!("path must start with /");
                }
            }
            "-f" | "--format" => {
                let s = it.next().context("-f requires format")?;
                format = OutputFormat::parse(&s)?;
            }
            "--delay-ms" => {
                let s = it.next().context("--delay-ms requires integer")?;
                delay_ms = s.parse().context("invalid --delay-ms")?;
            }
            "--stats-interval-s" => {
                let s = it.next().context("--stats-interval-s requires integer")?;
                stats_interval_s = s.parse().context("invalid --stats-interval-s")?;
            }
            "--once" => once = true,
            other => bail!("unknown argument: {other}"),
        }
    }

    Ok(Args {
        listen,
        path,
        format,
        delay_ms,
        stats_interval_s,
        once,
    })
}

#[derive(Default)]
struct Counters {
    accepts: AtomicU64,
    failures: AtomicU64,
}

#[derive(Serialize)]
struct StatsSnapshot {
    accepts: u64,
    failures: u64,
}

impl Counters {
    fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            accepts: self.accepts.load(Ordering::SeqCst),
            failures: self.failures.load(Ordering::SeqCst),
        }
    }
}

struct AppState {
    path: String,
    format: OutputFormat,
    delay_ms: u64,
    once: bool,
    counters: Counters,
    once_done: AtomicBool,
    shutdown: Notify,
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn emit_debug(
    format: OutputFormat,
    accepts: u64,
    failures: u64,
    body_len: usize,
    content_type: &str,
) {
    match format {
        OutputFormat::Log => {
            println!(
                "otlp-metrics accept ts_ms={ts} body_bytes={body_len} content_type={ct} accepts={accepts} failures={failures}",
                ts = now_unix_ms(),
                ct = if content_type.is_empty() { "-" } else { content_type },
            );
        }
        OutputFormat::Json => {
            let line = serde_json::json!({
                "event": "accept",
                "ts_ms": now_unix_ms(),
                "body_bytes": body_len,
                "content_type": content_type,
                "accepts": accepts,
                "failures": failures,
            });
            println!("{line}");
        }
    }
}

fn emit_stats_line(format: OutputFormat, snap: &StatsSnapshot, reason: &str) {
    match format {
        OutputFormat::Log => {
            println!(
                "otlp-metrics stats reason={reason} accepts={} failures={}",
                snap.accepts, snap.failures
            );
        }
        OutputFormat::Json => {
            let line = serde_json::json!({
                "event": "stats",
                "reason": reason,
                "accepts": snap.accepts,
                "failures": snap.failures,
            });
            println!("{line}");
        }
    }
}

fn json_response(status: StatusCode, body: impl Into<Bytes>) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Full::new(body.into()))
        .expect("response builder")
}

fn empty_response(status: StatusCode) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .body(Full::new(Bytes::new()))
        .expect("response builder")
}

async fn handle_request(
    req: Request<hyper::body::Incoming>,
    state: Arc<AppState>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path();
    let method = req.method().clone();

    if method == Method::GET && path == STATS_PATH {
        let snap = state.counters.snapshot();
        let body = serde_json::to_vec(&snap).unwrap_or_else(|_| b"{}".to_vec());
        return Ok(json_response(StatusCode::OK, body));
    }

    if path != state.path {
        state.counters.failures.fetch_add(1, Ordering::SeqCst);
        return Ok(empty_response(StatusCode::NOT_FOUND));
    }

    if method != Method::POST {
        state.counters.failures.fetch_add(1, Ordering::SeqCst);
        return Ok(empty_response(StatusCode::METHOD_NOT_ALLOWED));
    }

    let content_type = req
        .headers()
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let collected = match req.collect().await {
        Ok(c) => c,
        Err(_) => {
            state.counters.failures.fetch_add(1, Ordering::SeqCst);
            return Ok(empty_response(StatusCode::BAD_REQUEST));
        }
    };
    let body = collected.to_bytes();
    if body.is_empty() {
        state.counters.failures.fetch_add(1, Ordering::SeqCst);
        return Ok(empty_response(StatusCode::BAD_REQUEST));
    }

    if state.delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(state.delay_ms)).await;
    }

    let accepts = state.counters.accepts.fetch_add(1, Ordering::SeqCst) + 1;
    let failures = state.counters.failures.load(Ordering::SeqCst);
    emit_debug(state.format, accepts, failures, body.len(), &content_type);

    if state.once
        && state
            .once_done
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    {
        state.shutdown.notify_waiters();
    }

    // Empty 200 is what Conduit's OTLP HTTP exporter expects on success.
    Ok(empty_response(StatusCode::OK))
}

async fn run(args: Args) -> Result<()> {
    let listener = TcpListener::bind(args.listen)
        .await
        .with_context(|| format!("bind {}", args.listen))?;
    let bound = listener.local_addr().unwrap_or(args.listen);
    eprintln!(
        "conduit-otlp-metrics-tracer: listening on http://{bound}{} (format={:?}, delay_ms={}, once={})",
        args.path, args.format, args.delay_ms, args.once
    );
    eprintln!("conduit-otlp-metrics-tracer: stats at http://{bound}{STATS_PATH}");

    let state = Arc::new(AppState {
        path: args.path,
        format: args.format,
        delay_ms: args.delay_ms,
        once: args.once,
        counters: Counters::default(),
        once_done: AtomicBool::new(false),
        shutdown: Notify::new(),
    });

    if args.stats_interval_s > 0 {
        let stats_state = state.clone();
        let interval = Duration::from_secs(args.stats_interval_s);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                let snap = stats_state.counters.snapshot();
                emit_stats_line(stats_state.format, &snap, "interval");
            }
        });
    }

    let shutdown_state = state.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = match signal(SignalKind::terminate()) {
                Ok(s) => s,
                Err(_) => {
                    let _ = tokio::signal::ctrl_c().await;
                    shutdown_state.shutdown.notify_waiters();
                    return;
                }
            };
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = sigterm.recv() => {}
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        shutdown_state.shutdown.notify_waiters();
    });

    loop {
        tokio::select! {
            _ = state.shutdown.notified() => break,
            accept = listener.accept() => {
                let Ok((stream, _)) = accept else {
                    continue;
                };
                let state = state.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let svc = service_fn(move |req| handle_request(req, state.clone()));
                    let _ = http1::Builder::new().serve_connection(io, svc).await;
                });
            }
        }
    }

    let snap = state.counters.snapshot();
    emit_stats_line(state.format, &snap, "shutdown");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args()?;
    run(args).await
}
