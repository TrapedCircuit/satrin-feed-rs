//! # k4-runner
//!
//! Main entry point for the crypto-gateway system.
//!
//! Loads a JSON configuration file, creates market data and trading modules
//! for each configured exchange connection, and manages their lifecycle.
//!
//! # Usage
//!
//! ```bash
//! k4-runner config.json --log-level info
//! ```

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use tracing::{error, info};

/// Crypto Gateway Market Data & Trading Runner.
#[derive(Parser)]
#[command(name = "k4-runner", about = "Crypto Gateway Market Data & Trading Runner")]
struct Cli {
    /// Configuration file path (JSON).
    config: PathBuf,

    /// Log level (trace, debug, info, warn, error).
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Optional log directory for file output.
    #[arg(long)]
    log_dir: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // 1. Initialize logging
    k4_core::logging::init_logging(&cli.log_level, cli.log_dir.as_deref(), "k4-runner");

    info!("k4-runner starting — config={}, log_level={}", cli.config.display(), cli.log_level,);

    // 2. Load configuration
    let config = k4_core::config::load_config(&cli.config)?;
    info!("config loaded — {} connection(s)", config.connections.len(),);

    // 3. Create and start MD modules from the connections array
    let mut md_modules: Vec<Box<dyn k4_md::MdModule>> = Vec::new();

    for (idx, conn_config) in config.connections.iter().enumerate() {
        match k4_md::registry::create_md_module(conn_config) {
            Ok(module) => {
                info!("connection[{idx}]: created MD module '{}' (exchange={})", module.name(), conn_config.exchange,);
                md_modules.push(module);
            }
            Err(e) => {
                error!("connection[{idx}]: failed to create module for '{}': {e}", conn_config.exchange,);
            }
        }
    }

    // Initialize shared memory for each module
    for module in &mut md_modules {
        module.init_shm().await?;
    }

    // Start all modules
    for module in &mut md_modules {
        module.start().await?;
        info!("module '{}' started", module.name());
    }

    info!("all {} module(s) started — press Ctrl+C to stop", md_modules.len(),);

    // 4. Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("shutdown signal received");

    // 5. Stop all modules gracefully
    for module in &mut md_modules {
        info!("stopping module '{}'", module.name());
        if let Err(e) = module.stop().await {
            error!("error stopping '{}': {e}", module.name());
        }
    }

    info!("all modules stopped — goodbye");
    Ok(())
}
