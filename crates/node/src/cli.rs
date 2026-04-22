//! CLI definitions for `pinaivu`.
//!
//! ```
//! pinaivu init         — create default config at ~/.pinaivu/config.toml
//! pinaivu start        — start the daemon (reads config file)
//! pinaivu status       — show node status (connects to running daemon)
//! pinaivu models       — list available models from Ollama
//! ```

use std::path::PathBuf;

use clap::{Parser, Subcommand};

// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(
    name    = "pinaivu",
    version = env!("CARGO_PKG_VERSION"),
    about   = "Pinaivu — decentralised AI inference node",
    long_about = None,
)]
pub struct Cli {
    /// Path to the config file (default: ~/.pinaivu/config.toml).
    #[arg(long, short = 'c', global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialise a new node — creates default config at ~/.pinaivu/config.toml.
    Init {
        /// Force overwrite if config already exists.
        #[arg(long)]
        force: bool,
    },

    /// Start the daemon.
    Start {
        /// Operation mode override (standalone | network | network_paid).
        #[arg(long, env = "DEAI_MODE")]
        mode: Option<String>,

        /// Health/metrics port override.
        #[arg(long, env = "DEAI_METRICS_PORT")]
        metrics_port: Option<u16>,
    },

    /// Print current node status and exit.
    Status,

    /// List models available from Ollama.
    Models,
}

// ---------------------------------------------------------------------------
// Default config path
// ---------------------------------------------------------------------------

pub fn default_config_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".pinaivu").join("config.toml")
}
