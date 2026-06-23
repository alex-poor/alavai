# Handoff: alavai — UI design pass

## Overview
**alavai** is a lightweight, unofficial **Tailscale client for Linux**, built in Rust with the `iced` toolkit (software-rendered, no GPU dependency). This handoff covers a complete visual-design pass: a from-scratch visual system (type, color tokens for light **and** dark, spacing, status semantics, iconography), the headline **tailnet switcher**, every key screen, the tray surface, and the cross-cutting states.

The headline feature — and the thing to keep most polished — is **one-click tailnet switching** for users juggling multiple Tailscale accounts/networks. Exit-node picking is the second most-used flow.

## About the Design Files
The single file in this bundle — **`alavai Design.dc.html`** — is a **design reference created in HTML**. It is a presentation board (labeled frames on a gray canvas), **not production code to copy directly**.

The implementation target is **Rust + `iced`**, which is **not** HTML/CSS — the UI is a tree of widgets composed in code (rows, columns, containers, with a theme palette). The task is to **recreate the look and behavior described here using `iced`'s widget set and a theme palette**, not to port HTML. Where this doc gives pixel values, treat them as **spacing/sizing/color intent** to translate into widget layout (fill vs fixed sizing, padding, spacing, container styles), exactly as the original handover requested.

Feasible `iced` building blocks this design assumes: text, buttons, icon buttons, checkboxes, **togglers** (on/off switches), radio groups, **pick_list**/**combo_box** (searchable dropdown), text inputs, sliders, **progress bars**, tooltips, rules/dividers, scrollables, tables/grids, rounded "card" containers, SVG/PNG icons, tab-like layouts (buttons + state), and **file drag-and-drop** from the desktop. Native file-picker dialogs would come from a small portable crate (`rfd` / XDG portal).

## Fidelity
**High-fidelity.** Final colors, typography, spacing, component styling, copy, and state designs are all specified. Recreate the UI to match using `iced`'s palette + container styling. The one necessary translation is HTML→widgets; visual values are final.

---

## Design Tokens

### Typography
Bundle two open-source families (no web fonts at runtime — embed them):
- **IBM Plex Sans** — all UI text. Weights used: 400, 500, 600.
- **IBM Plex Mono** — IP addresses, CIDRs, MagicDNS names, server hostnames, section labels, country codes, latency values, and other technical/tabular values.

Type scale (px / weight):
| Role | Size | Weight | Notes |
|---|---|---|---|
| Window/screen title | 20 / 19 | 600 | letter-spacing -0.01em |
| Section heading (popovers) | 14–16 | 600 | |
| Item title (peer name, setting) | 13–14 | 600 | |
| Body / secondary | 12.5–13 | 400/500 | |
| Mono value (IP, DNS, CIDR) | 12.5 | 400 | Plex Mono |
| Sub-label / meta | 11–12 | 400 | secondary/muted color |
| Section label (caps) | 10.5–11 | 600 | Plex Mono, letter-spacing 0.1em, UPPERCASE |
| Badge / micro | 8.5–10 | 600 | UPPERCASE for status badges |

Utility density: base body text is **13px**. Never smaller than ~10.5px (caps labels only).

### Color — Dark theme (default)
| Token | Hex | Use |
|---|---|---|
| `bg` | `#14181E` | Window base / detail pane |
| `surface` | `#1B212A` | Sidebar, cards |
| `raised` | `#232B36` | Inputs, hover, secondary buttons, chips |
| `overlay` | `#262F3B` | (popover base; popovers use `surface` #1B212A in practice) |
| `line` | `#2C3542` | Borders. Hairline dividers: `#232B36` |
| `text` | `#E9EDF3` | Primary text |
| `text-2` | `#98A2B3` | Secondary text |
| `text-3` | `#6A7485` | Muted / placeholder |
| `accent` | `#4D8DF5` | Primary, active tailnet, links, focus |
| `accent-bg` | `rgba(77,141,245,0.13)` | Selected row tint |
| `online` | `#34D399` | Online / connected |
| `offline` | `#5B6573` (border `#5B6573`) | Offline (hollow dot) |
| `exit` | `#A78BFA` | Exit-node active (violet) |
| `exit-bg` | `rgba(167,139,250,0.16)` | Exit badge/tint |
| `warn` | `#FBBF24` | Warning, pending approval |
| `danger` | `#F87171` | Errors, disconnect, destructive |

### Color — Light theme
| Token | Hex | Use |
|---|---|---|
| `bg` | `#F4F6F9` | Window base / detail pane |
| `surface` | `#FFFFFF` | Sidebar, cards |
| `raised` | `#EEF1F5` | Inputs, hover |
| `line` | `#E4E8EE` | Borders |
| `text` | `#161B22` | Primary text |
| `text-2` | `#59616E` | Secondary text |
| `text-3` | `#899099` | Muted / placeholder |
| `accent` | `#2563EB` | Primary, active tailnet, links |
| `accent-bg` | `rgba(37,99,235,0.10)` | Selected row tint |
| `online` | `#059669` | Online / connected |
| `offline` | `#AAB1BB` | Offline (hollow dot) |
| `exit` | `#7C3AED` | Exit-node active |
| `warn` | `#B45309` | Warning |
| `danger` | `#DC2626` | Errors, destructive |

> Ship a manual light/dark toggle in Preferences. (Auto-detect is possible but currently disabled for a technical reason — manual is the safe path.)

### Status semantics (icons/dots)
- **Online / connected** → filled dot, `online` color.
- **Offline** → hollow dot (1.5px ring in `offline` color), name in secondary text + "last seen".
- **Exit node active** → `exit` (violet). When an exit node is active, the **header status region shifts to the exit accent** so the changed location is unmissable.
- **Warning / needs action** → `warn` (amber).
- **Error / failed** → `danger` (red).

### Spacing scale
4px base: **4 · 8 · 12 · 16 · 20 · 24 · 32**. Card padding typically 13–18px; row padding 7–10px vertical.

### Radius
- Controls / chips / inputs: **6–7px**
- Cards: **8–10px**
- Window / popover / modal: **10–12px**
- Status dots, avatars-as-circles, toggles: **pill / 50%**

### Shadows
- Popovers/menus: `0 18px 44px rgba(0,0,0,0.45)` (dark)
- Modals: `0 22px 50px rgba(0,0,0,0.42)` (dark)
- Toasts: `0 10px 30px rgba(0,0,0,0.35)`
- Window (on board): `0 24px 60px rgba(0,0,0,0.28)` dark / `0 24px 60px rgba(40,50,70,0.16)` light
- In `iced`, drop shadows are limited — approximate with a subtle border + the overlay sitting above a dimmed scrim where needed.

### Component specs
- **Toggler:** 38×22px track, 11px radius; knob 16px circle, 3px inset. ON = `accent` (or `online` for "advertise") track, knob white, knob right. OFF = `raised`/`#2C3542` track, knob `#8A93A3`, knob left. Disabled = reduced opacity.
- **Primary button:** `accent` fill, white text, 600, radius 7–9px, padding ~9×14px.
- **Secondary button:** `raised` fill, `line` border, secondary text, 500.
- **Ghost/text button:** transparent, `accent` or secondary text.
- **Copy button:** small icon button, `line` border, "Copy" label optional; on click → success toast.
- **Chip/badge:** radius 4–5px, padding 2–8px, UPPERCASE for status (`exit`, `Headscale`, `add-on`).
- **Status dot:** 7–9px circle.
- **Selected list row:** `accent-bg` fill + a 3px `accent` bar on the left edge (radius 0 3px 3px 0).

---

## Information Architecture (the central decision)

**Recommendation: Sidebar + detail**, with a **persistent header** (spanning both columns) that always shows the **tailnet switcher** and **connection status + connect/disconnect**. Rationale: scales to many peers (25+), gives exit-nodes / files / settings first-class homes, and is familiar. It **degrades to a single column** in narrow/tiling windows (sidebar becomes the whole view; selecting an item pushes a detail view over it).

Layout:
- **Header (~56px, full width):** app mark + wordmark · **tailnet switcher chip** · flex spacer · **connection status pill** · **Connect/Disconnect** button.
- **Sidebar (~248px):** filter input · pinned **This machine** row · `PEERS · N` label · scrollable peer list · footer toolbar (Exit node · Incoming files w/ badge · Settings).
- **Detail pane (fills):** content for the selected item (This machine, a peer, etc.), scrollable.

---

## Screens / Views

### 1. Overview — This machine selected (hero, dark) — board row B
- **Purpose:** at-a-glance status + the most-used controls; default landing.
- **Header:** mark (26px rounded `surface` tile) + "alavai" (14/600). **Switcher chip**: `surface` bg, `line` border, radius 9; contains a 26px `accent` square avatar with the tailnet initial (white, 13/600), a two-line block (tailnet name 13/600 + login email 10.5 mono `text-3`), and a down chevron. Right side: status pill (`online`-tinted bg `rgba(52,211,153,0.12)`, dot with a 3px soft ring, "Connected" 12.5/500 `online`) and a **Disconnect** secondary button.
- **Sidebar:** filter input ("Filter peers", search icon). **This machine** row is selected (accent-bg + left bar): monitor icon, "diablo" 13.5/600, "This machine" 11 `text-3`, trailing online dot. `PEERS · 25` caps label. Peer rows: leading status dot (filled online / hollow offline), name (offline names in `text-2`), trailing mono IP fragment (e.g. `…130.96`) or an `EXIT` badge (violet). Footer toolbar: "Exit node" (globe) pill-button, Incoming files (inbox icon + `2` accent badge), Settings (sliders icon).
- **Detail pane (This machine):** title "This machine" 20/600 + OS chip ("Linux · amd64", `raised` bg). 
  - **Identity card** (`surface`, `line`, radius 10): "Connected" row (online dot + label); then rows of `label (108px, text-3) · mono value · Copy button`: **MagicDNS** `diablo.tail9c2.ts.net`, **IPv4** `100.69.38.30` (accent color), **IPv6** `fd7a:115c:a1e0:…:7e37:261e` (truncated, text-2). Hairline dividers between.
  - **Settings group** card with togglers: **Advertise as exit node** (off; sub "Offer this device to route others' traffic. Needs admin approval."), **Accept subnet routes** (on; sub mentions `192.168.1.0/24`).
  - **Advertised routes** card: existing route row `192.168.1.0/24` with a `Pending approval` amber chip + remove (×); a dashed placeholder row `10.0.0.0/24` + an **Add route** button (accent-tinted).

### 2. Overview — peer selected (light) — board row C
Same shell in the **light** palette, with a **peer** selected to show peer detail:
- Title = peer name "karo-tailscale-01" + online dot + **Exit node** chip (violet-tinted). Subline mono `karo-tailscale-01.tail9c2.ts.net · Linux`.
- **Action row:** primary **Use as exit node** (globe icon, accent fill) + secondary **Send files** (upload icon).
- **Detail card** rows: IPv4 `100.121.21.28` + copy; Routes `0.0.0.0/0 · ::/0`; Connection `Direct · DERP syd`; Last seen `Active now`.

### 3. Narrow / tiling window — board row C (right)
~340px wide. Header compresses: mark + switcher chip (avatar + name + chevron, no email) + a bare status dot. Filter, pinned This machine, peer list (rows gain a trailing chevron to indicate drill-in). Footer toolbar becomes full-width buttons. Selecting a peer pushes a detail view over the list (back navigation).

### 4. Exit-node picker popover — board row D
Drops from the "Exit node" button. `surface`/`line`, radius 11, popover shadow.
- Header: "Exit node" 14/600 + a search input ("Search peers & locations").
- **None** (selected; accent-bg, accent check-circle icon; "Use your own internet connection"). **Automatic** (sun-rays icon; "Pick the best available node").
- `YOUR PEERS` label → eligible peer rows (online dot + name + DERP region e.g. `syd`).
- `MULLVAD` label + `add-on` violet chip → country rows: **country-code chip** (mono, e.g. `AU`, `US`) + country name + server count + chevron. (See note on flags below.)
- Footer: **Allow LAN access** toggler ("Keep reaching printers & NAS"), only meaningful while an exit node is active.

### 5. Mullvad locations (expanded, searchable) — board row D
Header with a back chevron + "Mullvad locations" + a focused search input (accent border) showing query `syd`. Country row **Australia** expanded (down chevron, `12 servers`); nested city rows with a location-pin icon + city name + mono server id (`au-syd-wg-301`); selected city (Sydney) has exit-tinted bg + an accent/exit check. Collapsed countries (Singapore) show a right chevron. Hidden entirely when the user isn't entitled.

### 6. Active exit-node state — board row D
- Compact card: 30px exit-tinted tile with a globe (violet), "EXIT NODE ACTIVE" 11/600 violet caps + "Sydney · Mullvad" 13.5/600, a **Disable** secondary button.
- "Advertise as exit node" card with the toggler ON (`online` track) and an amber inline note "Awaiting approval in the admin console".

### 7. Taildrop — send & receive — board row E
- **Drag-to-send:** while files are dragged over the window, the header shows "2 files · 18.4 MB"; eligible peers are normal rows, the **hovered target** gets an accent dashed border + accent-tinted bg + "Drop to send 2 files" + a plus glyph; **offline/ineligible** peers dim to ~45% opacity and show "offline".
- **Send progress:** card with a destination header ("Sending to macbook-air", "2 files · 11.2 of 18.4 MB"), per-file progress bars (done = `online`, in-progress = `accent`, with %), **Cancel** button. Completion fires a desktop notification.
- **Incoming inbox** (the sidebar badge target): header "Incoming files" + count badge. Each row: file-type icon tile, name (ellipsized) + "from <peer> · <size>", **Save** primary button, delete (trash) icon. Footer: "Saving to ~/Downloads" + "Save all". 
- **Empty state:** inbox icon tile, "No incoming files", "Files sent to diablo from your other devices will appear here."

### 8. Onboarding & access — board row F
- **Operator-not-set warning (A3):** full explainer card (amber-tinted header strip with warning triangle, "alavai can't control Tailscale yet", one-line reason) + a code block `$ sudo tailscale set --operator=$USER` with a **Copy** button + **Recheck** primary + **Learn more** ghost. Also a **compact banner** variant for the top of the window: "No operator permission — most actions are disabled." + **Fix**.
- **Add tailnet / login (B2):** modal, dimmed scrim. Spinner-ring + link icon, "Finish signing in", explainer ("We opened your browser to authenticate… the new tailnet will appear and become active."), a truncated login URL `login.tailscale.com/a/3f9c…` + **Reopen**, "Waiting for authentication…" with a pulsing dot, a collapsible **Use a custom control server** (Headscale, I1), and **Cancel**.
- **Logged out / login required (whole window):** centered 54px app mark, "Welcome to alavai", "Log in to a Tailscale account to see your devices and start switching tailnets.", primary **Log in to Tailscale** (link icon), and "or connect a custom control server".

### 9. The tailnet switcher (the headline) — board row G
- **Popover:** opens from the header chip (chip border becomes accent + chevron flips). Header `TAILNETS` caps label + **Edit**. Each account row: 30px rounded avatar (per-account color: blue/green/violet) with initial, name 13.5/600 + login email (mono, text-3), and a trailing **radio** (active = accent check-circle; others = hollow ring). Custom-server accounts show a `Headscale` violet chip next to the name. Footer: **Add tailnet** row (plus tile, accent text).
- **Manage / Edit mode:** header "MANAGE TAILNETS" + **Done**. Active account labeled "active now". Other accounts show a **Remove** danger-outline button. Removing reveals an **inline confirm** (danger-tinted block): "Forget <name>? You'll need to sign in again to use it." + **Remove** (danger fill) / **Keep**.
- **Switching transition:** header stays; switcher avatar+name already shows the target account; status pill reads "Switching" (accent). Body shows a centered spinner + "Switching to <name>…". Everything drops then repopulates.

### 10. Tray menu & status icons — board row H
Tray is plain text + icons + separators + checkable/radio + submenus (no rich layout).
- **Main menu:** disabled header `diablo — karo.co.nz` · sep · **Open window** · sep · **radio list of tailnets** (active has a filled accent dot) · sep · **Exit node ▸** submenu (trailing "None" + chevron) · **Disconnect** · sep · **Incoming files** (count badge) · **Refresh** · sep · **Quit alavai**.
- **Exit-node submenu:** **None** (✓) · **Automatic** · `PEERS` (eligible peers) · **Mullvad ▸**.
- **Tray status icons** (symbolic, monochrome, drawn in `currentColor`, legible at 18px on light **and** dark panels): the brand mark is a **triangular 3-node mesh**.
  - **Connected** → filled nodes + solid connecting lines.
  - **Disconnected** → hollow (outline) nodes + faint dashed lines.
  - **Exit active** → filled mesh + a small "+"/arrow badge knockout at top-right.
  - **Attention** → filled mesh + a small "!" badge knockout.
  Ship these as monochrome symbolic SVG/PNG so panels can recolor them.
- **App icon:** rounded-square (radius ~17px at 72px) with a dark blue-charcoal gradient (`#1B2330`→`#11141A`) and the 3-node mark (blue `#4D8DF5`, green `#34D399`, white nodes; muted `#5A6678` lines). Provided at 72 / 48 / 32px.

### 11. Diagnostics & system states — board row I
- **Netcheck (H1):** card. Header (activity icon + "Netcheck") + **Run again**. Pass/fail rows, each: status icon (green check-circle / red ×-circle / neutral) + label + value/detail: **UDP** Working, **IPv4** `203.0.113.42`, **IPv6** Not available, **NAT traversal** UPnP, **Captive portal** None. Footer strip: **Preferred relay** "Sydney `12 ms`" + expandable "Latency by region" → per-region bars (`syd 12ms`, `mel 24ms`, `sin 88ms`; nearest in accent, others muted). **Running state:** spinner + "Testing connectivity…".
- **Daemon down:** red-tinted broken-wifi icon, "Can't reach Tailscale", "The `tailscaled` service doesn't seem to be running.", **Retry**.
- **No peers:** monitor icon, "Just this machine, for now", "Add another device to your tailnet and it'll show up here."
- **Toasts (transient, bottom of window, `raised` bg):** "Copied `100.69.38.30`" (green check) · "Now on **personal**" (green check) · "Couldn't switch tailnet" (red ×, **Retry**) · "2 files received from **pixel-7**" (accent, **View**). Mirror these as OS desktop notifications where appropriate.

### 12. Iconography & microcopy — board row J
- **Icon set** (one coherent symbolic set, 1.6px stroke, 24px grid, bundled SVG): This machine (monitor), Peer (laptop), Exit node (globe), Routes (sitemap), Send (upload-to-tray), Incoming (inbox), Copy, Search, Add (+), Settings (sliders), Refresh, Admin (external-link), Netcheck (activity), Switch (swap arrows), Warning (triangle), Remove (trash), Check, Chevron, Quit (logout), Location (pin).
- **Microcopy** (final copy — use verbatim as sub-labels/tooltips):
  - **Use as exit node** — "Send all of this device's internet traffic through the one you pick."
  - **Allow LAN access** — "Keep reaching your local printer and NAS while an exit node is on."
  - **Advertise as exit node** — "Offer this device so others can route through it. An admin approves it."
  - **Accept subnet routes** — "Reach the LANs that other devices share, like a home or office network."
  - **Operator permission** — "Lets your Linux user control Tailscale, so alavai can act for you."
  - **Quit vs close** — "Closing keeps alavai in the tray. Quit stops it watching the tailnet."

---

## Interactions & Behavior
- **Tailnet switch:** instant on select; the whole view (peers, IPs, status) repopulates. Show the **Switching** transitional state; disable controls while in flight. Reachable from both the header switcher and the tray radio list.
- **Connect/Disconnect:** toggles the connection. States: connected, disconnected, **connecting** (brief), error. When disconnected, the action becomes a prominent **Connect** (accent fill); when connected, **Disconnect** (secondary).
- **Exit node:** single selection (None / Automatic / a peer / a Mullvad city). Selecting changes effective location — confirm prominently via the header accent shift + a toast. "Allow LAN access" only matters while active.
- **Copy actions** (IPs, names, FQDN, the operator command): always confirm with a transient success toast.
- **Destructive actions** (remove/forget tailnet, delete incoming file): always an inline confirm.
- **Taildrop drag-drop:** the toolkit reports file hover/drop on the window; highlight the hovered peer as a drop target; only enable eligible (online, capable) peers.
- **Async feedback:** actions hit a local daemon — usually fast but not instant. Always show busy/disabled states; never let the UI look frozen.
- **Search/filter:** the peer list and especially Mullvad locations can be long — provide a filter box (the Mullvad list filters across countries + cities).
- **Window vs tray:** closing the window keeps the tray running; **Quit** exits entirely — make the distinction explicit.

## State Management (data the UI needs)
- **Profiles/accounts:** list of `{ display name, tailnet domain, login email, control-server URL, color }` + which is active. Drives the switcher and tray radio.
- **Self node:** name, MagicDNS/FQDN, all Tailscale IPs, online/connected status, OS; toggles: advertise-exit-node, accept-routes; advertised routes (CIDR list, each with approval status).
- **Peers:** per peer — name + MagicDNS, OS, online/offline, all IPs, is-exit-node / exit-option, advertised routes, last seen, created, last handshake, bytes sent/received, DERP region, connection type (direct vs relayed).
- **Exit node:** which peers/Mullvad locations are eligible; current selection; allow-LAN flag. Mullvad entitlement flag (hide the whole section when false).
- **Taildrop:** outgoing transfers (per-file progress); incoming waiting files (name, size, sender).
- **Diagnostics:** last netcheck result + running flag.
- **App/system:** operator-permission status (detect on launch); daemon reachable flag; logged-in/login-required; preferences (theme light/dark, tray visibility, autostart); pending login URL.
- **Transient:** toasts queue; busy/in-flight flags per action.

## Cross-cutting states to implement (don't skip)
Logged out / login required · `tailscaled` not running · **no operator permission** (likely first hit) · connecting/switching · busy/action-in-flight (disable controls) · empty states (no peers, no exit nodes, no incoming files, single tailnet) · errors (toast vs inline) · entitlement-gated (Mullvad hidden when unavailable).

## Notes & gotchas
- **No flag emoji.** Flag emoji frequently don't render on Linux desktops. Use the **two-letter country code in a mono chip** (e.g. `AU`, `US`, `SG`) for Mullvad countries — this is the intended design, not a placeholder.
- **Distro-agnostic:** do **not** mimic any one desktop (GNOME/KDE/etc.). The look is self-consistent and should feel at home everywhere. trayscale (GNOME/Adwaita) is a **feature reference only**.
- **Theme:** implement both palettes; expose a manual light/dark toggle in Preferences (auto-detect is intentionally deferred).
- Translate all px values to `iced` spacing/sizing intent; prefer composing standard widgets over custom canvas drawing.

## Assets
- **Fonts:** IBM Plex Sans + IBM Plex Mono (OFL, embed/bundle).
- **Icons:** the symbolic set in row J — recreate as bundled SVG/PNG (no icon-font/CDN dependency). All single-color, drawable in `currentColor`.
- **App icon + tray status icons:** specified in row H; produce as SVG masters + exported PNG sizes (tray: monochrome symbolic for panel recoloring).
- No photographic or third-party brand assets are used.

## Files
- `alavai Design.dc.html` — the full design board (all screens/states above). Open in a browser to inspect any frame; values in this README are authoritative.
- `screenshots/` — rendered reference images of the board (see below).
