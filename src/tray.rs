//! System-tray daemon (StatusNotifierItem via `ksni`).
//!
//! This is alavai's headline surface: right-click the tray icon, pick a
//! configured tailnet, and switch to it instantly. The menu also exposes
//! connect/disconnect and quit.
//!
//! Design: menu callbacks must not block (or the menu freezes — see ksni's
//! docs), so they only *send* a [`Cmd`] down a channel. A single worker thread
//! owns the blocking [`Client`] and the tray [`Handle`], applies each command,
//! then refreshes the rendered snapshot. A ticker thread polls periodically so
//! external changes (e.g. `tailscale up` elsewhere) show up. The event-driven
//! `watch-ipn-bus` stream replaces polling in Phase 2.

use std::process::Command as ProcCommand;
use std::sync::mpsc::{Sender, channel};
use std::thread;
use std::time::Duration;

use anyhow::{Result, anyhow};
use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{RadioGroup, RadioItem, StandardItem};
use ksni::{Category, MenuItem, Status as IconStatus, ToolTip, Tray};

use crate::localapi::{Client, Profile};

const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// The slice of daemon state the tray renders.
struct Snapshot {
    online: bool,
    exit_node_active: bool,
    machine: String,
    address: String,
    tailnets: Vec<Profile>,
    current_id: String,
}

impl Snapshot {
    /// Fetches a fresh snapshot from the local daemon. Best-effort: any failed
    /// call degrades to an empty/offline field rather than erroring out, so the
    /// tray keeps running even while tailscaled is down.
    fn fetch(client: &Client) -> Snapshot {
        let status = client.status().ok();
        let online = status.as_ref().is_some_and(|s| s.online());
        let machine = status
            .as_ref()
            .and_then(|s| s.self_node.as_ref())
            .map(|n| n.hostname.clone())
            .unwrap_or_default();
        let address = status
            .as_ref()
            .and_then(|s| s.tailscale_ips.first().cloned())
            .unwrap_or_default();
        let tailnets = client
            .profiles()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect();
        let current_id = client
            .current_profile()
            .ok()
            .map(|p| p.id)
            .unwrap_or_default();
        // exit_node_active is wired up in Phase 2 (reads prefs from the bus).
        Snapshot {
            online,
            exit_node_active: false,
            machine,
            address,
            tailnets,
            current_id,
        }
    }
}

/// A request from the UI (menu) to the worker thread.
enum Cmd {
    /// Switch to the tailnet/profile with this LocalAPI id.
    Switch(String),
    /// Connect if offline, disconnect if online.
    ToggleConn,
    /// Re-poll the daemon and re-render.
    Refresh,
    /// Tear down the tray and exit the process.
    Quit,
}

struct AppTray {
    snap: Snapshot,
    tx: Sender<Cmd>,
}

impl Tray for AppTray {
    fn id(&self) -> String {
        "alavai".into()
    }

    fn title(&self) -> String {
        "alavai".into()
    }

    fn category(&self) -> Category {
        Category::Communications
    }

    fn status(&self) -> IconStatus {
        IconStatus::Active
    }

    fn icon_name(&self) -> String {
        // Themed names; Phase 5 embeds PNG fallbacks via `icon_data` for
        // minimal WMs that ship no icon theme.
        if !self.snap.online {
            "network-offline-symbolic".into()
        } else if self.snap.exit_node_active {
            "network-vpn-acquiring-symbolic".into()
        } else {
            "network-vpn-symbolic".into()
        }
    }

    fn tool_tip(&self) -> ToolTip {
        let description = if self.snap.online {
            let mut d = self.machine_line();
            if !self.snap.address.is_empty() {
                d.push('\n');
                d.push_str(&self.snap.address);
            }
            d
        } else {
            "Disconnected".into()
        };
        ToolTip {
            icon_name: self.icon_name(),
            icon_pixmap: Vec::new(),
            title: "alavai".into(),
            description,
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = Vec::new();

        // Header: current machine / tailnet (non-interactive).
        items.push(
            StandardItem {
                label: self.header_line(),
                enabled: false,
                ..Default::default()
            }
            .into(),
        );
        items.push(MenuItem::Separator);

        // Headline: one-click tailnet switcher.
        if !self.snap.tailnets.is_empty() {
            let selected = self
                .snap
                .tailnets
                .iter()
                .position(|p| p.id == self.snap.current_id)
                .unwrap_or(0);
            let tx = self.tx.clone();
            items.push(
                RadioGroup {
                    selected,
                    select: Box::new(move |this: &mut Self, idx| {
                        if let Some(p) = this.snap.tailnets.get(idx) {
                            let id = p.id.clone();
                            // Optimistic: reflect the choice immediately, then
                            // let the worker confirm via a refresh.
                            this.snap.current_id = id.clone();
                            let _ = tx.send(Cmd::Switch(id));
                        }
                    }),
                    options: self
                        .snap
                        .tailnets
                        .iter()
                        .map(|p| RadioItem {
                            label: p.label(),
                            ..Default::default()
                        })
                        .collect(),
                }
                .into(),
            );
            items.push(MenuItem::Separator);
        }

        // Connect / disconnect.
        let tx = self.tx.clone();
        items.push(
            StandardItem {
                label: if self.snap.online {
                    "Disconnect".into()
                } else {
                    "Connect".into()
                },
                activate: Box::new(move |_| {
                    let _ = tx.send(Cmd::ToggleConn);
                }),
                ..Default::default()
            }
            .into(),
        );

        // Manual refresh.
        let tx = self.tx.clone();
        items.push(
            StandardItem {
                label: "Refresh".into(),
                activate: Box::new(move |_| {
                    let _ = tx.send(Cmd::Refresh);
                }),
                ..Default::default()
            }
            .into(),
        );

        items.push(MenuItem::Separator);

        // Quit.
        let tx = self.tx.clone();
        items.push(
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(move |_| {
                    let _ = tx.send(Cmd::Quit);
                }),
                ..Default::default()
            }
            .into(),
        );

        items
    }
}

impl AppTray {
    fn current_label(&self) -> Option<String> {
        self.snap
            .tailnets
            .iter()
            .find(|p| p.id == self.snap.current_id)
            .map(Profile::label)
    }

    fn machine_line(&self) -> String {
        match self.current_label() {
            Some(t) if !self.snap.machine.is_empty() => format!("{} — {}", self.snap.machine, t),
            Some(t) => t,
            None => self.snap.machine.clone(),
        }
    }

    fn header_line(&self) -> String {
        if self.snap.online {
            let l = self.machine_line();
            if l.is_empty() { "Connected".into() } else { l }
        } else {
            "Tailscale: disconnected".into()
        }
    }
}

/// Runs the tray daemon. Blocks until the user quits.
pub fn run() -> Result<()> {
    let client = Client::default();
    let (tx, rx) = channel::<Cmd>();

    let tray = AppTray {
        snap: Snapshot::fetch(&client),
        tx: tx.clone(),
    };
    let handle = tray.spawn().map_err(|e| {
        anyhow!("could not start the tray ({e}); is a StatusNotifierItem host running in your desktop?")
    })?;

    // Periodic refresh so external state changes appear.
    {
        let tx = tx.clone();
        thread::spawn(move || {
            loop {
                thread::sleep(POLL_INTERVAL);
                if tx.send(Cmd::Refresh).is_err() {
                    break;
                }
            }
        });
    }

    // Worker owns the blocking client and the tray handle.
    worker(client, rx, handle);
    Ok(())
}

fn worker(client: Client, rx: std::sync::mpsc::Receiver<Cmd>, handle: Handle<AppTray>) {
    for cmd in rx {
        match cmd {
            Cmd::Switch(id) => {
                if let Err(e) = client.switch_profile(&id) {
                    eprintln!("alavai: switch tailnet failed: {e}");
                }
                refresh(&client, &handle);
            }
            Cmd::ToggleConn => {
                let online = handle.update(|t| t.snap.online).unwrap_or(false);
                let action = if online { "down" } else { "up" };
                if let Err(e) = ProcCommand::new("tailscale").arg(action).status() {
                    eprintln!("alavai: `tailscale {action}` failed: {e}");
                }
                refresh(&client, &handle);
            }
            Cmd::Refresh => {
                if refresh(&client, &handle).is_none() {
                    break; // tray service gone
                }
            }
            Cmd::Quit => {
                handle.shutdown().wait();
                std::process::exit(0);
            }
        }
    }
}

/// Re-polls and pushes a fresh snapshot to the tray. Returns `None` if the tray
/// service has shut down.
fn refresh(client: &Client, handle: &Handle<AppTray>) -> Option<()> {
    let snap = Snapshot::fetch(client);
    handle.update(move |t| t.snap = snap)
}
