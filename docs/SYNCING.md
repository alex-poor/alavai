# Staying in sync with upstream Tailscale

alavai is a **clean-room Rust re-implementation** of the Tailscale LocalAPI
client. It shares **no code** with Tailscale: it talks to `tailscaled` over the
unix socket ([src/localapi.rs](../src/localapi.rs)) with hand-rolled HTTP/1.1 and
serde structs that *mirror* Tailscale's Go types. This doc is how we keep those
mirrors aligned with what `tailscaled` actually emits.

See also [ARCHITECTURE.md](ARCHITECTURE.md).

## The core constraint: the LocalAPI is not a stable contract

Tailscale treats `/localapi/v0/*` as **internal** and may change it between
releases. (Their stable, versioned API is the control-plane REST API at
`api.tailscale.com` — but that's for *tailnet administration*, not local node
control, so it's unusable for a local client.) alavai is therefore coupled, by
necessity, to `tailscaled`'s internal wire format — exactly like the official
`tailscale` CLI and GUI, which just happen to share Go structs at compile time.

This is a maintenance task, not a one-time port. The job is to make drift
**loud** (a failing test) instead of **silent** (a field that quietly stops
populating in a user's hands).

## Where drift happens (the coupling surface)

| What | Upstream Go source of truth |
| --- | --- |
| Endpoint paths + client | `tailscale.com/client/local` |
| `Status`, `PeerStatus` | `tailscale.com/ipn/ipnstate` |
| `Prefs`, `MaskedPrefs` | `tailscale.com/ipn` |
| `Notify` (IPN bus) | `tailscale.com/ipn` |
| `LoginProfile` (profiles) | `tailscale.com/ipn` |
| `ipn.State` enum values | `tailscale.com/ipn` |
| `NotifyWatchOpt` mask bits | `tailscale.com/ipn` |
| netcheck `Report`, `NetInfo` | `tailscale.com/net/netcheck`, `tailscale.com/tailcfg` |

In code these are all gathered in [src/localapi.rs](../src/localapi.rs); the
version-coupling constants (`TESTED_TAILSCALE_VERSION`, the `ipn.State` ints, the
`NotifyWatchOpt` bits) live in one block near the top with source-linked comments.

## The defenses (in order of importance)

1. **Lenient parsing.** serde ignores unknown fields by default and every field
   uses `#[serde(default)]`, so *additive* upstream changes (new fields) are
   non-events. **Never** add `#[serde(deny_unknown_fields)]`. The `opt_bool`
   helper (Tailscale's tri-state `opt.Bool`) and `null_as_empty_vec` (empty lists
   serialize as `null`) are deliberate defensive shims — follow that pattern.

2. **Golden-fixture tests** (`testdata/*.json` + `#[cfg(test)] mod tests` in
   `localapi.rs`). PII-free representative JSON deserialized into our structs,
   asserting the fields alavai relies on. They include extra/unknown keys to keep
   us honest about lenient parsing. These run in CI (`cargo test`).

3. **The live-daemon test** — `live_daemon_matches_fixtures`, `#[ignore]`d by
   default (needs a running `tailscaled` + operator perms). This is the real
   tripwire: it deserializes the *actual* daemon output. Run it when bumping the
   tested version (below).

4. **Soft version hint.** `untested_version_warning` prints a one-line stderr note
   if `tailscaled`'s **major** version differs from `TESTED_TAILSCALE_VERSION`.
   Minor/patch differences are expected and never warned about. It never fails —
   it only nudges.

## Routine: keeping current

- **Watch upstream.** Subscribe to [tailscale/tailscale releases][rel] and skim
  the changelog for churn in `ipn` / `ipnstate` / `localapi`. The first-party
  `tailscale systray` subcommand is a second consumer of the same API worth
  tracking.
- **Bump the tested version** when you've verified against a newer `tailscaled`:
  1. `tailscale version` to confirm the daemon version.
  2. `cargo test -- --ignored` — the live test must pass against it.
  3. If a field changed, update the struct + fixture together.
  4. Update `TESTED_TAILSCALE_VERSION` in `localapi.rs` and the `.TH` line / tested
     version note in docs as needed.
  5. Refresh fixtures if shapes changed (see below).

## Refreshing the golden fixtures

Fixtures are **representative and PII-free** — hand-authored, *not* raw daemon
dumps (real output contains login emails, IPs, hostnames, node keys that must not
land in a public repo). To refresh after a wire-format change, capture the real
shape locally and port only the *structure* into the fixture with fake values:

```sh
S=/run/tailscale/tailscaled.sock; H=local-tailscaled.sock
curl -s --unix-socket $S -H "Host: $H" http://x/localapi/v0/status  | python3 -m json.tool
curl -s --unix-socket $S -H "Host: $H" http://x/localapi/v0/prefs   | python3 -m json.tool
curl -s --unix-socket $S -H "Host: $H" http://x/localapi/v0/profiles/current | python3 -m json.tool
tailscale netcheck --format=json | python3 -m json.tool
```

Keep fake values obvious: `example.com` logins, `100.100.0.x` / `203.0.113.x`
IPs, generic hostnames (`laptop`, `fileserver`). Then `cargo test` to confirm the
fixtures still satisfy the assertions.

[rel]: https://github.com/tailscale/tailscale/releases
