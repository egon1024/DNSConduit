//! Operator CLI for the DNS Conduit control plane (design §8.3).

use anyhow::Context;
use clap::{Parser, Subcommand};
use conduit_config::{
    export_metrics_yaml, load_metrics_yaml, load_overlay_patch, load_yaml, validate,
};
use conduit_core::{check_client_acl, RuntimeSnapshot};
use conduit_proto::config::Config as RuntimeConfig;
use conduit_proto::config::MetricsConfig as RuntimeMetricsConfig;
use conduit_proto::control::backend_health_client::BackendHealthClient;
use conduit_proto::control::conduit_caches_client::ConduitCachesClient;
use conduit_proto::control::conduit_control_client::ConduitControlClient;
use conduit_proto::control::conduit_data_sources_client::ConduitDataSourcesClient;
use conduit_proto::control::conduit_events_client::ConduitEventsClient;
use conduit_proto::control::conduit_metrics_client::ConduitMetricsClient;
use conduit_proto::control::conduit_orchestrator_client::ConduitOrchestratorClient;
use conduit_proto::control::conduit_pools_client::ConduitPoolsClient;
use conduit_proto::control::conduit_rhai_client::ConduitRhaiClient;
use conduit_proto::control::Config as ControlConfig;
use conduit_proto::control::{
    ApplyConfigRequest, BackendHealthFilter, CheckAclRequest, DataSource as ControlDataSource,
    DataSourceLimits as ControlLimits, EventSinkFilters as ControlFilters, ExportConfigRequest,
    GetBackendHealthRequest, GetCacheRequest, GetDataSourceLimitsRequest, GetDataSourceRequest,
    GetEventSinkRequest, GetEventsRequest, GetMetricsRequest, GetOrchestratorRequest,
    GetPoolRequest, GetRhaiRequest, GetTraceRequest, HealthControlAction, HealthScope,
    HealthScopeLevel, ListCachesRequest, ListDataSourcesRequest, ListPoolsRequest,
    MetricsConfig as ControlMetricsConfig, OverlayApplyMode, PatchMetricsRequest,
    ReloadFromFileRequest, RemoveBackendRequest, RemoveDataSourceRequest, SetBackendWeightRequest,
    SetCacheLmdbHotRequest, SetCacheMaxEntriesRequest, SetCachePolicyHotRequest,
    SetDataSourceLimitsRequest, SetEventSinkEmitRequest, SetEventSinkFiltersRequest,
    SetHealthControlRequest, SetOrchestratorLimitsRequest, SetRhaiLimitsRequest,
    UpsertDataSourceRequest,
};
use conduitctl::{
    connect_channel, resolve_connect, with_auth, ConnectCliOverrides, ResolvedConnect,
};
use prost::Message;
use serde::Serialize;
use std::net::IpAddr;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "conduitctl", about = "DNS Conduit control plane client")]
struct Cli {
    /// Path to YAML client config (default: platform path or CONDUITCTL_CONFIG)
    #[arg(long)]
    config: Option<PathBuf>,

    /// gRPC control address (overrides env / client config; default http://127.0.0.1:5199)
    #[arg(long)]
    endpoint: Option<String>,

    /// API key (Authorization: Bearer); overrides CONDUIT_API_KEY / client config
    #[arg(long)]
    api_key: Option<String>,

    /// Read API key from a file (used when --api-key / CONDUIT_API_KEY unset)
    #[arg(long)]
    api_key_file: Option<PathBuf>,

    /// PEM CA / trust bundle for verifying the control server
    #[arg(long)]
    tls_ca: Option<PathBuf>,

    /// Client certificate PEM (mTLS)
    #[arg(long)]
    tls_cert: Option<PathBuf>,

    /// Client private key PEM (mTLS)
    #[arg(long)]
    tls_key: Option<PathBuf>,

    /// Skip TLS chain and hostname verification (explicit opt-out)
    #[arg(long = "tls-insecure", action = clap::ArgAction::SetTrue)]
    tls_insecure: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Apply an API overlay patch (default: merge into active overlay)
    Apply {
        /// YAML patch file (required unless --clear)
        #[arg(long)]
        file: Option<PathBuf>,

        /// Merge patch into the active overlay (default when neither --replace nor --clear is set)
        #[arg(long, conflicts_with_all = ["replace", "clear"])]
        merge: bool,

        /// Replace the entire overlay with this patch (empty patch clears overlay)
        #[arg(long, conflicts_with_all = ["merge", "clear"])]
        replace: bool,

        /// Clear the active overlay without re-reading the config file
        #[arg(long, conflicts_with_all = ["merge", "replace", "file"])]
        clear: bool,
    },
    /// Export effective configuration as YAML
    Export {
        #[arg(long, default_value = "-")]
        output: String,
    },
    /// Validate a config file without applying
    Validate {
        #[arg(long)]
        file: PathBuf,
    },
    /// Reload configuration from the server's startup file (clears API overlay)
    Reload,
    /// Fetch trace events for a transaction id
    Trace { txn_id: String },
    /// Backend health operator controls (phase 1c)
    Health {
        #[command(subcommand)]
        command: HealthCommands,
    },
    /// Pool config primitives (list / get)
    Pool {
        #[command(subcommand)]
        command: PoolCommands,
    },
    /// Backend config primitives (set-weight / remove)
    Backend {
        #[command(subcommand)]
        command: BackendCommands,
    },
    /// Client ACL inspection
    Acl {
        #[command(subcommand)]
        command: AclCommands,
    },
    /// Orchestrator config primitives
    Orchestrator {
        #[command(subcommand)]
        command: OrchestratorCommands,
    },
    /// Data source config primitives
    #[command(name = "data-source")]
    DataSource {
        #[command(subcommand)]
        command: DataSourceCommands,
    },
    /// Data source limits config primitives
    #[command(name = "data-source-limits")]
    DataSourceLimits {
        #[command(subcommand)]
        command: DataSourceLimitsCommands,
    },
    /// Events config primitives
    Events {
        #[command(subcommand)]
        command: EventsCommands,
    },
    /// Rhai scripting config primitives
    Rhai {
        #[command(subcommand)]
        command: RhaiCommands,
    },
    /// Metrics config primitives
    Metrics {
        #[command(subcommand)]
        command: MetricsCommands,
    },
    /// Cache config primitives
    Cache {
        #[command(subcommand)]
        command: CacheCommands,
    },
}

#[derive(Subcommand)]
enum AclCommands {
    /// Evaluate effective ACL policy for a client IP (pretty JSON on stdout)
    Check {
        /// Client IP address (IPv4 or IPv6)
        ip: String,
        /// Limit to one listener name (default: all listeners)
        #[arg(long)]
        listener: Option<String>,
        /// Offline: compile this config file instead of querying the live process
        #[arg(long)]
        file: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum PoolCommands {
    /// List pools in effective config
    List,
    /// Show one pool (including backends)
    Get {
        /// Pool name
        name: String,
    },
}

#[derive(Subcommand)]
enum BackendCommands {
    /// Set a backend's load-balancing weight
    #[command(name = "set-weight")]
    SetWeight {
        #[arg(long)]
        pool: String,
        /// Backend name or address (host:port)
        #[arg(long, value_name = "NAME|HOST:PORT")]
        backend: String,
        #[arg(long)]
        weight: u32,
    },
    /// Remove a backend from a pool
    Remove {
        #[arg(long)]
        pool: String,
        /// Backend name or address (host:port)
        #[arg(long, value_name = "NAME|HOST:PORT")]
        backend: String,
    },
}

#[derive(Subcommand)]
enum OrchestratorCommands {
    /// Get orchestrator config
    Get,
    /// Set orchestrator limits (at least one flag required)
    #[command(name = "set-limits")]
    SetLimits {
        #[arg(long)]
        max_attempts: Option<u32>,
        #[arg(long)]
        max_txn_duration_ms: Option<u32>,
    },
}

#[derive(Subcommand)]
enum DataSourceCommands {
    /// List data sources
    List,
    /// Get a data source
    Get {
        /// Data source name
        name: String,
    },
    /// Upsert a data source
    Upsert {
        #[arg(long)]
        name: String,
        /// Data source type (csv, cidr)
        #[arg(long, value_name = "TYPE")]
        r#type: String,
        #[arg(long)]
        path: String,
        #[arg(long)]
        key_column: Option<String>,
        #[arg(long)]
        value_column: Option<String>,
    },
    /// Remove a data source
    Remove {
        /// Data source name
        name: String,
    },
}

#[derive(Subcommand)]
enum DataSourceLimitsCommands {
    /// Get data source limits
    Get,
    /// Set data source limits
    Set {
        #[arg(long)]
        max_file_bytes: Option<u64>,
        #[arg(long)]
        max_entries: Option<u64>,
        #[arg(long)]
        max_key_bytes: Option<u32>,
        #[arg(long)]
        max_value_bytes: Option<u32>,
        #[arg(long)]
        max_tables: Option<u32>,
    },
}

#[derive(Subcommand)]
enum EventsCommands {
    /// Get events config
    Get,
    /// Get an event sink
    #[command(name = "get-sink")]
    GetSink {
        #[arg(long)]
        name: String,
    },
    /// Set event sink filters
    #[command(name = "set-filters")]
    SetFilters {
        #[arg(long)]
        name: String,
        #[arg(long)]
        sample_percent: Option<f64>,
        #[arg(long)]
        tag_required: Option<String>,
        #[arg(long)]
        pool: Option<String>,
        #[arg(long)]
        backend: Option<String>,
    },
    /// Set event sink emit list
    #[command(name = "set-emit")]
    SetEmit {
        #[arg(long)]
        name: String,
        /// Comma-separated emit types
        #[arg(long, value_delimiter = ',')]
        emit: Vec<String>,
    },
}

#[derive(Subcommand)]
enum RhaiCommands {
    /// Get Rhai config
    Get,
    /// Set Rhai limits (at least one flag required)
    #[command(name = "set-limits")]
    SetLimits {
        #[arg(long)]
        max_operations: Option<u64>,
        #[arg(long)]
        max_call_depth: Option<u32>,
        #[arg(long)]
        hook_timeout_ms: Option<u32>,
    },
}

#[derive(Subcommand)]
enum MetricsCommands {
    /// Get effective metrics config (YAML)
    Get,
    /// Patch metrics config (sparse deep-merge)
    Patch {
        /// Sparse metrics YAML file (bare object or top-level `metrics:` key)
        #[arg(long)]
        file: Option<PathBuf>,
        /// Enable or disable metrics
        #[arg(long)]
        enabled: Option<bool>,
        /// Set base (`none`, `minimal`, `standard`)
        #[arg(long)]
        base: Option<String>,
    },
}

#[derive(Subcommand)]
enum CacheCommands {
    /// List caches
    List,
    /// Get a cache
    Get {
        /// Cache name
        name: String,
    },
    /// Set cache max_entries
    #[command(name = "set-max-entries")]
    SetMaxEntries {
        #[arg(long)]
        name: String,
        #[arg(long)]
        max_entries: u64,
    },
    /// Set cache LMDB hot fields
    #[command(name = "set-lmdb-hot")]
    SetLmdbHot {
        #[arg(long)]
        name: String,
        #[arg(long)]
        when_full: Option<String>,
        #[arg(long)]
        sample_size: Option<u32>,
        #[arg(long)]
        sync: Option<String>,
        #[arg(long)]
        sync_interval: Option<String>,
        #[arg(long)]
        map_size_bytes: Option<u64>,
    },
    /// Set cache policy hot fields
    #[command(name = "set-policy-hot")]
    SetPolicyHot {
        #[arg(long)]
        name: String,
        #[arg(long)]
        rotate_rrset_on_serve: Option<bool>,
    },
}

#[derive(Subcommand)]
enum HealthCommands {
    /// Show per-backend health state
    Show {
        #[arg(long)]
        pool: Option<String>,
        #[arg(long, value_name = "HOST:PORT|NAME")]
        backend: Option<String>,
    },
    /// Freeze probe-driven transitions at a scope
    Freeze {
        #[arg(long)]
        global: bool,
        #[arg(long, conflicts_with = "global")]
        pool: Option<String>,
        #[arg(long, conflicts_with = "global", value_name = "HOST:PORT|NAME")]
        backend: Option<String>,
    },
    /// Manually set applied health (implies freeze / drain)
    Set {
        /// Target health: up or down
        state: String,
        #[arg(long)]
        global: bool,
        #[arg(long, conflicts_with = "global")]
        pool: Option<String>,
        #[arg(long, conflicts_with = "global", value_name = "HOST:PORT|NAME")]
        backend: Option<String>,
    },
    /// Resume automatic operation and snap applied := observed
    Resume {
        #[arg(long)]
        global: bool,
        #[arg(long, conflicts_with = "global")]
        pool: Option<String>,
        #[arg(long, conflicts_with = "global", value_name = "HOST:PORT|NAME")]
        backend: Option<String>,
    },
}

#[derive(Serialize)]
struct AclCheckOutput {
    ip: String,
    source: String,
    results: Vec<AclCheckResultOut>,
}

#[derive(Serialize)]
struct AclCheckResultOut {
    listener: String,
    decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tag: Option<String>,
    matched: String,
    action: String,
}

fn runtime_to_control(cfg: RuntimeConfig) -> ControlConfig {
    let bytes = cfg.encode_to_vec();
    ControlConfig::decode(bytes.as_slice()).expect("config and control Config are compatible")
}

fn runtime_metrics_to_control(metrics: RuntimeMetricsConfig) -> ControlMetricsConfig {
    let bytes = metrics.encode_to_vec();
    ControlMetricsConfig::decode(bytes.as_slice())
        .expect("config and control MetricsConfig are compatible")
}

fn control_metrics_to_runtime(metrics: ControlMetricsConfig) -> RuntimeMetricsConfig {
    let bytes = metrics.encode_to_vec();
    RuntimeMetricsConfig::decode(bytes.as_slice())
        .expect("config and control MetricsConfig are compatible")
}

/// True when a metrics patch carries no sparse fields (flags/file produced nothing).
fn metrics_patch_is_empty(m: &RuntimeMetricsConfig) -> bool {
    m.enabled.is_none()
        && m.profile.is_empty()
        && m.base.is_empty()
        && m.prometheus.is_none()
        && m.otel.is_none()
        && m.user_metrics.is_empty()
        && m.categories.is_none()
        && m.granularity.is_none()
        && m.collection.is_empty()
        && m.event_export.is_none()
}

fn connect_overrides(cli: &Cli) -> ConnectCliOverrides {
    ConnectCliOverrides {
        config_path: cli.config.clone(),
        endpoint: cli.endpoint.clone(),
        api_key: cli.api_key.clone(),
        api_key_file: cli.api_key_file.clone(),
        tls_ca: cli.tls_ca.clone(),
        tls_cert: cli.tls_cert.clone(),
        tls_key: cli.tls_key.clone(),
        tls_insecure_flag: cli.tls_insecure,
    }
}

fn resolve(cli: &Cli) -> anyhow::Result<ResolvedConnect> {
    resolve_connect(&connect_overrides(cli))
}

async fn client(
    resolved: &ResolvedConnect,
) -> anyhow::Result<ConduitControlClient<tonic::transport::Channel>> {
    Ok(ConduitControlClient::new(connect_channel(resolved).await?))
}

async fn health_client(
    resolved: &ResolvedConnect,
) -> anyhow::Result<BackendHealthClient<tonic::transport::Channel>> {
    Ok(BackendHealthClient::new(connect_channel(resolved).await?))
}

async fn pools_client(
    resolved: &ResolvedConnect,
) -> anyhow::Result<ConduitPoolsClient<tonic::transport::Channel>> {
    Ok(ConduitPoolsClient::new(connect_channel(resolved).await?))
}

async fn orchestrator_client(
    resolved: &ResolvedConnect,
) -> anyhow::Result<ConduitOrchestratorClient<tonic::transport::Channel>> {
    Ok(ConduitOrchestratorClient::new(
        connect_channel(resolved).await?,
    ))
}

async fn data_sources_client(
    resolved: &ResolvedConnect,
) -> anyhow::Result<ConduitDataSourcesClient<tonic::transport::Channel>> {
    Ok(ConduitDataSourcesClient::new(
        connect_channel(resolved).await?,
    ))
}

async fn events_client(
    resolved: &ResolvedConnect,
) -> anyhow::Result<ConduitEventsClient<tonic::transport::Channel>> {
    Ok(ConduitEventsClient::new(connect_channel(resolved).await?))
}

async fn rhai_client(
    resolved: &ResolvedConnect,
) -> anyhow::Result<ConduitRhaiClient<tonic::transport::Channel>> {
    Ok(ConduitRhaiClient::new(connect_channel(resolved).await?))
}

async fn metrics_client(
    resolved: &ResolvedConnect,
) -> anyhow::Result<ConduitMetricsClient<tonic::transport::Channel>> {
    Ok(ConduitMetricsClient::new(connect_channel(resolved).await?))
}

async fn caches_client(
    resolved: &ResolvedConnect,
) -> anyhow::Result<ConduitCachesClient<tonic::transport::Channel>> {
    Ok(ConduitCachesClient::new(connect_channel(resolved).await?))
}

fn health_scope(
    global: bool,
    pool: Option<String>,
    backend: Option<String>,
) -> anyhow::Result<HealthScope> {
    if global {
        return Ok(HealthScope {
            level: HealthScopeLevel::Global.into(),
            pool: None,
            backend: None,
        });
    }
    if let Some(pool) = pool {
        if let Some(backend) = backend {
            return Ok(HealthScope {
                level: HealthScopeLevel::Backend.into(),
                pool: Some(pool),
                backend: Some(backend),
            });
        }
        return Ok(HealthScope {
            level: HealthScopeLevel::Pool.into(),
            pool: Some(pool),
            backend: None,
        });
    }
    anyhow::bail!("scope required: use --global, --pool NAME, or --pool NAME --backend ADDR")
}

fn liveness_name(v: i32) -> &'static str {
    use conduit_proto::control::HealthLiveness;
    match HealthLiveness::try_from(v).unwrap_or(HealthLiveness::Unspecified) {
        HealthLiveness::Up => "up",
        HealthLiveness::Down => "down",
        HealthLiveness::Unknown => "unknown",
        HealthLiveness::Unspecified => "?",
    }
}

fn scope_name(v: i32) -> &'static str {
    use conduit_proto::control::HealthScopeState;
    match HealthScopeState::try_from(v).unwrap_or(HealthScopeState::Unspecified) {
        HealthScopeState::Automatic => "automatic",
        HealthScopeState::Frozen => "frozen",
        HealthScopeState::Inherit => "inherit",
        HealthScopeState::Unspecified => "?",
    }
}

fn print_acl_check(out: &AclCheckOutput) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(out)?);
    Ok(())
}

fn acl_check_offline(
    file: &PathBuf,
    ip: IpAddr,
    listener: Option<&str>,
) -> anyhow::Result<AclCheckOutput> {
    let yaml = std::fs::read_to_string(file).with_context(|| format!("reading {:?}", file))?;
    let cfg = load_yaml(&yaml)?;
    let v = validate(&cfg);
    if !v.ok {
        for e in &v.errors {
            eprintln!("{e}");
        }
        anyhow::bail!("validation failed");
    }
    let base_dir = file.parent();
    let snap = RuntimeSnapshot::try_from_config_with_base(cfg, base_dir).map_err(|e| {
        eprintln!("{e}");
        anyhow::anyhow!("compile failed")
    })?;
    let results = check_client_acl(&snap.config, &snap.scripting.data_sources, ip, listener)?;
    Ok(AclCheckOutput {
        ip: ip.to_string(),
        source: "file".into(),
        results: results
            .into_iter()
            .map(|r| AclCheckResultOut {
                listener: r.listener,
                decision: r.decision,
                tag: r.tag,
                matched: r.matched,
                action: r.action,
            })
            .collect(),
    })
}

async fn acl_check_live(
    resolved: &ResolvedConnect,
    ip: IpAddr,
    listener: Option<String>,
) -> anyhow::Result<AclCheckOutput> {
    let mut client = client(resolved).await?;
    let resp = client
        .check_acl(with_auth(
            resolved,
            tonic::Request::new(CheckAclRequest {
                ip: ip.to_string(),
                listener,
            }),
        )?)
        .await?
        .into_inner();
    Ok(AclCheckOutput {
        ip: resp.ip,
        source: "live".into(),
        results: resp
            .results
            .into_iter()
            .map(|r| AclCheckResultOut {
                listener: r.listener,
                decision: r.decision,
                tag: r.tag,
                matched: r.matched,
                action: r.action,
            })
            .collect(),
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    // Offline commands skip connect resolution except where they still parse CLI
    // (validate / acl check --file do not need a live channel).
    match &cli.command {
        Commands::Validate { file } => {
            let yaml =
                std::fs::read_to_string(file).with_context(|| format!("reading {:?}", file))?;
            let cfg = load_yaml(&yaml)?;
            let v = validate(&cfg);
            if !v.ok {
                for e in &v.errors {
                    eprintln!("{e}");
                }
                anyhow::bail!("validation failed");
            }
            let base_dir = file.parent();
            let snap = RuntimeSnapshot::try_from_config_with_base(cfg, base_dir).map_err(|e| {
                eprintln!("{e}");
                anyhow::anyhow!("compile failed")
            })?;
            for w in &snap.scripting.compile_warnings {
                eprintln!("warning: {w}");
            }
            println!("ok");
            return Ok(());
        }
        Commands::Acl {
            command: AclCommands::Check { ip, listener, file },
        } if file.is_some() => {
            let addr: IpAddr = ip
                .parse()
                .with_context(|| format!("invalid ip address '{ip}'"))?;
            let out = acl_check_offline(file.as_ref().unwrap(), addr, listener.as_deref())?;
            print_acl_check(&out)?;
            return Ok(());
        }
        _ => {}
    }

    let resolved = resolve(&cli)?;

    match cli.command {
        Commands::Apply {
            ref file,
            replace,
            clear,
            ..
        } => {
            let (mode, overlay) = if clear {
                (OverlayApplyMode::Clear, None)
            } else {
                let path = file
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("--file is required unless --clear is set"))?;
                let yaml =
                    std::fs::read_to_string(path).with_context(|| format!("reading {:?}", path))?;
                let overlay = load_overlay_patch(&yaml)?;
                let mode = if replace {
                    OverlayApplyMode::Replace
                } else {
                    OverlayApplyMode::Merge
                };
                (mode, Some(runtime_to_control(overlay)))
            };

            let mut client = client(&resolved).await?;
            let resp = client
                .apply_config(with_auth(
                    &resolved,
                    tonic::Request::new(ApplyConfigRequest {
                        overlay,
                        mode: mode.into(),
                    }),
                )?)
                .await?
                .into_inner();
            if !resp.ok {
                anyhow::bail!("apply failed: {}", resp.errors.join("; "));
            }
            println!("ok generation={}", resp.generation);
        }
        Commands::Export { ref output } => {
            let mut client = client(&resolved).await?;
            let resp = client
                .export_config(with_auth(
                    &resolved,
                    tonic::Request::new(ExportConfigRequest {
                        format: "yaml".into(),
                    }),
                )?)
                .await?
                .into_inner();
            if output == "-" {
                print!("{}", resp.body);
            } else {
                std::fs::write(output, &resp.body)
                    .with_context(|| format!("writing {:?}", output))?;
            }
        }
        Commands::Validate { .. } => unreachable!("handled above"),
        Commands::Reload => {
            let mut client = client(&resolved).await?;
            let resp = client
                .reload_from_file(with_auth(
                    &resolved,
                    tonic::Request::new(ReloadFromFileRequest {}),
                )?)
                .await?
                .into_inner();
            if !resp.ok {
                anyhow::bail!("reload failed: {}", resp.errors.join("; "));
            }
            println!("ok generation={}", resp.generation);
        }
        Commands::Trace { ref txn_id } => {
            let mut client = client(&resolved).await?;
            let resp = client
                .get_trace(with_auth(
                    &resolved,
                    tonic::Request::new(GetTraceRequest {
                        txn_id: txn_id.clone(),
                    }),
                )?)
                .await?
                .into_inner();
            if !resp.found {
                anyhow::bail!("trace not found");
            }
            for e in resp.events {
                println!(
                    "{} +{}us pool={:?} backend={:?} cache={:?} {}",
                    e.phase,
                    e.elapsed_us,
                    e.pool,
                    e.backend,
                    e.cache,
                    e.message.as_deref().unwrap_or("")
                );
            }
        }
        Commands::Acl { ref command } => match command {
            AclCommands::Check { ip, listener, file } => {
                debug_assert!(file.is_none());
                let addr: IpAddr = ip
                    .parse()
                    .with_context(|| format!("invalid ip address '{ip}'"))?;
                let out = acl_check_live(&resolved, addr, listener.clone()).await?;
                print_acl_check(&out)?;
            }
        },
        Commands::Health { ref command } => match command {
            HealthCommands::Show { pool, backend } => {
                let mut client = health_client(&resolved).await?;
                let filter = if pool.is_some() || backend.is_some() {
                    Some(BackendHealthFilter {
                        pool: pool.clone(),
                        backend: backend.clone(),
                    })
                } else {
                    None
                };
                let resp = client
                    .get_backend_health(with_auth(
                        &resolved,
                        tonic::Request::new(GetBackendHealthRequest { filter }),
                    )?)
                    .await?
                    .into_inner();
                for e in resp.entries {
                    let ewma = e
                        .latency_ewma_ms
                        .map(|v| format!("{v:.1}"))
                        .unwrap_or_else(|| "-".into());
                    let last = e
                        .last_transition_unix_ms
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".into());
                    println!(
                        "{} {} observed={} applied={} scope={} eligible={} ewma_ms={} last_transition_ms={}",
                        e.pool,
                        e.backend,
                        liveness_name(e.observed),
                        liveness_name(e.applied),
                        scope_name(e.scope_state),
                        e.eligible,
                        ewma,
                        last,
                    );
                }
            }
            HealthCommands::Freeze {
                global,
                pool,
                backend,
            } => {
                let mut client = health_client(&resolved).await?;
                let scope = health_scope(*global, pool.clone(), backend.clone())?;
                let resp = client
                    .set_health_control(with_auth(
                        &resolved,
                        tonic::Request::new(SetHealthControlRequest {
                            scope: Some(scope),
                            action: HealthControlAction::Freeze.into(),
                        }),
                    )?)
                    .await?
                    .into_inner();
                for r in resp.results {
                    println!(
                        "{} {} applied={} scope={}",
                        r.pool.unwrap_or_default(),
                        r.backend.unwrap_or_default(),
                        liveness_name(r.applied),
                        scope_name(r.scope_state),
                    );
                }
            }
            HealthCommands::Set {
                state,
                global,
                pool,
                backend,
            } => {
                let action = match state.to_ascii_lowercase().as_str() {
                    "up" => HealthControlAction::SetUp,
                    "down" => HealthControlAction::SetDown,
                    other => anyhow::bail!("state must be up or down, got {other:?}"),
                };
                let mut client = health_client(&resolved).await?;
                let scope = health_scope(*global, pool.clone(), backend.clone())?;
                let resp = client
                    .set_health_control(with_auth(
                        &resolved,
                        tonic::Request::new(SetHealthControlRequest {
                            scope: Some(scope),
                            action: action.into(),
                        }),
                    )?)
                    .await?
                    .into_inner();
                for r in resp.results {
                    println!(
                        "{} {} applied={} scope={}",
                        r.pool.unwrap_or_default(),
                        r.backend.unwrap_or_default(),
                        liveness_name(r.applied),
                        scope_name(r.scope_state),
                    );
                }
            }
            HealthCommands::Resume {
                global,
                pool,
                backend,
            } => {
                let mut client = health_client(&resolved).await?;
                let scope = health_scope(*global, pool.clone(), backend.clone())?;
                let resp = client
                    .set_health_control(with_auth(
                        &resolved,
                        tonic::Request::new(SetHealthControlRequest {
                            scope: Some(scope),
                            action: HealthControlAction::ResumeAutomatic.into(),
                        }),
                    )?)
                    .await?
                    .into_inner();
                for r in resp.results {
                    println!(
                        "{} {} applied={} scope={}",
                        r.pool.unwrap_or_default(),
                        r.backend.unwrap_or_default(),
                        liveness_name(r.applied),
                        scope_name(r.scope_state),
                    );
                }
            }
        },
        Commands::Pool { ref command } => match command {
            PoolCommands::List => {
                let mut client = pools_client(&resolved).await?;
                let resp = client
                    .list_pools(with_auth(
                        &resolved,
                        tonic::Request::new(ListPoolsRequest {}),
                    )?)
                    .await?
                    .into_inner();
                for p in resp.pools {
                    println!("{} backends={}", p.name, p.backend_count);
                }
            }
            PoolCommands::Get { name } => {
                let mut client = pools_client(&resolved).await?;
                let resp = client
                    .get_pool(with_auth(
                        &resolved,
                        tonic::Request::new(GetPoolRequest { name: name.clone() }),
                    )?)
                    .await?
                    .into_inner();
                let pool = resp
                    .pool
                    .ok_or_else(|| anyhow::anyhow!("empty GetPool response"))?;
                println!("pool {}", pool.name);
                for b in pool.backends {
                    let bname = b.name.as_deref().unwrap_or("-");
                    let weight = b
                        .weight
                        .map(|w| w.to_string())
                        .unwrap_or_else(|| "-".into());
                    println!(
                        "  backend name={bname} address={} weight={weight}",
                        b.address
                    );
                }
            }
        },
        Commands::Backend { ref command } => match command {
            BackendCommands::SetWeight {
                pool,
                backend,
                weight,
            } => {
                let mut client = pools_client(&resolved).await?;
                let resp = client
                    .set_backend_weight(with_auth(
                        &resolved,
                        tonic::Request::new(SetBackendWeightRequest {
                            pool: pool.clone(),
                            backend: backend.clone(),
                            weight: *weight,
                        }),
                    )?)
                    .await?
                    .into_inner();
                if !resp.ok {
                    anyhow::bail!("set-weight failed: {}", resp.errors.join("; "));
                }
                println!("ok generation={}", resp.generation);
            }
            BackendCommands::Remove { pool, backend } => {
                let mut client = pools_client(&resolved).await?;
                let resp = client
                    .remove_backend(with_auth(
                        &resolved,
                        tonic::Request::new(RemoveBackendRequest {
                            pool: pool.clone(),
                            backend: backend.clone(),
                        }),
                    )?)
                    .await?
                    .into_inner();
                if !resp.ok {
                    anyhow::bail!("remove failed: {}", resp.errors.join("; "));
                }
                println!("ok generation={}", resp.generation);
            }
        },
        Commands::Orchestrator { ref command } => match command {
            OrchestratorCommands::Get => {
                let mut client = orchestrator_client(&resolved).await?;
                let resp = client
                    .get_orchestrator(with_auth(
                        &resolved,
                        tonic::Request::new(GetOrchestratorRequest {}),
                    )?)
                    .await?
                    .into_inner();
                if let Some(orch) = resp.orchestrator {
                    println!(
                        "max_attempts={} max_txn_duration_ms={} txn_table_capacity={}",
                        orch.max_attempts, orch.max_txn_duration_ms, orch.txn_table_capacity
                    );
                } else {
                    println!("(no orchestrator config)");
                }
            }
            OrchestratorCommands::SetLimits {
                max_attempts,
                max_txn_duration_ms,
            } => {
                if max_attempts.is_none() && max_txn_duration_ms.is_none() {
                    anyhow::bail!(
                        "at least one of --max-attempts or --max-txn-duration-ms required"
                    );
                }
                let mut client = orchestrator_client(&resolved).await?;
                let resp = client
                    .set_orchestrator_limits(with_auth(
                        &resolved,
                        tonic::Request::new(SetOrchestratorLimitsRequest {
                            max_attempts: *max_attempts,
                            max_txn_duration_ms: *max_txn_duration_ms,
                        }),
                    )?)
                    .await?
                    .into_inner();
                if !resp.ok {
                    anyhow::bail!("set-limits failed: {}", resp.errors.join("; "));
                }
                println!("ok generation={}", resp.generation);
            }
        },
        Commands::DataSource { ref command } => match command {
            DataSourceCommands::List => {
                let mut client = data_sources_client(&resolved).await?;
                let resp = client
                    .list_data_sources(with_auth(
                        &resolved,
                        tonic::Request::new(ListDataSourcesRequest {}),
                    )?)
                    .await?
                    .into_inner();
                for s in resp.sources {
                    println!("{} type={} path={}", s.name, s.r#type, s.path);
                }
            }
            DataSourceCommands::Get { name } => {
                let mut client = data_sources_client(&resolved).await?;
                let resp = client
                    .get_data_source(with_auth(
                        &resolved,
                        tonic::Request::new(GetDataSourceRequest { name: name.clone() }),
                    )?)
                    .await?
                    .into_inner();
                if let Some(src) = resp.source {
                    println!("name={}", src.name);
                    println!("type={}", src.r#type);
                    println!("path={}", src.path);
                    println!("key_column={}", src.key_column);
                    println!("value_column={}", src.value_column);
                }
            }
            DataSourceCommands::Upsert {
                name,
                r#type,
                path,
                key_column,
                value_column,
            } => {
                let mut client = data_sources_client(&resolved).await?;
                let resp = client
                    .upsert_data_source(with_auth(
                        &resolved,
                        tonic::Request::new(UpsertDataSourceRequest {
                            source: Some(ControlDataSource {
                                name: name.clone(),
                                r#type: r#type.clone(),
                                path: path.clone(),
                                key_column: key_column.clone().unwrap_or_default(),
                                value_column: value_column.clone().unwrap_or_default(),
                                ..Default::default()
                            }),
                        }),
                    )?)
                    .await?
                    .into_inner();
                if !resp.ok {
                    anyhow::bail!("upsert failed: {}", resp.errors.join("; "));
                }
                println!("ok generation={}", resp.generation);
            }
            DataSourceCommands::Remove { name } => {
                let mut client = data_sources_client(&resolved).await?;
                let resp = client
                    .remove_data_source(with_auth(
                        &resolved,
                        tonic::Request::new(RemoveDataSourceRequest { name: name.clone() }),
                    )?)
                    .await?
                    .into_inner();
                if !resp.ok {
                    anyhow::bail!("remove failed: {}", resp.errors.join("; "));
                }
                println!("ok generation={}", resp.generation);
            }
        },
        Commands::DataSourceLimits { ref command } => match command {
            DataSourceLimitsCommands::Get => {
                let mut client = data_sources_client(&resolved).await?;
                let resp = client
                    .get_data_source_limits(with_auth(
                        &resolved,
                        tonic::Request::new(GetDataSourceLimitsRequest {}),
                    )?)
                    .await?
                    .into_inner();
                if let Some(limits) = resp.limits {
                    println!(
                        "max_file_bytes={} max_entries={} max_key_bytes={} max_value_bytes={} max_tables={} max_total_bytes={}",
                        limits.max_file_bytes, limits.max_entries, limits.max_key_bytes,
                        limits.max_value_bytes, limits.max_tables, limits.max_total_bytes
                    );
                }
            }
            DataSourceLimitsCommands::Set {
                max_file_bytes,
                max_entries,
                max_key_bytes,
                max_value_bytes,
                max_tables,
            } => {
                let mut client = data_sources_client(&resolved).await?;
                let resp = client
                    .set_data_source_limits(with_auth(
                        &resolved,
                        tonic::Request::new(SetDataSourceLimitsRequest {
                            limits: Some(ControlLimits {
                                max_file_bytes: max_file_bytes.unwrap_or(0),
                                max_entries: max_entries.unwrap_or(0),
                                max_key_bytes: max_key_bytes.unwrap_or(0),
                                max_value_bytes: max_value_bytes.unwrap_or(0),
                                max_tables: max_tables.unwrap_or(0),
                                max_total_bytes: 0,
                            }),
                        }),
                    )?)
                    .await?
                    .into_inner();
                if !resp.ok {
                    anyhow::bail!("set-limits failed: {}", resp.errors.join("; "));
                }
                println!("ok generation={}", resp.generation);
            }
        },
        Commands::Events { ref command } => match command {
            EventsCommands::Get => {
                let mut client = events_client(&resolved).await?;
                let resp = client
                    .get_events(with_auth(
                        &resolved,
                        tonic::Request::new(GetEventsRequest {}),
                    )?)
                    .await?
                    .into_inner();
                if let Some(events) = resp.events {
                    println!(
                        "queue_depth={} drop_policy={} sinks={}",
                        events.queue_depth,
                        events.drop_policy,
                        events.sinks.len()
                    );
                    for s in events.sinks {
                        let name = s
                            .name
                            .as_deref()
                            .filter(|n| !n.is_empty())
                            .unwrap_or(&s.export_id);
                        println!("  sink {} type={}", name, s.r#type);
                    }
                }
            }
            EventsCommands::GetSink { name } => {
                let mut client = events_client(&resolved).await?;
                let resp = client
                    .get_event_sink(with_auth(
                        &resolved,
                        tonic::Request::new(GetEventSinkRequest { name: name.clone() }),
                    )?)
                    .await?
                    .into_inner();
                if let Some(sink) = resp.sink {
                    println!("type={}", sink.r#type);
                    println!("export_id={}", sink.export_id);
                    println!("emit={:?}", sink.emit);
                    if let Some(f) = sink.filters {
                        println!("filters.sample_percent={:?}", f.sample_percent);
                    }
                }
            }
            EventsCommands::SetFilters {
                name,
                sample_percent,
                tag_required,
                pool,
                backend,
            } => {
                let mut client = events_client(&resolved).await?;
                let resp = client
                    .set_event_sink_filters(with_auth(
                        &resolved,
                        tonic::Request::new(SetEventSinkFiltersRequest {
                            name: name.clone(),
                            filters: Some(ControlFilters {
                                sample_percent: *sample_percent,
                                tag_required: tag_required.clone(),
                                pool: pool.clone(),
                                backend: backend.clone(),
                                selectors: vec![],
                                sample_key: None,
                                sample_key_from: None,
                            }),
                        }),
                    )?)
                    .await?
                    .into_inner();
                if !resp.ok {
                    anyhow::bail!("set-filters failed: {}", resp.errors.join("; "));
                }
                println!("ok generation={}", resp.generation);
            }
            EventsCommands::SetEmit { name, emit } => {
                let mut client = events_client(&resolved).await?;
                let resp = client
                    .set_event_sink_emit(with_auth(
                        &resolved,
                        tonic::Request::new(SetEventSinkEmitRequest {
                            name: name.clone(),
                            emit: emit.clone(),
                            extra_fields: vec![],
                            extra_tags: vec![],
                            extra_fields_set: false,
                            extra_tags_set: false,
                        }),
                    )?)
                    .await?
                    .into_inner();
                if !resp.ok {
                    anyhow::bail!("set-emit failed: {}", resp.errors.join("; "));
                }
                println!("ok generation={}", resp.generation);
            }
        },
        Commands::Rhai { ref command } => match command {
            RhaiCommands::Get => {
                let mut client = rhai_client(&resolved).await?;
                let resp = client
                    .get_rhai(with_auth(
                        &resolved,
                        tonic::Request::new(GetRhaiRequest {}),
                    )?)
                    .await?
                    .into_inner();
                if let Some(rhai) = resp.rhai {
                    println!(
                        "max_operations={} max_call_depth={} hook_timeout_ms={}",
                        rhai.max_operations, rhai.max_call_depth, rhai.hook_timeout_ms
                    );
                }
            }
            RhaiCommands::SetLimits {
                max_operations,
                max_call_depth,
                hook_timeout_ms,
            } => {
                if max_operations.is_none() && max_call_depth.is_none() && hook_timeout_ms.is_none()
                {
                    anyhow::bail!("at least one of --max-operations, --max-call-depth, or --hook-timeout-ms required");
                }
                let mut client = rhai_client(&resolved).await?;
                let resp = client
                    .set_rhai_limits(with_auth(
                        &resolved,
                        tonic::Request::new(SetRhaiLimitsRequest {
                            max_operations: *max_operations,
                            max_call_depth: *max_call_depth,
                            hook_timeout_ms: *hook_timeout_ms,
                        }),
                    )?)
                    .await?
                    .into_inner();
                if !resp.ok {
                    anyhow::bail!("set-limits failed: {}", resp.errors.join("; "));
                }
                println!("ok generation={}", resp.generation);
            }
        },
        Commands::Metrics { ref command } => match command {
            MetricsCommands::Get => {
                let mut client = metrics_client(&resolved).await?;
                let resp = client
                    .get_metrics(with_auth(
                        &resolved,
                        tonic::Request::new(GetMetricsRequest {}),
                    )?)
                    .await?
                    .into_inner();
                let metrics = resp
                    .metrics
                    .map(control_metrics_to_runtime)
                    .unwrap_or_default();
                print!("{}", export_metrics_yaml(&metrics)?);
            }
            MetricsCommands::Patch {
                file,
                enabled,
                base,
            } => {
                let mut patch = if let Some(path) = file {
                    let yaml = std::fs::read_to_string(path)
                        .with_context(|| format!("reading metrics patch {:?}", path))?;
                    load_metrics_yaml(&yaml)?
                } else {
                    RuntimeMetricsConfig::default()
                };
                if let Some(v) = enabled {
                    patch.enabled = Some(*v);
                }
                if let Some(v) = base {
                    patch.base = v.clone();
                }
                if metrics_patch_is_empty(&patch) {
                    anyhow::bail!(
                        "metrics patch requires --file and/or at least one of --enabled / --base"
                    );
                }
                let mut client = metrics_client(&resolved).await?;
                let resp = client
                    .patch_metrics(with_auth(
                        &resolved,
                        tonic::Request::new(PatchMetricsRequest {
                            metrics: Some(runtime_metrics_to_control(patch)),
                        }),
                    )?)
                    .await?
                    .into_inner();
                if !resp.ok {
                    anyhow::bail!("patch failed: {}", resp.errors.join("; "));
                }
                println!("ok generation={}", resp.generation);
            }
        },
        Commands::Cache { ref command } => match command {
            CacheCommands::List => {
                let mut client = caches_client(&resolved).await?;
                let resp = client
                    .list_caches(with_auth(
                        &resolved,
                        tonic::Request::new(ListCachesRequest {}),
                    )?)
                    .await?
                    .into_inner();
                for c in resp.caches {
                    let max = c
                        .max_entries
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".into());
                    println!("{} type={} max_entries={}", c.name, c.r#type, max);
                }
            }
            CacheCommands::Get { name } => {
                let mut client = caches_client(&resolved).await?;
                let resp = client
                    .get_cache(with_auth(
                        &resolved,
                        tonic::Request::new(GetCacheRequest { name: name.clone() }),
                    )?)
                    .await?
                    .into_inner();
                if let Some(cache) = resp.cache {
                    println!("name={}", cache.name);
                    println!("type={}", cache.r#type);
                    println!("max_entries={:?}", cache.max_entries);
                }
            }
            CacheCommands::SetMaxEntries { name, max_entries } => {
                let mut client = caches_client(&resolved).await?;
                let resp = client
                    .set_cache_max_entries(with_auth(
                        &resolved,
                        tonic::Request::new(SetCacheMaxEntriesRequest {
                            name: name.clone(),
                            max_entries: *max_entries,
                        }),
                    )?)
                    .await?
                    .into_inner();
                if !resp.ok {
                    anyhow::bail!("set-max-entries failed: {}", resp.errors.join("; "));
                }
                println!("ok generation={}", resp.generation);
            }
            CacheCommands::SetLmdbHot {
                name,
                when_full,
                sample_size,
                sync,
                sync_interval,
                map_size_bytes,
            } => {
                if when_full.is_none()
                    && sample_size.is_none()
                    && sync.is_none()
                    && sync_interval.is_none()
                    && map_size_bytes.is_none()
                {
                    anyhow::bail!("at least one LMDB hot field required");
                }
                let mut client = caches_client(&resolved).await?;
                let resp = client
                    .set_cache_lmdb_hot(with_auth(
                        &resolved,
                        tonic::Request::new(SetCacheLmdbHotRequest {
                            name: name.clone(),
                            when_full: when_full.clone(),
                            sample_size: *sample_size,
                            sync: sync.clone(),
                            sync_interval: sync_interval.clone(),
                            map_size_bytes: *map_size_bytes,
                        }),
                    )?)
                    .await?
                    .into_inner();
                if !resp.ok {
                    anyhow::bail!("set-lmdb-hot failed: {}", resp.errors.join("; "));
                }
                println!("ok generation={}", resp.generation);
            }
            CacheCommands::SetPolicyHot {
                name,
                rotate_rrset_on_serve,
            } => {
                if rotate_rrset_on_serve.is_none() {
                    anyhow::bail!("at least one policy field required");
                }
                let mut client = caches_client(&resolved).await?;
                let resp = client
                    .set_cache_policy_hot(with_auth(
                        &resolved,
                        tonic::Request::new(SetCachePolicyHotRequest {
                            name: name.clone(),
                            negative_cache: None,
                            on_hit: None,
                            truncated_udp: None,
                            rotate_rrset_on_serve: *rotate_rrset_on_serve,
                        }),
                    )?)
                    .await?
                    .into_inner();
                if !resp.ok {
                    anyhow::bail!("set-policy-hot failed: {}", resp.errors.join("; "));
                }
                println!("ok generation={}", resp.generation);
            }
        },
    }
    Ok(())
}
