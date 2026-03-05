#![allow(unused_assignments)]

mod ai;
mod audio;
mod config;
mod context;
mod daemon;
mod dictionary;
mod hotkey;
mod history;
mod memory;
mod input;
mod ipc;
mod recognition;
mod dbus;
mod sound;
#[cfg(feature = "gui")]
mod ui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "koe", about = "Ubuntu voice input system", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Open the settings window
    Settings,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        None => {
            // Default: run daemon
            tracing::info!("koe - Ubuntu Voice Input System starting...");
            let config = config::Config::load().context("loading config")?;
            tracing::info!(
                "Config loaded: recognition={:?}, ai={:?}",
                config.recognition.engine,
                config.ai.engine
            );
            daemon::run_daemon(config).await?;
        }
        Some(Commands::Settings) => {
            #[cfg(feature = "gui")]
            {
                ui::run_settings()?;
            }
            #[cfg(not(feature = "gui"))]
            {
                anyhow::bail!("GUI support not compiled. Rebuild with: cargo build --features gui");
            }
        }
    }

    Ok(())
}
