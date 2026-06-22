# alavai

A lightweight, distro-agnostic **Tailscale client for Linux** — a Rust
re-implementation of the core functionality of
[trayscale](https://github.com/DeedleFake/trayscale), with **one-click tailnet
switching** built in from the start.

> Status: **Phase 1.** A working system-tray daemon delivers the headline
> feature — right-click → pick a tailnet → switch instantly — alongside a CLI
> over the Tailscale LocalAPI. The GUI is next; see [docs/PLAN.md](docs/PLAN.md).

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

cargo run -- status      # current connection status + active tailnet
cargo run -- tailnets    # list configured tailnets (● = active)
cargo run -- switch karo # switch by id, name, or domain
```

The tray exposes each configured tailnet as a one-click item, plus
connect/disconnect, refresh, and quit. It needs a StatusNotifierItem host
(KDE, GNOME with the AppIndicator extension, or most tray-capable WMs).

## License

GPL-3.0-or-later — see [LICENSE](LICENSE).
