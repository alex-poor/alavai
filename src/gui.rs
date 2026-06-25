//! The main window (iced, tiny-skia software renderer).
//!
//! Implements the design in docs/design/DESIGN.md: a persistent header (tailnet
//! switcher + connection status/toggle) over a sidebar (filterable peer list)
//! and a detail pane (this-machine settings or a selected peer). The view is a
//! pure function of one `GuiSnapshot`, rebuilt from the LocalAPI `status` +
//! `prefs` + profiles whenever the IPN bus signals a change.

use std::process::Command as ProcCommand;

use anyhow::Result;
use iced::futures::Stream;
use iced::widget::{
    Space, button, column, container, mouse_area, row, scrollable, stack, text, text_input, toggler,
};
use iced::{Center, Color, Element, Fill, Font, Length, Padding, Size, Subscription, Task};

use iced::alignment::{Horizontal, Vertical};

use crate::icon::{self, icon};
use crate::localapi::{self, Client, LiveState, NetcheckReport, Profile};
use crate::theme::{self, Palette};

const ADMIN_URL: &str = "https://login.tailscale.com/admin";
const MONO: Font = Font::MONOSPACE;
/// System sans at semibold weight (for titles / names) — no font bundled.
const SEMI: Font = Font {
    family: iced::font::Family::SansSerif,
    weight: iced::font::Weight::Semibold,
    stretch: iced::font::Stretch::Normal,
    style: iced::font::Style::Normal,
};
const SIDEBAR_W: f32 = 248.0;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct GuiSnapshot {
    online: bool,
    machine: String,
    fqdn: String,
    os: String,
    /// This machine's preferred DERP relay region (from status, no CLI needed).
    self_relay: String,
    addrs: Vec<String>,
    tailnets: Vec<Profile>,
    current_id: String,
    // prefs
    accept_routes: bool,
    advertise_exit_node: bool,
    allow_lan: bool,
    advertised_routes: Vec<String>,
    peers: Vec<PeerView>,
    /// Pending interactive-login URL from the bus (set while adding/logging in).
    login_url: Option<String>,
    /// False when the local tailscaled daemon couldn't be reached.
    reachable: bool,
    /// False when this Linux user isn't the Tailscale operator.
    operator_ok: bool,
    /// True when the daemon is reachable but no tailnet is logged in.
    needs_login: bool,
}

#[derive(Debug, Clone)]
struct PeerView {
    id: String,
    name: String,
    fqdn: String,
    os: String,
    online: bool,
    addrs: Vec<String>,
    exit_node: bool,
    exit_node_option: bool,
    relay: String,
    last_seen: String,
    routes: Vec<String>,
}

impl PeerView {
    fn primary_addr(&self) -> &str {
        self.addrs
            .iter()
            .find(|a| !a.contains(':'))
            .or_else(|| self.addrs.first())
            .map(String::as_str)
            .unwrap_or("")
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Selection {
    ThisMachine,
    Peer(String),
    ExitNode,
}

struct State {
    dark: bool,
    snap: GuiSnapshot,
    selection: Selection,
    filter: String,
    busy: bool,
    switching: bool,
    /// Label of the tailnet being switched to (shown during the transition).
    switching_to: Option<String>,
    /// Profile id being switched to; the transition ends when the daemon's
    /// current profile actually becomes this (or login is needed / it times out).
    switching_target: Option<String>,
    toast: Option<String>,
    /// Whether the tailnet-switcher popover is open.
    switcher_open: bool,
    /// Whether the switcher is in "manage" (remove) mode.
    manage: bool,
    /// Profile id pending a remove confirmation.
    confirm_remove: Option<String>,
    /// Last login URL we already opened, to avoid reopening on every refresh.
    last_login_url: Option<String>,
    /// Latest netcheck result, and whether one is running.
    netcheck: Option<NetcheckReport>,
    netcheck_running: bool,
    /// Message from the last failed netcheck (e.g. CLI not installed).
    netcheck_error: Option<String>,
    /// Whether the operator-permission banner has been dismissed this session.
    operator_dismissed: bool,
    /// Draft CIDR in the advertised-routes "add" field.
    route_input: String,
    /// In narrow layout, whether a detail view is pushed over the list.
    drilled: bool,
}

#[derive(Debug, Clone)]
enum Message {
    /// Full refresh (boot, and on netmap changes where peers may have changed).
    Snapshot(GuiSnapshot),
    /// Lightweight delta from the IPN bus — applied without any refetch.
    Live(LiveState),
    Select(Selection),
    Filter(String),
    SwitchTailnet(String),
    ToggleConnection,
    SetAcceptRoutes(bool),
    SetAdvertiseExit(bool),
    SetAllowLan(bool),
    UseExitNode(String),
    UseExitNodeAuto,
    ClearExitNode,
    Copy(String),
    ToggleTheme,
    OpenAdmin,
    Toast(String),
    ClearToast,
    // Tailnet switcher popover
    ToggleSwitcher,
    CloseSwitcher,
    ToggleManage,
    RequestRemove(String),
    ConfirmRemove(String),
    CancelRemove,
    AddTailnet,
    StartLogin,
    // Diagnostics & robustness
    RunNetcheck,
    NetcheckDone(Result<NetcheckReport, String>),
    DismissOperator,
    Retry,
    // Advertised routes editor
    RouteInput(String),
    AddRoute,
    RemoveRoute(String),
    // Narrow-layout navigation
    Back,
    /// Safety net to end a stuck switching transition.
    ClearSwitching,
    /// A fire-and-forget action completed; nothing to do.
    Noop,
}

// ---------------------------------------------------------------------------
// Data fetch
// ---------------------------------------------------------------------------

fn fetch_gui(client: &Client) -> GuiSnapshot {
    let status = client.status().ok();
    let prefs = client.prefs().ok();
    let online = status.as_ref().is_some_and(|s| s.online());
    let reachable = status.is_some();
    let operator_ok = match (std::env::var("USER").ok(), prefs.as_ref()) {
        (Some(user), Some(p)) => p.operator_user == user,
        // If we can't determine it, don't nag.
        _ => true,
    };

    let (machine, fqdn, os, self_relay) = match status.as_ref().and_then(|s| s.self_node.as_ref()) {
        Some(n) => (
            n.hostname.clone(),
            n.dns_name.trim_end_matches('.').to_string(),
            n.os.clone(),
            n.relay.clone(),
        ),
        None => (String::new(), String::new(), String::new(), String::new()),
    };
    let addrs = status
        .as_ref()
        .map(|s| s.tailscale_ips.clone())
        .unwrap_or_default();

    let mut peers: Vec<PeerView> = status
        .as_ref()
        .map(|s| {
            s.peers
                .values()
                .map(|p| PeerView {
                    id: p.id.clone(),
                    name: if !p.hostname.is_empty() {
                        p.hostname.clone()
                    } else {
                        p.dns_name.trim_end_matches('.').to_string()
                    },
                    fqdn: p.dns_name.trim_end_matches('.').to_string(),
                    os: p.os.clone(),
                    online: p.online,
                    addrs: p.tailscale_ips.clone(),
                    exit_node: p.exit_node,
                    exit_node_option: p.exit_node_option,
                    relay: p.relay.clone(),
                    last_seen: p.last_seen.clone(),
                    routes: p.primary_routes.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    peers.sort_by_key(|p| p.name.to_lowercase());

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

    GuiSnapshot {
        online,
        machine,
        fqdn,
        os,
        self_relay,
        addrs,
        tailnets,
        current_id,
        accept_routes: prefs.as_ref().is_some_and(|p| p.route_all),
        advertise_exit_node: prefs.as_ref().is_some_and(|p| p.advertises_exit_node()),
        allow_lan: prefs.as_ref().is_some_and(|p| p.exit_node_allow_lan),
        advertised_routes: prefs
            .as_ref()
            .map(|p| p.subnet_routes())
            .unwrap_or_default(),
        peers,
        login_url: None,
        reachable,
        operator_ok,
        needs_login: status
            .as_ref()
            .is_some_and(|s| s.backend_state == "NeedsLogin"),
    }
}

impl GuiSnapshot {
    /// Applies a bus delta in place — everything the IPN bus carries (self,
    /// prefs, connection state), without touching peers/profiles or refetching.
    fn apply_live(&mut self, live: &LiveState) {
        self.online = live.online;
        self.needs_login = live.needs_login;
        self.reachable = true;
        self.accept_routes = live.accept_routes;
        self.advertise_exit_node = live.advertise_exit_node;
        self.allow_lan = live.allow_lan;
        self.advertised_routes = live.advertised_routes.clone();
        if !live.machine.is_empty() {
            self.machine = live.machine.clone();
        }
        if !live.fqdn.is_empty() {
            self.fqdn = live.fqdn.clone();
        }
        if !live.os.is_empty() {
            self.os = live.os.clone();
        }
        if !live.addresses.is_empty() {
            self.addrs = live.addresses.clone();
        }
    }
}

/// Runs a blocking mutation, then refetches — as one iced task.
fn act<F>(f: F) -> Task<Message>
where
    F: FnOnce(&Client) + Send + 'static,
{
    Task::perform(
        async move {
            let client = Client::default();
            f(&client);
            fetch_gui(&client)
        },
        Message::Snapshot,
    )
}

/// Runs a blocking mutation fire-and-forget — relies on the bus to deliver the
/// resulting state (used where a full refetch would catch a transient state).
fn fire<F>(f: F) -> Task<Message>
where
    F: FnOnce(&Client) + Send + 'static,
{
    Task::perform(
        async move {
            let client = Client::default();
            f(&client);
        },
        |()| Message::Noop,
    )
}

/// Opens a URL in the user's browser. Prefers `xdg-open` (honors the desktop's
/// default browser); falls back to `$BROWSER` if `xdg-open` isn't installed
/// (common on minimal tiling-WM setups without xdg-utils). Best-effort: the
/// login URL is also kept in state so the user is never stranded if both fail.
fn open_url(url: &str) {
    if ProcCommand::new("xdg-open").arg(url).spawn().is_ok() {
        return;
    }
    // $BROWSER may be a command with its own args; the URL is appended.
    if let Ok(browser) = std::env::var("BROWSER")
        && let Some(cmd) = browser.split_whitespace().next()
    {
        let args: Vec<&str> = browser.split_whitespace().skip(1).collect();
        let _ = ProcCommand::new(cmd).args(args).arg(url).spawn();
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

fn boot() -> (State, Task<Message>) {
    let snap = fetch_gui(&Client::default());
    (
        State {
            dark: true,
            snap,
            selection: Selection::ThisMachine,
            filter: String::new(),
            busy: false,
            switching: false,
            switching_to: None,
            switching_target: None,
            toast: None,
            switcher_open: false,
            manage: false,
            confirm_remove: None,
            last_login_url: None,
            netcheck: None,
            netcheck_running: false,
            netcheck_error: None,
            operator_dismissed: false,
            route_input: String::new(),
            drilled: false,
        },
        Task::none(),
    )
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::Snapshot(snap) => {
            // Keep the selected peer valid across refreshes.
            if let Selection::Peer(id) = &state.selection
                && !snap.peers.iter().any(|p| &p.id == id)
            {
                state.selection = Selection::ThisMachine;
            }
            // A new interactive-login URL means a sign-in is in progress: open
            // the browser once and let the user know.
            let new_login = snap
                .login_url
                .as_ref()
                .filter(|u| state.last_login_url.as_deref() != Some(u.as_str()))
                .cloned();
            state.snap = snap;
            state.busy = false;
            // End the switching transition only once it's genuinely settled:
            // the daemon's current profile is now the target and connected, or
            // the new tailnet needs login. Transient offline states in between
            // keep the "Switching…" screen instead of flashing disconnected.
            let arrived = state.snap.online
                && state.switching_target.as_deref() == Some(state.snap.current_id.as_str());
            if arrived || state.snap.needs_login {
                state.switching = false;
                state.switching_to = None;
                state.switching_target = None;
            }
            if let Some(url) = new_login {
                state.last_login_url = Some(url.clone());
                open_url(&url);
                state.toast = Some("Opened your browser to finish signing in".into());
                return delayed_clear();
            }
            Task::none()
        }
        Message::Live(live) => {
            // Pure delta from the bus: no refetch. Peers/profiles are untouched
            // (they refresh via Snapshot when the netmap changes).
            let new_login = live
                .browse_to_url
                .as_ref()
                .filter(|u| state.last_login_url.as_deref() != Some(u.as_str()))
                .cloned();
            state.snap.apply_live(&live);
            state.busy = false;
            // Live deltas don't refresh the active profile, so the only switch
            // outcome they can confirm is "needs login". Arrival on the target
            // is confirmed by a Snapshot (above). Otherwise hold the transition.
            if state.snap.needs_login {
                state.switching = false;
                state.switching_to = None;
                state.switching_target = None;
            }
            if let Some(url) = new_login {
                state.last_login_url = Some(url.clone());
                open_url(&url);
                state.toast = Some("Opened your browser to finish signing in".into());
                return delayed_clear();
            }
            Task::none()
        }
        Message::Select(sel) => {
            state.selection = sel;
            state.drilled = true; // narrow layout: push the detail view
            Task::none()
        }
        Message::Back => {
            state.drilled = false;
            Task::none()
        }
        Message::ClearSwitching => {
            state.switching = false;
            state.switching_to = None;
            state.switching_target = None;
            state.busy = false;
            Task::none()
        }
        Message::Noop => Task::none(),
        Message::Filter(f) => {
            state.filter = f;
            Task::none()
        }
        Message::SwitchTailnet(id) => {
            state.switcher_open = false;
            state.manage = false;
            if id == state.snap.current_id {
                return Task::none();
            }
            state.switching_to = state
                .snap
                .tailnets
                .iter()
                .find(|t| t.id == id)
                .map(|t| t.label());
            state.switching_target = Some(id.clone());
            // Optimistically show the target in the header while we transition.
            state.snap.current_id = id.clone();
            state.busy = true;
            state.switching = true;
            // Fire-and-forget the switch and rely on the bus to deliver the
            // settled state; a timeout clears the transition if it never does.
            Task::batch([
                fire(move |c| {
                    let _ = c.switch_profile(&id);
                }),
                switching_timeout(),
            ])
        }
        Message::ToggleConnection => {
            let online = state.snap.online;
            state.busy = true;
            act(move |c| {
                let _ = c.set_want_running(!online);
            })
        }
        Message::SetAcceptRoutes(v) => {
            state.busy = true;
            act(move |c| {
                let _ = c.set_accept_routes(v);
            })
        }
        Message::SetAdvertiseExit(v) => {
            state.busy = true;
            act(move |c| {
                let _ = c.set_advertise_exit_node(v);
            })
        }
        Message::SetAllowLan(v) => {
            state.busy = true;
            act(move |c| {
                let _ = c.set_exit_node_allow_lan(v);
            })
        }
        Message::UseExitNode(id) => {
            state.busy = true;
            act(move |c| {
                let _ = c.set_exit_node(&id);
            })
        }
        Message::UseExitNodeAuto => {
            state.busy = true;
            act(|c| {
                if let Ok(id) = c.suggest_exit_node()
                    && !id.is_empty()
                {
                    let _ = c.set_exit_node(&id);
                }
            })
        }
        Message::ClearExitNode => {
            state.busy = true;
            act(|c| {
                let _ = c.set_exit_node("");
            })
        }
        Message::Copy(value) => {
            let toast = format!("Copied {value}");
            Task::batch([
                iced::clipboard::write(value),
                Task::done(Message::Toast(toast)),
            ])
        }
        Message::ToggleTheme => {
            state.dark = !state.dark;
            Task::none()
        }
        Message::OpenAdmin => {
            open_url(ADMIN_URL);
            Task::none()
        }
        Message::Toast(t) => {
            state.toast = Some(t);
            delayed_clear()
        }
        Message::ClearToast => {
            state.toast = None;
            Task::none()
        }
        Message::ToggleSwitcher => {
            state.switcher_open = !state.switcher_open;
            state.manage = false;
            state.confirm_remove = None;
            Task::none()
        }
        Message::CloseSwitcher => {
            state.switcher_open = false;
            state.manage = false;
            state.confirm_remove = None;
            Task::none()
        }
        Message::ToggleManage => {
            state.manage = !state.manage;
            state.confirm_remove = None;
            Task::none()
        }
        Message::RequestRemove(id) => {
            state.confirm_remove = Some(id);
            Task::none()
        }
        Message::CancelRemove => {
            state.confirm_remove = None;
            Task::none()
        }
        Message::ConfirmRemove(id) => {
            state.confirm_remove = None;
            state.busy = true;
            act(move |c| {
                let _ = c.delete_profile(&id);
            })
        }
        Message::StartLogin => {
            state.busy = true;
            state.toast = Some("Opening your browser to sign in…".into());
            Task::batch([
                act(|c| {
                    let _ = c.start_login();
                }),
                delayed_clear(),
            ])
        }
        Message::AddTailnet => {
            state.switcher_open = false;
            state.manage = false;
            state.busy = true;
            state.toast = Some("Creating tailnet — opening your browser to sign in…".into());
            Task::batch([
                act(|c| {
                    if c.new_profile().is_ok() {
                        let _ = c.start_login();
                    }
                }),
                delayed_clear(),
            ])
        }
        Message::RunNetcheck => {
            state.netcheck_running = true;
            state.netcheck_error = None;
            Task::perform(
                async { localapi::netcheck().map_err(|e| e.to_string()) },
                Message::NetcheckDone,
            )
        }
        Message::NetcheckDone(result) => {
            state.netcheck_running = false;
            match result {
                Ok(report) => {
                    state.netcheck = Some(report);
                    state.netcheck_error = None;
                }
                Err(msg) => state.netcheck_error = Some(msg),
            }
            Task::none()
        }
        Message::DismissOperator => {
            state.operator_dismissed = true;
            Task::none()
        }
        Message::Retry => act(|_| {}),
        Message::RouteInput(s) => {
            state.route_input = s;
            Task::none()
        }
        Message::AddRoute => {
            let cidr = state.route_input.trim().to_string();
            if !is_valid_cidr(&cidr) {
                state.toast = Some(format!("Invalid CIDR: {cidr}"));
                return delayed_clear();
            }
            let mut routes = state.snap.advertised_routes.clone();
            if !routes.contains(&cidr) {
                routes.push(cidr);
            }
            state.route_input.clear();
            state.busy = true;
            act(move |c| {
                let _ = c.set_advertise_routes(&routes);
            })
        }
        Message::RemoveRoute(cidr) => {
            let routes: Vec<String> = state
                .snap
                .advertised_routes
                .iter()
                .filter(|r| **r != cidr)
                .cloned()
                .collect();
            state.busy = true;
            act(move |c| {
                let _ = c.set_advertise_routes(&routes);
            })
        }
    }
}

/// Light validation of a CIDR prefix (e.g. `192.168.1.0/24`).
fn is_valid_cidr(s: &str) -> bool {
    let Some((addr, bits)) = s.split_once('/') else {
        return false;
    };
    let Ok(ip) = addr.parse::<std::net::IpAddr>() else {
        return false;
    };
    let max = if ip.is_ipv6() { 128 } else { 32 };
    bits.parse::<u8>().is_ok_and(|b| b <= max)
}

fn theme(state: &State) -> iced::Theme {
    theme::dark().base(state.dark)
}

fn palette(state: &State) -> Palette {
    if state.dark {
        theme::dark()
    } else {
        theme::light()
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

fn view(state: &State) -> Element<'_, Message> {
    let p = palette(state);

    // Logged-out: a full-window welcome (no header/sidebar — there's no tailnet).
    // Suppressed mid-switch so the transition shows a "Switching…" body instead.
    if !state.switching && state.snap.reachable && state.snap.needs_login {
        return container(welcome(p))
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill)
            .style(move |_| theme::window(p))
            .into();
    }

    // Layout adapts to the available width (sidebar+detail when wide; a single
    // column with drill-in navigation when narrow / tiling).
    let content = iced::widget::responsive(move |size| layout(state, p, size.width < 560.0));

    let base = container(content)
        .width(Fill)
        .height(Fill)
        .style(move |_| theme::window(p));

    if state.switcher_open {
        let scrim = mouse_area(container(Space::new().width(Fill).height(Fill)).style(|_| {
            container::Style {
                background: Some(
                    Color {
                        a: 0.35,
                        ..Color::BLACK
                    }
                    .into(),
                ),
                ..Default::default()
            }
        }))
        .on_press(Message::CloseSwitcher);

        let overlay = container(switcher_popover(state, p))
            .width(Fill)
            .height(Fill)
            .align_x(Horizontal::Left)
            .align_y(Vertical::Top)
            .padding(Padding {
                top: 54.0,
                left: 12.0,
                right: 0.0,
                bottom: 0.0,
            });

        stack![base, scrim, overlay].into()
    } else {
        base.into()
    }
}

/// Builds the header + body for a given width regime.
fn layout(state: &State, p: Palette, narrow: bool) -> Element<'_, Message> {
    let mut root = column![header(state, p, narrow)];
    if !state.snap.operator_ok && !state.operator_dismissed {
        root = root.push(operator_banner(p));
    }

    let body: Element<Message> = if state.switching {
        // Transitional state: the tailnet is being torn down and brought back
        // up; show one calm "Switching…" body instead of the disconnected screen.
        switching_body(state, p)
    } else if narrow {
        if state.drilled {
            column![back_bar(p), detail(state, p)].height(Fill).into()
        } else {
            sidebar(state, p, true)
        }
    } else {
        row![sidebar(state, p, false), detail(state, p)]
            .height(Fill)
            .into()
    };
    root = root.push(body);

    if let Some(t) = &state.toast {
        root = root.push(
            container(text(t.clone()).size(13).color(p.text))
                .padding([8, 14])
                .style(move |_| theme::chip(p)),
        );
    }
    root.into()
}

/// Back bar shown above a pushed detail view in the narrow layout.
fn back_bar(p: Palette) -> Element<'static, Message> {
    container(
        button(
            row![
                icon(icon::CHEVRON, 14.0, p.accent),
                text("Back").size(13).color(p.accent)
            ]
            .spacing(4)
            .align_y(Center),
        )
        .style(theme::small_btn(p))
        .padding([5, 10])
        .on_press(Message::Back),
    )
    .padding([8, 12])
    .into()
}

fn header(state: &State, p: Palette, narrow: bool) -> Element<'_, Message> {
    let snap = &state.snap;

    let mark = container(icon::raw(icon::TRAY_CONNECTED, 22.0))
        .width(26)
        .height(26)
        .center_x(26)
        .center_y(26);
    let brand: Element<Message> = if narrow {
        mark.into()
    } else {
        row![mark, text("alavai").size(15).font(SEMI).color(p.text)]
            .spacing(8)
            .align_y(Center)
            .into()
    };

    // Tailnet switcher chip → opens the popover.
    let cur_idx = snap
        .tailnets
        .iter()
        .position(|t| t.id == snap.current_id)
        .unwrap_or(0);
    let (cur_name, cur_email) = match snap.tailnets.iter().find(|t| t.id == snap.current_id) {
        Some(t) => (t.label(), t.user.login_name.clone()),
        None => ("No tailnet".to_string(), String::new()),
    };
    let mut chip_text =
        column![text(cur_name.clone()).size(13).font(SEMI).color(p.text)].spacing(0);
    if !narrow && !cur_email.is_empty() {
        chip_text = chip_text.push(text(cur_email).size(10.5).font(MONO).color(p.text3));
    }
    let switcher = button(
        row![
            avatar_tile(&cur_name, theme::account_color(p, cur_idx), 24.0),
            chip_text,
            icon(icon::CHEVRON, 14.0, p.text3),
        ]
        .spacing(8)
        .align_y(Center),
    )
    .style(theme::secondary_btn(p))
    .padding([5, 10])
    .on_press(Message::ToggleSwitcher);

    // Status pill.
    let (status_label, status_color, tint) = if state.switching {
        ("Switching", p.accent, p.accent_bg)
    } else if snap.online {
        ("Connected", p.online, with_alpha(p.online, 0.12))
    } else {
        ("Disconnected", p.text2, p.raised)
    };
    let mut pill_row = row![dot(status_color, snap.online || state.switching)]
        .spacing(7)
        .align_y(Center);
    if !narrow {
        pill_row = pill_row.push(text(status_label).size(12.5).color(status_color));
    }
    let pill = container(pill_row)
        .padding([5, 10])
        .style(theme::pill(tint, 8.0));

    // Connect / disconnect.
    let conn_label = if snap.online { "Disconnect" } else { "Connect" };
    let conn_msg = (!state.busy).then_some(Message::ToggleConnection);
    let conn = if snap.online {
        button(text(conn_label).size(13)).style(theme::secondary_btn(p))
    } else {
        button(text(conn_label).size(13).color(Color::WHITE)).style(theme::primary_btn(p))
    }
    .padding([7, 14])
    .on_press_maybe(conn_msg);

    let mut bar = row![brand, switcher, Space::new().width(Fill), pill, conn]
        .spacing(if narrow { 8 } else { 12 })
        .align_y(Center)
        .padding([0, if narrow { 10 } else { 16 }]);
    if !narrow {
        bar = bar.push(
            button(text(if state.dark { "☀" } else { "☾" }).size(14))
                .padding([6, 10])
                .style(theme::small_btn(p))
                .on_press(Message::ToggleTheme),
        );
    }

    container(bar)
        .height(56)
        .width(Fill)
        .style(move |_| theme::header(p))
        .into()
}

fn sidebar(state: &State, p: Palette, full: bool) -> Element<'_, Message> {
    let snap = &state.snap;

    let filter = row![
        icon(icon::SEARCH, 15.0, p.text3),
        text_input("Filter peers", &state.filter)
            .on_input(Message::Filter)
            .size(13)
            .padding([7, 8])
            .style(theme::input(p)),
    ]
    .spacing(6)
    .align_y(Center);

    // This machine pinned row.
    let this_selected = state.selection == Selection::ThisMachine;
    let this_row = button(
        row![
            icon(icon::MONITOR, 18.0, p.text2),
            column![
                text(if snap.machine.is_empty() {
                    "This machine".into()
                } else {
                    snap.machine.clone()
                })
                .size(13.5)
                .font(SEMI)
                .color(p.text),
                text("This machine").size(11).color(p.text3),
            ]
            .spacing(1),
            Space::new().width(Fill),
            dot(if snap.online { p.online } else { p.offline }, snap.online),
        ]
        .spacing(9)
        .align_y(Center),
    )
    .width(Fill)
    .padding([7, 9])
    .style(theme::row_btn(p, this_selected))
    .on_press(Message::Select(Selection::ThisMachine));

    // Peer rows (filtered).
    let q = state.filter.to_lowercase();
    let mut peer_col = column![].spacing(2);
    let mut shown = 0usize;
    for peer in &snap.peers {
        if !q.is_empty() && !peer.name.to_lowercase().contains(&q) {
            continue;
        }
        shown += 1;
        let selected = state.selection == Selection::Peer(peer.id.clone());
        let trailing: Element<Message> = if peer.exit_node {
            badge("EXIT", p.exit, p.exit_bg)
        } else {
            let frag = peer
                .primary_addr()
                .rsplit('.')
                .next()
                .map(|s| format!("…{s}"))
                .unwrap_or_default();
            text(frag).size(11.5).font(MONO).color(p.text3).into()
        };
        let name_color = if peer.online { p.text } else { p.text2 };
        peer_col = peer_col.push(
            button(
                row![
                    dot(if peer.online { p.online } else { p.offline }, peer.online),
                    text(peer.name.clone())
                        .size(13)
                        .font(SEMI)
                        .color(name_color),
                    Space::new().width(Fill),
                    trailing,
                ]
                .spacing(8)
                .align_y(Center),
            )
            .width(Fill)
            .padding([6, 9])
            .style(theme::row_btn(p, selected))
            .on_press(Message::Select(Selection::Peer(peer.id.clone()))),
        );
    }
    if shown == 0 {
        peer_col = peer_col.push(text("No matching peers.").size(12).color(p.text3));
    }

    let exit_active = snap.peers.iter().any(|x| x.exit_node);
    let footer = row![
        button(
            row![
                icon(
                    icon::GLOBE,
                    15.0,
                    if exit_active { p.exit } else { p.text2 }
                ),
                text("Exit node").size(12)
            ]
            .spacing(6)
            .align_y(Center)
        )
        .style(theme::small_btn(p))
        .padding([6, 10])
        .on_press(Message::Select(Selection::ExitNode)),
        Space::new().width(Fill),
        button(icon(icon::EXTERNAL, 15.0, p.text2))
            .style(theme::small_btn(p))
            .padding([6, 8])
            .on_press(Message::OpenAdmin),
    ]
    .spacing(6)
    .align_y(Center);

    container(
        column![
            filter,
            this_row,
            caps(format!("PEERS · {}", snap.peers.len()), p),
            scrollable(peer_col).height(Fill),
            footer,
        ]
        .spacing(10)
        .padding(10),
    )
    .width(if full { Fill } else { Length::Fixed(SIDEBAR_W) })
    .height(Fill)
    .style(move |_| theme::sidebar(p))
    .into()
}

fn detail(state: &State, p: Palette) -> Element<'_, Message> {
    if !state.snap.reachable {
        return container(daemon_down(p))
            .padding(20)
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill)
            .into();
    }

    let content = match &state.selection {
        Selection::ThisMachine => this_machine(state, p),
        Selection::ExitNode => exit_picker(state, p),
        Selection::Peer(id) => match state.snap.peers.iter().find(|x| &x.id == id) {
            Some(peer) => peer_detail(peer, p),
            None => text("Peer not found.").size(14).color(p.text2).into(),
        },
    };
    container(scrollable(content).height(Fill))
        .padding(20)
        .width(Fill)
        .height(Fill)
        .into()
}

fn this_machine(state: &State, p: Palette) -> Element<'static, Message> {
    let snap = &state.snap;

    let os_chip = if snap.os.is_empty() {
        Space::new().width(0).into()
    } else {
        badge_text(snap.os.clone(), p.text2, p.raised)
    };
    let title = row![
        icon(icon::MONITOR, 22.0, p.text),
        text("This machine").size(20).font(SEMI).color(p.text),
        os_chip
    ]
    .spacing(10)
    .align_y(Center);

    // Identity card.
    let mut ident = column![status_line(snap.online, p)].spacing(2);
    if !snap.fqdn.is_empty() {
        ident = ident.push(kv_row("MagicDNS", snap.fqdn.clone(), p.text, p));
    }
    for addr in &snap.addrs {
        let color = if addr.contains(':') {
            p.text2
        } else {
            p.accent
        };
        ident = ident.push(kv_row(
            if addr.contains(':') { "IPv6" } else { "IPv4" },
            addr.clone(),
            color,
            p,
        ));
    }

    // Settings card.
    let settings = column![
        setting_toggle(
            "Advertise as exit node",
            "Offer this device so others can route through it. An admin approves it.",
            snap.advertise_exit_node,
            Message::SetAdvertiseExit,
            p,
        ),
        divider(p),
        setting_toggle(
            "Accept subnet routes",
            "Reach the LANs that other devices share, like a home or office network.",
            snap.accept_routes,
            Message::SetAcceptRoutes,
            p,
        ),
        divider(p),
        setting_toggle(
            "Allow LAN access",
            "Keep reaching your local printer and NAS while an exit node is on.",
            snap.allow_lan,
            Message::SetAllowLan,
            p,
        ),
    ]
    .spacing(10);

    // Advertised routes card (editable).
    let mut routes = column![caps("ADVERTISED ROUTES".into(), p)].spacing(6);
    if snap.advertised_routes.is_empty() {
        routes = routes.push(text("None advertised.").size(13).color(p.text3));
    } else {
        for r in &snap.advertised_routes {
            let r = r.clone();
            routes = routes.push(
                row![
                    text(r.clone()).size(13).font(MONO).color(p.text),
                    Space::new().width(Fill),
                    button(icon(icon::CLOSE, 13.0, p.text2))
                        .style(theme::small_btn(p))
                        .padding([4, 6])
                        .on_press(Message::RemoveRoute(r)),
                ]
                .align_y(Center),
            );
        }
    }
    routes = routes.push(
        row![
            text_input("Add route, e.g. 192.168.1.0/24", &state.route_input)
                .on_input(Message::RouteInput)
                .on_submit(Message::AddRoute)
                .size(12.5)
                .padding([6, 9])
                .style(theme::input(p)),
            button(
                row![
                    icon(icon::PLUS, 13.0, p.accent),
                    text("Add").size(12).color(p.accent)
                ]
                .spacing(5)
                .align_y(Center)
            )
            .style(theme::secondary_btn(p))
            .padding([6, 12])
            .on_press(Message::AddRoute),
        ]
        .spacing(8)
        .align_y(Center),
    );

    column![
        title,
        card(ident.into(), p),
        card(settings.into(), p),
        card(routes.into(), p),
        card(netcheck_card(state, p), p),
    ]
    .spacing(16)
    .into()
}

/// The netcheck (connectivity diagnostics) card.
fn netcheck_card(state: &State, p: Palette) -> Element<'static, Message> {
    let run = button(
        row![
            icon(icon::REFRESH, 14.0, p.text2),
            text(if state.netcheck_running {
                "Running…"
            } else {
                "Run"
            })
            .size(12),
        ]
        .spacing(6)
        .align_y(Center),
    )
    .style(theme::secondary_btn(p))
    .padding([5, 12])
    .on_press_maybe((!state.netcheck_running).then_some(Message::RunNetcheck));

    let head = row![
        icon(icon::ACTIVITY, 16.0, p.text),
        text("Netcheck").size(13.5).font(SEMI).color(p.text),
        Space::new().width(Fill),
        run,
    ]
    .spacing(8)
    .align_y(Center);

    let mut col = column![head].spacing(8);

    if let Some(r) = &state.netcheck {
        let bool_row = |label: &str, ok: bool, detail: String| -> Element<'static, Message> {
            let (glyph, color) = if ok {
                (icon::CHECK, p.online)
            } else {
                (icon::CLOSE, p.text3)
            };
            row![
                icon(glyph, 14.0, color),
                text(label.to_string())
                    .size(12.5)
                    .color(p.text2)
                    .width(Length::Fixed(120.0)),
                text(detail).size(12.5).font(MONO).color(p.text),
            ]
            .spacing(8)
            .align_y(Center)
            .into()
        };
        let opt = |b: Option<bool>| match b {
            Some(true) => "yes",
            Some(false) => "no",
            None => "—",
        };
        col = col.push(divider(p));
        col = col.push(bool_row(
            "UDP",
            r.udp,
            if r.udp {
                "working".into()
            } else {
                "blocked".into()
            },
        ));
        col = col.push(bool_row("IPv4", r.ipv4, r.global_v4.clone()));
        col = col.push(bool_row(
            "IPv6",
            r.ipv6,
            if r.ipv6 {
                r.global_v6.clone()
            } else {
                "not available".into()
            },
        ));
        col = col.push(bool_row(
            "NAT traversal",
            r.upnp == Some(true) || r.pmp == Some(true) || r.pcp == Some(true),
            format!(
                "UPnP {} · PMP {} · PCP {}",
                opt(r.upnp),
                opt(r.pmp),
                opt(r.pcp)
            ),
        ));
        col = col.push(bool_row(
            "Captive portal",
            r.captive_portal != Some(true),
            if r.captive_portal == Some(true) {
                "detected".into()
            } else {
                "none".into()
            },
        ));

        // Preferred relay + nearest latencies.
        let mut lats: Vec<(&String, &i64)> = r.region_latency.iter().collect();
        lats.sort_by_key(|(_, ns)| **ns);
        col = col.push(divider(p));
        if let Some((region, ns)) = lats.first() {
            col = col.push(
                row![
                    icon(icon::PIN, 14.0, p.accent),
                    text("Preferred relay")
                        .size(12.5)
                        .color(p.text2)
                        .width(Length::Fixed(120.0)),
                    text(format!("region {region} · {:.0} ms", **ns as f64 / 1e6))
                        .size(12.5)
                        .font(MONO)
                        .color(p.text),
                ]
                .spacing(8)
                .align_y(Center),
            );
        }
        for (region, ns) in lats.iter().take(5) {
            col = col.push(
                row![
                    Space::new().width(22),
                    text(format!("region {region}"))
                        .size(12)
                        .color(p.text3)
                        .width(Length::Fixed(110.0)),
                    text(format!("{:.0} ms", **ns as f64 / 1e6))
                        .size(12)
                        .font(MONO)
                        .color(p.text2),
                ]
                .spacing(8),
            );
        }
    } else if state.netcheck_running {
        col = col.push(text("Testing connectivity…").size(12.5).color(p.text3));
    } else {
        // No full report yet. Surface what we have *without* the tailscale CLI:
        // the preferred DERP relay (from status), so the panel isn't empty even
        // when the CLI is absent.
        if !state.snap.self_relay.is_empty() {
            col = col.push(divider(p));
            col = col.push(
                row![
                    icon(icon::PIN, 14.0, p.accent),
                    text("Preferred relay")
                        .size(12.5)
                        .color(p.text2)
                        .width(Length::Fixed(120.0)),
                    text(state.snap.self_relay.clone())
                        .size(12.5)
                        .font(MONO)
                        .color(p.text),
                ]
                .spacing(8)
                .align_y(Center),
            );
        }
        if let Some(err) = &state.netcheck_error {
            col = col.push(
                row![
                    icon(icon::WARN, 14.0, p.danger),
                    text(err.clone()).size(12.5).color(p.text2),
                ]
                .spacing(8)
                .align_y(Center),
            );
        } else {
            col = col.push(
                text("Run a check for full diagnostics.")
                    .size(12.5)
                    .color(p.text3),
            );
        }
    }

    col.into()
}

fn peer_detail(peer: &PeerView, p: Palette) -> Element<'static, Message> {
    let mut title = row![
        icon(icon::LAPTOP, 22.0, p.text),
        text(peer.name.clone()).size(20).font(SEMI).color(p.text),
        dot(if peer.online { p.online } else { p.offline }, peer.online),
    ]
    .spacing(10)
    .align_y(Center);
    if peer.exit_node {
        title = title.push(badge("EXIT NODE", p.exit, p.exit_bg));
    }

    let sub = text(format!(
        "{}{}",
        peer.fqdn,
        if peer.os.is_empty() {
            String::new()
        } else {
            format!("  ·  {}", peer.os)
        }
    ))
    .size(12.5)
    .font(MONO)
    .color(p.text2);

    // Action row.
    let mut actions = row![].spacing(8);
    if peer.exit_node {
        actions = actions.push(
            button(text("Disable exit node").size(13))
                .style(theme::secondary_btn(p))
                .padding([7, 14])
                .on_press(Message::ClearExitNode),
        );
    } else if peer.exit_node_option {
        actions = actions.push(
            button(text("Use as exit node").size(13).color(Color::WHITE))
                .style(theme::primary_btn(p))
                .padding([7, 14])
                .on_press(Message::UseExitNode(peer.id.clone())),
        );
    }
    let primary = peer.primary_addr().to_string();
    if !primary.is_empty() {
        actions = actions.push(
            button(text("Copy IP").size(13))
                .style(theme::secondary_btn(p))
                .padding([7, 14])
                .on_press(Message::Copy(primary)),
        );
    }

    // Detail card.
    let mut info = column![].spacing(2);
    for addr in &peer.addrs {
        let color = if addr.contains(':') {
            p.text2
        } else {
            p.accent
        };
        info = info.push(kv_row(
            if addr.contains(':') { "IPv6" } else { "IPv4" },
            addr.clone(),
            color,
            p,
        ));
    }
    if !peer.routes.is_empty() {
        info = info.push(meta_row("Routes", peer.routes.join("  ·  "), p));
    }
    let conn = if !peer.online {
        "Offline".to_string()
    } else if peer.relay.is_empty() {
        "Direct".to_string()
    } else {
        format!("Relay · DERP {}", peer.relay)
    };
    info = info.push(meta_row("Connection", conn, p));
    let seen = if peer.online {
        "Active now".to_string()
    } else if peer.last_seen.is_empty() || peer.last_seen.starts_with("0001-01-01") {
        "Unknown".to_string()
    } else {
        peer.last_seen.replace('T', " ").chars().take(16).collect()
    };
    info = info.push(meta_row("Last seen", seen, p));

    column![title, sub, actions, card(info.into(), p)]
        .spacing(12)
        .into()
}

fn exit_picker(state: &State, p: Palette) -> Element<'static, Message> {
    let snap = &state.snap;
    let active = snap.peers.iter().any(|x| x.exit_node);

    let title = row![
        icon(icon::GLOBE, 22.0, p.text),
        text("Exit node").size(20).font(SEMI).color(p.text)
    ]
    .spacing(10)
    .align_y(Center);

    // None / Automatic options.
    let mut options = column![
        picker_row(
            "None",
            "Use your own internet connection",
            !active,
            p,
            Message::ClearExitNode
        ),
        picker_row(
            "Automatic",
            "Pick the best available node",
            false,
            p,
            Message::UseExitNodeAuto
        ),
    ]
    .spacing(2);

    // Eligible peers.
    let mut eligible: Vec<&PeerView> = snap.peers.iter().filter(|x| x.exit_node_option).collect();
    eligible.sort_by_key(|p| p.name.to_lowercase());
    let mut peer_rows = column![caps("YOUR PEERS".into(), p)].spacing(2);
    if eligible.is_empty() {
        peer_rows = peer_rows.push(
            text("No exit nodes available on this tailnet.")
                .size(12.5)
                .color(p.text3),
        );
    } else {
        for peer in eligible {
            let sub = if peer.relay.is_empty() {
                if peer.online {
                    "online".to_string()
                } else {
                    "offline".to_string()
                }
            } else {
                format!("DERP {}", peer.relay)
            };
            peer_rows = peer_rows.push(picker_row(
                &peer.name,
                &sub,
                peer.exit_node,
                p,
                Message::UseExitNode(peer.id.clone()),
            ));
        }
    }

    // Allow LAN access toggle.
    let lan = card(
        setting_toggle(
            "Allow LAN access",
            "Keep reaching your local printer and NAS while an exit node is on.",
            snap.allow_lan,
            Message::SetAllowLan,
            p,
        ),
        p,
    );

    options = options.push(Space::new().height(8));
    column![
        title,
        card(options.into(), p),
        card(peer_rows.into(), p),
        lan
    ]
    .spacing(16)
    .into()
}

/// A selectable row used in the exit-node picker.
fn picker_row(
    title: &str,
    sub: &str,
    selected: bool,
    p: Palette,
    msg: Message,
) -> Element<'static, Message> {
    let trailing: Element<Message> = if selected {
        icon(icon::CHECK, 18.0, p.accent)
    } else {
        Space::new().width(18).into()
    };
    button(
        row![
            column![
                text(title.to_string()).size(13.5).font(SEMI).color(p.text),
                text(sub.to_string()).size(11.5).color(p.text3),
            ]
            .spacing(1),
            Space::new().width(Fill),
            trailing,
        ]
        .spacing(10)
        .align_y(Center),
    )
    .width(Fill)
    .padding([8, 10])
    .style(theme::row_btn(p, selected))
    .on_press(msg)
    .into()
}

/// The tailnet-switcher popover (drops from the header chip).
fn switcher_popover(state: &State, p: Palette) -> Element<'static, Message> {
    let snap = &state.snap;

    let edit_label = if state.manage { "Done" } else { "Edit" };
    let header = row![
        caps("TAILNETS".into(), p),
        Space::new().width(Fill),
        button(text(edit_label).size(12).color(p.accent))
            .style(theme::small_btn(p))
            .padding([3, 8])
            .on_press(Message::ToggleManage),
    ]
    .align_y(Center);

    let mut col = column![header].spacing(6);

    for (idx, t) in snap.tailnets.iter().enumerate() {
        let active = t.id == snap.current_id;
        let name = t.label();
        let info = column![
            text(name.clone()).size(13.5).font(SEMI).color(p.text),
            text(t.user.login_name.clone())
                .size(10.5)
                .font(MONO)
                .color(p.text3),
        ]
        .spacing(0);
        let av = avatar_tile(&name, theme::account_color(p, idx), 30.0);

        if state.manage {
            let remove = button(text("Remove").size(11.5).color(p.danger))
                .style(theme::secondary_btn(p))
                .padding([4, 10])
                .on_press(Message::RequestRemove(t.id.clone()));
            col = col.push(
                container(
                    row![av, info, Space::new().width(Fill), remove]
                        .spacing(10)
                        .align_y(Center),
                )
                .padding([7, 8]),
            );
            if state.confirm_remove.as_deref() == Some(t.id.as_str()) {
                col = col.push(confirm_block(&name, &t.id, p));
            }
        } else {
            let trailing: Element<Message> = if active {
                icon(icon::CHECK, 18.0, p.accent)
            } else {
                Space::new().width(18).into()
            };
            col = col.push(
                button(
                    row![av, info, Space::new().width(Fill), trailing]
                        .spacing(10)
                        .align_y(Center),
                )
                .width(Fill)
                .padding([7, 8])
                .style(theme::row_btn(p, active))
                .on_press(Message::SwitchTailnet(t.id.clone())),
            );
        }
    }

    col = col.push(divider(p));
    col = col.push(
        button(
            row![
                icon(icon::PLUS, 16.0, p.accent),
                text("Add tailnet").size(13).color(p.accent)
            ]
            .spacing(8)
            .align_y(Center),
        )
        .width(Fill)
        .padding([8, 8])
        .style(theme::row_btn(p, false))
        .on_press(Message::AddTailnet),
    );

    container(col)
        .width(Length::Fixed(330.0))
        .padding(12)
        .style(move |_| theme::card(p))
        .into()
}

/// A small square avatar tile showing a name's initial.
fn avatar_tile(name: &str, color: Color, size: f32) -> Element<'static, Message> {
    let initial = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into());
    container(
        text(initial)
            .size(size * 0.5)
            .font(SEMI)
            .color(Color::WHITE),
    )
    .center_x(size)
    .center_y(size)
    .style(theme::avatar(color))
    .into()
}

/// The inline "forget this tailnet?" confirmation block.
fn confirm_block(name: &str, id: &str, p: Palette) -> Element<'static, Message> {
    let id = id.to_string();
    container(
        column![
            text(format!(
                "Forget {name}? You'll need to sign in again to use it."
            ))
            .size(12)
            .color(p.text2),
            row![
                button(text("Remove").size(12).color(Color::WHITE))
                    .style(theme::danger_btn(p))
                    .padding([5, 12])
                    .on_press(Message::ConfirmRemove(id)),
                button(text("Keep").size(12).color(p.text))
                    .style(theme::secondary_btn(p))
                    .padding([5, 12])
                    .on_press(Message::CancelRemove),
            ]
            .spacing(8),
        ]
        .spacing(8),
    )
    .padding(10)
    .style(move |_| container::Style {
        background: Some(with_alpha(p.danger, 0.10).into()),
        border: iced::Border {
            color: p.danger,
            width: 1.0,
            radius: 7.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// Amber banner shown when the user isn't the Tailscale operator.
fn operator_banner(p: Palette) -> Element<'static, Message> {
    const CMD: &str = "sudo tailscale set --operator=$USER";
    container(
        row![
            icon(icon::WARN, 18.0, p.warn),
            column![
                text("alavai can't control Tailscale yet")
                    .size(13)
                    .font(SEMI)
                    .color(p.text),
                text("Your Linux user isn't the Tailscale operator, so most actions are disabled.")
                    .size(11.5)
                    .color(p.text2),
            ]
            .spacing(1),
            Space::new().width(Fill),
            button(text(CMD).size(11.5).font(MONO).color(p.text))
                .style(theme::secondary_btn(p))
                .padding([5, 10])
                .on_press(Message::Copy(CMD.to_string())),
            button(text("Recheck").size(12).color(Color::WHITE))
                .style(theme::primary_btn(p))
                .padding([5, 12])
                .on_press(Message::Retry),
            button(icon(icon::CLOSE, 14.0, p.text2))
                .style(theme::small_btn(p))
                .padding([5, 7])
                .on_press(Message::DismissOperator),
        ]
        .spacing(10)
        .align_y(Center),
    )
    .padding([8, 16])
    .width(Fill)
    .style(move |_| container::Style {
        background: Some(with_alpha(p.warn, 0.12).into()),
        border: iced::Border {
            color: with_alpha(p.warn, 0.4),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// Full-window welcome shown when no tailnet is logged in.
fn welcome(p: Palette) -> Element<'static, Message> {
    column![
        icon::raw(icon::TRAY_CONNECTED, 56.0),
        text("Welcome to alavai").size(22).font(SEMI).color(p.text),
        text("Log in to a Tailscale account to see your devices and start switching tailnets.")
            .size(13)
            .color(p.text2),
        Space::new().height(4),
        button(
            row![
                icon(icon::GLOBE, 16.0, Color::WHITE),
                text("Log in to Tailscale").size(13.5).color(Color::WHITE)
            ]
            .spacing(8)
            .align_y(Center)
        )
        .style(theme::primary_btn(p))
        .padding([9, 18])
        .on_press(Message::StartLogin),
        text("or connect a custom control server")
            .size(11.5)
            .color(p.text3),
    ]
    .spacing(10)
    .align_x(Center)
    .width(Length::Fixed(380.0))
    .into()
}

/// Centered state shown when the local daemon can't be reached.
fn daemon_down(p: Palette) -> Element<'static, Message> {
    column![
        icon(icon::WIFI_OFF, 40.0, p.danger),
        text("Can't reach Tailscale")
            .size(18)
            .font(SEMI)
            .color(p.text),
        text("The tailscaled service doesn't seem to be running.")
            .size(13)
            .color(p.text2),
        button(text("Retry").size(13).color(Color::WHITE))
            .style(theme::primary_btn(p))
            .padding([7, 16])
            .on_press(Message::Retry),
    ]
    .spacing(12)
    .align_x(Center)
    .into()
}

/// Centered body shown while switching tailnets (between teardown and connect).
fn switching_body(state: &State, p: Palette) -> Element<'static, Message> {
    let name = state
        .switching_to
        .clone()
        .unwrap_or_else(|| "your tailnet".into());
    container(
        column![
            icon::raw(icon::TRAY_CONNECTED, 48.0),
            text(format!("Switching to {name}…"))
                .size(16)
                .font(SEMI)
                .color(p.text),
            text("Reconnecting to the tailnet.")
                .size(12.5)
                .color(p.text3),
        ]
        .spacing(10)
        .align_x(Center),
    )
    .width(Fill)
    .height(Fill)
    .center_x(Fill)
    .center_y(Fill)
    .into()
}

// ---------------------------------------------------------------------------
// Small view helpers
// ---------------------------------------------------------------------------

fn card(body: Element<'static, Message>, p: Palette) -> Element<'static, Message> {
    container(body)
        .padding(14)
        .width(Fill)
        .style(move |_| theme::card(p))
        .into()
}

fn dot(color: Color, filled: bool) -> Element<'static, Message> {
    let style = move |_: &iced::Theme| container::Style {
        background: if filled { Some(color.into()) } else { None },
        border: iced::Border {
            color,
            width: if filled { 0.0 } else { 1.5 },
            radius: 5.0.into(),
        },
        ..Default::default()
    };
    container(Space::new().width(9).height(9))
        .style(style)
        .into()
}

fn caps(s: String, p: Palette) -> Element<'static, Message> {
    text(s.to_uppercase())
        .size(10.5)
        .font(MONO)
        .color(p.text3)
        .into()
}

fn badge(label: &str, fg: Color, bg: Color) -> Element<'static, Message> {
    badge_text(label.to_string(), fg, bg)
}

fn badge_text(label: String, fg: Color, bg: Color) -> Element<'static, Message> {
    container(text(label).size(9.5).font(MONO).color(fg))
        .padding([2, 6])
        .style(theme::pill(bg, 5.0))
        .into()
}

fn status_line(online: bool, p: Palette) -> Element<'static, Message> {
    let (label, color) = if online {
        ("Connected", p.online)
    } else {
        ("Disconnected", p.text2)
    };
    row![dot(color, online), text(label).size(13).color(color)]
        .spacing(8)
        .align_y(Center)
        .into()
}

/// A `label · mono value · Copy` row (for IPs, MagicDNS).
fn kv_row(label: &str, value: String, value_color: Color, p: Palette) -> Element<'static, Message> {
    row![
        text(label.to_string())
            .size(12)
            .color(p.text3)
            .width(Length::Fixed(100.0)),
        text(value.clone()).size(12.5).font(MONO).color(value_color),
        Space::new().width(Fill),
        button(icon(icon::COPY, 14.0, p.text2))
            .style(theme::small_btn(p))
            .padding([5, 7])
            .on_press(Message::Copy(value)),
    ]
    .spacing(8)
    .align_y(Center)
    .padding([6, 0])
    .into()
}

/// A `label · value` meta row (no copy).
fn meta_row(label: &str, value: String, p: Palette) -> Element<'static, Message> {
    row![
        text(label.to_string())
            .size(12)
            .color(p.text3)
            .width(Length::Fixed(100.0)),
        text(value).size(12.5).color(p.text2),
    ]
    .spacing(8)
    .align_y(Center)
    .padding([6, 0])
    .into()
}

fn setting_toggle(
    title: &str,
    sub: &str,
    value: bool,
    on_toggle: fn(bool) -> Message,
    p: Palette,
) -> Element<'static, Message> {
    row![
        column![
            text(title.to_string()).size(13.5).color(p.text),
            text(sub.to_string()).size(11.5).color(p.text3),
        ]
        .spacing(2),
        Space::new().width(Fill),
        toggler(value).on_toggle(on_toggle).size(22),
    ]
    .spacing(12)
    .align_y(Center)
    .into()
}

fn divider(p: Palette) -> Element<'static, Message> {
    container(Space::new().width(Fill).height(1))
        .style(move |_| container::Style {
            background: Some(p.line.into()),
            ..Default::default()
        })
        .into()
}

fn with_alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

// ---------------------------------------------------------------------------
// Subscriptions + run
// ---------------------------------------------------------------------------

fn subscription(_state: &State) -> Subscription<Message> {
    Subscription::run(watch_stream)
}

/// Clears the toast after a short delay. Uses a background sleep fulfilling a
/// oneshot (no timer backend / async runtime required).
fn delayed_clear() -> Task<Message> {
    Task::perform(
        async {
            let (tx, rx) = iced::futures::channel::oneshot::channel::<()>();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(2500));
                let _ = tx.send(());
            });
            let _ = rx.await;
        },
        |_| Message::ClearToast,
    )
}

/// Safety net: ends the switching transition if the new tailnet never settles
/// (so the spinner can't hang forever).
fn switching_timeout() -> Task<Message> {
    Task::perform(
        async {
            let (tx, rx) = iced::futures::channel::oneshot::channel::<()>();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(12));
                let _ = tx.send(());
            });
            let _ = rx.await;
        },
        |_| Message::ClearSwitching,
    )
}

fn watch_stream() -> impl Stream<Item = Message> {
    use iced::futures::channel::mpsc::Sender;
    iced::stream::channel(8, |mut output: Sender<Message>| async move {
        std::thread::spawn(move || {
            let client = Client::default();
            let _ = output.try_send(Message::Snapshot(fetch_gui(&client)));
            let probe = client.clone();
            client.watch_live(move |live| {
                if live.netmap_changed {
                    // Peers may have changed — do the one heavier refresh here.
                    let mut snap = fetch_gui(&probe);
                    snap.login_url = live.browse_to_url.clone();
                    let _ = output.try_send(Message::Snapshot(snap));
                } else {
                    // State / prefs / login change only — apply as a pure delta.
                    let _ = output.try_send(Message::Live(live));
                }
            });
        });
        std::future::pending::<()>().await;
    })
}

/// Runs the GUI. Blocks until the window is closed.
pub fn run() -> Result<()> {
    localapi::warn_if_untested_daemon(&Client::default());

    let window_icon = icon::render_rgba(icon::TRAY_CONNECTED, 64)
        .and_then(|(w, h, rgba)| iced::window::icon::from_rgba(rgba, w, h).ok());

    iced::application(boot, update, view)
        .title("alavai")
        .subscription(subscription)
        .theme(theme)
        .window(iced::window::Settings {
            size: Size::new(920.0, 640.0),
            min_size: Some(Size::new(340.0, 480.0)),
            icon: window_icon,
            ..Default::default()
        })
        .run()
        .map_err(|e| anyhow::anyhow!("run GUI: {e}"))
}
