//! Operator CLI for the DNS Conduit control plane (design §8.3).

use anyhow::Context;
use clap::{Parser, Subcommand};
use conduit_config::{load_overlay_patch, load_yaml, validate};
use conduit_core::{check_client_acl, RuntimeSnapshot};
use conduit_proto::config::Config as RuntimeConfig;
use conduit_proto::control::backend_health_client::BackendHealthClient;
use conduit_proto::control::conduit_control_client::ConduitControlClient;
use conduit_proto::control::Config as ControlConfig;
use conduit_proto::control::{
    ApplyConfigRequest, BackendHealthFilter, CheckAclRequest, ExportConfigRequest,
    GetBackendHealthRequest, GetTraceRequest, HealthControlAction, HealthScope, HealthScopeLevel,
    OverlayApplyMode, ReloadFromFileRequest, SetHealthControlRequest,
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
    /// Client ACL inspection
    Acl {
        #[command(subcommand)]
        command: AclCommands,
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
            println!("ok");
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
            println!("ok");
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
    }
    Ok(())
}
