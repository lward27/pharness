mod api;
mod config;
mod executor;
mod service;
mod workspace;

use clap::{Parser, Subcommand};
use config::{HostConfig, LeaseExecutionConfig};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "pharness-codex-host",
    version,
    about = "Portable PHarness Codex agent host"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Enroll this host using a short-lived operator-created token.
    Enroll {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        enrollment_id: String,
        #[arg(
            long,
            env = "PHARNESS_AGENT_HOST_ENROLLMENT_TOKEN",
            hide_env_values = true
        )]
        enrollment_token: String,
    },
    /// Long-poll for leases and execute them on this host.
    Serve {
        #[arg(long, default_value = "/etc/pharness-codex-host/config.toml")]
        config: PathBuf,
    },
    /// Execute one already-claimed lease inside a runner image.
    #[command(hide = true)]
    ExecuteLease {
        #[arg(long)]
        config: PathBuf,
    },
    /// Validate local configuration and prerequisites without claiming work.
    Check {
        #[arg(long, default_value = "/etc/pharness-codex-host/config.toml")]
        config: PathBuf,
    },
    /// Run one read-only structured App Server turn for protocol calibration.
    #[command(hide = true)]
    ProtocolSmoke {
        #[arg(long, default_value = "/etc/pharness-codex-host/config.toml")]
        config: PathBuf,
        #[arg(long, default_value = "gpt-5.6-sol")]
        model: String,
        #[arg(long, default_value = "low")]
        effort: String,
    },
    /// Print immutable prompt and output-schema material for GitOps registry generation.
    #[command(hide = true)]
    PolicyMaterial,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("pharness_codex_host=info")),
        )
        .with_target(false)
        .init();
    match Cli::parse().command {
        Command::Enroll {
            config,
            enrollment_id,
            enrollment_token,
        } => service::enroll(&config, &enrollment_id, &enrollment_token).await,
        Command::Serve { config } => service::serve(HostConfig::load(&config)?).await,
        Command::ExecuteLease { config } => {
            let lease: LeaseExecutionConfig = serde_json::from_slice(&std::fs::read(config)?)?;
            executor::execute_lease(lease).await
        }
        Command::Check { config } => service::check(&HostConfig::load(&config)?).await,
        Command::ProtocolSmoke {
            config,
            model,
            effort,
        } => service::protocol_smoke(&HostConfig::load(&config)?, &model, &effort).await,
        Command::PolicyMaterial => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &pharness_codex_host::stage_contract::policy_material()
                )?
            );
            Ok(())
        }
    }
}
