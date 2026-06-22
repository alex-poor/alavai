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
| Concurrency | OS threads + channels (no async runtime) | the `watch-ipn-bus` stream is a long-lived blocking read on its own thread; this keeps the binary lean and avoids mixing an async runtime with ksni's blocking API. Revisit `tokio` only if a future feature truly needs it. |
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
| Show status (online/offline, self addr) | ✅ CLI + tray + gui | `GET /status` |
| Connect / disconnect | ✅ tray + gui (via `tailscale up/down`) | `EditPrefs{WantRunning}` + `Start` planned |
| Status tray icon (active/inactive/exit-node) | ✅ tray, live (themed; pixmap fallback in P5) | derived from bus state/prefs |
| Live event updates | ✅ | `GET /watch-ipn-bus` (blocking stream thread) |
| Operator-not-set warning dialog | ⬜ | compare prefs operator vs `$USER` |
| Desktop notifications on connect/disconnect | ⬜ | `notify-rust` |

### Tailnet / profile switching (headline)
| trayscale feature | alavai | LocalAPI |
| --- | --- | --- |
| List login profiles | ✅ CLI + tray | `GET /profiles/` |
| Current profile + active indicator | ✅ CLI + tray | `GET /profiles/current` |
| **One-click switch from tray** | ✅ tray | `POST /profiles/{id}` (validated, HTTP 204) |
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
| Per-peer: addresses, online, last seen, created, last handshake, rx/tx | 🟡 gui (name/online/IP); detail page in P4 | `status` peers; netmap/engine for the rest |
| Copy address / FQDN to clipboard | ✅ gui (addresses) | `iced::clipboard` (no extra crate) |

### App / settings
| trayscale feature | alavai | mechanism |
| --- | --- | --- |
| Change control server URL (+ reset to default) | ⬜ | `EditPrefs{ControlURL}` + `Start` |
| Admin console link | ✅ gui | open `https://login.tailscale.com/admin` |
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

### Phase 1 — Tray daemon with one-click tailnet switching  ✅
- `ksni` (blocking API) SNI tray icon + menu — `alavai tray`.
- Radio-group tailnet switcher: one click → `switch_profile`, with optimistic
  active indicator and worker-thread confirmation.
- Connect/disconnect (via `tailscale up/down`), manual refresh, quit.
- Status-driven themed icon (connected / disconnected / exit-node) + tooltip.
- 5s interval refresh (watch stream replaces it in Phase 2).
- Non-blocking menu callbacks (channel → worker thread owning the client).
- **Delivered: the headline feature, usable from the system tray.**

### Phase 2 — Event-driven state via `watch-ipn-bus`  ✅
- `Client::watch_live`: a blocking IPN-bus reader (chunked HTTP, newline-delimited
  JSON) on its own thread — **no async runtime, no new dependencies**.
- Folds delta notifications (State / Prefs / NetMap) into a merged `LiveState`
  and emits only on change.
- Tray is now event-driven: online state, exit-node state, machine and address
  update live; the old 5s full poll is gone. Profiles (not on the bus) still poll
  every 10s.
- Auto-reconnects when the stream drops (e.g. after a profile switch).
- `alavai watch` debug subcommand streams `LiveState` changes.
- Verified live: `[online] diablo 100.69.38.30`.

### Phase 3 — GUI shell (iced)  ✅
- `alavai gui`: an iced window using the **tiny-skia software renderer** (no
  wgpu/GPU dependency) — opened from the tray menu or left-click.
- Shows connection state, this machine (with copy buttons), a one-click tailnet
  switcher (pick-list), and the live peer list (online dots, IPs, exit-node tags).
- Stays live via the same `watch_live` stream, exposed as an iced `Subscription`;
  the view is a pure function of one `GuiSnapshot` rebuilt from `status` + profiles.
- Connect/disconnect, copy-to-clipboard, admin-console link.
- Switched ksni to its **async-io** backend so `zbus` runs its own executor —
  iced_winit also opens a session bus, which panics under the tokio backend. Side
  benefit: **tokio is no longer a dependency at all.**

### Phase 4 — Full feature parity  ⬅ next
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

- **LocalAPI POST/CSRF:** ✅ resolved. A no-op `POST /profiles/{current}`
  returned `HTTP 204` with only `Host: local-tailscaled.sock` — no CSRF
  rejection. Mutating calls work; `EditPrefs` (PATCH) still to be exercised in
  Phase 2.
- **`watch-ipn-bus` framing:** ✅ resolved. Chunked HTTP carrying
  newline-delimited JSON `Notify` deltas; parsed by a blocking stream thread
  (`Client::watch_live`). No async runtime required.
- **netmap richness:** some per-peer fields trayscale reads come from Go-typed
  views; confirm they're all present in the JSON `status`/netmap. May need
  `tailscale status --json` to supplement.
- **iced wgpu on headless/odd GPUs:** ✅ resolved by building iced with the
  `tiny-skia` software renderer only (no wgpu). Verified: window renders correctly
  over Wayland. Also required switching ksni to the `async-io` zbus backend so
  iced_winit's session-bus connection doesn't panic.
- **Mullvad capability detection:** depends on `CapMap`/tags in the JSON status —
  verify the field names.
