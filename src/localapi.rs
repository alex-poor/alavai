//! Minimal client for the Tailscale LocalAPI.
//!
//! `tailscaled` exposes an HTTP API over a unix domain socket
//! (`/run/tailscale/tailscaled.sock`). Trayscale talks to this same API via
//! the Go `tailscale.com/client/local` package; we reimplement just the slice
//! of it that alavai needs, with no Go and no bundled Tailscale library.
//!
//! Phase 0 uses a blocking, one-shot-per-request client — perfectly adequate
//! for a CLI and for the tray's occasional commands. Phase 2 adds an async
//! client (tokio + hyper) for the long-lived `watch-ipn-bus` event stream.
//!
//! Accessing the socket requires the current user to be the Tailscale
//! "operator" (`sudo tailscale set --operator=$USER`), exactly as trayscale
//! documents.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

const SOCKET_PATHS: &[&str] = &[
    "/run/tailscale/tailscaled.sock",
    "/var/run/tailscale/tailscaled.sock",
];

/// A blocking handle to the local `tailscaled` daemon.
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
    #[serde(rename = "ControlURL", default)]
    pub control_url: String,
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
    #[serde(rename = "MagicDNSName", default)]
    pub magic_dns_name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserProfile {
    #[serde(rename = "LoginName", default)]
    pub login_name: String,
    #[serde(rename = "DisplayName", default)]
    pub display_name: String,
}

/// A minimal view of the LocalAPI `status` response. Expanded in later phases.
#[derive(Debug, Clone, Deserialize)]
pub struct Status {
    #[serde(rename = "BackendState")]
    pub backend_state: String,
    #[serde(rename = "TailscaleIPs", default)]
    pub tailscale_ips: Vec<String>,
    #[serde(rename = "Self")]
    pub self_node: Option<Node>,
}

impl Status {
    pub fn online(&self) -> bool {
        self.backend_state == "Running"
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Node {
    #[serde(rename = "HostName", default)]
    pub hostname: String,
    #[serde(rename = "DNSName", default)]
    pub dns_name: String,
}

/// Splits an HTTP/1.1 response into status + body, dechunking if necessary, and
/// returns the body bytes. Errors on a non-2xx status, surfacing the body text.
fn parse_http_response(raw: &[u8]) -> Result<Vec<u8>> {
    let split = find(raw, b"\r\n\r\n").ok_or_else(|| anyhow!("malformed HTTP response (no header terminator)"))?;
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
        bail!("LocalAPI returned HTTP {code}: {}", String::from_utf8_lossy(&body).trim());
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
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}
