//! Minimal client for the Tailscale LocalAPI.
//!
//! `tailscaled` exposes an HTTP API over a unix domain socket
//! (`/run/tailscale/tailscaled.sock`). Tailscale's own Go `client/local`
//! package speaks the same API; we implement just the slice alavai needs, with
//! no Go and no bundled Tailscale library.
//!
//! It's a blocking client throughout — one-shot request/response for commands,
//! plus a long-lived blocking reader for the `watch-ipn-bus` event stream
//! ([`Client::watch_live`]). No async runtime is involved.
//!
//! Accessing the socket requires the current user to be the Tailscale
//! "operator" (`sudo tailscale set --operator=$USER`).

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Deserializer};
use serde_json::json;

const SOCKET_PATHS: &[&str] = &[
    "/run/tailscale/tailscaled.sock",
    "/var/run/tailscale/tailscaled.sock",
];

// ---------------------------------------------------------------------------
// Upstream coupling (read docs/SYNCING.md before bumping)
// ---------------------------------------------------------------------------
//
// alavai mirrors `tailscaled`'s LocalAPI wire format by hand; it shares no code
// with Tailscale. The LocalAPI is explicitly *not* a stable contract, so the
// constants and structs here track these upstream Go packages:
//   - endpoints + client : tailscale.com/client/local
//   - Status, PeerStatus : tailscale.com/ipn/ipnstate
//   - Prefs, Notify      : tailscale.com/ipn
//   - LoginProfile       : tailscale.com/ipn  (ipn.LoginProfile)
//   - NetInfo, netcheck  : tailscale.com/tailcfg, tailscale.com/net/netcheck
//
// The Tailscale version this snapshot was last verified against. Bumping it is a
// reminder to refresh the golden fixtures in testdata/ (see SYNCING.md). Parsing
// stays lenient (serde ignores unknown fields; `#[serde(default)]` everywhere)
// so *additive* upstream changes are non-events; this constant only gates a soft
// runtime hint on a major-version mismatch, never a hard failure.
pub const TESTED_TAILSCALE_VERSION: &str = "1.104";

/// The major version component of [`TESTED_TAILSCALE_VERSION`] (e.g. `1`).
const TESTED_MAJOR: &str = "1";

/// A blocking handle to the local `tailscaled` daemon.
#[derive(Clone)]
pub struct Client {
    socket_path: String,
}

impl Default for Client {
    fn default() -> Self {
        let path = SOCKET_PATHS
            .iter()
            .find(|p| Path::new(p).exists())
            .copied()
            .unwrap_or(SOCKET_PATHS[0]);
        Client {
            socket_path: path.to_string(),
        }
    }
}

impl Client {
    /// Performs an HTTP request against the LocalAPI and returns the response
    /// body. The body of a request (for POST/PATCH) is sent as JSON.
    fn request(&self, method: &str, path: &str, body: &[u8]) -> Result<Vec<u8>> {
        let mut stream = UnixStream::connect(&self.socket_path).with_context(|| {
            format!(
                "connect to tailscaled socket at {} (is tailscaled running, and are you the operator?)",
                self.socket_path
            )
        })?;

        // tailscaled validates the Host header; the literal value the Go client
        // uses is "local-tailscaled.sock". `Connection: close` lets us read the
        // whole response to EOF.
        let mut req = format!(
            "{method} {path} HTTP/1.1\r\n\
             Host: local-tailscaled.sock\r\n\
             Connection: close\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        req.extend_from_slice(body);
        stream.write_all(&req).context("write LocalAPI request")?;
        stream.flush().ok();

        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .context("read LocalAPI response")?;
        parse_http_response(&raw)
    }

    fn get(&self, path: &str) -> Result<Vec<u8>> {
        self.request("GET", path, &[])
    }

    /// Returns the current network status (the LocalAPI `status` endpoint).
    pub fn status(&self) -> Result<Status> {
        let body = self.get("/localapi/v0/status")?;
        serde_json::from_slice(&body).context("parse status JSON")
    }

    /// Lists all configured tailnets (Tailscale login profiles).
    pub fn profiles(&self) -> Result<Vec<Profile>> {
        let body = self.get("/localapi/v0/profiles/")?;
        serde_json::from_slice(&body).context("parse profiles JSON")
    }

    /// Returns the currently active tailnet/profile.
    pub fn current_profile(&self) -> Result<Profile> {
        let body = self.get("/localapi/v0/profiles/current")?;
        serde_json::from_slice(&body).context("parse current profile JSON")
    }

    /// Switches to the tailnet/profile with the given LocalAPI profile ID.
    ///
    /// This is the heart of the headline "one-click tailnet switching"
    /// feature: the tray menu calls straight into here.
    pub fn switch_profile(&self, id: &str) -> Result<()> {
        self.request("POST", &format!("/localapi/v0/profiles/{id}"), &[])?;
        Ok(())
    }

    /// Creates a new, empty login profile and switches to it. Follow with
    /// [`Self::start_login`] to authenticate a new tailnet.
    pub fn new_profile(&self) -> Result<()> {
        self.request("PUT", "/localapi/v0/profiles/", &[])?;
        Ok(())
    }

    /// Starts interactive login. The daemon then emits a `BrowseToURL` on the
    /// IPN bus for the user to open and authenticate.
    pub fn start_login(&self) -> Result<()> {
        self.request("POST", "/localapi/v0/login-interactive", &[])?;
        Ok(())
    }

    /// Deletes (forgets) the profile with the given ID.
    pub fn delete_profile(&self, id: &str) -> Result<()> {
        self.request("DELETE", &format!("/localapi/v0/profiles/{id}"), &[])?;
        Ok(())
    }

    /// Returns the current node preferences (the toggles behind exit nodes,
    /// routes, etc.).
    pub fn prefs(&self) -> Result<Prefs> {
        let body = self.get("/localapi/v0/prefs")?;
        serde_json::from_slice(&body).context("parse prefs JSON")
    }

    /// Applies a "masked prefs" edit. `masked` must contain the changed `Prefs`
    /// fields plus a `<Field>Set: true` marker for each, e.g.
    /// `{"RouteAll": true, "RouteAllSet": true}`.
    fn edit_prefs(&self, masked: serde_json::Value) -> Result<()> {
        let body = serde_json::to_vec(&masked).context("serialize masked prefs")?;
        self.request("PATCH", "/localapi/v0/prefs", &body)?;
        Ok(())
    }

    /// Uses the peer with the given stable node ID as this node's exit node.
    /// An empty `id` clears the exit node.
    pub fn set_exit_node(&self, id: &str) -> Result<()> {
        let masked = if id.is_empty() {
            json!({"ExitNodeID": "", "ExitNodeIDSet": true, "ExitNodeIP": "", "ExitNodeIPSet": true})
        } else {
            json!({"ExitNodeID": id, "ExitNodeIDSet": true})
        };
        self.edit_prefs(masked)
    }

    /// Whether to keep access to the local LAN while using an exit node.
    pub fn set_exit_node_allow_lan(&self, allow: bool) -> Result<()> {
        self.edit_prefs(json!({
            "ExitNodeAllowLANAccess": allow,
            "ExitNodeAllowLANAccessSet": true,
        }))
    }

    /// Connects (`true`) or disconnects (`false`) this node by setting the
    /// `WantRunning` preference — the native LocalAPI equivalent of
    /// `tailscale up` / `tailscale down`, without shelling out to the CLI.
    pub fn set_want_running(&self, run: bool) -> Result<()> {
        self.edit_prefs(json!({"WantRunning": run, "WantRunningSet": true}))
    }

    /// Whether to accept subnet routes advertised by other nodes.
    pub fn set_accept_routes(&self, accept: bool) -> Result<()> {
        self.edit_prefs(json!({"RouteAll": accept, "RouteAllSet": true}))
    }

    /// Advertises (or stops advertising) this machine as an exit node, while
    /// preserving any advertised subnet routes.
    pub fn set_advertise_exit_node(&self, enable: bool) -> Result<()> {
        let prefs = self.prefs()?;
        let mut routes = prefs.subnet_routes();
        if enable {
            routes.push("0.0.0.0/0".into());
            routes.push("::/0".into());
        }
        self.edit_prefs(json!({"AdvertiseRoutes": routes, "AdvertiseRoutesSet": true}))
    }

    /// Returns the stable node ID of the daemon's suggested "best" exit node
    /// (for the picker's "Automatic" option). Empty if none is suggested.
    pub fn suggest_exit_node(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct Resp {
            #[serde(rename = "ID", default)]
            id: String,
        }
        let body = self.get("/localapi/v0/suggest-exit-node")?;
        let r: Resp = serde_json::from_slice(&body).context("parse exit-node suggestion")?;
        Ok(r.id)
    }

    /// Sets the advertised subnet routes, preserving exit-node advertisement.
    pub fn set_advertise_routes(&self, subnets: &[String]) -> Result<()> {
        let prefs = self.prefs()?;
        let mut routes: Vec<String> = subnets.to_vec();
        if prefs.advertises_exit_node() {
            routes.push("0.0.0.0/0".into());
            routes.push("::/0".into());
        }
        self.edit_prefs(json!({"AdvertiseRoutes": routes, "AdvertiseRoutesSet": true}))
    }
}

/// A tailnet/login profile as reported by the LocalAPI.
#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Name", default)]
    pub name: String,
    #[serde(rename = "NetworkProfile", default)]
    pub network: NetworkProfile,
    #[serde(rename = "UserProfile", default)]
    pub user: UserProfile,
}

impl Profile {
    /// True for the empty placeholder profile (no tailnet logged in).
    pub fn is_empty(&self) -> bool {
        self.id.is_empty()
    }

    /// A human-friendly label for the tailnet.
    pub fn label(&self) -> String {
        if !self.network.domain.is_empty() {
            self.network.domain.clone()
        } else if !self.name.is_empty() {
            self.name.clone()
        } else {
            self.id.clone()
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NetworkProfile {
    #[serde(rename = "DomainName", default)]
    pub domain: String,
    #[serde(rename = "DisplayName", default)]
    pub display_name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserProfile {
    #[serde(rename = "LoginName", default)]
    pub login_name: String,
}

/// A view of the LocalAPI `status` response (the fields alavai consumes).
#[derive(Debug, Clone, Deserialize)]
pub struct Status {
    #[serde(rename = "BackendState")]
    pub backend_state: String,
    /// The running `tailscaled` version (e.g. "1.98.4"). Used for the soft
    /// untested-version hint; empty if the daemon omits it.
    #[serde(rename = "Version", default)]
    pub version: String,
    // On the IPN bus (`NotifyInitialStatus`) a logged-out daemon sends these as
    // explicit JSON `null`, which `default` alone does not absorb (it only
    // covers an *absent* key), so both tolerate null.
    #[serde(
        rename = "TailscaleIPs",
        default,
        deserialize_with = "null_as_empty_vec"
    )]
    pub tailscale_ips: Vec<String>,
    #[serde(rename = "Self", default)]
    pub self_node: Option<Node>,
    #[serde(rename = "Peer", default, deserialize_with = "null_as_default")]
    pub peers: std::collections::HashMap<String, Peer>,
}

impl Status {
    pub fn online(&self) -> bool {
        self.backend_state == "Running"
    }
}

/// Fetches the daemon version and prints the untested-version hint (if any) to
/// stderr. Best-effort: a status fetch failure is silently ignored, since the
/// caller surfaces real connection errors elsewhere.
pub fn warn_if_untested_daemon(client: &Client) {
    if let Ok(status) = client.status()
        && let Some(msg) = untested_version_warning(&status.version)
    {
        eprintln!("{msg}");
    }
}

/// Returns a one-line warning if the daemon's major version differs from the
/// one alavai was verified against ([`TESTED_TAILSCALE_VERSION`]), else `None`.
/// Minor/patch differences are expected and never warned about — only a major
/// jump (e.g. 1.x → 2.x) is a likely wire-format break worth surfacing.
pub fn untested_version_warning(daemon_version: &str) -> Option<String> {
    let major = daemon_version.split('.').next().unwrap_or("");
    if major.is_empty() || major == TESTED_MAJOR {
        return None;
    }
    Some(format!(
        "alavai: tailscaled {daemon_version} is a different major version than the \
         tested {TESTED_TAILSCALE_VERSION}.x; some features may misbehave"
    ))
}

#[derive(Debug, Clone, Deserialize)]
pub struct Node {
    #[serde(rename = "HostName", default)]
    pub hostname: String,
    #[serde(rename = "DNSName", default)]
    pub dns_name: String,
    #[serde(rename = "OS", default)]
    pub os: String,
    /// Preferred DERP relay region code (e.g. "syd"); empty if direct/unknown.
    /// The one netcheck-ish datum available without the `tailscale` CLI.
    #[serde(rename = "Relay", default)]
    pub relay: String,
}

/// A peer (another machine on the tailnet) from the `status` response.
#[derive(Debug, Clone, Deserialize)]
pub struct Peer {
    /// Stable node ID, used as the exit-node identifier.
    #[serde(rename = "ID", default)]
    pub id: String,
    #[serde(rename = "HostName", default)]
    pub hostname: String,
    #[serde(rename = "DNSName", default)]
    pub dns_name: String,
    #[serde(rename = "OS", default)]
    pub os: String,
    #[serde(rename = "TailscaleIPs", default)]
    pub tailscale_ips: Vec<String>,
    #[serde(rename = "Online", default)]
    pub online: bool,
    /// True if this peer is the node's *currently active* exit node.
    #[serde(rename = "ExitNode", default)]
    pub exit_node: bool,
    /// True if this peer is *available* as an exit node.
    #[serde(rename = "ExitNodeOption", default)]
    pub exit_node_option: bool,
    /// True if there's an active (recent) connection to this peer.
    #[serde(rename = "Active", default)]
    pub active: bool,
    #[serde(rename = "RxBytes", default)]
    pub rx_bytes: i64,
    #[serde(rename = "TxBytes", default)]
    pub tx_bytes: i64,
    /// DERP relay region in use (empty if a direct connection).
    #[serde(rename = "Relay", default)]
    pub relay: String,
    #[serde(rename = "LastSeen", default)]
    pub last_seen: String,
    #[serde(rename = "LastHandshake", default)]
    pub last_handshake: String,
    /// Subnet routes this peer advertises and serves.
    #[serde(
        rename = "PrimaryRoutes",
        default,
        deserialize_with = "null_as_empty_vec"
    )]
    pub primary_routes: Vec<String>,
}

/// The current node preferences (the toggles behind exit nodes and routes).
#[derive(Debug, Clone, Deserialize)]
pub struct Prefs {
    #[serde(rename = "ControlURL", default)]
    pub control_url: String,
    #[serde(rename = "RouteAll", default)]
    pub route_all: bool,
    #[serde(rename = "ExitNodeID", default)]
    pub exit_node_id: String,
    #[serde(rename = "ExitNodeIP", default)]
    pub exit_node_ip: String,
    #[serde(rename = "ExitNodeAllowLANAccess", default)]
    pub exit_node_allow_lan: bool,
    #[serde(
        rename = "AdvertiseRoutes",
        default,
        deserialize_with = "null_as_empty_vec"
    )]
    pub advertise_routes: Vec<String>,
    #[serde(rename = "WantRunning", default)]
    pub want_running: bool,
    #[serde(rename = "OperatorUser", default)]
    pub operator_user: String,
    #[serde(rename = "ProfileName", default)]
    pub profile_name: String,
}

impl Prefs {
    /// True if this node currently uses an exit node.
    pub fn exit_node_active(&self) -> bool {
        !self.exit_node_id.is_empty() || !self.exit_node_ip.is_empty()
    }

    /// True if this node advertises itself as an exit node.
    pub fn advertises_exit_node(&self) -> bool {
        self.advertise_routes.iter().any(|r| is_default_route(r))
    }

    /// The advertised subnet routes, excluding the exit-node default routes.
    pub fn subnet_routes(&self) -> Vec<String> {
        self.advertise_routes
            .iter()
            .filter(|r| !is_default_route(r))
            .cloned()
            .collect()
    }
}

fn is_default_route(route: &str) -> bool {
    route == "0.0.0.0/0" || route == "::/0"
}

/// Deserializes a JSON array or `null` into a `Vec`, treating `null`/missing as
/// empty. (Tailscale serializes empty route lists as `null`.)
fn null_as_empty_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<Vec<String>>::deserialize(deserializer)?.unwrap_or_default())
}

/// Deserializes any `Default` type, treating an explicit JSON `null` as the
/// default. (`#[serde(default)]` only covers an *absent* key, not `null`.)
fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// Splits an HTTP/1.1 response into status + body, dechunking if necessary, and
/// returns the body bytes. Errors on a non-2xx status, surfacing the body text.
fn parse_http_response(raw: &[u8]) -> Result<Vec<u8>> {
    let split = find(raw, b"\r\n\r\n")
        .ok_or_else(|| anyhow!("malformed HTTP response (no header terminator)"))?;
    let headers = &raw[..split];
    let mut body = raw[split + 4..].to_vec();

    let header_text = String::from_utf8_lossy(headers);
    let mut lines = header_text.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| anyhow!("malformed HTTP status line: {status_line:?}"))?;

    let chunked = lines.any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    if chunked {
        body = dechunk(&body)?;
    }

    if !(200..300).contains(&code) {
        bail!(
            "LocalAPI returned HTTP {code}: {}",
            String::from_utf8_lossy(&body).trim()
        );
    }
    Ok(body)
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let pos = find(data, b"\r\n").ok_or_else(|| anyhow!("malformed chunk header"))?;
        let size_str = std::str::from_utf8(&data[..pos])?.trim();
        let size_field = size_str.split(';').next().unwrap_or("0");
        let size = usize::from_str_radix(size_field, 16).context("parse chunk size")?;
        data = &data[pos + 2..];
        if size == 0 {
            break;
        }
        if data.len() < size {
            bail!("truncated chunk body");
        }
        out.extend_from_slice(&data[..size]);
        // Skip chunk data plus its trailing CRLF.
        data = &data[(size + 2).min(data.len())..];
    }
    Ok(out)
}

/// Finds the first index of `needle` within `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ---------------------------------------------------------------------------
// Event stream (watch-ipn-bus)
// ---------------------------------------------------------------------------

// `ipn.State` enum values (upstream: tailscale.com/ipn ipn.State).
// Full set: NoState=0, InUseOtherUser=1, NeedsLogin=2, NeedsMachineAuth=3,
// Stopped=4, Starting=5, Running=6. alavai only distinguishes the two it acts on.
/// `ipn.State` value for a fully-connected backend (`ipn.Running`).
const STATE_RUNNING: i64 = 6;
/// `ipn.State` value for a backend with no tailnet logged in (`ipn.NeedsLogin`).
const STATE_NEEDS_LOGIN: i64 = 2;

// `ipn.NotifyWatchOpt` bit flags (upstream: tailscale.com/ipn ipn.NotifyWatch*).
//
// tailscale 1.104 removed the NetMap from the IPN bus: the former
// `NotifyInitialNetMap` (1<<3) and `NotifyRateLimit` (1<<8) bits are now
// *rejected* with HTTP 400 (ValidateNotifyWatchOpt). Self state is seeded once
// from `NotifyInitialStatus` and then kept current by the always-on `SelfChange`
// runtime notify; peer-set changes arrive as `PeersChanged`/`PeersRemoved`
// deltas gated by `NotifyPeerChanges`, which we use purely as a "refresh peers
// via status()" trigger (the peer list itself is still fetched, not parsed off
// the bus).
/// `NotifyInitialState` — emit the current State (+BrowseToURL) on connect.
const NOTIFY_INITIAL_STATE: u64 = 1 << 1;
/// `NotifyInitialPrefs` — include Prefs in the initial burst and on change.
const NOTIFY_INITIAL_PREFS: u64 = 1 << 2;
/// `NotifyPeerChanges` — opt into PeersChanged/PeersRemoved peer-set deltas.
const NOTIFY_PEER_CHANGES: u64 = 1 << 12;
/// `NotifyInitialStatus` — seed the current `ipnstate.Status` (self + scalar
/// fields) in the first Notify, replacing the removed initial NetMap.
const NOTIFY_INITIAL_STATUS: u64 = 1 << 14;
/// The notify-mask bits we subscribe to. (Engine/bandwidth updates are
/// intentionally omitted until per-peer stats need them.)
const WATCH_MASK: u64 =
    NOTIFY_INITIAL_STATE | NOTIFY_INITIAL_PREFS | NOTIFY_PEER_CHANGES | NOTIFY_INITIAL_STATUS;

/// The live, continuously-updated slice of daemon state derived from the IPN
/// bus. Fields persist across the delta notifications the bus emits.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiveState {
    pub online: bool,
    /// True when the daemon is reachable but no tailnet is logged in.
    pub needs_login: bool,
    pub exit_node_active: bool,
    pub machine: String,
    pub fqdn: String,
    pub os: String,
    /// First (preferred) Tailscale address — convenience for the tray.
    pub address: String,
    pub addresses: Vec<String>,
    pub accept_routes: bool,
    pub advertise_exit_node: bool,
    pub allow_lan: bool,
    pub advertised_routes: Vec<String>,
    /// Set when the daemon wants the user to open a browser to authenticate.
    pub browse_to_url: Option<String>,
    /// Transient: this delta carried a NetMap, so peers may have changed and a
    /// consumer should refresh its peer list. (Not part of equality.)
    pub netmap_changed: bool,
}

impl Client {
    /// Subscribes to the IPN bus and invokes `on_change` with the merged
    /// [`LiveState`] every time it changes. Blocks forever, reconnecting after a
    /// short delay if the stream drops (e.g. after a profile switch).
    pub fn watch_live(&self, on_change: impl FnMut(LiveState)) {
        self.watch_live_with(on_change, || {});
    }

    /// Like [`Self::watch_live`], but also invokes `on_stream_end` each time the
    /// bus stream closes cleanly. A profile switch tears the stream down, so a
    /// clean close is the event-driven signal that the active profile (and list)
    /// may have changed — letting consumers refresh profiles (which aren't on the
    /// bus) without polling on a tight timer.
    pub fn watch_live_with(
        &self,
        mut on_change: impl FnMut(LiveState),
        mut on_stream_end: impl FnMut(),
    ) {
        loop {
            match self.watch_once(&mut on_change) {
                // Clean close (EOF / final chunk): typically a profile switch.
                Ok(()) => on_stream_end(),
                Err(e) => eprintln!("alavai: watch-ipn-bus: {e}; reconnecting in 2s…"),
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    fn watch_once(&self, on_change: &mut impl FnMut(LiveState)) -> Result<()> {
        let stream = UnixStream::connect(&self.socket_path)
            .with_context(|| format!("connect to {}", self.socket_path))?;

        let req = format!(
            "GET /localapi/v0/watch-ipn-bus?mask={WATCH_MASK} HTTP/1.1\r\n\
             Host: local-tailscaled.sock\r\n\
             Connection: keep-alive\r\n\r\n"
        );
        (&stream).write_all(req.as_bytes())?;

        let mut reader = BufReader::new(&stream);
        let head = read_headers(&mut reader)?;
        if !(200..300).contains(&head.code) {
            // A rejected subscription (e.g. an unsupported mask bit after a
            // tailscaled upgrade) comes back as a normal HTTP error response,
            // *not* a chunked stream. Surface it verbatim so the failure is
            // actionable instead of looping on a bogus "parse chunk size".
            let mut body = Vec::new();
            match head.content_length {
                Some(len) => {
                    let mut buf = vec![0u8; len.min(4096)];
                    let _ = reader.read_exact(&mut buf);
                    body = buf;
                }
                None => {
                    let _ = reader.read_to_end(&mut body);
                }
            }
            bail!(
                "watch-ipn-bus rejected with HTTP {}: {}",
                head.code,
                String::from_utf8_lossy(&body).trim()
            );
        }

        let mut live = LiveState::default();
        let mut linebuf: Vec<u8> = Vec::new();
        loop {
            let Some(size) = read_chunk_size(&mut reader)? else {
                return Ok(()); // EOF / final chunk → let caller reconnect
            };
            let mut data = vec![0u8; size];
            reader.read_exact(&mut data)?;
            let mut crlf = [0u8; 2];
            reader.read_exact(&mut crlf)?; // trailing CRLF after chunk data

            linebuf.extend_from_slice(&data);
            while let Some(pos) = linebuf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = linebuf.drain(..=pos).collect();
                let trimmed = line.strip_suffix(b"\n").unwrap_or(&line);
                if trimmed.is_empty() {
                    continue;
                }
                match serde_json::from_slice::<Notify>(trimmed) {
                    Ok(n) => {
                        if merge_notify(&mut live, n) {
                            on_change(live.clone());
                        }
                    }
                    Err(e) => eprintln!("alavai: parse notify: {e}"),
                }
            }
        }
    }
}

/// The parsed status line and framing of an HTTP response head.
struct HttpHead {
    code: u16,
    content_length: Option<usize>,
}

/// Reads the HTTP status line and headers up to (and including) the blank line,
/// returning the status code and any `Content-Length`. Unlike the chunked body
/// that follows a 200, this makes no success assumption: callers must check
/// `code` before treating the stream as the event feed.
fn read_headers(reader: &mut impl BufRead) -> Result<HttpHead> {
    let mut status_line = String::new();
    if reader.read_line(&mut status_line)? == 0 {
        bail!("connection closed before headers completed");
    }
    let code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| anyhow!("malformed HTTP status line: {:?}", status_line.trim()))?;

    let mut content_length = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            bail!("connection closed before headers completed");
        }
        if line == "\r\n" || line == "\n" {
            return Ok(HttpHead {
                code,
                content_length,
            });
        }
        if let Some((_, v)) = line
            .split_once(':')
            .filter(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        {
            content_length = v.trim().parse().ok();
        }
    }
}

/// Reads one chunked-transfer size line. Returns `None` at end of stream or on
/// the terminating zero-size chunk.
fn read_chunk_size(reader: &mut impl BufRead) -> Result<Option<usize>> {
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        let s = line.trim();
        if s.is_empty() {
            continue; // tolerate stray blank lines between chunks
        }
        let hex = s.split(';').next().unwrap_or("0");
        let size = usize::from_str_radix(hex, 16).context("parse chunk size")?;
        return Ok(if size == 0 { None } else { Some(size) });
    }
}

/// Folds a single notification into the running [`LiveState`], returning whether
/// anything user-visible changed.
fn merge_notify(live: &mut LiveState, n: Notify) -> bool {
    let mut changed = false;
    // The bus no longer carries the NetMap. Peer-set changes arrive as
    // PeersChanged/PeersRemoved deltas, and the initial status implies a fresh
    // peer baseline; any of these means the consumer should refresh its peer
    // list (which it fetches via `status()`), even if no self/prefs field moved.
    let peers_touched =
        n.initial_status.is_some() || n.peers_changed.is_some() || n.peers_removed.is_some();
    live.netmap_changed = peers_touched;

    if let Some(state) = n.state {
        let online = state == STATE_RUNNING;
        let needs_login = state == STATE_NEEDS_LOGIN;
        if online != live.online || needs_login != live.needs_login {
            live.online = online;
            live.needs_login = needs_login;
            changed = true;
        }
    }

    if let Some(p) = n.prefs {
        let exit = !p.exit_node_id.is_empty() || !p.exit_node_ip.is_empty();
        let advertise_exit = p.advertise_routes.iter().any(|r| is_default_route(r));
        let subnets: Vec<String> = p
            .advertise_routes
            .iter()
            .filter(|r| !is_default_route(r))
            .cloned()
            .collect();
        if exit != live.exit_node_active
            || p.route_all != live.accept_routes
            || advertise_exit != live.advertise_exit_node
            || p.exit_node_allow_lan != live.allow_lan
            || subnets != live.advertised_routes
        {
            live.exit_node_active = exit;
            live.accept_routes = p.route_all;
            live.advertise_exit_node = advertise_exit;
            live.allow_lan = p.exit_node_allow_lan;
            live.advertised_routes = subnets;
            changed = true;
        }
    }

    // Self state: seeded once from the initial `ipnstate.Status`, then kept
    // current by the always-on `SelfChange` runtime notify (a `tailcfg.Node`).
    // The two carry the same facts under different field names, so each is
    // normalized before folding in.
    if let Some(self_node) = n.initial_status.and_then(|s| {
        let ips = s.tailscale_ips;
        s.self_node.map(|node| (node, ips))
    }) {
        let (node, ips) = self_node;
        let machine = if !node.hostname.is_empty() {
            node.hostname
        } else {
            node.dns_name.trim_end_matches('.').to_string()
        };
        let fqdn = node.dns_name.trim_end_matches('.').to_string();
        // `ipnstate.Status` IPs are already bare (no CIDR suffix).
        changed |= apply_self(live, machine, fqdn, node.os, &ips);
    }
    if let Some(sn) = n.self_change {
        let machine = if !sn.hostinfo.hostname.is_empty() {
            sn.hostinfo.hostname.clone()
        } else {
            sn.name.trim_end_matches('.').to_string()
        };
        let fqdn = sn.name.trim_end_matches('.').to_string();
        // `tailcfg.Node` addresses are CIDRs (e.g. 100.69.38.30/32).
        changed |= apply_self(live, machine, fqdn, sn.hostinfo.os, &sn.addresses);
    }

    if let Some(url) = n.browse_to_url
        && !url.is_empty()
        && live.browse_to_url.as_deref() != Some(url.as_str())
    {
        live.browse_to_url = Some(url);
        changed = true;
    }

    changed || peers_touched
}

/// Updates the self-identity fields of `live` from a node's machine name, FQDN,
/// OS, and raw address list (CIDRs or bare IPs — both normalized to bare IPs).
/// Returns whether anything user-visible changed.
fn apply_self(
    live: &mut LiveState,
    machine: String,
    fqdn: String,
    os: String,
    raw_addrs: &[String],
) -> bool {
    let addresses: Vec<String> = raw_addrs
        .iter()
        .map(|a| a.split('/').next().unwrap_or(a).to_string())
        .collect();
    if machine == live.machine && fqdn == live.fqdn && os == live.os && addresses == live.addresses
    {
        return false;
    }
    live.address = pick_address(raw_addrs);
    live.machine = machine;
    live.fqdn = fqdn;
    live.os = os;
    live.addresses = addresses;
    true
}

/// Picks a display address from a node's CIDR list, preferring IPv4.
fn pick_address(addrs: &[String]) -> String {
    let strip = |a: &str| a.split('/').next().unwrap_or(a).to_string();
    addrs
        .iter()
        .find(|a| !a.contains(':'))
        .map(|a| strip(a))
        .or_else(|| addrs.first().map(|a| strip(a)))
        .unwrap_or_default()
}

// --- Notify wire types (only the fields alavai consumes) ---

#[derive(Deserialize)]
struct Notify {
    #[serde(rename = "State", default)]
    state: Option<i64>,
    #[serde(rename = "Prefs", default)]
    prefs: Option<NotifyPrefs>,
    /// Full `ipnstate.Status`, sent once in the first frame (NotifyInitialStatus).
    #[serde(rename = "InitialStatus", default)]
    initial_status: Option<Status>,
    /// This node's own `tailcfg.Node`, pushed on every runtime self change.
    #[serde(rename = "SelfChange", default)]
    self_change: Option<NotifySelfNode>,
    /// Peer-set deltas (NotifyPeerChanges). Parsed only for their presence — the
    /// peer list itself is (re)fetched via `status()`, not read off the bus.
    #[serde(rename = "PeersChanged", default)]
    peers_changed: Option<serde_json::Value>,
    #[serde(rename = "PeersRemoved", default)]
    peers_removed: Option<serde_json::Value>,
    #[serde(rename = "BrowseToURL", default)]
    browse_to_url: Option<String>,
}

#[derive(Deserialize)]
struct NotifyPrefs {
    #[serde(rename = "ExitNodeID", default)]
    exit_node_id: String,
    #[serde(rename = "ExitNodeIP", default)]
    exit_node_ip: String,
    #[serde(rename = "RouteAll", default)]
    route_all: bool,
    #[serde(rename = "ExitNodeAllowLANAccess", default)]
    exit_node_allow_lan: bool,
    #[serde(
        rename = "AdvertiseRoutes",
        default,
        deserialize_with = "null_as_empty_vec"
    )]
    advertise_routes: Vec<String>,
}

/// A node's identity as carried by the bus `SelfChange` field (`tailcfg.Node`):
/// FQDN in `Name`, machine name + OS in `Hostinfo`, addresses as CIDRs.
#[derive(Deserialize)]
struct NotifySelfNode {
    #[serde(rename = "Name", default)]
    name: String,
    #[serde(rename = "Addresses", default)]
    addresses: Vec<String>,
    #[serde(rename = "Hostinfo", default)]
    hostinfo: NotifyHostinfo,
}

#[derive(Deserialize, Default)]
struct NotifyHostinfo {
    #[serde(rename = "Hostname", default)]
    hostname: String,
    #[serde(rename = "OS", default)]
    os: String,
}

// ---------------------------------------------------------------------------
// netcheck (connectivity diagnostics)
// ---------------------------------------------------------------------------

/// A connectivity diagnostics report.
///
/// This is the **one** place alavai shells out to the `tailscale` CLI. It is
/// deliberate, not a gap: the LocalAPI exposes no netcheck endpoint (verified —
/// `/localapi/v0/netcheck` etc. 404), and the report's data (DERP latencies,
/// UPnP/PMP/PCP, global addresses) is *not* carried on the IPN bus — the netmap
/// SelfNode `Hostinfo` omits `NetInfo`. The CLI computes the report by probing
/// in-process; replicating that natively would mean re-implementing core
/// Tailscale netcheck/STUN/DERP code (a moving target) for a single on-demand
/// diagnostic, so we don't. The CLI is therefore an *optional* dependency: when
/// absent, only this diagnostics panel is unavailable (it degrades gracefully).
#[derive(Debug, Clone, Deserialize)]
pub struct NetcheckReport {
    #[serde(rename = "UDP", default)]
    pub udp: bool,
    #[serde(rename = "IPv4", default)]
    pub ipv4: bool,
    #[serde(rename = "IPv6", default)]
    pub ipv6: bool,
    #[serde(
        rename = "MappingVariesByDestIP",
        default,
        deserialize_with = "opt_bool"
    )]
    pub mapping_varies: Option<bool>,
    #[serde(rename = "UPnP", default, deserialize_with = "opt_bool")]
    pub upnp: Option<bool>,
    #[serde(rename = "PMP", default, deserialize_with = "opt_bool")]
    pub pmp: Option<bool>,
    #[serde(rename = "PCP", default, deserialize_with = "opt_bool")]
    pub pcp: Option<bool>,
    #[serde(rename = "CaptivePortal", default, deserialize_with = "opt_bool")]
    pub captive_portal: Option<bool>,
    #[serde(rename = "PreferredDERP", default)]
    pub preferred_derp: i64,
    #[serde(rename = "GlobalV4", default)]
    pub global_v4: String,
    #[serde(rename = "GlobalV6", default)]
    pub global_v6: String,
    /// DERP region ID → round-trip latency in nanoseconds.
    #[serde(rename = "RegionLatency", default)]
    pub region_latency: std::collections::HashMap<String, i64>,
}

/// Runs a connectivity check via the `tailscale` CLI. Returns a clear,
/// actionable error when the CLI isn't installed so callers can degrade rather
/// than fail silently.
pub fn netcheck() -> Result<NetcheckReport> {
    let out = match std::process::Command::new("tailscale")
        .args(["netcheck", "--format=json"])
        .output()
    {
        Ok(out) => out,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            bail!("the tailscale CLI is not installed (required only for netcheck diagnostics)")
        }
        Err(e) => return Err(e).context("run `tailscale netcheck`"),
    };
    if !out.status.success() {
        bail!(
            "tailscale netcheck failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    serde_json::from_slice(&out.stdout).context("parse netcheck JSON")
}

/// Deserializes Tailscale's tri-state `opt.Bool` (a bool, or the strings
/// "true"/"false"/"", or null) into `Option<bool>`.
fn opt_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum BoolOrStr {
        Bool(bool),
        Str(String),
    }
    Ok(match Option::<BoolOrStr>::deserialize(deserializer)? {
        None => None,
        Some(BoolOrStr::Bool(b)) => Some(b),
        Some(BoolOrStr::Str(s)) => match s.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// Golden-fixture tests: deserialize representative (PII-free) LocalAPI JSON from
// testdata/ into our mirror structs and assert the fields alavai relies on. This
// is the drift tripwire — if an upstream rename/removal breaks a field we use,
// these fail. The fixtures intentionally include extra/unknown keys to prove
// parsing stays lenient (additive upstream changes must remain non-events).
//
// `live_daemon_matches_fixtures` (ignored by default) is the stronger check: it
// hits the real running tailscaled and deserializes its actual output. Run it
// against a pinned daemon — `cargo test -- --ignored` — when bumping
// TESTED_TAILSCALE_VERSION or refreshing fixtures (see docs/SYNCING.md).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_fixture() {
        let s: Status = serde_json::from_str(include_str!("../testdata/status.json"))
            .expect("status.json should deserialize");
        assert!(s.online());
        assert_eq!(s.version, "1.98.4");
        let self_node = s.self_node.as_ref().unwrap();
        assert_eq!(self_node.hostname, "laptop");
        assert_eq!(self_node.relay, "syd");
        assert_eq!(s.tailscale_ips.len(), 2);
        assert_eq!(s.peers.len(), 2);

        let fileserver = s
            .peers
            .values()
            .find(|p| p.hostname == "fileserver")
            .expect("fileserver peer present");
        assert!(fileserver.online);
        assert!(fileserver.exit_node_option);
        assert_eq!(fileserver.primary_routes, vec!["192.168.1.0/24"]);

        let phone = s.peers.values().find(|p| p.hostname == "phone").unwrap();
        assert!(!phone.online);
        // PrimaryRoutes: null must decode to an empty Vec, not error.
        assert!(phone.primary_routes.is_empty());
    }

    #[test]
    fn parses_prefs_fixture() {
        let p: Prefs = serde_json::from_str(include_str!("../testdata/prefs.json"))
            .expect("prefs.json should deserialize");
        assert!(p.route_all);
        assert!(p.want_running);
        assert!(p.exit_node_active());
        assert!(p.advertises_exit_node());
        assert_eq!(p.operator_user, "alex");
        // The default routes are filtered out of the user-facing subnet list.
        assert_eq!(p.subnet_routes(), vec!["192.168.50.0/24"]);
    }

    #[test]
    fn parses_profiles_fixture() {
        let profiles: Vec<Profile> =
            serde_json::from_str(include_str!("../testdata/profiles.json"))
                .expect("profiles.json should deserialize");
        assert_eq!(profiles.len(), 3);
        // label() prefers the domain name.
        assert_eq!(profiles[0].label(), "example-tnet.ts.net");
        assert_eq!(profiles[0].user.login_name, "alice@example.com");
        // The trailing empty placeholder profile is recognised as empty.
        assert!(profiles[2].is_empty());
    }

    #[test]
    fn parses_netcheck_fixture() {
        let r: NetcheckReport = serde_json::from_str(include_str!("../testdata/netcheck.json"))
            .expect("netcheck.json should deserialize");
        assert!(r.udp);
        assert!(r.ipv4);
        assert!(!r.ipv6);
        assert_eq!(r.mapping_varies, Some(false));
        assert_eq!(r.upnp, Some(false));
        assert_eq!(r.pcp, None); // null tri-state → None
        assert_eq!(r.preferred_derp, 5);
        assert_eq!(r.global_v4, "203.0.113.7:41641");
        assert_eq!(r.region_latency.get("5"), Some(&27412187));
    }

    #[test]
    fn merges_state_notify() {
        let n: Notify =
            serde_json::from_str(include_str!("../testdata/notify_state.json")).unwrap();
        assert_eq!(n.state, Some(STATE_RUNNING));
        let mut live = LiveState::default();
        assert!(merge_notify(&mut live, n));
        assert!(live.online);
        assert!(!live.needs_login);
    }

    #[test]
    fn merges_prefs_notify() {
        let n: Notify =
            serde_json::from_str(include_str!("../testdata/notify_prefs.json")).unwrap();
        let mut live = LiveState::default();
        assert!(merge_notify(&mut live, n));
        assert!(live.exit_node_active);
        assert!(live.accept_routes);
        assert!(live.advertise_exit_node);
        assert!(live.allow_lan);
        assert_eq!(live.advertised_routes, vec!["192.168.50.0/24"]);
    }

    #[test]
    fn merges_initial_status_notify() {
        // The first bus frame carries the full ipnstate.Status; self identity is
        // seeded from Status.Self + the bare top-level TailscaleIPs.
        let n: Notify =
            serde_json::from_str(include_str!("../testdata/notify_initialstatus.json")).unwrap();
        let mut live = LiveState::default();
        assert!(merge_notify(&mut live, n));
        assert_eq!(live.machine, "laptop");
        assert_eq!(live.fqdn, "laptop.example-tnet.ts.net");
        assert_eq!(live.os, "linux");
        assert_eq!(live.address, "100.100.0.1"); // IPv4 preferred
        // Initial status implies a fresh peer baseline → trigger a peer refresh.
        assert!(live.netmap_changed);
    }

    #[test]
    fn tolerates_logged_out_initial_status() {
        // A logged-out daemon sends TailscaleIPs/Peer as explicit JSON null on
        // the bus; parsing must not fail (regression: tailscale 1.104 bus).
        let n: Notify = serde_json::from_str(
            r#"{"InitialStatus":{"BackendState":"NeedsLogin","TailscaleIPs":null,"Peer":null,"Self":{"HostName":"diablo","DNSName":"","OS":"linux","TailscaleIPs":null}}}"#,
        )
        .expect("logged-out InitialStatus must parse");
        let mut live = LiveState::default();
        merge_notify(&mut live, n);
        assert_eq!(live.machine, "diablo");
        assert!(live.addresses.is_empty());
        assert!(live.netmap_changed);
    }

    #[test]
    fn merges_self_change_notify() {
        // Runtime self updates arrive as a tailcfg.Node under SelfChange, whose
        // addresses are CIDRs and get stripped to bare IPs.
        let n: Notify =
            serde_json::from_str(include_str!("../testdata/notify_selfchange.json")).unwrap();
        let mut live = LiveState::default();
        assert!(merge_notify(&mut live, n));
        assert_eq!(live.machine, "laptop");
        assert_eq!(live.fqdn, "laptop.example-tnet.ts.net");
        assert_eq!(live.os, "linux");
        assert_eq!(live.address, "100.100.0.1");
        // A bare SelfChange is not a peer-set delta.
        assert!(!live.netmap_changed);
    }

    #[test]
    fn merges_peers_changed_notify() {
        // A peer-set delta carries no self fields but must flag a peer refresh.
        let n: Notify = serde_json::from_str(r#"{"PeersRemoved":["nABC"]}"#).unwrap();
        let mut live = LiveState::default();
        assert!(merge_notify(&mut live, n));
        assert!(live.netmap_changed);
    }

    #[test]
    fn version_warning_only_on_major_mismatch() {
        assert_eq!(untested_version_warning("1.98.4"), None);
        assert_eq!(untested_version_warning("1.2.0"), None);
        assert_eq!(untested_version_warning(""), None);
        assert!(untested_version_warning("2.0.0").is_some());
    }

    /// Drift tripwire against the *real* daemon. Ignored by default (needs a
    /// running tailscaled + operator perms); run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "requires a running tailscaled and operator permissions"]
    fn live_daemon_matches_fixtures() {
        let c = Client::default();
        let status = c.status().expect("live status should parse");
        assert!(!status.backend_state.is_empty());
        c.prefs().expect("live prefs should parse");
        c.profiles().expect("live profiles should parse");
        c.current_profile()
            .expect("live current profile should parse");
    }
}
