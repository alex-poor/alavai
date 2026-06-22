# alavai — Architecture Notes

Companion to [PLAN.md](PLAN.md). Captures how the pieces fit and the key
LocalAPI details so future work doesn't have to re-derive them.

## Module layout (target)

```
src/
  main.rs        CLI entry (Phase 0); will gain `tray`/`gui`/`daemon` subcommands
  localapi.rs    typed LocalAPI client (blocking now; async added in Phase 2)
  state.rs       state engine: watch-bus subscription + interval poll + broadcast
  tray.rs        ksni tray: status icon + one-click tailnet switcher
  config.rs      XDG TOML config (tray on/off, poll interval, pinned tailnets)
  notify.rs      desktop notifications wrapper
  gui/           iced application (main window, pages)
```

A future split into a reusable `tsclient` crate + the `alavai` app is possible,
but a single crate with modules is lighter and sufficient for now.

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
| Add profile | POST | `/localapi/v0/profiles/` |
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
stream of length/newline-delimited JSON `ipn.Notify` objects — handled by the
async client in Phase 2, not the blocking one.

## State model (mirrors trayscale's Poller)

- One task subscribes to `watch-ipn-bus` and converts notifications into state
  deltas (BackendState, Prefs, NetMap, Engine, BrowseToURL).
- A second, slower interval tick provides a safety-net refresh and drives
  profile/waiting-file polling (those aren't on the bus).
- A canonical `AppState` is published via `tokio::sync::watch`; tray and GUI both
  subscribe. Commands from the views go through a command channel so mutations are
  serialised and always reflected back through the bus.

## Tray (headline feature)

`ksni` exposes a `Tray` trait → menu items. The tailnet switcher renders each
configured profile as a top-level `StandardItem` (or `RadioGroup`) with the
active one marked; clicking calls `switch_profile(id)` and optimistically
updates, then reconciles on the next status event. Icon state:
disconnected / connected / exit-node-active.
