# alavai — UI Design Handover

**Purpose.** alavai's functional plumbing is well ahead of its visual design. This
document hands the UI off for a proper design pass *before* we build out the
remaining features. It explains what alavai is, what already exists, the data and
toolkit you have to work with, and — feature by feature — what each remaining
capability does, how a user would use it, and where it might live in the UI.

Read alongside:
- [PLAN.md](PLAN.md) — roadmap + feature-parity matrix (status of each item).
- [ARCHITECTURE.md](ARCHITECTURE.md) — how the app is wired.
- [screenshot.png](screenshot.png) — the current window.

---

## 1. What alavai is

A **lightweight Tailscale client for Linux**. Tailscale is a mesh VPN: it puts all
your devices (and shared ones) on a private network ("tailnet") where they can
reach each other by name/IP regardless of physical location. There is no official
Linux GUI; alavai is an unofficial, native Rust one. It takes its feature set as
a reference from an existing GTK/Go app, trayscale.

Design pillars, in priority order:
1. **One-click tailnet switching** is the headline feature and must stay
   front-and-centre. (A user with multiple Tailscale accounts/networks switches
   between them constantly.)
2. **Lightweight & universal** — must look good and run cleanly on *any* Linux
   desktop (GNOME, KDE, tiling WMs) and *any* distro. We do **not** need to match
   any particular desktop's look; we need a clean, self-consistent look that feels
   at home everywhere.
3. **Feature parity** with trayscale — everything it does, alavai should do.

There are **two UI surfaces**:
- A **system-tray icon + menu** (always running; the fast path).
- A **main window** (opened on demand for richer interaction).

---

## 2. Tailscale concepts (glossary for the designer)

You don't need to be a Tailscale expert, but these terms drive the UI:

- **Tailnet / profile / account** — one Tailscale network you're logged into.
  A user can have several (e.g. personal + two work orgs) and switch the active
  one. In the API these are "login profiles". *This is the headline feature.*
- **Node / peer** — a device on the tailnet. "Self" is this machine; "peers" are
  all the others.
- **Exit node** — a peer that routes *all* your internet traffic (like a full
  VPN). You pick one to use; you can also offer this machine as one.
- **Subnet routes** — a peer can advertise a physical LAN (e.g. `192.168.1.0/24`)
  so others can reach devices behind it. You choose whether to "accept" routes.
- **Taildrop** — send files directly to another of your devices, peer-to-peer.
- **MagicDNS name** — a friendly hostname for each device (e.g. `diablo.tailnet.ts.net`).
- **Mullvad exit nodes** — if your tailnet has the Mullvad VPN add-on, hundreds of
  Mullvad servers appear as exit nodes, organised by country/city.
- **Operator** — Tailscale must be told the current Linux user may control it
  (`sudo tailscale set --operator=$USER`). Without it, alavai can't do much.

---

## 3. Current implementation state

### 3.1 Surfaces that exist today

**Tray menu** (right-click the icon):
```
  <machine — tailnet>        (header, disabled)
  ─────────────
  Open window
  ─────────────
  ( ) tailnet A              ← radio group, one-click switch
  (•) tailnet B
  ─────────────
  Connect / Disconnect
  Refresh
  ─────────────
  Quit
```
Left-clicking the icon opens the main window. The icon changes with state
(connected / disconnected / exit-node-active). Today the icons are generic themed
names and the menu is plain — both are open to design.

**Main window** (see [screenshot.png](screenshot.png)) — a single scrolling column:
```
┌─────────────────────────────────────────────┐
│ alavai                          [Disconnect] │
│ Tailnet  [ karo.co.nz ▼ ]                    │
│ This machine ─────────────────────────────── │
│   ● Connected                                 │
│   diablo                                       │
│   100.69.38.30                     [Copy]     │
│   fd7a:115c:…:7e37:261e            [Copy]     │
│ Peers (25) ───────────────────────────────── │
│   ● ai-modeller2          100.110.130.96 [Copy]│
│   ○ airflow-dev           100.74.164.94  [Copy]│
│   ● karo-tailscale-01     100.121.21.28  [Copy]│
│     [exit option]                              │
│   … (scrolls)                                  │
│ ────────────────────────────  [Admin console] │
└─────────────────────────────────────────────┘
```
`●` = online, `○` = offline. The whole window is **live** — it updates instantly
when anything changes on the tailnet. It is currently a hardcoded **dark** theme.

### 3.2 What works
Tailnet switching (tray + window), connect/disconnect, live status, this-machine
info with copy, peer list with copy, admin-console link. That's the end of the
current feature set — **everything in §5 below is still to be designed and built.**

---

## 4. The toolkit & what's feasible (important constraints)

alavai's GUI is built in **Rust with the `iced` toolkit**, software-rendered
(no GPU dependency). This is **not** HTML/CSS — the UI is a tree of widgets laid
out in code. That shapes what's cheap vs expensive to build:

**Readily available widgets** (use these freely): text, buttons, icon buttons,
checkboxes, **togglers** (on/off switches), radio groups, **pick_list**
(dropdown), **combo_box** (searchable dropdown — great for long peer/exit-node
lists), text inputs, sliders, **progress bars**, **tooltips**, rules/dividers,
scrollable areas, **tables**, **grids**, cards/containers with rounded
backgrounds, SVG/PNG icons, tab-like layouts (built from buttons + state),
resizable split panes, and **drag-and-drop of files from the desktop** (the
toolkit reports files hovered/dropped on the window — important for Taildrop).

**Things to keep in mind:**
- **Layout is flexbox-like** (rows, columns, spacing, padding, alignment, fill vs
  fixed sizing). Think in terms of stacked rows/columns and cards, not absolute
  positioning.
- **Theming** is via a palette (background, text, primary/accent, success, danger,
  etc.). Custom per-widget colours and rounded "cards" are easy. Arbitrary
  pixel-perfect custom drawing is possible (a canvas widget) but heavier — prefer
  composing standard widgets.
- **Native OS file-picker dialogs** aren't built in; we'd add a small portable
  crate (`rfd`, uses the XDG portal) for "choose files to send".
- **Notifications** are OS-level desktop notifications (via the notification
  daemon), shown outside the window. We can *also* do in-window "toasts" for
  transient feedback ("Copied to clipboard").
- The window is a **separate process** launched from the tray; it can be opened,
  closed, and reopened. Plan for it to be opened fresh fairly often.
- **No web fonts / arbitrary CSS** — but we can bundle a font and an icon set.

**Deliverables that suit this best:** annotated wireframes / layout specs,
component inventory, spacing & colour tokens, state diagrams, iconography, and
copy (microcopy/labels). Pixel-exact mockups are welcome but will be *translated*
into widget layouts, so call out spacing/sizing intent explicitly.

---

## 5. Feature catalogue — what to design

Each item: **what it does**, **how the user uses it**, **data available**, **UI
notes / states**. Items marked ✅ exist (may still want redesign); the rest are
to-build. This is the full target feature set.

### A. Connection & status

**A1. Connect / disconnect** ✅
- *What*: turn the tailnet connection on/off for this machine.
- *Use*: one toggle/button. Primary action.
- *Data*: online/offline; "connecting" is transient.
- *States*: connected, disconnected, connecting (brief), error.

**A2. Connection status display** ✅
- *What*: show whether you're on the tailnet, your machine name, and your
  Tailscale IPs.
- *UI*: prominent status indicator (colour + label). This + tailnet switching is
  the "at a glance" core.

**A3. Operator-not-set warning**
- *What*: if the Linux user isn't the Tailscale "operator", most actions fail.
- *Use*: detect on launch; show a clear, friendly explainer with the exact command
  to run (`sudo tailscale set --operator=$USER`) and a copy button.
- *UI*: a dismissible banner or first-run dialog. Important for onboarding.

### B. Tailnets / accounts (the headline)

**B1. Switch tailnet** ✅
- *What*: change which Tailscale account/network is active. The whole app's view
  (peers, IPs) changes to that tailnet.
- *Use*: pick from a list; switch is instant. Must be effortless from both the
  tray (already top-of-menu) and the window.
- *Data*: list of profiles (display name, domain, login email, control server);
  which is active.
- *UI notes*: Consider showing the account/email and tailnet domain, not just a
  name, so multi-account users can tell them apart. This deserves the most polish.

**B2. Add / log in to a new tailnet**
- *What*: authenticate a new account/network.
- *Use*: click "Add tailnet" → the app starts a login → opens the user's browser
  to authenticate → on success the new tailnet appears and becomes active.
- *Data*: a login URL the daemon provides ("browse to URL"); optionally a custom
  control-server URL for self-hosted setups.
- *UI*: an "Add tailnet" affordance near the switcher; a waiting/confirm state
  ("Opened your browser — finish signing in there"). Handle the "login required"
  state on launch too (when not authenticated at all).

**B3. Remove / forget a tailnet**
- *What*: delete a stored profile.
- *Use*: per-tailnet "remove", with a confirmation.
- *UI*: in a tailnet-management list/menu (e.g. an "edit" affordance on the
  switcher, or a small accounts screen).

### C. Exit nodes

**C1. Use a peer as exit node**
- *What*: route *all* this machine's internet traffic through a chosen peer.
- *Use*: from the list of exit-node-capable peers, select one (or "None"). Only
  one active at a time. There's also an "auto-pick the best one" option.
- *Data*: which peers offer exit-node service; which (if any) is currently active;
  whether LAN access is allowed while using it.
- *UI*: a dedicated "Exit node" control — a dropdown/menu of eligible peers with a
  clear "None" and maybe "Automatic". Show the active exit node prominently
  (it changes your effective location/IP, so users want to confirm it). This is a
  frequently-used feature and a candidate for the tray menu too.

**C2. Allow LAN access while using an exit node**
- *What*: keep access to your *local* network (printer, NAS) even while all
  traffic goes through the exit node.
- *Use*: a single on/off toggle, only relevant when an exit node is active.
- *UI*: a sub-option under the exit-node control.

**C3. Advertise this machine as an exit node**
- *What*: offer *this* device as an exit node for others on the tailnet.
- *Use*: one toggle. (Note: needs admin approval server-side; we just set the
  flag.)
- *UI*: in "this machine" settings.

**C4. Mullvad exit nodes**
- *What*: if the tailnet has the Mullvad add-on, hundreds of Mullvad servers are
  available as exit nodes, grouped by **country → city**.
- *Use*: browse by country, expand to cities, pick one. Only shown if the user is
  entitled.
- *UI*: a separate, searchable, grouped list (lots of entries — use search /
  collapsible country groups). Show the currently selected location. Hidden
  entirely when not entitled.

### D. Routes (subnets)

**D1. Accept subnet routes**
- *What*: use the LAN subnets that other peers advertise (so you can reach devices
  behind them).
- *Use*: one on/off toggle.
- *UI*: a toggle in "this machine" settings, with a one-line explanation.

**D2. Advertise subnet routes**
- *What*: share a physical subnet *you* can reach with the rest of the tailnet.
- *Use*: add/remove CIDR entries (e.g. `192.168.1.0/24`). Add via a small input
  with validation; remove via per-row delete.
- *UI*: an editable list with an "Add route" input. Needs inline validation
  (invalid CIDR feedback). (Also needs admin approval server-side.)

**D3. View a peer's routes**
- *What*: see which subnets a given peer advertises/serves.
- *UI*: read-only list on the peer's detail view.

### E. Taildrop (file transfer)

**E1. Send file(s) / folder to a peer**
- *What*: send files directly to one of your devices.
- *Use*: from a peer, choose "Send files…" → OS file picker → confirm. Multiple
  files/folders allowed.
- *UI*: a "Send" action on each (eligible) peer; a picker; progress/notification
  feedback. Not all peers can receive (depends on capability) — only show/enable
  where valid.

**E2. Drag-and-drop to send**
- *What*: drag files from the file manager onto a peer to send them.
- *Use*: drag over the window → peers highlight as drop targets → drop on one.
- *UI*: design the drop-target affordance (highlight, "Drop to send to X"). The
  toolkit gives us file-hover/drop events to drive this.

**E3. Receive files (incoming)**
- *What*: files others send you wait until you save them.
- *Use*: a list of waiting files (name, size); per-file **Save** (to a chosen
  location) and **Delete**. A desktop notification fires on arrival.
- *UI*: an "Incoming files" area (could be on "this machine", or a global
  inbox/badge). Empty state when none.

### F. Per-peer detail

**F1. Peer detail view**
- *What*: everything about one peer.
- *Data available per peer*: name + MagicDNS name, OS, online/offline, all
  Tailscale IPs, whether it's an exit node / exit-node option, advertised routes,
  last seen, created, last handshake time, bytes sent/received, relay/DERP region,
  connection type (direct vs relayed).
- *Use*: click a peer in the list → see details; act on it (use as exit node, send
  files, copy address/name).
- *UI*: this is the main "drill-in". Decide: detail pane beside the list, or a
  push/back navigation, or an expanding row. Include copy buttons for name & IPs.
- *States*: online vs offline (offline peers show "last seen").

### G. This machine (self) detail

**G1. Self detail & settings**
- *What*: this device's identity + the toggles that affect it.
- *Contents*: machine name, MagicDNS name, all IPs (copy), copy FQDN; **toggles**:
  advertise exit node (C3), allow LAN access (C2), accept routes (D1); **advertised
  routes editor** (D2); **netcheck** (H).
- *UI*: a "This machine" screen grouping identity + a settings group of toggles +
  the routes editor + diagnostics. Today only identity + IPs exist.

### H. Diagnostics

**H1. Netcheck**
- *What*: a connectivity self-test. Reports: can it do UDP? IPv4/IPv6 reachable
  (and the detected public IPs)? NAT traversal methods available (UPnP / NAT-PMP /
  PCP)? Captive portal present? Nearest Tailscale relay ("DERP") region and the
  latency to each region.
- *Use*: click "Run netcheck" → results populate (takes a couple of seconds).
- *UI*: a "Run" button + a results panel: a set of pass/fail rows with icons, the
  preferred relay, and an expandable per-region latency list. Show a running/spinner
  state. Good candidate for an expandable/"advanced" section.

### I. App / system

**I1. Change control server URL**
- *What*: point at a self-hosted Tailscale control server (Headscale) instead of
  the default.
- *Use*: an input with the current URL, "Set" and "Reset to default". Advanced.
- *UI*: tuck into settings / add-tailnet advanced options.

**I2. Admin console link** ✅
- *What*: open the web admin in a browser. Simple link/button.

**I3. Preferences**
- *What*: app settings — show/hide the tray icon, start hidden / autostart on
  login, theme (light/dark/auto). (trayscale also had a polling interval; alavai
  is event-driven so that's no longer needed.)
- *UI*: a small preferences screen/dialog.

**I4. Notifications**
- *What*: desktop notifications for: connected/disconnected, incoming Taildrop
  file, exit-node enabled/disabled, errors.
- *UI*: OS notifications (no in-window design needed) — but decide *which* events
  notify, and whether there's a "do not disturb"/toggle.

**I5. About**
- *What*: app name, version, license (GPL-3.0), links (repo, issues).
- *UI*: a small about dialog.

**I6. Quit / window behaviour**
- *What*: closing the window keeps the tray running; "Quit" exits entirely.
- *UI*: make the distinction clear (a common point of confusion for tray apps).

---

## 6. Information architecture — the big decision

The central design question: **how is the window organised** as we add F/G/H and
the rest? Options (not mutually exclusive):

- **A. Sidebar + detail** (what trayscale did): left column lists "This machine"
  then all peers (with status icons); right pane shows the selected item's detail
  and actions. Mullvad exit nodes were a separate sidebar entry. Scales well to
  many peers; familiar; needs a wider window.
- **B. Tabbed / sectioned**: top-level sections (Overview · Peers · Exit nodes ·
  Settings). Compact; good for narrow windows; exit nodes get a first-class home.
- **C. Single scroll + drill-in**: today's single column, with peers/exit-nodes
  opening a detail view (push/back). Simplest; most "appy"; can get long.

Whatever the structure, keep **connection status + tailnet switcher** persistently
visible (a header), since those are the most-used controls. Consider how the
design degrades to a **narrow window** (tiling-WM users may keep it small) and how
it looks **maximised**.

Reference: trayscale used GNOME/Adwaita with a sidebar-and-pages layout. We are
**not** bound to that look — treat it only as a feature reference.

---

## 7. Tray menu — also needs design

The tray is the fast path and is used more than the window. Decide what belongs
here vs the window. Current items: header, Open window, tailnet radio list,
connect/disconnect, refresh, quit. Candidates to add: **exit-node submenu**
(quick pick — very commonly wanted), incoming-files indicator, "this machine"
copy-IP. Constraint: tray menus are plain text + icons + separators +
checkable/radio items + submenus (no rich layout). The tray **icon** itself needs
designed states (see §9).

---

## 8. Cross-cutting states to design

Please design these explicitly — they're easy to forget and matter a lot here:
- **Logged out / login required** (no active tailnet yet).
- **tailscaled not running / not reachable** (daemon down).
- **No operator permission** (A3) — likely the first thing many users hit.
- **Connecting / switching** (brief transitional states; switching tailnets
  momentarily drops everything then repopulates).
- **Busy / action in flight** (a switch or connect is processing — buttons
  disable).
- **Empty states**: no peers, no exit nodes available, no incoming files, single
  tailnet (is the switcher still shown?).
- **Errors**: action failed (e.g. switch failed, send failed) — toast vs inline.
- **Entitlement-gated**: Mullvad section hidden when not available.

---

## 9. Visual & branding

- **App identity**: name is "alavai". Needs a logo/app icon and, importantly, a
  set of **tray status icons**: connected, disconnected, exit-node-active (and
  possibly "needs attention"/error). These must read clearly at 16–24px on both
  light and dark panels and in monochrome/symbolic form (many panels recolour
  tray icons).
- **Status colour semantics**: define colours for online/connected (success),
  offline/disconnected (neutral/muted), exit-node-active (accent), warning/error.
  Currently we use `●`/`○` glyphs and a single accent — replace with a considered
  palette + icon set.
- **Theme**: currently dark-only. Decide light + dark (and whether to auto-detect;
  auto-detection is possible but currently disabled for a technical reason — a
  manual light/dark toggle in Preferences is the safe path).
- **Iconography**: peers, exit nodes, routes, files, copy, etc. Prefer a single
  coherent symbolic icon set we can bundle (SVG).
- **Density & sizing**: it's a utility app — favour clarity and reasonable density
  (the peer list can be long). Define spacing/type scale tokens.

---

## 10. Interaction details / microcopy

- **Copy actions** everywhere (IPs, names) → confirm with a transient toast.
- **Confirmations** for destructive actions (remove tailnet, delete incoming
  file).
- **Async feedback**: actions hit a local daemon and are usually fast but not
  instant — show busy/disabled states; never let the UI look frozen.
- **Explanatory microcopy**: several features (exit node, accept routes, advertise
  exit node, operator) benefit from a one-line plain-English description or a
  tooltip. Please write this copy — it's part of the design.
- **Search/filter**: the peer list and especially Mullvad locations can be large;
  consider a filter box.

---

## 11. What we'd love back

In rough priority:
1. **Information architecture** recommendation (§6) with a wireframe of the main
   window across its key screens (overview, peer detail, this-machine/settings,
   exit-node picker, incoming files, add-tailnet/login).
2. **The headline tailnet switcher** — its most polished form, in both window and
   tray.
3. **Component & token specs**: colours (light+dark), spacing, type, the status
   colour/icon system, and the **tray status icons**.
4. **State designs** (§8) and **microcopy** (§10).
5. Anything that makes a lightweight, distro-agnostic utility feel considered and
   trustworthy (it handles networking/security, so it should feel solid).

Constraints to respect: it's `iced` (Rust widgets, software-rendered), must stay
lightweight, and must look at home on **any** Linux desktop rather than mimicking
one. When in doubt, optimise for the multi-account user switching tailnets and
picking exit nodes quickly.
