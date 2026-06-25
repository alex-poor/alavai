# alavai — Architecture Notes

Companion to [PLAN.md](PLAN.md). Captures how the pieces fit and the key
LocalAPI details so future work doesn't have to re-derive them. For how alavai
tracks upstream Tailscale's (unstable) LocalAPI wire format — the version
coupling, golden fixtures, and refresh routine — see [SYNCING.md](SYNCING.md).

## Module layout (target)

```
src/
  main.rs        CLI entry + subcommands (status/tailnets/switch/tray/watch)
  localapi.rs    typed LocalAPI client: blocking request/response + the blocking
                 watch-ipn-bus stream (Client::watch_live → LiveState)
  tray.rs        ksni tray: status icon + one-click tailnet switcher (done)
  gui.rs         iced window: status, switcher, peers, copy, connect (done)
  config.rs      XDG TOML config (tray on/off, pinned tailnets)        [planned]
  notify.rs      desktop notifications wrapper                          [planned]
```

A future split into a reusable `tsclient` crate + the `alavai` app is possible,
but a single crate with modules is lighter and sufficient for now.

## GUI notes (iced)

- Built with the **tiny-skia software renderer** (no wgpu) so it runs on any
  distro/VM/GPU-less setup and ships a smaller binary.
- The window is a separate process (`alavai gui`), launched by the tray. This
  keeps iced's winit event loop on the main thread, free of the tray's threads.
- ksni uses its **async-io** zbus backend (not tokio). iced_winit opens a D-Bus
  session connection, which panics under zbus's tokio backend with no ambient
  runtime; async-io makes zbus run its own executor. As a bonus, the whole
  project no longer depends on tokio.
- The window subscribes to `Client::watch_live` (bridged to an iced
  `Subscription` via `iced::stream::channel`) and updates **incrementally**:
  - bus deltas that don't carry a NetMap (connection state, prefs/toggles,
    login URL) are applied directly to the in-memory `GuiSnapshot` with **no
    refetch** (`Message::Live` → `GuiSnapshot::apply_live`);
  - only when a delta carries a NetMap (peers may have changed) does it do the
    one heavier refresh — `status` + `prefs` + profiles → `Message::Snapshot`.
  - User mutations refresh as needed; toggles/connect rely on the resulting bus
    delta. This keeps the high-frequency event path allocation/round-trip free
    while peers stay fresh on netmap changes (which the daemon rate-limits).

## The LocalAPI

`tailscaled` serves HTTP/1.1 on a unix socket:

- Socket: `/run/tailscale/tailscaled.sock` (fallback `/var/run/tailscale/tailscaled.sock`).
- Required header: `Host: local-tailscaled.sock`.
- Access requires the caller be the Tailscale **operator**
  (`sudo tailscale set --operator=$USER`).

### Endpoints alavai uses

| Purpose | Method | Path |
| --- | --- | --- |
| Status | GET | `/localapi/v0/status` |
| Prefs (read) | GET | `/localapi/v0/prefs` |
| Prefs (edit) | PATCH | `/localapi/v0/prefs` (MaskedPrefs JSON) |
| Start backend | POST | `/localapi/v0/start` |
| Login (interactive) | POST | `/localapi/v0/login-interactive` |
| Logout | POST | `/localapi/v0/logout` |
| Event stream | GET | `/localapi/v0/watch-ipn-bus?mask=...` |
| List profiles | GET | `/localapi/v0/profiles/` |
| Current profile | GET | `/localapi/v0/profiles/current` |
| Add profile | PUT | `/localapi/v0/profiles/` |
| **Switch profile** | POST | `/localapi/v0/profiles/{id}` |
| Delete profile | DELETE | `/localapi/v0/profiles/{id}` |
| File targets | GET | `/localapi/v0/file-targets` |
| Send file | PUT | `/localapi/v0/file-put/{stableID}/{name}` |
| Waiting files | GET | `/localapi/v0/files/` |
| Get / delete file | GET/DELETE | `/localapi/v0/files/{name}` |

netcheck is obtained via the CLI (`tailscale netcheck --format=json`) rather than
the LocalAPI.

### Editing prefs

Tailscale uses "masked prefs": send the changed `Prefs` fields plus a `*Set`
boolean for each field being changed, e.g. to set an exit node:

```json
{ "ExitNodeID": "nodeid", "ExitNodeIDSet": true }
```

Connect/disconnect is `{"WantRunning": true|false, "WantRunningSet": true}`
followed by `POST /start` (changing the control URL similarly needs a restart).

### Response framing

Regular endpoints return a normal body (we read to EOF with `Connection: close`,
dechunking if `Transfer-Encoding: chunked`). `watch-ipn-bus` is a long-lived
chunked stream of newline-delimited JSON `ipn.Notify` objects — read
incrementally by the blocking `Client::watch_live` reader (no async runtime).

## State model (mirrors trayscale's Poller)

Implemented with OS threads + `std::sync::mpsc` channels — no async runtime:

- A **watch thread** runs `Client::watch_live`, a blocking reader of
  `watch-ipn-bus`. It folds delta notifications (State, Prefs, NetMap;
  BrowseToURL for login) into a merged `LiveState` and forwards it on change.
- A **profile-poll thread** ticks every 10s for the profile list, which is *not*
  delivered on the bus.
- A **worker thread** owns the blocking `Client` and the `ksni` handle. It is the
  single consumer of a command channel fed by the menu callbacks, the watch
  thread (`Cmd::Live`), and the poll thread (`Cmd::RefreshProfiles`). It applies
  each command and pushes the updated snapshot to the tray. Keeping all mutation
  on one thread serialises updates and keeps blocking I/O out of `ksni`'s
  update closures (which run on the tray service thread).

The GUI (Phase 3) will subscribe to the same command/update flow.

## Tray (headline feature)

`ksni` exposes a `Tray` trait → menu items. The tailnet switcher renders each
configured profile as a top-level `StandardItem` (or `RadioGroup`) with the
active one marked; clicking calls `switch_profile(id)` and optimistically
updates, then reconciles on the next status event. Icon state:
disconnected / connected / exit-node-active.
