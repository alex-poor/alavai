# alavai — Implementation Plan

A Rust re-implementation of [trayscale](https://github.com/DeedleFake/trayscale)
that keeps **all of its features**, makes **one-click tailnet switching** a
first-class tray action, and ships as a **lightweight, distro- and
DE/WM-agnostic** binary.

This document is the roadmap. It is organised as:

1. [Design decisions](#1-design-decisions)
2. [Architecture](#2-architecture) (see also [ARCHITECTURE.md](ARCHITECTURE.md))
3. [Feature parity matrix](#3-feature-parity-matrix) — every trayscale feature, mapped
4. [Phased delivery plan](#4-phased-delivery-plan)
5. [Risks & open questions](#5-risks--open-questions)

---

## 1. Design decisions

| Decision | Choice | Rationale |
| --- | --- | --- |
| Language | Rust 2024 | requested; single static-ish binary |
| Tailscale access | **Custom LocalAPI client** over `tailscaled.sock` | no Go, no bundled `tailscale.com` lib; validated against a live daemon (status + profile list/switch work). Mirrors trayscale's use of `tailscale.com/client/local`. |
| Up/down/login | LocalAPI where possible, shell `tailscale` as fallback | trayscale itself shells out for `up`/`down`; we prefer LocalAPI (`EditPrefs` + `Start`, `login-interactive`) and fall back to the CLI only if needed |
| netcheck | shell `tailscale netcheck --format=json` | LocalAPI does not expose a full netcheck report cleanly; the CLI does. Avoids reimplementing DERP probing. |
| Tray | `ksni` (StatusNotifierItem over D-Bus) | pure Rust; works on KDE, GNOME (+extension), and most WMs; this is where one-click switching lives |
| GUI | `iced` | pure Rust, no GTK/Qt **system** libraries, single binary, consistent look in any DE/WM, software-render fallback for any distro |
| Async | `tokio` | required for the long-lived `watch-ipn-bus` stream and concurrent commands |
| Notifications | `notify-rust` | freedesktop `org.freedesktop.Notifications`; DE-agnostic |
| Config | `toml` + `directories` (XDG) | lets the user pin/order tailnets for the tray; stores prefs (tray on/off, poll interval) without GNOME `gsettings` |

**Why not GTK4 + libadwaita (what trayscale uses)?** The user explicitly does not
need matching look/workflow — only features — and wants a lightweight binary that
runs cleanly on any distro and any DE/WM. GTK4/libadwaita pulls heavy system
dependencies and renders with a distinctly GNOME look. `iced` keeps the binary
self-contained and visually neutral everywhere.

**Alternatives considered for GUI:** `slint` (also pure Rust, lighter binary, strong
software renderer) is a viable fallback if `iced`'s wgpu backend proves
troublesome on exotic GPUs; `egui` is lighter still but looks tool-like. `iced`'s
Elm architecture maps cleanly onto the poller→update event model trayscale
already uses.

---

## 2. Architecture

```
                 ┌──────────────────────────────────────────┐
                 │                  alavai                    │
                 │                                            │
   D-Bus  ◄──────┤  tray (ksni)        gui (iced)            │
 (SNI menu)      │     ▲                   ▲                  │
                 │     │   events / commands │                │
                 │     └─────────┬───────────┘                │
                 │               │                            │
                 │        state engine (tokio)                │
                 │     - holds latest Status/Prefs/Profiles   │
                 │     - broadcasts updates to tray + gui      │
                 │               │                            │
                 │        localapi client (async)             │
                 └───────────────┼────────────────────────────┘
                                 │ HTTP/1.1 over unix socket
                                 ▼
                          tailscaled.sock  (LocalAPI)
```

- **`localapi`** — typed client. Phase 0 is blocking (one request per call);
  Phase 2 adds an async client plus a `watch-ipn-bus` subscriber that yields a
  stream of notifications (state, prefs, netmap, engine, browse-to-URL).
- **state engine** — owns the canonical app state, fed by the watch stream and a
  fallback interval poll (mirrors trayscale's `Poller`). Broadcasts snapshots to
  the tray and GUI via a `tokio::sync::watch`/`broadcast` channel. Serialises
  mutating commands.
- **tray** — renders the SNI menu and, crucially, the **tailnet switcher** as
  top-level one-click items. Reflects live status (icon: connected / disconnected
  / exit-node-active) and the active tailnet (`●`).
- **gui** — the optional main window with full per-peer detail, exit nodes,
  Mullvad, Taildrop, netcheck, routes, settings. Opened from the tray.

Data flows one way (daemon → state → views); views send commands back through the
state engine, which calls the LocalAPI and lets the resulting watch event update
everything. This avoids UI/daemon state drift.

---

## 3. Feature parity matrix

Every trayscale capability, with the alavai plan and the LocalAPI surface it maps
to. ✅ done · 🟡 in progress · ⬜ planned.

### Connection & status
| trayscale feature | alavai | LocalAPI / mechanism |
| --- | --- | --- |
| Show status (online/offline, self addr) | ✅ CLI · ⬜ tray/gui | `GET /status` |
| Connect / disconnect | ⬜ | `EditPrefs{WantRunning}` + `Start`, or `tailscale up/down` |
| Status tray icon (active/inactive/exit-node) | ⬜ | derived from status/prefs |
| Live event updates | ⬜ | `GET /watch-ipn-bus` (streaming) |
| Operator-not-set warning dialog | ⬜ | compare prefs operator vs `$USER` |
| Desktop notifications on connect/disconnect | ⬜ | `notify-rust` |

### Tailnet / profile switching (headline)
| trayscale feature | alavai | LocalAPI |
| --- | --- | --- |
| List login profiles | ✅ CLI | `GET /profiles/` |
| Current profile + active indicator | ✅ CLI | `GET /profiles/current` |
| **One-click switch from tray** | 🟡 CLI done; tray next | `POST /profiles/{id}` |
| Add / log in to a new tailnet | ⬜ | `POST /profiles/` + `login-interactive` |
| Remove / forget a tailnet | ⬜ | `DELETE /profiles/{id}` |
| Interactive login (browse-to-URL) | ⬜ | `login-interactive` + watch `BrowseToURL` |

### Exit nodes
| trayscale feature | alavai | LocalAPI |
| --- | --- | --- |
| Use a peer as exit node | ⬜ | `EditPrefs{ExitNodeID}` |
| Toggle "use exit node" (auto-suggest) | ⬜ | `SetUseExitNode` / suggest endpoint |
| Advertise this machine as exit node | ⬜ | `EditPrefs{AdvertiseRoutes 0.0.0.0/0,::/0}` |
| Allow LAN access while using exit node | ⬜ | `EditPrefs{ExitNodeAllowLANAccess}` |
| Mullvad exit nodes by country/city | ⬜ | filter peers tagged `mullvad-exit-node`; cap `mullvad` |

### Routes
| trayscale feature | alavai | LocalAPI |
| --- | --- | --- |
| Accept subnet routes | ⬜ | `EditPrefs{RouteAll}` |
| Advertise subnet routes (add/remove prefix) | ⬜ | `EditPrefs{AdvertiseRoutes}` |
| View peer's advertised/primary routes | ⬜ | from netmap |

### Taildrop (file transfer)
| trayscale feature | alavai | LocalAPI |
| --- | --- | --- |
| Send file(s)/dir to a peer | ⬜ | `PUT /file-put/{stableID}/{name}` |
| Drag-and-drop / "open with" send | ⬜ | GUI + `file-targets` |
| Receive: list waiting files | ⬜ | `GET /files/` |
| Save / delete incoming file | ⬜ | `GET`/`DELETE /files/{name}` |
| Notify on incoming file | ⬜ | `notify-rust` |

### Diagnostics & per-peer detail
| trayscale feature | alavai | source |
| --- | --- | --- |
| netcheck (UDP, IPv4/6, UPnP/PMP/PCP, captive portal, DERP, latencies) | ⬜ | `tailscale netcheck --format=json` |
| Per-peer: addresses, online, last seen, created, last handshake, rx/tx | ⬜ | netmap + engine status |
| Copy address / FQDN to clipboard | ⬜ | `arboard` crate |

### App / settings
| trayscale feature | alavai | mechanism |
| --- | --- | --- |
| Change control server URL (+ reset to default) | ⬜ | `EditPrefs{ControlURL}` + `Start` |
| Admin console link | ⬜ | open `https://login.tailscale.com/admin` |
| Preferences: toggle tray icon, poll interval | ⬜ | TOML config |
| Hide-window-on-start flag, autostart | ⬜ | CLI flag + `.desktop` autostart |
| About dialog | ⬜ | GUI |
| Quit | ⬜ | tray/gui |

---

## 4. Phased delivery plan

Each phase is independently useful and ends in something runnable.

### Phase 0 — Repo + LocalAPI core + CLI  ✅ (this commit)
- Git repo, license, docs, `iced`/`ksni`-ready `Cargo.toml`.
- Blocking `localapi` client: `status`, `profiles`, `current_profile`,
  `switch_profile`.
- CLI: `alavai status | tailnets | switch <tailnet>`.
- **Proves the headline path in Rust against a live daemon.**

### Phase 1 — Tray daemon with one-click tailnet switching  ⬅ next
- Add `ksni`; render an SNI tray icon with a menu.
- Top-level, one-click tailnet items (radio-style, `●` on active) → `switch_profile`.
- Connect/disconnect, "open window" (stub), quit.
- Status-driven icon (connected / disconnected / exit-node).
- Interval refresh of profile + status (watch stream comes in Phase 2).
- **Deliverable: the headline feature, usable from the system tray.**

### Phase 2 — Async state engine + live events
- Async `localapi` client (tokio + hyper + hyperlocal).
- `watch-ipn-bus` subscriber → typed notification stream.
- State engine broadcasting snapshots; tray becomes event-driven + instant.
- Connect/disconnect, exit-node toggle, notifications on state change.

### Phase 3 — GUI shell (iced)
- Main window opened from tray; self page (addresses, toggles) + peer list.
- Reactive binding to the state engine.
- Copy-to-clipboard, toasts, connect/disconnect, basic exit-node selection.

### Phase 4 — Full feature parity
- Exit nodes (incl. advertise + LAN access), Mullvad picker.
- Advertise/accept routes (add/remove prefixes).
- Taildrop send (dialog + drag-and-drop + "open with") and receive (save/delete).
- netcheck diagnostics panel.
- Add/remove tailnet + interactive login flow; control-server URL.
- Operator check, admin link, preferences, about.

### Phase 5 — Packaging & polish
- `.desktop` file, icon set, autostart entry, `--hide-window`.
- Release profile already size-optimised (`opt-level=z`, LTO, strip).
- Distribution: prebuilt binary + AUR; evaluate Flatpak. CI for build/test/clippy.
- README install docs, screenshots.

---

## 5. Risks & open questions

- **LocalAPI POST/CSRF:** GET works unauthenticated over the socket (validated).
  Mutating calls (`switch_profile`, `EditPrefs`) need confirming against the
  daemon's header checks; the Go client sets only `Host: local-tailscaled.sock`.
  Switch was implemented but intentionally **not** executed during scaffolding to
  avoid disrupting the live tailnet. First Phase-1 task: verify on a test profile.
- **`watch-ipn-bus` framing:** the stream is length-delimited JSON; needs the
  async client (Phase 2). Until then the tray interval-polls.
- **netmap richness:** some per-peer fields trayscale reads come from Go-typed
  views; confirm they're all present in the JSON `status`/netmap. May need
  `tailscale status --json` to supplement.
- **iced wgpu on headless/odd GPUs:** enable the `tiny-skia` software fallback;
  keep `slint` as the escape hatch if needed.
- **Mullvad capability detection:** depends on `CapMap`/tags in the JSON status —
  verify the field names.
