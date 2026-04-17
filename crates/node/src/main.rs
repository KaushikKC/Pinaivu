//! `deai-node` — entry point.
//!
//! Parses CLI, loads config, assembles services, runs the daemon.

mod api;
mod cli;
mod daemon;
mod health;

use clap::Parser as _;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

use cli::{Cli, Commands, default_config_path};
use common::config::{NodeConfig, OperationMode};

// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // ── init subcommand — does not need logging or config ───────────────────
    if let Commands::Init { force } = &cli.command {
        return cmd_init(*force, &cli);
    }

    // ── Load config ──────────────────────────────────────────────────────────
    let config_path = cli.config.clone().unwrap_or_else(default_config_path);
    let mut config  = if config_path.exists() {
        NodeConfig::from_file(&config_path)
            .map_err(|e| anyhow::anyhow!("failed to read config at {}: {e}", config_path.display()))?
    } else {
        eprintln!(
            "Config not found at {}. Run `deai-node init` first.",
            config_path.display()
        );
        std::process::exit(1);
    };

    // ── Start logging ────────────────────────────────────────────────────────
    init_logging(&config.node.log_level);

    // ── Apply CLI overrides ───────────────────────────────────────────────────
    if let Commands::Start { mode, metrics_port } = &cli.command {
        if let Some(mode_str) = mode {
            config.node.mode = parse_mode(mode_str)?;
        }
        if let Some(port) = metrics_port {
            config.health.metrics_port = *port;
        }
    }

    // ── Dispatch ──────────────────────────────────────────────────────────────
    match &cli.command {
        Commands::Init { .. } => unreachable!(),

        Commands::Start { .. } => cmd_start(config).await?,

        Commands::Status => {
            println!("deai-node v{}", env!("CARGO_PKG_VERSION"));
            println!("mode:    {:?}", config.node.mode);
            println!("storage: {}", config.storage.backend);
            println!("metrics: :{}", config.health.metrics_port);
        }

        Commands::Models => cmd_models(&config).await?,
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Subcommand handlers
// ---------------------------------------------------------------------------

fn cmd_init(force: bool, cli: &Cli) -> anyhow::Result<()> {
    let path = cli.config.clone().unwrap_or_else(default_config_path);

    if path.exists() && !force {
        eprintln!("Config already exists at {}. Use --force to overwrite.", path.display());
        std::process::exit(1);
    }

    let config = NodeConfig::default();
    config.write_to_file(&path)?;
    println!("Config written to {}", path.display());
    println!("Edit it to configure your node, then run `deai-node start`.");
    Ok(())
}

async fn cmd_start(config: NodeConfig) -> anyhow::Result<()> {
    info!(
        version = env!("CARGO_PKG_VERSION"),
        mode    = ?config.node.mode,
        storage = %config.storage.backend,
        "starting deai-node"
    );

    // Assemble daemon
    let daemon = daemon::DeAIDaemon::from_config(config.clone()).await?;

    // Start health/metrics server
    let health_state = health::HealthState {
        p2p:          daemon.p2p_service(),
        node_version: env!("CARGO_PKG_VERSION").to_string(),
        mode:         daemon.mode_str(),
    };
    health::start(config.health.metrics_port, health_state).await?;

    // Start inference API server (used by the TS SDK + web UI in standalone mode)
    let api_state = api::ApiState {
        engine:      daemon.inference_engine(),
        settlements: daemon.settlements().to_vec(),
        version:     env!("CARGO_PKG_VERSION").to_string(),
        mode:        daemon.mode_str(),
    };
    api::start(config.health.api_port, api_state).await?;

    info!(
        health_port = config.health.metrics_port,
        api_port    = config.health.api_port,
        "health: http://localhost:{}/health  |  api: http://localhost:{}/v1/infer",
        config.health.metrics_port,
        config.health.api_port,
    );

    // Run until shutdown
    daemon.run().await
}

async fn cmd_models(config: &NodeConfig) -> anyhow::Result<()> {
    use inference::ollama::OllamaClient;
    let client = OllamaClient::new("http://localhost:11434");
    match client.list_models().await {
        Ok(models) => {
            if models.is_empty() {
                println!("No models found. Run: ollama pull llama3.1:8b");
            } else {
                for m in &models {
                    println!("  {}", m.name);
                }
            }
        }
        Err(e) => {
            eprintln!("Could not reach Ollama at http://localhost:11434: {e}");
            eprintln!("Is Ollama running? Start it with: ollama serve");
            std::process::exit(1);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn init_logging(level: &str) {
    let filter = EnvFilter::try_from_env("RUST_LOG")
        .unwrap_or_else(|_| EnvFilter::new(level));

    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}

fn parse_mode(s: &str) -> anyhow::Result<OperationMode> {
    match s.to_lowercase().as_str() {
        "standalone"    => Ok(OperationMode::Standalone),
        "network"       => Ok(OperationMode::Network),
        "network_paid"  => Ok(OperationMode::NetworkPaid),
        other           => Err(anyhow::anyhow!(
            "unknown mode '{}' — expected: standalone | network | network_paid", other
        )),
    }
}
