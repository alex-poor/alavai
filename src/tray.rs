//! System-tray daemon (StatusNotifierItem via `ksni`).
//!
//! This is alavai's headline surface: right-click the tray icon, pick a
//! configured tailnet, and switch to it instantly. The menu also exposes
//! connect/disconnect and quit.
//!
//! Design: menu callbacks must not block (or the menu freezes — see ksni's
//! docs), so they only *send* a [`Cmd`] down a channel. A single worker thread
//! owns the blocking [`Client`] and the tray [`Handle`], applies each command,
//! then refreshes the rendered snapshot. Updates are event-driven off the
//! `watch-ipn-bus` stream; a profile switch closes that stream, which we use to
//! refresh the (off-bus) profile list. A slow backstop poll covers the rest.

use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;
use std::sync::mpsc::{Sender, channel};
use std::thread;
use std::time::Duration;

use anyhow::{Result, anyhow};
use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{StandardItem, SubMenu};
use ksni::{Category, Icon, MenuItem, OfflineReason, Status as IconStatus, ToolTip, Tray};

use crate::icon;
use crate::instance::{self, Instance};
use crate::localapi::{self, Client, LiveState, Profile};
use crate::notify::Notifier;

/// Backstop interval for re-polling the profile list. Profiles aren't on the IPN
/// bus, but a switch closes the bus stream and we refresh on that event (see
/// [`Client::watch_live_with`]), so this timer is only a slow safety net for the
/// rare profile-list change that doesn't drop the bus. Everything else is
/// event-driven.
const PROFILE_POLL_INTERVAL: Duration = Duration::from_secs(60);

/// How often to check whether our own on-disk binary changed (an upgrade). A
/// resident daemon keeps running the old code after a package upgrade replaces
/// `/usr/bin/alavai`; we detect that and offer a one-click restart.
const UPGRADE_POLL_INTERVAL: Duration = Duration::from_secs(30);

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
        Snapshot {
            online,
            // Exit-node state is corrected within milliseconds by the first
            // live delta from the bus.
            exit_node_active: false,
            machine,
            address,
            tailnets,
            current_id,
        }
    }

    /// Applies a live delta from the IPN bus.
    fn apply_live(&mut self, live: LiveState) {
        self.online = live.online;
        self.exit_node_active = live.exit_node_active;
        if !live.machine.is_empty() {
            self.machine = live.machine;
        }
        if !live.address.is_empty() {
            self.address = live.address;
        }
    }
}

/// A request to the worker thread, from either the menu or the watch stream.
enum Cmd {
    /// Switch to the tailnet/profile with this LocalAPI id.
    Switch(String),
    /// Connect if offline, disconnect if online.
    ToggleConn,
    /// Live state delta from the IPN bus (online / exit-node / machine / addr).
    Live(LiveState),
    /// Re-poll the profile list (the one thing not on the bus).
    RefreshProfiles,
    /// Open the main window.
    OpenWindow,
    /// Our on-disk binary changed (a package upgrade); offer to restart.
    UpgradeAvailable,
    /// Re-exec into the upgraded binary.
    Restart,
    /// Tear down the tray and exit the process.
    Quit,
}

struct AppTray {
    snap: Snapshot,
    tx: Sender<Cmd>,
    /// True once an upgrade was detected on disk; surfaces a "Restart to update"
    /// menu item.
    upgrade_available: bool,
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

    /// Left-click opens the main window.
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.send(Cmd::OpenWindow);
    }

    /// No StatusNotifierWatcher on the bus (yet). Returning `true` keeps the
    /// service running so ksni registers automatically once a watcher appears
    /// — covers the login race (we start before the panel) and shell restarts.
    /// If SNI is genuinely absent (e.g. GNOME without the AppIndicator
    /// extension) the icon just never shows, so leave a breadcrumb.
    fn watcher_offline(&self, reason: OfflineReason) -> bool {
        eprintln!(
            "alavai: no system-tray host yet ({reason:?}); waiting — \
             on GNOME, enable the AppIndicator/KStatusNotifierItem extension"
        );
        true
    }

    fn icon_name(&self) -> String {
        // Empty so SNI hosts use our rendered brand mesh in `icon_pixmap` — a
        // non-empty IconName takes precedence over IconPixmap per the spec.
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        let svg = if !self.snap.online {
            icon::TRAY_DISCONNECTED
        } else if self.snap.exit_node_active {
            icon::TRAY_EXIT
        } else {
            icon::TRAY_CONNECTED
        };
        // Provide a couple of sizes so the panel can pick the crispest.
        [22u32, 44]
            .iter()
            .filter_map(|&size| {
                icon::render_argb(svg, size).map(|(width, height, data)| Icon {
                    width,
                    height,
                    data,
                })
            })
            .collect()
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

        // ── Status header (non-interactive): machine + connection state. ──
        items.push(disabled(self.header_line()));
        items.push(MenuItem::Separator);

        // ── Update alert: shown only after an on-disk upgrade is detected, at
        //    the top so it's the first thing seen. Restarts into the new binary. ──
        if self.upgrade_available {
            let tx = self.tx.clone();
            items.push(
                StandardItem {
                    label: "Restart to update".into(),
                    icon_name: "system-software-update".into(),
                    activate: Box::new(move |_| {
                        let _ = tx.send(Cmd::Restart);
                    }),
                    ..Default::default()
                }
                .into(),
            );
            items.push(MenuItem::Separator);
        }

        // ── Headline: one-click tailnet switcher. A labelled section plus an
        //    explicit ✓ on the active tailnet — the bare radio dot read
        //    ambiguously on some panels, and which one is current is the whole
        //    point of this menu. ──
        if !self.snap.tailnets.is_empty() {
            items.push(disabled("Switch tailnet".into()));
            for p in &self.snap.tailnets {
                let active = p.id == self.snap.current_id;
                // Marker column keeps every row aligned whether ticked or not.
                let label = format!("{}  {}", if active { "✓" } else { " " }, p.label());
                if active {
                    // Already current: show it, but don't re-switch on click.
                    items.push(
                        StandardItem {
                            label,
                            ..Default::default()
                        }
                        .into(),
                    );
                } else {
                    let id = p.id.clone();
                    let tx = self.tx.clone();
                    items.push(
                        StandardItem {
                            label,
                            activate: Box::new(move |this: &mut Self| {
                                // Optimistic: reflect the choice immediately, then
                                // let the worker confirm via a refresh.
                                this.snap.current_id = id.clone();
                                let _ = tx.send(Cmd::Switch(id.clone()));
                            }),
                            ..Default::default()
                        }
                        .into(),
                    );
                }
            }
            items.push(MenuItem::Separator);
        }

        // ── Connection toggle. ──
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

        // ── Open the main window. ──
        let tx = self.tx.clone();
        items.push(
            StandardItem {
                label: "Open window…".into(),
                activate: Box::new(move |_| {
                    let _ = tx.send(Cmd::OpenWindow);
                }),
                ..Default::default()
            }
            .into(),
        );

        items.push(MenuItem::Separator);

        // ── About. ──
        items.push(about_submenu());

        // ── Quit. ──
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

/// A non-interactive, greyed label used for section headers in the menu.
fn disabled(label: String) -> MenuItem<AppTray> {
    StandardItem {
        label,
        enabled: false,
        ..Default::default()
    }
    .into()
}

/// The "About" submenu: identity (from Cargo metadata) plus links that open in
/// the browser. Plain disabled lines + link items — all a tray menu supports.
fn about_submenu() -> MenuItem<AppTray> {
    const REPO: &str = env!("CARGO_PKG_REPOSITORY");
    let submenu = vec![
        disabled(concat!("alavai ", env!("CARGO_PKG_VERSION")).into()),
        disabled("Lightweight Tailscale client for Linux".into()),
        disabled(concat!("License: ", env!("CARGO_PKG_LICENSE")).into()),
        MenuItem::Separator,
        StandardItem {
            label: "View source…".into(),
            activate: Box::new(|_| open_url(REPO)),
            ..Default::default()
        }
        .into(),
        StandardItem {
            label: "Report an issue…".into(),
            activate: Box::new(|_| open_url(concat!(env!("CARGO_PKG_REPOSITORY"), "/issues"))),
            ..Default::default()
        }
        .into(),
    ];
    SubMenu {
        label: "About".into(),
        icon_name: "help-about".into(),
        submenu,
        ..Default::default()
    }
    .into()
}

/// Spawns a fire-and-forget child and reaps it on a short-lived thread. The tray
/// is long-lived and never `wait()`s on the windows / `xdg-open` helpers it
/// launches, so without this they linger as `<defunct>` zombies until the tray
/// exits. The reaper thread blocks on `wait()` and ends when the child does.
fn spawn_reaped(mut cmd: ProcCommand) -> std::io::Result<()> {
    let mut child = cmd.spawn()?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Opens a URL in the user's browser via `xdg-open` (a packaged dependency).
fn open_url(url: &str) {
    let mut cmd = ProcCommand::new("xdg-open");
    cmd.arg(url);
    if let Err(e) = spawn_reaped(cmd) {
        eprintln!("alavai: open url failed: {e}");
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

    /// Top line of the menu: machine + connection state. The tailnet is shown
    /// (and marked active) in the switcher section below, so it isn't repeated
    /// here. Hover the icon for the full machine — tailnet — IP tooltip.
    fn header_line(&self) -> String {
        if self.snap.online {
            if self.snap.machine.is_empty() {
                "Connected".into()
            } else {
                format!("{} — Connected", self.snap.machine)
            }
        } else {
            "Disconnected".into()
        }
    }
}

/// Spawns the main window as a separate process (`alavai gui`). Used both by the
/// menu's "Open window" item and at launch, so a click on the app gives visible
/// feedback even when a panel makes the new tray icon easy to miss.
fn open_main_window() {
    match std::env::current_exe() {
        Ok(exe) => {
            let mut cmd = ProcCommand::new(exe);
            cmd.arg("gui");
            if let Err(e) = spawn_reaped(cmd) {
                eprintln!("alavai: open window failed: {e}");
            }
        }
        Err(e) => eprintln!("alavai: locate executable: {e}"),
    }
}

/// Runs the tray daemon. Blocks until the user quits.
///
/// `open_window` shows the main window once at startup — true for an explicit
/// launch (the app launcher), false for a silent autostart-on-login. If a tray
/// is already running, this instead just opens a window (when `open_window`) and
/// returns, so re-launching the app never plants a second icon.
pub fn run(open_window: bool) -> Result<()> {
    if let Instance::AlreadyRunning = instance::acquire() {
        if open_window {
            open_main_window();
        }
        return Ok(());
    }

    let client = Client::default();
    localapi::warn_if_untested_daemon(&client);
    let (tx, rx) = channel::<Cmd>();

    let tray = AppTray {
        snap: Snapshot::fetch(&client),
        tx: tx.clone(),
        upgrade_available: false,
    };
    // `assume_sni_available(true)`: treat "no watcher on the bus" as a soft
    // error routed to `watcher_offline` instead of a hard spawn failure, so the
    // icon appears whenever the host shows up rather than only when one is
    // already running at launch (the login-startup race).
    let handle = tray.assume_sni_available(true).spawn().map_err(|e| {
        anyhow!(
            "could not start the tray ({e}); is a StatusNotifierItem host running in your desktop?"
        )
    })?;

    // Visible feedback for an explicit launch: open the window once the tray is
    // registered.
    if open_window {
        open_main_window();
    }

    // Event-driven updates: stream the IPN bus and forward live deltas. A clean
    // stream close means the profile was switched (locally or externally), so
    // refresh the profile list then — profiles aren't on the bus.
    {
        let tx = tx.clone();
        let reconnect_tx = tx.clone();
        thread::spawn(move || {
            Client::default().watch_live_with(
                move |live| {
                    let _ = tx.send(Cmd::Live(live));
                },
                move || {
                    let _ = reconnect_tx.send(Cmd::RefreshProfiles);
                },
            );
        });
    }

    // Backstop poll: switches are now caught event-driven via the stream close
    // above, so this only needs to catch the rare profile-list change that
    // doesn't drop the bus (e.g. another tool adding a profile). Slow on purpose.
    {
        let tx = tx.clone();
        thread::spawn(move || {
            loop {
                thread::sleep(PROFILE_POLL_INTERVAL);
                if tx.send(Cmd::RefreshProfiles).is_err() {
                    break;
                }
            }
        });
    }

    // Watch our own binary for an in-place upgrade so we can offer to restart.
    let exe = std::env::current_exe().ok();
    if let Some(exe) = exe.clone() {
        spawn_upgrade_watcher(exe, tx.clone());
    }

    // Worker owns the blocking client and the tray handle.
    worker(client, rx, handle, exe.unwrap_or_default());
    Ok(())
}

/// Identity of a binary on disk — `(inode, mtime)`. A package upgrade replaces
/// `/usr/bin/alavai` with a new file, changing both.
fn binary_fingerprint(path: &Path) -> Option<(u64, i64)> {
    std::fs::metadata(path).ok().map(|m| (m.ino(), m.mtime()))
}

/// Polls our own executable; sends [`Cmd::UpgradeAvailable`] once it changes on
/// disk (then stops — one alert is enough until the user restarts).
fn spawn_upgrade_watcher(exe: PathBuf, tx: Sender<Cmd>) {
    let Some(baseline) = binary_fingerprint(&exe) else {
        return; // can't fingerprint (e.g. odd FS) → skip; not worth failing over
    };
    thread::spawn(move || {
        loop {
            thread::sleep(UPGRADE_POLL_INTERVAL);
            match binary_fingerprint(&exe) {
                // Changed and readable → an upgrade landed.
                Some(cur) if cur != baseline => {
                    let _ = tx.send(Cmd::UpgradeAvailable);
                    break;
                }
                // Missing/unreadable (mid-upgrade rename) → ignore, keep watching.
                _ => {}
            }
        }
    });
}

/// Re-execs into the (upgraded) binary at `exe`, replacing this process. Tears
/// the tray down first and frees the single-instance lock so the new image binds
/// cleanly. Only returns (with an error logged) if the exec itself fails.
fn restart(exe: &Path, handle: &Handle<AppTray>) -> ! {
    handle.shutdown().wait();
    instance::release();
    // `exec` replaces the process image; restart the tray surface explicitly.
    let err = ProcCommand::new(exe).arg("tray").exec();
    eprintln!("alavai: restart failed: {err}; exiting so a relaunch picks up the update");
    std::process::exit(1);
}

fn worker(
    client: Client,
    rx: std::sync::mpsc::Receiver<Cmd>,
    handle: Handle<AppTray>,
    exe: PathBuf,
) {
    let mut notifier = Notifier::new();
    // Track connection state so we only notify on real transitions (including
    // ones driven from elsewhere), not on every bus delta.
    let mut last_online = handle.update(|t| t.snap.online).unwrap_or(false);

    for cmd in rx {
        let alive = match cmd {
            Cmd::Switch(id) => {
                match client.switch_profile(&id) {
                    Ok(()) => {
                        let label = client
                            .current_profile()
                            .ok()
                            .filter(|p| !p.is_empty())
                            .map(|p| p.label());
                        match label {
                            Some(l) => notifier.show("alavai", &format!("Switched to {l}")),
                            None => notifier.show("alavai", "Switched tailnet"),
                        }
                    }
                    Err(e) => {
                        eprintln!("alavai: switch tailnet failed: {e}");
                        notifier.show("alavai", "Couldn’t switch tailnet");
                    }
                }
                // Live bits update via the bus; confirm the active profile here.
                refresh_profiles(&client, &handle)
            }
            Cmd::ToggleConn => {
                let online = handle.update(|t| t.snap.online).unwrap_or(false);
                if let Err(e) = client.set_want_running(!online) {
                    eprintln!("alavai: toggle connection failed: {e}");
                }
                Some(()) // connection state (and its notification) arrive via the bus
            }
            Cmd::Live(live) => {
                if live.online != last_online {
                    last_online = live.online;
                    notifier.show(
                        "alavai",
                        if live.online {
                            "Connected"
                        } else {
                            "Disconnected"
                        },
                    );
                }
                handle.update(move |t| t.snap.apply_live(live))
            }
            Cmd::RefreshProfiles => refresh_profiles(&client, &handle),
            Cmd::OpenWindow => {
                open_main_window();
                Some(())
            }
            Cmd::UpgradeAvailable => {
                notifier.show(
                    "alavai",
                    "A new version is installed — choose “Restart to update”.",
                );
                handle.update(|t| t.upgrade_available = true)
            }
            Cmd::Restart => restart(&exe, &handle),
            Cmd::Quit => {
                handle.shutdown().wait();
                std::process::exit(0);
            }
        };
        if alive.is_none() {
            break; // tray service shut down
        }
    }
}

/// Re-polls the profile list and pushes it to the tray. Returns `None` if the
/// tray service has shut down.
fn refresh_profiles(client: &Client, handle: &Handle<AppTray>) -> Option<()> {
    let tailnets = client
        .profiles()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>();
    let current_id = client.current_profile().ok().map(|p| p.id);
    handle.update(move |t| {
        t.snap.tailnets = tailnets;
        if let Some(id) = current_id {
            t.snap.current_id = id;
        }
    })
}
