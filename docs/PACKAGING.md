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

#### Publishing / updating on the AUR

The AUR package `alavai` builds from the tagged release tarball. The AUR is a
separate git repo (only `PKGBUILD` + `.SRCINFO` are tracked).

**First-time publish** (needs an [AUR account](https://aur.archlinux.org) with
your SSH public key added under *My Account*):

```sh
git clone ssh://aur@aur.archlinux.org/alavai.git aur-alavai   # empty for a new pkg
cd aur-alavai
cp /path/to/alavai/packaging/PKGBUILD .
makepkg --printsrcinfo > .SRCINFO          # regenerate whenever PKGBUILD changes
git add PKGBUILD .SRCINFO
git commit -m "alavai <version>"
git push origin master                     # AUR's branch is master
```

**On each new release:**

1. Bump `pkgver` (reset `pkgrel=1`) in `packaging/PKGBUILD`.
2. `updpkgsums` (or compute the tag-tarball sha256) to refresh `sha256sums`.
3. `makepkg --printsrcinfo > .SRCINFO`.
4. Verify a clean build: `makepkg -f` (and `namcap PKGBUILD *.pkg.tar.zst`).
5. Commit both files and `git push origin master`.

Keep `packaging/PKGBUILD` here in sync with the one pushed to the AUR.

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

## Cutting a release

A tag push drives everything. The
[`release` workflow](../.github/workflows/release.yml) builds the binary and
produces the `.deb`, `.rpm` and AppImage, then creates the GitHub Release and
attaches them (with a `SHA256SUMS`). It triggers on any `v*` tag.

Steps for version `X.Y.Z`:

1. Bump `version` in `Cargo.toml` (and run a build so `Cargo.lock` updates).
2. Bump `pkgver` and refresh `sha256sums` in `packaging/PKGBUILD` — but note the
   tag tarball the sha covers only exists **after** you push the tag, so this is
   usually a follow-up commit (see the recent history for the two-commit
   pattern).
3. Commit (`release: X.Y.Z`), then tag and push:
   ```sh
   git tag vX.Y.Z
   git push origin main --tags
   ```
4. The `release` workflow runs on the tag and publishes the GitHub Release with
   the built packages. **No manual `gh release create` is needed.** If you want
   curated notes instead of auto-generated ones, create the release as a draft
   *before* pushing the tag — the workflow detects the existing release and only
   attaches assets to it.
5. Update the AUR package separately (see
   [Publishing / updating on the AUR](#publishing--updating-on-the-aur)); it
   builds from the source tarball and is not produced by the workflow.

The package layout is defined once in
[`packaging/nfpm.yaml`](../packaging/nfpm.yaml); keep it in sync with the
[install table](#what-to-install) above.

## Notes

- alavai requires the invoking user to be the Tailscale **operator**
  (`sudo tailscale set --operator=$USER`); this is a user setup step, not a
  package dependency. Consider mentioning it in package notes.
- It does **not** bundle the `tailscale` binary (unlike the trayscale Flatpak);
  it uses the system one.
- See [`man/alavai.1`](../man/alavai.1) for the full command reference.
