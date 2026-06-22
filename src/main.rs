//! alavai — a lightweight Tailscale client for Linux.
//!
//! Phase 0 ships a small CLI over the LocalAPI client. The tray daemon
//! (`ksni`) and GUI (`iced`) land in later phases — see docs/PLAN.md.

mod gui;
mod localapi;
mod tray;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use localapi::Client;

#[derive(Parser)]
#[command(
    name = "alavai",
    version,
    about = "A lightweight Tailscale client for Linux with one-click tailnet switching"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show the current Tailscale status.
    Status,
    /// List configured tailnets (Tailscale login profiles).
    Tailnets,
    /// Switch to a configured tailnet, by ID, name, or domain.
    Switch {
        /// Tailnet ID, profile name, or domain (e.g. "karo.co.nz").
        tailnet: String,
    },
    /// Run the system-tray daemon (one-click tailnet switching).
    Tray,
    /// Open the main window.
    Gui,
    /// Stream live state changes from the daemon's IPN bus (debug).
    Watch,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = Client::default();

    match cli.command {
        Command::Status => {
            let s = client.status()?;
            println!("Backend:  {}", s.backend_state);
            if let Some(node) = &s.self_node {
                println!("Machine:  {} ({})", node.hostname, node.dns_name.trim_end_matches('.'));
            }
            if !s.tailscale_ips.is_empty() {
                println!("Address:  {}", s.tailscale_ips.join(", "));
            }
            if let Ok(p) = client.current_profile() {
                if !p.is_empty() {
                    println!("Tailnet:  {}", p.label());
                }
            }
        }

        Command::Tailnets => {
            let profiles = client.profiles()?;
            let current = client.current_profile().ok();
            let current_id = current.as_ref().map(|p| p.id.as_str()).unwrap_or("");
            if profiles.is_empty() {
                println!("No tailnets configured.");
            }
            for p in &profiles {
                if p.is_empty() {
                    continue;
                }
                let marker = if p.id == current_id { "●" } else { " " };
                println!(
                    "{marker} {:<10} {:<24} {}",
                    p.id,
                    p.label(),
                    p.user.login_name
                );
            }
        }

        Command::Switch { tailnet } => {
            let profiles = client.profiles()?;
            let matched = profiles.iter().find(|p| {
                !p.is_empty()
                    && (p.id == tailnet
                        || p.name == tailnet
                        || p.network.domain == tailnet
                        || p.network.display_name == tailnet)
            });
            let Some(p) = matched else {
                bail!(
                    "no configured tailnet matches {tailnet:?}; run `alavai tailnets` to list them"
                );
            };
            client.switch_profile(&p.id)?;
            println!("Switched to {}", p.label());
        }

        Command::Tray => {
            tray::run()?;
        }

        Command::Gui => {
            gui::run()?;
        }

        Command::Watch => {
            client.watch_live(|s| {
                let conn = if s.online { "online" } else { "offline" };
                let exit = if s.exit_node_active { " exit-node" } else { "" };
                println!("[{conn}{exit}] {} {}", s.machine, s.address);
            });
        }
    }

    Ok(())
}
