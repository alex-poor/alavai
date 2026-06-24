# Packaging alavai

Notes for distribution packagers. alavai is a single Rust binary plus a few data
files; it bundles no C libraries (see [Dependencies](#dependencies)).

## What to install

| Source | Destination | Mode |
| --- | --- | --- |
| `target/release/alavai` | `/usr/bin/alavai` | 755 |
| `man/alavai.1` | `/usr/share/man/man1/alavai.1` | 644 |
| `dist/alavai.desktop` | `/usr/share/applications/alavai.desktop` | 644 |
| `dist/alavai.svg` | `/usr/share/icons/hicolor/scalable/apps/alavai.svg` | 644 |
| `dist/alavai-tray.desktop` | `/etc/xdg/autostart/alavai-tray.desktop` *(optional)* | 644 |
| `LICENSE` | `/usr/share/licenses/alavai/LICENSE` | 644 |

`dist/alavai.desktop` launches the main window (`alavai gui`). The optional
autostart entry starts the tray daemon (`alavai tray`) on login.

## Build

```sh
cargo build --release --locked
```

Requirements: **Rust** (2024 edition, 1.85+) and `cargo`. The build is fully
offline-capable with `--locked` once crates are fetched; `Cargo.lock` is
committed. The release profile is size-optimised (`opt-level=z`, LTO, stripped).

## Dependencies

alavai's rendering, SVG, and font handling are **pure Rust** (tiny-skia, resvg,
cosmic-text), so it does **not** require Skia, Cairo, librsvg, FreeType,
HarfBuzz, fontconfig, Pango, GTK, or Qt. It also has no TLS/crypto dependency
(the LocalAPI is plain HTTP over a unix socket). The native libraries it does use
are loaded from the system.

### Build dependencies

- `cargo` / `rust` (1.85+)
- `pkg-config` (probes for system Wayland/X11 during the build)
- A C compiler (a small Wayland glue shim is compiled by `wayland-backend`)
- Wayland and X11 client headers:
  - Debian/Ubuntu: `libwayland-dev`, `libxkbcommon-dev`
  - Arch: `wayland`, `libxkbcommon`, `libx11` (headers are not split out)
  - Fedora: `wayland-devel`, `libxkbcommon-devel`, `libX11-devel`

### Runtime dependencies

- **`tailscale`** — the daemon (`tailscaled`) is required; the CLI is used by the
  `netcheck` subcommand. Declare a hard dependency.
- **`xdg-utils`** (`xdg-open`) — opens the admin console and interactive-login
  URLs.
- System libraries (loaded at runtime): `libwayland-client`, `libxkbcommon`,
  `libX11`. On any GUI-capable system these are already present; list them per
  your distro's conventions:
  - Debian: `libwayland-client0`, `libxkbcommon0`, `libx11-6`
  - Arch: `wayland`, `libxkbcommon`, `libx11`
- A **monospace and sans-serif font** — alavai uses the system fonts (it bundles
  none). Most desktops already provide these (e.g. `ttf-dejavu`).

### Optional (recommended)

- A **StatusNotifierItem host** for the tray icon (KDE Plasma, GNOME with the
  AppIndicator extension, Waybar, or most tray-capable panels). The window
  (`alavai gui`) works without one.

## Per-distro

### Arch Linux

A reference `PKGBUILD` is provided at [`packaging/PKGBUILD`](../packaging/PKGBUILD).
Build with:

```sh
cd packaging
makepkg -si
```

### Debian / Ubuntu

`debian/control` dependency sketch:

```
Build-Depends: debhelper-compat (= 13), cargo, rustc (>= 1.85), pkg-config,
               libwayland-dev, libxkbcommon-dev
Depends: ${shlibs:Depends}, ${misc:Depends}, tailscale, xdg-utils
Recommends: fonts-dejavu
```

### Fedora / RPM

```
BuildRequires: cargo, rust >= 1.85, pkgconfig, wayland-devel, libxkbcommon-devel, libX11-devel
Requires: tailscale, xdg-utils
```

## Notes

- alavai requires the invoking user to be the Tailscale **operator**
  (`sudo tailscale set --operator=$USER`); this is a user setup step, not a
  package dependency. Consider mentioning it in package notes.
- It does **not** bundle the `tailscale` binary (unlike the trayscale Flatpak);
  it uses the system one.
- See [`man/alavai.1`](../man/alavai.1) for the full command reference.
