# Contributing to alavai

Thanks for your interest — contributions of all kinds are welcome: bug reports,
features, docs, design, and testing on different desktops/distros.

By participating you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

## Getting started

Requirements:

- **Rust** 2024 edition (1.85+) — install via [rustup](https://rustup.rs).
- The **Tailscale** daemon (`tailscaled`) and CLI.
- Your user set as the Tailscale operator: `sudo tailscale set --operator=$USER`.

Build and run:

```sh
cargo build
cargo run -- tray     # tray daemon
cargo run -- gui      # main window
cargo run -- --help   # full CLI
```

A StatusNotifierItem host (KDE, GNOME + AppIndicator, or most tray-capable WMs)
is needed for the tray.

## Project layout

```
src/
  main.rs       CLI entry + subcommands
  localapi.rs   typed client for the tailscaled LocalAPI (incl. the watch stream)
  tray.rs       ksni system tray
  gui.rs        iced main window
  theme.rs      design tokens + widget styles
  icon.rs       bundled icons + SVG rasterizer
assets/icons/   bundled SVGs
docs/           architecture, roadmap, and the UI design system
```

Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how the pieces fit, and
[docs/PLAN.md](docs/PLAN.md) for the roadmap and what's left to build.

## How it talks to Tailscale

alavai speaks the `tailscaled` **LocalAPI** over its unix socket directly — there
is no Go and no bundled Tailscale library. Reads (`status`, `prefs`, profiles,
`watch-ipn-bus`) and writes (`EditPrefs`, profile switch/add/remove, login) all go
through `src/localapi.rs`. When adding a feature, prefer extending that typed
client over shelling out, and mirror how the official Go client uses the endpoint
(the Tailscale source is the reference).

## Before you open a PR

```sh
cargo fmt              # format
cargo clippy           # lint — keep it warning-free
cargo build            # must build with no warnings
```

CI runs `fmt --check`, `clippy -D warnings`, and a build, so please run these
locally first.

Guidelines:

- **Match the surrounding style** — comment density, naming, and idioms.
- **Keep it lightweight.** Avoid new heavy dependencies; call out the trade-off
  in the PR if one is genuinely needed (the project deliberately has no `tokio`,
  GTK/Qt, or GPU dependency).
- **Verify UI changes by running the app**, not just compiling — a screenshot in
  the PR is appreciated.
- Keep PRs focused; one logical change per PR.

## Commit & PR conventions

- Write clear, imperative commit messages (`Add exit-node picker`, not `added…`).
- Reference issues where relevant (`Fixes #12`).
- Fill in the PR template.

## Good places to start

- Issues labelled [`good first issue`](https://github.com/alex-poor/alavai/labels/good%20first%20issue)
  and [`help wanted`](https://github.com/alex-poor/alavai/labels/help%20wanted).
- **Testing on your setup** — alavai aims to work on any distro/DE/WM. Reports
  (with your environment) of how the tray and window behave are valuable.
- Features marked ⬜ in the [feature-parity matrix](docs/PLAN.md).

### A note on Taildrop & Mullvad

These are scoped in [docs/design/DESIGN.md](docs/design/DESIGN.md) but not yet
built, because the tailnet they were developed against had file-sharing and the
Mullvad add-on disabled — so they couldn't be verified end-to-end. If you have a
tailnet with these enabled, you're well placed to implement and test them.

## Questions

Open an issue or a discussion. Thanks for contributing!
