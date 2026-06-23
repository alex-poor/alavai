//! The main window (iced, tiny-skia software renderer).
//!
//! Implements the design in docs/design/DESIGN.md: a persistent header (tailnet
//! switcher + connection status/toggle) over a sidebar (filterable peer list)
//! and a detail pane (this-machine settings or a selected peer). The view is a
//! pure function of one `GuiSnapshot`, rebuilt from the LocalAPI `status` +
//! `prefs` + profiles whenever the IPN bus signals a change.

use std::fmt;
use std::process::Command as ProcCommand;

use anyhow::Result;
use iced::futures::Stream;
use iced::widget::{button, column, container, row, scrollable, text, text_input, toggler, Space};
use iced::{Center, Color, Element, Fill, Font, Length, Size, Subscription, Task};

use crate::localapi::{Client, Profile};
use crate::theme::{self, Palette};

const ADMIN_URL: &str = "https://login.tailscale.com/admin";
const MONO: Font = Font::MONOSPACE;
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
    addrs: Vec<String>,
    tailnets: Vec<Profile>,
    current_id: String,
    // prefs
    accept_routes: bool,
    advertise_exit_node: bool,
    allow_lan: bool,
    advertised_routes: Vec<String>,
    peers: Vec<PeerView>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct TailnetChoice {
    id: String,
    label: String,
}

impl fmt::Display for TailnetChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Selection {
    ThisMachine,
    Peer(String),
}

struct State {
    dark: bool,
    snap: GuiSnapshot,
    selection: Selection,
    filter: String,
    busy: bool,
    switching: bool,
    toast: Option<String>,
}

#[derive(Debug, Clone)]
enum Message {
    Snapshot(GuiSnapshot),
    Select(Selection),
    Filter(String),
    SwitchTailnet(TailnetChoice),
    ToggleConnection,
    SetAcceptRoutes(bool),
    SetAdvertiseExit(bool),
    SetAllowLan(bool),
    UseExitNode(String),
    ClearExitNode,
    Copy(String),
    ToggleTheme,
    OpenAdmin,
    Toast(String),
    ClearToast,
}

// ---------------------------------------------------------------------------
// Data fetch
// ---------------------------------------------------------------------------

fn fetch_gui(client: &Client) -> GuiSnapshot {
    let status = client.status().ok();
    let prefs = client.prefs().ok();
    let online = status.as_ref().is_some_and(|s| s.online());

    let (machine, fqdn, os) = match status.as_ref().and_then(|s| s.self_node.as_ref()) {
        Some(n) => (n.hostname.clone(), n.dns_name.trim_end_matches('.').to_string(), n.os.clone()),
        None => (String::new(), String::new(), String::new()),
    };
    let addrs = status.as_ref().map(|s| s.tailscale_ips.clone()).unwrap_or_default();

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
    peers.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let tailnets = client
        .profiles()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect();
    let current_id = client.current_profile().ok().map(|p| p.id).unwrap_or_default();

    GuiSnapshot {
        online,
        machine,
        fqdn,
        os,
        addrs,
        tailnets,
        current_id,
        accept_routes: prefs.as_ref().is_some_and(|p| p.route_all),
        advertise_exit_node: prefs.as_ref().is_some_and(|p| p.advertises_exit_node()),
        allow_lan: prefs.as_ref().is_some_and(|p| p.exit_node_allow_lan),
        advertised_routes: prefs.as_ref().map(|p| p.subnet_routes()).unwrap_or_default(),
        peers,
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
            toast: None,
        },
        Task::none(),
    )
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::Snapshot(snap) => {
            // Keep the selected peer valid across refreshes.
            if let Selection::Peer(id) = &state.selection {
                if !snap.peers.iter().any(|p| &p.id == id) {
                    state.selection = Selection::ThisMachine;
                }
            }
            state.snap = snap;
            state.busy = false;
            state.switching = false;
            Task::none()
        }
        Message::Select(sel) => {
            state.selection = sel;
            Task::none()
        }
        Message::Filter(f) => {
            state.filter = f;
            Task::none()
        }
        Message::SwitchTailnet(choice) => {
            if choice.id == state.snap.current_id {
                return Task::none();
            }
            state.busy = true;
            state.switching = true;
            let id = choice.id;
            act(move |c| {
                let _ = c.switch_profile(&id);
            })
        }
        Message::ToggleConnection => {
            let online = state.snap.online;
            state.busy = true;
            act(move |_| {
                let action = if online { "down" } else { "up" };
                let _ = ProcCommand::new("tailscale").arg(action).status();
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
        Message::ClearExitNode => {
            state.busy = true;
            act(|c| {
                let _ = c.set_exit_node("");
            })
        }
        Message::Copy(value) => {
            let toast = format!("Copied {value}");
            Task::batch([iced::clipboard::write(value), Task::done(Message::Toast(toast))])
        }
        Message::ToggleTheme => {
            state.dark = !state.dark;
            Task::none()
        }
        Message::OpenAdmin => {
            let _ = ProcCommand::new("xdg-open").arg(ADMIN_URL).spawn();
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
    }
}

fn theme(state: &State) -> iced::Theme {
    theme::dark().base(state.dark)
}

fn palette(state: &State) -> Palette {
    if state.dark { theme::dark() } else { theme::light() }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

fn view(state: &State) -> Element<'_, Message> {
    let p = palette(state);

    let body = row![sidebar(state, p), detail(state, p)].height(Fill);

    let mut root = column![header(state, p), body];

    if let Some(t) = &state.toast {
        root = root.push(
            container(text(t.clone()).size(13).color(p.text))
                .padding([8, 14])
                .style(move |_| theme::chip(p)),
        );
    }

    container(root)
        .width(Fill)
        .height(Fill)
        .style(move |_| theme::window(p))
        .into()
}

fn header(state: &State, p: Palette) -> Element<'_, Message> {
    let snap = &state.snap;

    let mark = container(text("a").size(15).color(Color::WHITE))
        .width(26)
        .height(26)
        .center_x(26)
        .center_y(26)
        .style(theme::avatar(p.accent));
    let brand = row![mark, text("alavai").size(15).color(p.text)]
        .spacing(8)
        .align_y(Center);

    // Tailnet switcher (pick-list for now; popover comes later).
    let choices: Vec<TailnetChoice> = snap
        .tailnets
        .iter()
        .map(|t| TailnetChoice { id: t.id.clone(), label: t.label() })
        .collect();
    let selected = choices.iter().find(|c| c.id == snap.current_id).cloned();
    let switcher = iced::widget::pick_list(choices, selected, Message::SwitchTailnet)
        .text_size(13)
        .padding([6, 10]);

    // Status pill.
    let (status_label, status_color, tint) = if state.switching {
        ("Switching", p.accent, p.accent_bg)
    } else if snap.online {
        ("Connected", p.online, with_alpha(p.online, 0.12))
    } else {
        ("Disconnected", p.text2, p.raised)
    };
    let pill = container(
        row![dot(status_color, snap.online || state.switching), text(status_label).size(12.5).color(status_color)]
            .spacing(7)
            .align_y(Center),
    )
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

    let theme_toggle = button(text(if state.dark { "☀" } else { "☾" }).size(14))
        .padding([6, 10])
        .style(theme::small_btn(p))
        .on_press(Message::ToggleTheme);

    container(
        row![brand, switcher, Space::new().width(Fill), pill, conn, theme_toggle]
            .spacing(12)
            .align_y(Center)
            .padding([0, 16]),
    )
    .height(56)
    .width(Fill)
    .style(move |_| theme::header(p))
    .into()
}

fn sidebar(state: &State, p: Palette) -> Element<'_, Message> {
    let snap = &state.snap;

    let filter = text_input("Filter peers", &state.filter)
        .on_input(Message::Filter)
        .size(13)
        .padding([7, 10])
        .style(theme::input(p));

    // This machine pinned row.
    let this_selected = state.selection == Selection::ThisMachine;
    let this_row = button(
        row![
            column![
                text(if snap.machine.is_empty() { "This machine".into() } else { snap.machine.clone() })
                    .size(13.5)
                    .color(p.text),
                text("This machine").size(11).color(p.text3),
            ]
            .spacing(1),
            Space::new().width(Fill),
            dot(if snap.online { p.online } else { p.offline }, snap.online),
        ]
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
            let frag = peer.primary_addr().rsplit('.').next().map(|s| format!("…{s}")).unwrap_or_default();
            text(frag).size(11.5).font(MONO).color(p.text3).into()
        };
        let name_color = if peer.online { p.text } else { p.text2 };
        peer_col = peer_col.push(
            button(
                row![
                    dot(if peer.online { p.online } else { p.offline }, peer.online),
                    text(peer.name.clone()).size(13).color(name_color),
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

    let footer = row![
        button(text("Admin console").size(12)).style(theme::small_btn(p)).padding([6, 10]).on_press(Message::OpenAdmin),
    ];

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
    .width(SIDEBAR_W)
    .height(Fill)
    .style(move |_| theme::sidebar(p))
    .into()
}

fn detail(state: &State, p: Palette) -> Element<'_, Message> {
    let content = match &state.selection {
        Selection::ThisMachine => this_machine(state, p),
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
    let title = row![text("This machine").size(20).color(p.text), os_chip]
        .spacing(10)
        .align_y(Center);

    // Identity card.
    let mut ident = column![status_line(snap.online, p)].spacing(2);
    if !snap.fqdn.is_empty() {
        ident = ident.push(kv_row("MagicDNS", snap.fqdn.clone(), p.text, p));
    }
    for addr in &snap.addrs {
        let color = if addr.contains(':') { p.text2 } else { p.accent };
        ident = ident.push(kv_row(if addr.contains(':') { "IPv6" } else { "IPv4" }, addr.clone(), color, p));
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

    // Advertised routes card.
    let mut routes = column![caps("ADVERTISED ROUTES".into(), p)].spacing(6);
    if snap.advertised_routes.is_empty() {
        routes = routes.push(text("None advertised.").size(13).color(p.text3));
    } else {
        for r in &snap.advertised_routes {
            routes = routes.push(text(r.clone()).size(13).font(MONO).color(p.text));
        }
    }

    column![title, card(ident.into(), p), card(settings.into(), p), card(routes.into(), p)]
        .spacing(16)
        .into()
}

fn peer_detail(peer: &PeerView, p: Palette) -> Element<'static, Message> {
    let mut title = row![
        text(peer.name.clone()).size(20).color(p.text),
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
        if peer.os.is_empty() { String::new() } else { format!("  ·  {}", peer.os) }
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
        let color = if addr.contains(':') { p.text2 } else { p.accent };
        info = info.push(kv_row(if addr.contains(':') { "IPv6" } else { "IPv4" }, addr.clone(), color, p));
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

    column![title, sub, actions, card(info.into(), p)].spacing(12).into()
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
    container(Space::new().width(9).height(9)).style(style).into()
}

fn caps(s: String, p: Palette) -> Element<'static, Message> {
    text(s.to_uppercase()).size(10.5).font(MONO).color(p.text3).into()
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
    let (label, color) = if online { ("Connected", p.online) } else { ("Disconnected", p.text2) };
    row![dot(color, online), text(label).size(13).color(color)]
        .spacing(8)
        .align_y(Center)
        .into()
}

/// A `label · mono value · Copy` row (for IPs, MagicDNS).
fn kv_row(label: &str, value: String, value_color: Color, p: Palette) -> Element<'static, Message> {
    row![
        text(label.to_string()).size(12).color(p.text3).width(Length::Fixed(100.0)),
        text(value.clone()).size(12.5).font(MONO).color(value_color),
        Space::new().width(Fill),
        button(text("Copy").size(11).color(p.text2))
            .style(theme::small_btn(p))
            .padding([3, 8])
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
        text(label.to_string()).size(12).color(p.text3).width(Length::Fixed(100.0)),
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

fn watch_stream() -> impl Stream<Item = Message> {
    use iced::futures::channel::mpsc::Sender;
    iced::stream::channel(8, |mut output: Sender<Message>| async move {
        std::thread::spawn(move || {
            let client = Client::default();
            let _ = output.try_send(Message::Snapshot(fetch_gui(&client)));
            let probe = client.clone();
            client.watch_live(move |_live| {
                let _ = output.try_send(Message::Snapshot(fetch_gui(&probe)));
            });
        });
        std::future::pending::<()>().await;
    })
}

/// Runs the GUI. Blocks until the window is closed.
pub fn run() -> Result<()> {
    iced::application(boot, update, view)
        .title("alavai")
        .subscription(subscription)
        .theme(theme)
        .window_size(Size::new(920.0, 640.0))
        .run()
        .map_err(|e| anyhow::anyhow!("run GUI: {e}"))
}
