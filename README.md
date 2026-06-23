# alavai

A lightweight, distro-agnostic **Tailscale client for Linux** — a Rust
re-implementation of the core functionality of
[trayscale](https://github.com/DeedleFake/trayscale), with **one-click tailnet
switching** built in from the start.

> Status: **Phase 3 + UI redesign (pass 1).** The main window now follows the
> design system in [docs/design](docs/design) — a sidebar + detail layout with
> light/dark tokens, the tailnet switcher and connection status in a persistent
> header, a filterable peer list, this-machine settings wired to the daemon
> (exit-node advertise, accept-routes, LAN access), and a peer detail view. The
> tray delivers the headline one-click tailnet switching, live from the event bus.
> Remaining design screens and feature parity are tracked in [docs/PLAN.md](docs/PLAN.md).

![alavai main window](docs/screenshot.png)

## Goals

- **Feature parity with trayscale** — exit nodes, Mullvad, Taildrop send/receive,
  advertised routes, subnet routes, netcheck diagnostics, control-server URL,
  login, notifications. (Look and workflow need *not* match — only features.)
- **One-click tailnet switching out of the box** — right-click the tray icon,
  pick a configured tailnet, switch instantly.
- **Lightweight and portable** — a single self-contained binary with no GTK/Qt
  system dependencies; runs cleanly on any Linux distro and looks consistent in
  any desktop environment or window manager.

## How it works

alavai talks directly to the local `tailscaled` daemon over its unix-socket
**LocalAPI** (`/run/tailscale/tailscaled.sock`) — no Go, no bundled Tailscale
library. The same prerequisite as trayscale applies: the current user must be
the Tailscale *operator*:

```sh
sudo tailscale set --operator=$USER
```

## Recommended stack

| Concern        | Choice                                  | Why |
| -------------- | --------------------------------------- | --- |
| Tailscale I/O  | custom LocalAPI client over unix socket | no Go runtime, full control, validated |
| Tray           | [`ksni`](https://crates.io/crates/ksni) | pure-Rust StatusNotifierItem; DE/WM-agnostic |
| GUI            | [`iced`](https://iced.rs)               | pure Rust, no system toolkit libs, consistent everywhere |
| Async runtime  | `tokio`                                 | drives the `watch-ipn-bus` event stream |
| Notifications  | `notify-rust`                           | freedesktop notifications, DE-agnostic |
| Config         | `toml` + `directories` (XDG)            | declare/pin tailnets for the tray |

## Usage

```sh
cargo run -- tray        # run the system-tray daemon (one-click tailnet switching)
cargo run -- gui         # open the main window

cargo run -- status      # current connection status + active tailnet
cargo run -- tailnets    # list configured tailnets (● = active)
cargo run -- switch karo # switch by id, name, or domain
cargo run -- watch       # stream live state changes (debug)

cargo run -- prefs       # show exit-node / routes / toggle state
cargo run -- peers       # list peers (online, IP, relay, routes, traffic)
cargo run -- netcheck    # connectivity diagnostics

# mutating (route through the daemon's prefs):
cargo run -- exit-node <peer|none>      # use a peer as exit node
cargo run -- accept-routes <on|off>
cargo run -- lan-access <on|off>        # LAN access while using an exit node
cargo run -- advertise-exit-node <on|off>
cargo run -- advertise-routes 192.168.1.0/24 …   # (omit to clear)
```

The tray exposes each configured tailnet as a one-click item, plus
connect/disconnect, refresh, and quit. It needs a StatusNotifierItem host
(KDE, GNOME with the AppIndicator extension, or most tray-capable WMs).

## License

GPL-3.0-or-later — see [LICENSE](LICENSE).
