//! alavai — a lightweight Tailscale client for Linux.
//!
//! Phase 0 ships a small CLI over the LocalAPI client. The tray daemon
//! (`ksni`) and GUI (`iced`) land in later phases — see docs/PLAN.md.

mod autostart;
mod gui;
mod icon;
mod localapi;
mod theme;
mod tray;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};

use localapi::Client;

/// A simple on/off argument for the preference toggles.
#[derive(Clone, Copy, ValueEnum)]
enum Toggle {
    On,
    Off,
}

impl Toggle {
    fn enabled(self) -> bool {
        matches!(self, Toggle::On)
    }
}

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
    /// Show current preferences (exit node, routes, toggles).
    Prefs,
    /// List peers with details.
    Peers,
    /// Run a connectivity check (netcheck).
    Netcheck,
    /// Use a peer as this machine's exit node ("none" clears it).
    ExitNode {
        /// Peer name, ID, IP, or "none".
        peer: String,
    },
    /// Accept subnet routes advertised by other nodes.
    AcceptRoutes { state: Toggle },
    /// Keep LAN access while using an exit node.
    LanAccess { state: Toggle },
    /// Advertise this machine as an exit node.
    AdvertiseExitNode { state: Toggle },
    /// Advertise subnet routes (CIDRs); pass none to clear them.
    AdvertiseRoutes {
        /// CIDR prefixes, e.g. 192.168.1.0/24 (omit to clear).
        routes: Vec<String>,
    },
    /// Launch the tray on login (no state shows the current setting).
    Autostart {
        /// Turn launch-on-login on or off (omit to query).
        state: Option<Toggle>,
    },
}

fn main() -> Result<()> {
    restore_default_sigpipe();
    let cli = Cli::parse();
    let client = Client::default();

    match cli.command {
        Command::Status => {
            let s = client.status()?;
            println!("Backend:  {}", s.backend_state);
            if let Some(node) = &s.self_node {
                println!(
                    "Machine:  {} ({})",
                    node.hostname,
                    node.dns_name.trim_end_matches('.')
                );
            }
            if !s.tailscale_ips.is_empty() {
                println!("Address:  {}", s.tailscale_ips.join(", "));
            }
            if let Ok(p) = client.current_profile()
                && !p.is_empty()
            {
                println!("Tailnet:  {}", p.label());
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
                let conn = if s.online {
                    "online"
                } else if s.needs_login {
                    "needs-login"
                } else {
                    "offline"
                };
                let exit = if s.exit_node_active { " exit-node" } else { "" };
                let nm = if s.netmap_changed { " [netmap]" } else { "" };
                println!(
                    "[{conn}{exit}]{nm} {} ({}, {}) {} | accept-routes={} advertise-exit={} allow-lan={} routes={:?}",
                    s.machine,
                    s.fqdn,
                    s.os,
                    s.address,
                    s.accept_routes,
                    s.advertise_exit_node,
                    s.allow_lan,
                    s.advertised_routes,
                );
            });
        }

        Command::Prefs => {
            let p = client.prefs()?;
            let yn = |b: bool| if b { "on" } else { "off" };
            println!("Tailnet:             {}", p.profile_name);
            println!("Control server:      {}", p.control_url);
            println!("Operator:            {}", p.operator_user);
            println!("Connected (want):    {}", yn(p.want_running));
            println!("Accept routes:       {}", yn(p.route_all));
            println!("Allow LAN access:    {}", yn(p.exit_node_allow_lan));
            println!("Advertise exit node: {}", yn(p.advertises_exit_node()));
            let exit = if p.exit_node_active() {
                if !p.exit_node_id.is_empty() {
                    p.exit_node_id.clone()
                } else {
                    p.exit_node_ip.clone()
                }
            } else {
                "none".into()
            };
            println!("Using exit node:     {exit}");
            let subnets = p.subnet_routes();
            println!(
                "Advertised routes:   {}",
                if subnets.is_empty() {
                    "none".into()
                } else {
                    subnets.join(", ")
                }
            );
        }

        Command::Peers => {
            let status = client.status()?;
            let mut peers: Vec<_> = status.peers.values().collect();
            peers.sort_by_key(|p| p.hostname.to_lowercase());
            for p in peers {
                let dot = if p.online { "●" } else { "○" };
                let ip = p.tailscale_ips.first().map(String::as_str).unwrap_or("-");
                let mut tags = Vec::new();
                if p.exit_node {
                    tags.push("active-exit".to_string());
                } else if p.exit_node_option {
                    tags.push("exit-option".to_string());
                }
                if !p.primary_routes.is_empty() {
                    tags.push(format!("routes:{}", p.primary_routes.join(",")));
                }
                if p.online {
                    tags.push(if p.relay.is_empty() {
                        "direct".to_string()
                    } else {
                        format!("relay:{}", p.relay)
                    });
                    if !is_zero_time(&p.last_handshake) {
                        tags.push(format!("hs:{}", short_time(&p.last_handshake)));
                    }
                } else if !is_zero_time(&p.last_seen) {
                    tags.push(format!("seen:{}", short_time(&p.last_seen)));
                }
                if p.active {
                    tags.push(format!(
                        "↓{} ↑{}",
                        human_bytes(p.rx_bytes),
                        human_bytes(p.tx_bytes)
                    ));
                }
                println!(
                    "{dot} {:<28} {:<16} {:<8} {}",
                    p.hostname,
                    ip,
                    p.os,
                    tags.join("  ")
                );
            }
        }

        Command::Netcheck => {
            let r = localapi::netcheck()?;
            let yn = |b: bool| if b { "yes" } else { "no" };
            let opt = |b: Option<bool>| match b {
                Some(true) => "yes",
                Some(false) => "no",
                None => "?",
            };
            println!("UDP:            {}", yn(r.udp));
            println!("IPv4:           {}{}", yn(r.ipv4), fmt_global(&r.global_v4));
            println!("IPv6:           {}{}", yn(r.ipv6), fmt_global(&r.global_v6));
            println!("UPnP:           {}", opt(r.upnp));
            println!("NAT-PMP:        {}", opt(r.pmp));
            println!("PCP:            {}", opt(r.pcp));
            println!("NAT mapping varies: {}", opt(r.mapping_varies));
            println!("Captive portal: {}", opt(r.captive_portal));
            println!("Preferred DERP: region {}", r.preferred_derp);
            let mut lats: Vec<(&String, &i64)> = r.region_latency.iter().collect();
            lats.sort_by_key(|(_, ns)| **ns);
            for (region, ns) in lats {
                println!("  region {region:<3} {:.1} ms", *ns as f64 / 1_000_000.0);
            }
        }

        Command::ExitNode { peer } => {
            if matches!(peer.as_str(), "none" | "off" | "") {
                client.set_exit_node("")?;
                println!("Exit node cleared.");
            } else {
                let status = client.status()?;
                let matched = status.peers.values().find(|p| {
                    p.id == peer
                        || p.hostname == peer
                        || p.dns_name.trim_end_matches('.') == peer
                        || p.tailscale_ips.iter().any(|ip| ip == &peer)
                });
                let Some(p) = matched else {
                    bail!("no peer matches {peer:?}; run `alavai peers` to list them");
                };
                if !p.exit_node_option {
                    bail!("{} is not available as an exit node", p.hostname);
                }
                client.set_exit_node(&p.id)?;
                println!("Using {} as exit node.", p.hostname);
            }
        }

        Command::AcceptRoutes { state } => {
            client.set_accept_routes(state.enabled())?;
            println!(
                "Accept routes: {}",
                if state.enabled() { "on" } else { "off" }
            );
        }

        Command::LanAccess { state } => {
            client.set_exit_node_allow_lan(state.enabled())?;
            println!(
                "Allow LAN access: {}",
                if state.enabled() { "on" } else { "off" }
            );
        }

        Command::AdvertiseExitNode { state } => {
            client.set_advertise_exit_node(state.enabled())?;
            println!(
                "Advertise exit node: {}",
                if state.enabled() { "on" } else { "off" }
            );
        }

        Command::AdvertiseRoutes { routes } => {
            client.set_advertise_routes(&routes)?;
            if routes.is_empty() {
                println!("Cleared advertised routes.");
            } else {
                println!("Advertising routes: {}", routes.join(", "));
            }
        }

        Command::Autostart { state } => match state {
            Some(Toggle::On) => {
                autostart::enable()?;
                println!("Launch on login: enabled.");
            }
            Some(Toggle::Off) => {
                autostart::disable()?;
                println!("Launch on login: disabled.");
            }
            None => {
                let on = autostart::is_enabled()?;
                println!(
                    "Launch on login: {}",
                    if on { "enabled" } else { "disabled" }
                );
            }
        },
    }

    Ok(())
}

/// Rust ignores SIGPIPE by default, which makes piping CLI output to `head`/
/// `less` panic with a broken-pipe error. Restore the default disposition so we
/// exit quietly like a normal Unix tool.
#[cfg(unix)]
fn restore_default_sigpipe() {
    unsafe extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    // SIGPIPE = 13, SIG_DFL = 0.
    unsafe {
        signal(13, 0);
    }
}

#[cfg(not(unix))]
fn restore_default_sigpipe() {}

/// Formats a byte count compactly (e.g. "1.2M").
fn human_bytes(n: i64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n}{}", UNITS[0])
    } else {
        format!("{v:.1}{}", UNITS[u])
    }
}

fn fmt_global(addr: &str) -> String {
    if addr.is_empty() {
        String::new()
    } else {
        format!(" ({addr})")
    }
}

/// Tailscale serializes "never" timestamps as a year-1 zero time.
fn is_zero_time(t: &str) -> bool {
    t.is_empty() || t.starts_with("0001-01-01")
}

/// Trims an RFC3339 timestamp to `YYYY-MM-DD HH:MM` for compact display.
fn short_time(t: &str) -> String {
    let t = t.replace('T', " ");
    match t.char_indices().nth(16) {
        Some((i, _)) => t[..i].to_string(),
        None => t,
    }
}
