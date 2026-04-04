#![allow(unused_assignments)]

mod ai;
mod audio;
mod config;
mod context;
mod daemon;
mod dbus;
mod dictionary;
mod history;
mod hotkey;
mod input;
mod ipc;
mod memory;
mod recognition;
mod sound;
#[cfg(feature = "gui")]
mod ui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "koe", about = "Ubuntu voice input system", version)]
struct Cli {
    /// Disable context awareness (don't send window title/app name to AI)
    #[arg(long)]
    no_context: bool,

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
            let mut config = config::Config::load().context("loading config")?;
            // CLI --no-context overrides config
            if cli.no_context {
                config.ai.context_enabled = false;
            }
            tracing::info!(
                "Config loaded: recognition={:?}, ai={:?}",
                config.recognition.engine,
                config.ai.engine
            );
            daemon::run_daemon(config, cli.no_context).await?;
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
