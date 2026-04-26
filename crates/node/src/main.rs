//! `pinaivu` — entry point.
//!
//! Parses CLI, loads config, assembles services, runs the daemon.

mod api;
mod cli;
mod daemon;
mod health;
mod identity;

use clap::Parser as _;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

use cli::{Cli, Commands, default_config_path};
use common::config::{NodeConfig, OperationMode};

// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // ── init subcommand — runs before logging/config setup ──────────────────
    if let Commands::Init { force } = &cli.command {
        return cmd_init(*force, &cli).await;
    }

    // ── Load config ──────────────────────────────────────────────────────────
    let config_path = cli.config.clone().unwrap_or_else(default_config_path);
    let mut config  = if config_path.exists() {
        NodeConfig::from_file(&config_path)
            .map_err(|e| anyhow::anyhow!("failed to read config at {}: {e}", config_path.display()))?
    } else {
        eprintln!(
            "Config not found at {}. Run `pinaivu init` first.",
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
            println!("pinaivu v{}", env!("CARGO_PKG_VERSION"));
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

async fn cmd_init(force: bool, cli: &Cli) -> anyhow::Result<()> {
    use inference::ollama::OllamaClient;

    let path = cli.config.clone().unwrap_or_else(default_config_path);

    if path.exists() && !force {
        eprintln!(
            "Config already exists at {}. Use --force to overwrite.",
            path.display()
        );
        std::process::exit(1);
    }

    println!("Initialising Pinaivu node...\n");

    let mut config = NodeConfig::default();

    // ── Step 1: Detect Ollama models ─────────────────────────────────────────
    print!("  Checking Ollama... ");
    let ollama = OllamaClient::default_local();
    match ollama.list_models().await {
        Ok(models) if !models.is_empty() => {
            // Prefer smaller/faster models for default; full list is advertised anyway.
            let preferred = ["gemma3:1b", "gemma3:4b", "llama3.2:1b", "llama3.1:8b", "deepseek-r1:7b"];
            let best = preferred
                .iter()
                .find(|&&p| models.iter().any(|m| m.name == p))
                .map(|s| s.to_string())
                .unwrap_or_else(|| models[0].name.clone());

            println!("found {} model(s)", models.len());
            for m in &models {
                println!("    ✓ {}", m.name);
            }
            println!("  Default model set to: {}", best);
            config.inference.default_model = best;
        }
        Ok(_) => {
            println!("running but no models installed");
            println!("  → Run: ollama pull gemma3:1b");
            println!("    (keeping default model — install a model before starting)");
        }
        Err(_) => {
            println!("not running");
            println!("  → Install Ollama from https://ollama.com then run: ollama pull gemma3:1b");
            println!("    (continuing with defaults — Ollama must be running before `pinaivu start`)");
        }
    }

    // ── Step 2: Detect public IP ──────────────────────────────────────────────
    print!("\n  Detecting public IP... ");
    let public_ip = detect_public_ip().await;
    match &public_ip {
        Some(ip) => {
            println!("{}", ip);
            // ── Step 3: Test if API port is reachable ─────────────────────────
            let api_port = config.health.api_port;
            print!("  Testing port {} reachability... ", api_port);
            if is_port_reachable(ip, api_port).await {
                let api_url = format!("http://{}:{}", ip, api_port);
                println!("open ✓");
                println!("  api_url set to: {}", api_url);
                config.health.api_url = Some(api_url);
            } else {
                println!("blocked (NAT/firewall)");
                println!("  → Your node can still earn — other nodes will reach you via P2P relay.");
                println!("    To enable direct connections: forward port {} on your router", api_port);
                println!("    or run: ngrok http {} and add the URL to api_url in your config.", api_port);
            }
        }
        None => {
            println!("unavailable (offline?)");
        }
    }

    // ── Step 4: Write config ──────────────────────────────────────────────────
    println!("\n  Writing config to {}...", path.display());
    config.write_to_file(&path)?;

    println!("\n✓ Node ready!\n");
    println!("  Start your node:   pinaivu start");
    println!("  List models:       pinaivu models");
    println!("  Node status:       pinaivu status");
    println!();

    Ok(())
}

async fn detect_public_ip() -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;
    // Try multiple services in order
    for url in &["https://api.ipify.org", "https://ifconfig.me/ip", "https://icanhazip.com"] {
        if let Ok(resp) = client.get(*url).send().await {
            if let Ok(text) = resp.text().await {
                let ip = text.trim().to_string();
                if !ip.is_empty() && ip.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    return Some(ip);
                }
            }
        }
    }
    None
}

async fn is_port_reachable(ip: &str, port: u16) -> bool {
    // Ask an external checker to avoid false positives from self-connection
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .unwrap_or_default();
    let url = format!("https://portchecker.io/api/v1/query?host={}&port={}", ip, port);
    if let Ok(resp) = client.get(&url).send().await {
        if let Ok(text) = resp.text().await {
            return text.contains("\"status\":true") || text.contains("\"open\":true");
        }
    }
    false
}

async fn cmd_start(mut config: NodeConfig) -> anyhow::Result<()> {
    use inference::ollama::OllamaClient;

    // Auto-detect best available model if configured model isn't in Ollama
    let ollama = OllamaClient::default_local();
    if let Ok(models) = ollama.list_models().await {
        let names: Vec<String> = models.iter().map(|m| m.name.clone()).collect();
        if !names.is_empty() {
            if !names.contains(&config.inference.default_model) {
                let preferred = ["gemma3:1b", "gemma3:4b", "llama3.2:1b", "llama3.1:8b", "deepseek-r1:7b"];
                let best = preferred
                    .iter()
                    .find(|&&p| names.iter().any(|n| n == p))
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| names[0].clone());
                info!(
                    configured = %config.inference.default_model,
                    selected   = %best,
                    "configured model not found in Ollama — switching to available model"
                );
                config.inference.default_model = best;
            }
            info!(available_models = ?names, "Ollama models detected");
        }
    }

    info!(
        version = env!("CARGO_PKG_VERSION"),
        mode    = ?config.node.mode,
        storage = %config.storage.backend,
        "starting pinaivu"
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
        engine:         daemon.inference_engine(),
        settlements:    daemon.settlements().to_vec(),
        identity:       daemon.identity(),
        version:        env!("CARGO_PKG_VERSION").to_string(),
        mode:           daemon.mode_str(),
        peer_registry:  daemon.peer_registry(),
        bid_collectors: daemon.bid_collectors(),
        p2p_service:    daemon.p2p_service_cloned(),
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
