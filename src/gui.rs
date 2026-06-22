//! The main window (iced, tiny-skia software renderer).
//!
//! A standalone window (`alavai gui`) launched from the tray. It shows the
//! connection state, this machine, a one-click tailnet switcher, and the list
//! of peers — and stays live by rebuilding its snapshot whenever the IPN bus
//! signals a change (the same `watch_live` stream the tray uses).
//!
//! Everything the window shows is derived from the LocalAPI `status` response
//! plus the profile list, so the view is a pure function of one `GuiSnapshot`.

use std::fmt;
use std::process::Command as ProcCommand;

use anyhow::Result;
use iced::futures::Stream;
use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Element, Length, Size, Subscription, Task};

use crate::localapi::{Client, Profile};

const ADMIN_URL: &str = "https://login.tailscale.com/admin";

/// Everything the window renders, rebuilt on each daemon change.
#[derive(Debug, Clone, Default)]
struct GuiSnapshot {
    online: bool,
    exit_node_active: bool,
    machine: String,
    addresses: Vec<String>,
    tailnets: Vec<Profile>,
    current_id: String,
    peers: Vec<PeerView>,
}

#[derive(Debug, Clone)]
struct PeerView {
    name: String,
    online: bool,
    address: String,
    exit_node: bool,
    exit_node_option: bool,
}

/// A tailnet option for the switcher (pick-list needs Display + PartialEq).
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

struct State {
    snap: GuiSnapshot,
    /// True while a switch / connect action is in flight.
    busy: bool,
}

#[derive(Debug, Clone)]
enum Message {
    /// A freshly fetched snapshot (from the bus subscription or an action).
    Snapshot(GuiSnapshot),
    SwitchTailnet(TailnetChoice),
    ToggleConnection,
    Copy(String),
    OpenAdmin,
}

/// Builds the complete snapshot from the daemon (blocking; runs off the UI
/// thread — either on the watch thread or an iced executor task).
fn fetch_gui(client: &Client) -> GuiSnapshot {
    let status = client.status().ok();
    let online = status.as_ref().is_some_and(|s| s.online());
    let (machine, addresses) = match &status {
        Some(s) => (
            s.self_node.as_ref().map(|n| n.hostname.clone()).unwrap_or_default(),
            s.tailscale_ips.clone(),
        ),
        None => (String::new(), Vec::new()),
    };

    let mut peers: Vec<PeerView> = status
        .as_ref()
        .map(|s| {
            s.peers
                .values()
                .map(|p| PeerView {
                    name: if !p.hostname.is_empty() {
                        p.hostname.clone()
                    } else {
                        p.dns_name.trim_end_matches('.').to_string()
                    },
                    online: p.online,
                    address: p.tailscale_ips.first().cloned().unwrap_or_default(),
                    exit_node: p.exit_node,
                    exit_node_option: p.exit_node_option,
                })
                .collect()
        })
        .unwrap_or_default();
    peers.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let exit_node_active = peers.iter().any(|p| p.exit_node);

    let tailnets = client
        .profiles()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect();
    let current_id = client.current_profile().ok().map(|p| p.id).unwrap_or_default();

    GuiSnapshot {
        online,
        exit_node_active,
        machine,
        addresses,
        tailnets,
        current_id,
        peers,
    }
}

fn boot() -> (State, Task<Message>) {
    let snap = fetch_gui(&Client::default());
    (State { snap, busy: false }, Task::none())
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::Snapshot(snap) => {
            state.snap = snap;
            state.busy = false;
            Task::none()
        }
        Message::SwitchTailnet(choice) => {
            if choice.id == state.snap.current_id {
                return Task::none();
            }
            state.busy = true;
            Task::perform(
                async move {
                    let client = Client::default();
                    if let Err(e) = client.switch_profile(&choice.id) {
                        eprintln!("alavai: switch tailnet failed: {e}");
                    }
                    fetch_gui(&client)
                },
                Message::Snapshot,
            )
        }
        Message::ToggleConnection => {
            let online = state.snap.online;
            state.busy = true;
            Task::perform(
                async move {
                    let action = if online { "down" } else { "up" };
                    if let Err(e) = ProcCommand::new("tailscale").arg(action).status() {
                        eprintln!("alavai: `tailscale {action}` failed: {e}");
                    }
                    fetch_gui(&Client::default())
                },
                Message::Snapshot,
            )
        }
        Message::Copy(value) => iced::clipboard::write(value),
        Message::OpenAdmin => {
            let _ = ProcCommand::new("xdg-open").arg(ADMIN_URL).spawn();
            Task::none()
        }
    }
}

fn view(state: &State) -> Element<'_, Message> {
    let snap = &state.snap;

    // --- Header: status + connect/disconnect ---
    let status_text = if snap.online {
        if snap.exit_node_active {
            "● Connected · exit node active"
        } else {
            "● Connected"
        }
    } else {
        "○ Disconnected"
    };
    let toggle_label = if snap.online { "Disconnect" } else { "Connect" };
    let toggle = button(text(toggle_label))
        .on_press_maybe((!state.busy).then_some(Message::ToggleConnection));

    let header = row![
        text("alavai").size(24),
        Space::new().width(Length::Fill),
        toggle,
    ]
    .spacing(12)
    .align_y(iced::Center);

    // --- Tailnet switcher ---
    let choices: Vec<TailnetChoice> = snap
        .tailnets
        .iter()
        .map(|p| TailnetChoice {
            id: p.id.clone(),
            label: p.label(),
        })
        .collect();
    let selected = choices.iter().find(|c| c.id == snap.current_id).cloned();
    let switcher = row![
        text("Tailnet").width(Length::Fixed(80.0)),
        iced::widget::pick_list(choices, selected, Message::SwitchTailnet),
    ]
    .spacing(12)
    .align_y(iced::Center);

    // --- This machine ---
    let mut machine_col = column![text(status_text)].spacing(6);
    if !snap.machine.is_empty() {
        machine_col = machine_col.push(text(snap.machine.clone()).size(18));
    }
    for addr in &snap.addresses {
        machine_col = machine_col.push(copy_row(addr.clone()));
    }
    let machine_card = section("This machine", machine_col.into());

    // --- Peers ---
    let mut peer_col = column![].spacing(4);
    if snap.peers.is_empty() {
        peer_col = peer_col.push(text("No peers.").size(14));
    } else {
        for p in &snap.peers {
            peer_col = peer_col.push(peer_row(p));
        }
    }
    let peers_card = section(
        &format!("Peers ({})", snap.peers.len()),
        scrollable(peer_col).height(Length::Fill).into(),
    );

    let footer = row![
        Space::new().width(Length::Fill),
        button(text("Admin console")).on_press(Message::OpenAdmin),
    ];

    container(
        column![header, switcher, machine_card, peers_card, footer]
            .spacing(16)
            .padding(16),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// A labelled section with a thin frame around its content.
fn section<'a>(title: &str, body: Element<'a, Message>) -> Element<'a, Message> {
    column![
        text(title.to_string()).size(13),
        container(body)
            .padding(12)
            .width(Length::Fill)
            .style(container::rounded_box),
    ]
    .spacing(6)
    .into()
}

/// A row showing a value with a copy-to-clipboard button.
fn copy_row(value: String) -> Element<'static, Message> {
    row![
        text(value.clone()),
        Space::new().width(Length::Fill),
        button(text("Copy").size(12)).on_press(Message::Copy(value)),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn peer_row(p: &PeerView) -> Element<'static, Message> {
    let dot = if p.online { "●" } else { "○" };
    let mut tags = String::new();
    if p.exit_node {
        tags = "  [exit node]".into();
    } else if p.exit_node_option {
        tags = "  [exit option]".into();
    }
    let label = format!("{dot}  {}{tags}", p.name);

    let mut r = row![text(label)].spacing(8).align_y(iced::Center);
    if !p.address.is_empty() {
        r = r.push(Space::new().width(Length::Fill));
        r = r.push(text(p.address.clone()).size(13));
        r = r.push(button(text("Copy").size(12)).on_press(Message::Copy(p.address.clone())));
    }
    r.into()
}

/// Subscription that streams a fresh snapshot whenever the IPN bus changes.
fn subscription(_state: &State) -> Subscription<Message> {
    Subscription::run(watch_stream)
}

fn theme(_state: &State) -> iced::Theme {
    iced::Theme::Dark
}

fn watch_stream() -> impl Stream<Item = Message> {
    use iced::futures::channel::mpsc::Sender;
    iced::stream::channel(8, |mut output: Sender<Message>| async move {
        std::thread::spawn(move || {
            let client = Client::default();
            // Push an initial snapshot, then one per bus change.
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
        .window_size(Size::new(440.0, 660.0))
        .run()
        .map_err(|e| anyhow::anyhow!("run GUI: {e}"))
}
