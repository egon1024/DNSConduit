//! Operator CLI for the DNS Conduit control plane (design §8.3).

use anyhow::Context;
use clap::{Parser, Subcommand};
use conduit_config::{load_yaml, validate};
use conduit_proto::config::Config as RuntimeConfig;
use conduit_proto::control::conduit_control_client::ConduitControlClient;
use conduit_proto::control::Config as ControlConfig;
use conduit_proto::control::{
    ApplyConfigRequest, ExportConfigRequest, GetTraceRequest, ReloadFromFileRequest,
};
use prost::Message;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "conduitctl", about = "DNS Conduit control plane client")]
struct Cli {
    /// gRPC control address (default from CONDUIT_CONTROL or 127.0.0.1:5199)
    #[arg(long, env = "CONDUIT_CONTROL", default_value = "http://127.0.0.1:5199")]
    endpoint: String,

    /// API key (Authorization: Bearer); overrides CONDUIT_API_KEY
    #[arg(long, env = "CONDUIT_API_KEY")]
    api_key: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Apply an API overlay from a YAML file
    Apply {
        #[arg(long)]
        file: PathBuf,
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
    Trace { trace_id: String },
}

fn runtime_to_control(cfg: RuntimeConfig) -> ControlConfig {
    let bytes = cfg.encode_to_vec();
    ControlConfig::decode(bytes.as_slice()).expect("config and control Config are compatible")
}

async fn client(cli: &Cli) -> anyhow::Result<ConduitControlClient<tonic::transport::Channel>> {
    tonic::transport::Endpoint::new(cli.endpoint.clone())?
        .connect()
        .await
        .map(ConduitControlClient::new)
        .context("connect to control plane")
}

fn auth_metadata(
    cli: &Cli,
) -> anyhow::Result<Option<tonic::metadata::MetadataValue<tonic::metadata::Ascii>>> {
    let Some(ref key) = cli.api_key else {
        return Ok(None);
    };
    let value = format!("Bearer {key}");
    let meta = tonic::metadata::MetadataValue::try_from(value.as_str())
        .context("invalid API key for metadata")?;
    Ok(Some(meta))
}

fn with_auth<T>(cli: &Cli, mut request: tonic::Request<T>) -> anyhow::Result<tonic::Request<T>> {
    if let Some(meta) = auth_metadata(cli)? {
        request.metadata_mut().insert("authorization", meta);
    }
    Ok(request)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Apply { ref file } => {
            let yaml =
                std::fs::read_to_string(file).with_context(|| format!("reading {:?}", file))?;
            let overlay = load_yaml(&yaml)?;
            let mut client = client(&cli).await?;
            let resp = client
                .apply_config(with_auth(
                    &cli,
                    tonic::Request::new(ApplyConfigRequest {
                        overlay: Some(runtime_to_control(overlay)),
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
            let mut client = client(&cli).await?;
            let resp = client
                .export_config(with_auth(
                    &cli,
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
        Commands::Validate { ref file } => {
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
            println!("ok");
        }
        Commands::Reload => {
            let mut client = client(&cli).await?;
            let resp = client
                .reload_from_file(with_auth(
                    &cli,
                    tonic::Request::new(ReloadFromFileRequest {}),
                )?)
                .await?
                .into_inner();
            if !resp.ok {
                anyhow::bail!("reload failed: {}", resp.errors.join("; "));
            }
            println!("ok");
        }
        Commands::Trace { ref trace_id } => {
            let mut client = client(&cli).await?;
            let resp = client
                .get_trace(with_auth(
                    &cli,
                    tonic::Request::new(GetTraceRequest {
                        trace_id: trace_id.clone(),
                    }),
                )?)
                .await?
                .into_inner();
            if !resp.found {
                anyhow::bail!("trace not found");
            }
            for e in resp.events {
                println!(
                    "{} +{}us pool={:?} backend={:?} {}",
                    e.phase,
                    e.elapsed_us,
                    e.pool,
                    e.backend,
                    e.message.as_deref().unwrap_or("")
                );
            }
        }
    }
    Ok(())
}
