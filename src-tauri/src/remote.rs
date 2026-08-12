use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::{
    store::{config_dir, glob_match},
    types::HostProfile,
};

/// The local daemon's default listen address. Single source of truth shared by
/// the daemon (what it binds to) and every client surface (what they dial).
pub const DEFAULT_DAEMON_ADDR: &str = "127.0.0.1:7722";

/// The local daemon's configured listen address (`host:port`), honoring
/// `AGENT2SSH_DAEMON_ADDR`; defaults to [`DEFAULT_DAEMON_ADDR`]. This is the raw
/// bind address — it may be an unspecified host like `0.0.0.0`/`::` — so the
/// daemon binds exactly here. Clients should use [`local_daemon_url`] /
/// [`local_daemon_connect_addr`] instead, which map a wildcard back to loopback.
pub fn local_daemon_addr() -> String {
    std::env::var("AGENT2SSH_DAEMON_ADDR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_DAEMON_ADDR.to_string())
}

/// The `host:port` a *client* should dial to reach the local daemon. Identical
/// to [`local_daemon_addr`], except an unspecified bind host (`0.0.0.0` / `::`)
/// is mapped to loopback — you cannot connect to the wildcard address. IPv6
/// hosts keep their brackets so the result parses as a `SocketAddr` and embeds
/// cleanly in a URL.
pub fn local_daemon_connect_addr() -> String {
    normalize_connect_addr(&local_daemon_addr())
}

/// The base URL (`http://host:port`, no trailing slash) for the local daemon,
/// derived from [`local_daemon_connect_addr`]. Use this everywhere a client
/// surface (CLI/MCP/Tauri/webhook links) needs to reach the local daemon so a
/// non-default `AGENT2SSH_DAEMON_ADDR` stays reachable end to end.
pub fn local_daemon_url() -> String {
    format!("http://{}", local_daemon_connect_addr())
}

/// Whether a `host:port` listen address resolves to loopback only. The daemon
/// uses this to warn when an operator binds it somewhere reachable off-host.
pub fn is_loopback_addr(addr: &str) -> bool {
    let host = match addr.rsplit_once(':') {
        Some((host, _port)) => host,
        None => addr,
    };
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

fn normalize_connect_addr(addr: &str) -> String {
    let (host, port) = match addr.rsplit_once(':') {
        Some((host, port)) => (host.trim(), port.trim()),
        None => (addr.trim(), "7722"),
    };
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    match bare.parse::<std::net::IpAddr>() {
        // Wildcard bind address: a client must dial loopback instead.
        Ok(ip) if ip.is_unspecified() => {
            if ip.is_ipv6() {
                format!("[::1]:{port}")
            } else {
                format!("127.0.0.1:{port}")
            }
        }
        // Concrete IPv6 literal: keep it bracketed for URL/SocketAddr use.
        Ok(std::net::IpAddr::V6(v6)) => format!("[{v6}]:{port}"),
        // Concrete IPv4 literal.
        Ok(_) => format!("{bare}:{port}"),
        // Hostname (e.g. "localhost"): pass through untouched.
        Err(_) => format!("{host}:{port}"),
    }
}

/// Permission scope for a remote daemon, controlling which hosts, tags,
/// and commands the daemon is allowed to execute.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DaemonScope {
    /// Allowed host names (empty = all hosts)
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    /// Allowed host tags (empty = all tags)
    #[serde(default)]
    pub allowed_tags: Vec<String>,
    /// Allowed command patterns (glob-style, empty = all commands)
    #[serde(default)]
    pub allowed_commands: Vec<String>,
    /// Denied command patterns (glob-style, checked BEFORE allowed_commands)
    #[serde(default)]
    pub denied_commands: Vec<String>,
}

/// A remote daemon entry configured in ~/.agent2ssh/remotes.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteDaemon {
    pub alias: String,
    pub url: String,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub token_env: Option<String>,
    /// Optional permission scope restricting what this daemon can execute.
    #[serde(default)]
    pub scope: Option<DaemonScope>,
}

/// The full remotes.toml file structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemotesFile {
    #[serde(default)]
    pub remotes: Vec<RemoteDaemon>,
}

/// A scoped bearer token accepted by the daemon server.
///
/// These are configured in `~/.agent2ssh/daemon_tokens.toml`. The legacy
/// `daemon.token` remains the unrestricted local admin token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopedDaemonToken {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(default)]
    pub scope: Option<DaemonScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DaemonTokensFile {
    #[serde(default)]
    pub tokens: Vec<ScopedDaemonToken>,
}

/// Daemon info returned to clients (CLI, MCP, Tauri, Web)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub alias: String,
    pub url: String,
    pub connected: bool,
    /// Permission scope, if configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<DaemonScope>,
}

/// Load remote daemons from ~/.agent2ssh/remotes.toml
pub fn load_remotes() -> Result<Vec<RemoteDaemon>> {
    let path = config_dir()?.join("remotes.toml");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path)?;
    let file: RemotesFile = toml::from_str(&raw)?;
    validate_remotes(&file.remotes)?;
    Ok(file.remotes)
}

fn validate_remotes(remotes: &[RemoteDaemon]) -> Result<()> {
    let mut aliases = HashSet::new();
    for remote in remotes {
        if remote.alias.trim().is_empty() {
            return Err(anyhow!("remote daemon alias cannot be empty"));
        }
        if remote.alias == "localhost" {
            return Err(anyhow!("remote daemon alias 'localhost' is reserved"));
        }
        if !aliases.insert(remote.alias.as_str()) {
            return Err(anyhow!("duplicate remote daemon alias '{}'", remote.alias));
        }
        if !(remote.url.starts_with("http://") || remote.url.starts_with("https://")) {
            return Err(anyhow!(
                "remote daemon '{}' URL must start with http:// or https://",
                remote.alias
            ));
        }
        if remote.token_env.as_deref().unwrap_or("").trim().is_empty()
            && remote.token.as_deref().unwrap_or("").trim().is_empty()
        {
            return Err(anyhow!(
                "remote daemon '{}' must set token_env or token",
                remote.alias
            ));
        }
    }
    Ok(())
}

static SCOPED_TOKENS_CACHE: crate::config_cache::ConfigCache<Vec<ScopedDaemonToken>> =
    crate::config_cache::ConfigCache::new();

pub fn load_scoped_daemon_tokens() -> Result<Vec<ScopedDaemonToken>> {
    let path = config_dir()?.join("daemon_tokens.toml");
    SCOPED_TOKENS_CACHE.load_with(&path, || {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&path)?;
        let file: DaemonTokensFile = toml::from_str(&raw)?;
        validate_scoped_daemon_tokens(&file.tokens)?;
        Ok(file.tokens)
    })
}

fn validate_scoped_daemon_tokens(tokens: &[ScopedDaemonToken]) -> Result<()> {
    let mut names = HashSet::new();
    for token in tokens {
        if let Some(name) = token.name.as_deref() {
            let name = name.trim();
            if name.is_empty() {
                return Err(anyhow!("scoped daemon token name cannot be empty"));
            }
            if !names.insert(name.to_string()) {
                return Err(anyhow!("duplicate scoped daemon token name '{}'", name));
            }
        }
        if token.token_env.as_deref().unwrap_or("").trim().is_empty()
            && token.token.as_deref().unwrap_or("").trim().is_empty()
        {
            return Err(anyhow!("scoped daemon token must set token_env or token"));
        }
        if token.scope.is_none() {
            return Err(anyhow!("scoped daemon token must define a scope"));
        }
    }
    Ok(())
}

/// List all configured daemons (localhost + remotes).
///
/// Connectivity is probed by hitting each daemon's /health endpoint
/// with a short timeout. Failures are reported as `connected: false`
/// rather than returning an error, so a single unreachable remote
/// never breaks the whole listing.
pub fn list_daemons_core() -> Result<Vec<DaemonInfo>> {
    let mut daemons = Vec::new();

    // Always include localhost as first entry
    let local_url = local_daemon_url();
    let local_connected = check_health_blocking(&local_url);
    daemons.push(DaemonInfo {
        alias: "localhost".to_string(),
        url: local_url,
        connected: local_connected,
        scope: None,
    });

    // Add all configured remotes
    let remotes = load_remotes()?;
    for remote in remotes {
        let connected = check_health_blocking(&remote.url);
        daemons.push(DaemonInfo {
            alias: remote.alias,
            url: remote.url,
            connected,
            scope: remote.scope,
        });
    }

    Ok(daemons)
}

/// Resolve the effective token for a remote daemon.
///
/// Priority: `token_env` (read from environment) > `token` (literal field).
pub fn resolve_token(remote: &RemoteDaemon) -> Option<String> {
    if let Some(env_var) = &remote.token_env {
        std::env::var(env_var).ok()
    } else {
        remote.token.clone()
    }
}

pub fn resolve_scoped_daemon_token(token: &ScopedDaemonToken) -> Option<String> {
    if let Some(env_var) = &token.token_env {
        std::env::var(env_var).ok()
    } else {
        token.token.clone()
    }
}

/// Get daemon URL and token by alias.
///
/// * `"localhost"` -> local daemon URL + token from `~/.agent2ssh/daemon.token`
/// * Anything else -> looked up in the remotes list
pub fn get_daemon(alias: &str) -> Result<(String, Option<String>)> {
    if alias == "localhost" {
        let token = read_local_token();
        return Ok((local_daemon_url(), token));
    }
    let remotes = load_remotes()?;
    for remote in &remotes {
        if remote.alias == alias {
            return Ok((remote.url.clone(), resolve_token(remote)));
        }
    }
    Err(anyhow!("daemon '{}' not found in remotes", alias))
}

/// Get daemon URL, token, and locally configured permission scope by alias.
pub fn get_daemon_with_scope(alias: &str) -> Result<(String, Option<String>, Option<DaemonScope>)> {
    if alias == "localhost" {
        let token = read_local_token();
        return Ok((local_daemon_url(), token, None));
    }
    let remotes = load_remotes()?;
    for remote in &remotes {
        if remote.alias == alias {
            return Ok((
                remote.url.clone(),
                resolve_token(remote),
                remote.scope.clone(),
            ));
        }
    }
    Err(anyhow!("daemon '{}' not found in remotes", alias))
}

pub async fn remote_host_tags(url: &str, token: &str, host_name: &str) -> Result<Vec<String>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let hosts = client
        .get(format!("{}/hosts", url.trim_end_matches('/')))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<HostProfile>>()
        .await?;

    hosts
        .into_iter()
        .find(|host| host.name.eq_ignore_ascii_case(host_name))
        .map(|host| host.tags)
        .ok_or_else(|| anyhow!("host '{}' not found on remote daemon", host_name))
}

pub async fn tags_for_remote_scope_check(
    scope: &Option<DaemonScope>,
    url: &str,
    token: &str,
    host_name: &str,
    fallback_tags: Vec<String>,
) -> Result<Vec<String>> {
    let needs_remote_tags = scope
        .as_ref()
        .map(|scope| !scope.allowed_tags.is_empty())
        .unwrap_or(false);
    if needs_remote_tags {
        remote_host_tags(url, token, host_name).await
    } else {
        Ok(fallback_tags)
    }
}

/// Read the local daemon token from ~/.agent2ssh/daemon.token
pub fn read_local_token() -> Option<String> {
    let path = config_dir().ok()?.join("daemon.token");
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
}

/// Synchronous health check (used by list_daemons_core which is not async).
/// Returns true if GET /health responds 200 within 2 seconds.
fn check_health_blocking(url: &str) -> bool {
    let health_url = format!("{}/health", url.trim_end_matches('/'));
    // Use a simple blocking HTTP GET via a short-lived thread + std TCP.
    // We avoid pulling in a blocking HTTP client just for this probe.
    std::thread::scope(|s| {
        let handle = s.spawn(move || -> bool {
            use std::io::{Read, Write};
            use std::net::TcpStream;
            use std::time::Duration;

            // Parse host:port from the URL
            let without_scheme = health_url
                .strip_prefix("http://")
                .or_else(|| health_url.strip_prefix("https://"))
                .unwrap_or(&health_url);
            let host_port = without_scheme.split('/').next().unwrap_or(without_scheme);

            let stream = match TcpStream::connect_timeout(
                &host_port.parse().unwrap_or_else(|_| {
                    // Fallback: dial the default loopback daemon address.
                    DEFAULT_DAEMON_ADDR.parse().unwrap()
                }),
                Duration::from_secs(2),
            ) {
                Ok(s) => s,
                Err(_) => return false,
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

            let mut stream = stream;
            let request = format!(
                "GET /health HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                host_port
            );
            if stream.write_all(request.as_bytes()).is_err() {
                return false;
            }

            let mut buf = Vec::new();
            if stream.read_to_end(&mut buf).is_err() {
                return false;
            }
            let response = String::from_utf8_lossy(&buf);
            response.contains("200")
        });
        handle.join().unwrap_or(false)
    })
}

// ── Multi-Daemon Unified View (F5-4) ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonUnifiedView {
    pub daemons: Vec<DaemonViewEntry>,
    pub total_hosts: usize,
    pub total_connected: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonViewEntry {
    pub alias: String,
    pub url: String,
    pub connected: bool,
    pub host_count: Option<usize>,
    pub health: Option<DaemonHealthSummary>,
    pub metrics: Option<DaemonMetricsSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonHealthSummary {
    pub version: Option<String>,
    pub uptime_secs: Option<u64>,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonMetricsSummary {
    pub request_count: Option<u64>,
    pub exec_count: Option<u64>,
    pub exec_blocked_count: Option<u64>,
    pub approval_count: Option<u64>,
}

/// Get a unified view of all daemons with their health and metrics.
/// For each connected daemon, fetches /health and /metrics endpoints.
pub async fn get_daemons_unified_view() -> Result<DaemonUnifiedView> {
    let daemons = list_daemons_core()?;
    let mut entries = Vec::new();
    let mut total_hosts: usize = 0;
    let mut total_connected: usize = 0;

    for d in &daemons {
        let mut entry = DaemonViewEntry {
            alias: d.alias.clone(),
            url: d.url.clone(),
            connected: d.connected,
            host_count: None,
            health: None,
            metrics: None,
        };

        if d.connected {
            total_connected += 1;

            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());

            let base_url = d.url.trim_end_matches('/');
            let token = if d.alias == "localhost" {
                read_local_token()
            } else {
                load_remotes().ok().and_then(|remotes| {
                    remotes
                        .iter()
                        .find(|r| r.alias == d.alias)
                        .and_then(resolve_token)
                })
            };

            // Fetch /health
            let mut health_req = client.get(format!("{}/health", base_url));
            if let Some(ref t) = token {
                health_req = health_req.bearer_auth(t);
            }
            if let Ok(resp) = health_req.send().await {
                if resp.status().is_success() {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        entry.health = Some(DaemonHealthSummary {
                            version: body
                                .get("version")
                                .and_then(|v| v.as_str())
                                .map(String::from),
                            uptime_secs: body.get("uptime_secs").and_then(|v| v.as_u64()),
                            pid: body.get("pid").and_then(|v| v.as_u64()).map(|v| v as u32),
                        });
                    }
                }
            }

            // Fetch /metrics
            let mut metrics_req = client.get(format!("{}/metrics", base_url));
            if let Some(ref t) = token {
                metrics_req = metrics_req.bearer_auth(t);
            }
            if let Ok(resp) = metrics_req.send().await {
                if resp.status().is_success() {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        entry.metrics = Some(DaemonMetricsSummary {
                            request_count: body.get("requests_total").and_then(|v| v.as_u64()),
                            exec_count: body.get("exec_total").and_then(|v| v.as_u64()),
                            exec_blocked_count: body
                                .get("exec_blocked_total")
                                .and_then(|v| v.as_u64()),
                            approval_count: body.get("approvals_total").and_then(|v| v.as_u64()),
                        });
                    }
                }
            }

            // Fetch /hosts count
            let mut hosts_req = client.get(format!("{}/hosts", base_url));
            if let Some(ref t) = token {
                hosts_req = hosts_req.bearer_auth(t);
            }
            if let Ok(resp) = hosts_req.send().await {
                if resp.status().is_success() {
                    if let Ok(hosts) = resp.json::<Vec<serde_json::Value>>().await {
                        let count = hosts.len();
                        total_hosts += count;
                        entry.host_count = Some(count);
                    }
                }
            }
        }

        entries.push(entry);
    }

    Ok(DaemonUnifiedView {
        daemons: entries,
        total_hosts,
        total_connected,
    })
}

// ── Daemon Permission Scope (F5-3) ──────────────────────────────────────────

/// Check if a remote daemon is allowed to execute a command on a host.
///
/// Rules (evaluated in order):
/// 1. If no scope is configured (`None`), all commands are allowed.
/// 2. If `denied_commands` is non-empty and the command matches any denied
///    pattern, the command is rejected.
/// 3. If `allowed_hosts` is non-empty and the host is not in the list,
///    the command is rejected.
/// 4. If `allowed_tags` is non-empty and no host tag matches any allowed
///    tag, the command is rejected.
/// 5. If `allowed_commands` is non-empty and the command does not match any
///    allowed pattern, the command is rejected.
/// 6. Otherwise the command is allowed.
pub fn check_daemon_scope(
    scope: &Option<DaemonScope>,
    host_name: &str,
    host_tags: &[String],
    command: &str,
) -> Result<(), String> {
    let scope = match scope {
        Some(s) => s,
        None => return Ok(()),
    };

    // Check denied commands first
    for pattern in &scope.denied_commands {
        if glob_match(pattern, command) {
            return Err(format!(
                "command '{}' is denied by pattern '{}'",
                command, pattern
            ));
        }
    }

    // Check allowed hosts
    if !scope.allowed_hosts.is_empty()
        && !scope
            .allowed_hosts
            .iter()
            .any(|h| h.eq_ignore_ascii_case(host_name))
    {
        return Err(format!(
            "host '{}' is not in the allowed hosts list",
            host_name
        ));
    }

    // Check allowed tags
    if !scope.allowed_tags.is_empty() {
        let has_matching_tag = scope
            .allowed_tags
            .iter()
            .any(|allowed| host_tags.iter().any(|t| t.eq_ignore_ascii_case(allowed)));
        if !has_matching_tag {
            return Err(format!(
                "host '{}' has no tags matching allowed tags {:?}",
                host_name, scope.allowed_tags
            ));
        }
    }

    // Check allowed commands
    if !scope.allowed_commands.is_empty() {
        let matches_allowed = scope
            .allowed_commands
            .iter()
            .any(|pattern| glob_match(pattern, command));
        if !matches_allowed {
            return Err(format!(
                "command '{}' does not match any allowed command pattern",
                command
            ));
        }
    }

    Ok(())
}

// ── Daemon Diagnostic Types (F5-1) ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonDiagnostic {
    pub alias: String,
    pub url: String,
    pub checks: Vec<DiagnosticCheck>,
    pub overall_status: DiagnosticStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticCheck {
    pub name: String,
    pub status: DiagnosticStatus,
    pub message: String,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticStatus {
    Ok,
    Warning,
    Error,
}

impl std::fmt::Display for DiagnosticStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiagnosticStatus::Ok => write!(f, "ok"),
            DiagnosticStatus::Warning => write!(f, "warning"),
            DiagnosticStatus::Error => write!(f, "error"),
        }
    }
}

/// Run diagnostic checks against a remote daemon.
pub async fn diagnose_daemon(alias: &str) -> Result<DaemonDiagnostic> {
    let (url, token) = get_daemon(alias)?;
    let mut checks = Vec::new();

    // Parse host:port from URL
    let without_scheme = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(&url);
    let host_port = without_scheme.split('/').next().unwrap_or(without_scheme);
    let is_https = url.starts_with("https://");

    // a. DNS/TCP connectivity
    let tcp_result: Result<std::net::SocketAddr> = host_port
        .parse()
        .map_err(|e: std::net::AddrParseError| anyhow!("invalid address '{}': {}", host_port, e));

    let tcp_addr = match tcp_result {
        Ok(addr) => {
            let connect_result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio::net::TcpStream::connect(addr),
            )
            .await;
            match connect_result {
                Ok(Ok(_)) => {
                    checks.push(DiagnosticCheck {
                        name: "TCP connectivity".to_string(),
                        status: DiagnosticStatus::Ok,
                        message: format!("Successfully connected to {}", host_port),
                        details: None,
                    });
                    Some(addr)
                }
                Ok(Err(e)) => {
                    checks.push(DiagnosticCheck {
                        name: "TCP connectivity".to_string(),
                        status: DiagnosticStatus::Error,
                        message: format!("Cannot connect to {}: {}", host_port, e),
                        details: None,
                    });
                    None
                }
                Err(_) => {
                    checks.push(DiagnosticCheck {
                        name: "TCP connectivity".to_string(),
                        status: DiagnosticStatus::Error,
                        message: format!("Connection to {} timed out (5s)", host_port),
                        details: None,
                    });
                    None
                }
            }
        }
        Err(e) => {
            checks.push(DiagnosticCheck {
                name: "TCP connectivity".to_string(),
                status: DiagnosticStatus::Error,
                message: format!("Invalid address: {}", e),
                details: None,
            });
            None
        }
    };

    // b. TLS check
    if is_https {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .danger_accept_invalid_certs(false)
            .build();
        match client {
            Ok(c) => match c.get(&url).send().await {
                Ok(_) => {
                    checks.push(DiagnosticCheck {
                        name: "TLS handshake".to_string(),
                        status: DiagnosticStatus::Ok,
                        message: "TLS handshake succeeded".to_string(),
                        details: None,
                    });
                }
                Err(e) => {
                    let status = if e.is_connect() {
                        DiagnosticStatus::Error
                    } else {
                        DiagnosticStatus::Warning
                    };
                    checks.push(DiagnosticCheck {
                        name: "TLS handshake".to_string(),
                        status,
                        message: format!("TLS issue: {}", e),
                        details: None,
                    });
                }
            },
            Err(e) => {
                checks.push(DiagnosticCheck {
                    name: "TLS handshake".to_string(),
                    status: DiagnosticStatus::Error,
                    message: format!("Failed to build TLS client: {}", e),
                    details: None,
                });
            }
        }
    } else {
        checks.push(DiagnosticCheck {
            name: "TLS handshake".to_string(),
            status: DiagnosticStatus::Warning,
            message: "Using plain HTTP (no TLS)".to_string(),
            details: Some("Consider using https:// for production".to_string()),
        });
    }

    // c. Token check
    let has_token = token.is_some();
    checks.push(DiagnosticCheck {
        name: "Token configured".to_string(),
        status: if has_token {
            DiagnosticStatus::Ok
        } else {
            DiagnosticStatus::Error
        },
        message: if has_token {
            "Authentication token is configured".to_string()
        } else {
            "No authentication token configured".to_string()
        },
        details: None,
    });

    // d. Auth check (GET /health with Bearer token) + e. Version check + f. Latency
    if tcp_addr.is_some() {
        let health_url = format!("{}/health", url.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .danger_accept_invalid_certs(false)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let start = std::time::Instant::now();
        let mut req = client.get(&health_url);
        if let Some(ref t) = token {
            req = req.bearer_auth(t);
        }

        match req.send().await {
            Ok(resp) => {
                let latency_ms = start.elapsed().as_millis();
                let status_code = resp.status();

                // Auth check
                if status_code == reqwest::StatusCode::UNAUTHORIZED
                    || status_code == reqwest::StatusCode::FORBIDDEN
                {
                    checks.push(DiagnosticCheck {
                        name: "Authentication".to_string(),
                        status: DiagnosticStatus::Error,
                        message: format!("Authentication failed (HTTP {})", status_code),
                        details: Some("Check that the token is valid".to_string()),
                    });
                } else if status_code.is_success() {
                    checks.push(DiagnosticCheck {
                        name: "Authentication".to_string(),
                        status: DiagnosticStatus::Ok,
                        message: format!("Authenticated successfully (HTTP {})", status_code),
                        details: None,
                    });

                    // Parse health body for version
                    match resp.json::<serde_json::Value>().await {
                        Ok(body) => {
                            // Version check
                            if let Some(version) = body.get("version").and_then(|v| v.as_str()) {
                                let compat = check_version_compatibility(Some(version));
                                let v_status = match compat.compatible {
                                    true if compat.local_version
                                        == compat.remote_version.as_deref().unwrap_or("") =>
                                    {
                                        DiagnosticStatus::Ok
                                    }
                                    true => DiagnosticStatus::Warning,
                                    false => DiagnosticStatus::Error,
                                };
                                checks.push(DiagnosticCheck {
                                    name: "Version".to_string(),
                                    status: v_status,
                                    message: compat.message,
                                    details: Some(format!(
                                        "local={}, remote={}",
                                        compat.local_version,
                                        compat.remote_version.as_deref().unwrap_or("unknown")
                                    )),
                                });
                            } else {
                                checks.push(DiagnosticCheck {
                                    name: "Version".to_string(),
                                    status: DiagnosticStatus::Warning,
                                    message: "Health response does not contain a version field"
                                        .to_string(),
                                    details: None,
                                });
                            }
                        }
                        Err(_) => {
                            checks.push(DiagnosticCheck {
                                name: "Version".to_string(),
                                status: DiagnosticStatus::Warning,
                                message: "Could not parse health response as JSON".to_string(),
                                details: None,
                            });
                        }
                    }
                } else {
                    checks.push(DiagnosticCheck {
                        name: "Authentication".to_string(),
                        status: DiagnosticStatus::Warning,
                        message: format!("Unexpected HTTP status: {}", status_code),
                        details: None,
                    });
                }

                // Latency check
                let lat_status = if latency_ms < 200 {
                    DiagnosticStatus::Ok
                } else if latency_ms < 1000 {
                    DiagnosticStatus::Warning
                } else {
                    DiagnosticStatus::Error
                };
                checks.push(DiagnosticCheck {
                    name: "Latency".to_string(),
                    status: lat_status,
                    message: format!("Round-trip: {}ms", latency_ms),
                    details: None,
                });
            }
            Err(e) => {
                checks.push(DiagnosticCheck {
                    name: "Authentication".to_string(),
                    status: DiagnosticStatus::Error,
                    message: format!("Health endpoint unreachable: {}", e),
                    details: None,
                });
            }
        }
    }

    // Aggregate overall status
    let overall = if checks.iter().any(|c| c.status == DiagnosticStatus::Error) {
        DiagnosticStatus::Error
    } else if checks.iter().any(|c| c.status == DiagnosticStatus::Warning) {
        DiagnosticStatus::Warning
    } else {
        DiagnosticStatus::Ok
    };

    Ok(DaemonDiagnostic {
        alias: alias.to_string(),
        url,
        checks,
        overall_status: overall,
    })
}

// ── Version Compatibility (F5-2) ────────────────────────────────────────────

/// The current protocol version for this build.
pub const PROTOCOL_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionCompatibility {
    pub local_version: String,
    pub remote_version: Option<String>,
    pub compatible: bool,
    pub message: String,
}

/// Check version compatibility with a remote daemon.
/// Compares the local PROTOCOL_VERSION with the version from the daemon's /health response.
pub fn check_version_compatibility(remote_version: Option<&str>) -> VersionCompatibility {
    let local = PROTOCOL_VERSION.to_string();
    match remote_version {
        None => VersionCompatibility {
            local_version: local,
            remote_version: None,
            compatible: true,
            message: "Unable to determine remote version".to_string(),
        },
        Some(rv) => {
            let normalize = |version: &str| {
                let core = version
                    .trim()
                    .trim_start_matches(['v', 'V'])
                    .split(['-', '+'])
                    .next()
                    .unwrap_or("");
                let parts = core
                    .split('.')
                    .map(str::parse::<u64>)
                    .collect::<Result<Vec<_>, _>>()
                    .ok()?;
                (parts.len() >= 2).then_some(parts)
            };
            let local_parts = normalize(&local).expect("PROTOCOL_VERSION must be numeric");
            let Some(remote_parts) = normalize(rv) else {
                return VersionCompatibility {
                    local_version: local,
                    remote_version: Some(rv.to_string()),
                    compatible: false,
                    message: format!("Invalid remote version: {rv}"),
                };
            };

            let local_major = local_parts.first().copied().unwrap_or(0);
            let local_minor = local_parts.get(1).copied().unwrap_or(0);
            let remote_major = remote_parts.first().copied().unwrap_or(0);
            let remote_minor = remote_parts.get(1).copied().unwrap_or(0);

            if local_major != remote_major {
                VersionCompatibility {
                    local_version: local.clone(),
                    remote_version: Some(rv.to_string()),
                    compatible: false,
                    message: format!(
                        "Major version mismatch: local {} vs remote {}. Protocol incompatible.",
                        local, rv
                    ),
                }
            } else if local_minor != remote_minor {
                VersionCompatibility {
                    local_version: local.clone(),
                    remote_version: Some(rv.to_string()),
                    compatible: true,
                    message: format!(
                        "Minor version difference: local {} vs remote {}. May have minor incompatibilities.",
                        local, rv
                    ),
                }
            } else {
                VersionCompatibility {
                    local_version: local.clone(),
                    remote_version: Some(rv.to_string()),
                    compatible: true,
                    message: format!("Versions match: local {} vs remote {}", local, rv),
                }
            }
        }
    }
}

/// Check version compatibility with a remote daemon by alias.
/// Contacts the daemon's /health endpoint and returns version compatibility info.
pub async fn check_daemon_version(alias: &str) -> Result<VersionCompatibility> {
    let (url, token) = get_daemon(alias)?;
    let health_url = format!("{}/health", url.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let mut req = client.get(&health_url);
    if let Some(ref t) = token {
        req = req.bearer_auth(t);
    }

    match req.send().await {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await?;
            let remote_version = body.get("version").and_then(|v| v.as_str());
            Ok(check_version_compatibility(remote_version))
        }
        Ok(resp) => {
            anyhow::bail!(
                "Daemon '{}' health check failed with status {}",
                alias,
                resp.status()
            );
        }
        Err(e) => {
            anyhow::bail!("Daemon '{}' unreachable: {}", alias, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(alias: &str) -> RemoteDaemon {
        RemoteDaemon {
            alias: alias.to_string(),
            url: "https://daemon.example.com:7722".to_string(),
            token: None,
            token_env: Some("AGENT2SSH_TOKEN".to_string()),
            scope: None,
        }
    }

    #[test]
    fn validate_remotes_accepts_valid_remote() {
        assert!(validate_remotes(&[remote("prod")]).is_ok());
    }

    #[test]
    fn normalize_connect_addr_maps_wildcard_and_brackets_ipv6() {
        // Loopback / concrete addresses pass through unchanged.
        assert_eq!(normalize_connect_addr("127.0.0.1:7722"), "127.0.0.1:7722");
        assert_eq!(
            normalize_connect_addr("192.168.1.5:9000"),
            "192.168.1.5:9000"
        );
        // Wildcard bind hosts are remapped to loopback for a client to dial.
        assert_eq!(normalize_connect_addr("0.0.0.0:7722"), "127.0.0.1:7722");
        assert_eq!(normalize_connect_addr("[::]:7722"), "[::1]:7722");
        // Concrete IPv6 keeps brackets; hostnames pass through.
        assert_eq!(normalize_connect_addr("[::1]:7722"), "[::1]:7722");
        assert_eq!(normalize_connect_addr("localhost:7722"), "localhost:7722");
        // A normalized address is always a valid SocketAddr (when host is an IP).
        assert!("127.0.0.1:7722".parse::<std::net::SocketAddr>().is_ok());
        assert!("[::1]:7722".parse::<std::net::SocketAddr>().is_ok());
    }

    #[test]
    fn is_loopback_addr_classifies_hosts() {
        assert!(is_loopback_addr("127.0.0.1:7722"));
        assert!(is_loopback_addr("localhost:7722"));
        assert!(is_loopback_addr("[::1]:7722"));
        assert!(!is_loopback_addr("0.0.0.0:7722"));
        assert!(!is_loopback_addr("192.168.1.5:7722"));
        assert!(!is_loopback_addr("[::]:7722"));
    }

    #[test]
    #[serial_test::serial]
    fn local_daemon_url_honors_env_override() {
        std::env::remove_var("AGENT2SSH_DAEMON_ADDR");
        assert_eq!(local_daemon_addr(), DEFAULT_DAEMON_ADDR);
        assert_eq!(local_daemon_url(), "http://127.0.0.1:7722");

        std::env::set_var("AGENT2SSH_DAEMON_ADDR", "0.0.0.0:9001");
        // The daemon binds to the wildcard, but clients dial loopback.
        assert_eq!(local_daemon_addr(), "0.0.0.0:9001");
        assert_eq!(local_daemon_url(), "http://127.0.0.1:9001");

        std::env::remove_var("AGENT2SSH_DAEMON_ADDR");
    }

    #[test]
    fn validate_remotes_rejects_reserved_alias() {
        assert!(validate_remotes(&[remote("localhost")]).is_err());
    }

    #[test]
    fn validate_remotes_rejects_duplicate_aliases() {
        assert!(validate_remotes(&[remote("prod"), remote("prod")]).is_err());
    }

    #[test]
    fn validate_remotes_rejects_missing_token() {
        let mut item = remote("prod");
        item.token_env = None;
        assert!(validate_remotes(&[item]).is_err());
    }

    #[test]
    fn validate_remotes_rejects_non_http_url() {
        let mut item = remote("prod");
        item.url = "file:///tmp/socket".to_string();
        assert!(validate_remotes(&[item]).is_err());
    }

    // ── F5-1: Diagnostic tests ──────────────────────────────────────────────

    #[test]
    fn test_diagnostic_status_display() {
        assert_eq!(DiagnosticStatus::Ok.to_string(), "ok");
        assert_eq!(DiagnosticStatus::Warning.to_string(), "warning");
        assert_eq!(DiagnosticStatus::Error.to_string(), "error");
    }

    #[test]
    fn test_diagnostic_check_serialization() {
        let check = DiagnosticCheck {
            name: "TCP connectivity".to_string(),
            status: DiagnosticStatus::Ok,
            message: "Connected".to_string(),
            details: Some("host:port reachable".to_string()),
        };
        let json = serde_json::to_string(&check).unwrap();
        let deserialized: DiagnosticCheck = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "TCP connectivity");
        assert_eq!(deserialized.status, DiagnosticStatus::Ok);
        assert_eq!(deserialized.message, "Connected");
        assert_eq!(deserialized.details.as_deref(), Some("host:port reachable"));
    }

    #[test]
    fn test_diagnostic_status_serde_roundtrip() {
        let statuses = vec![
            DiagnosticStatus::Ok,
            DiagnosticStatus::Warning,
            DiagnosticStatus::Error,
        ];
        for status in &statuses {
            let json = serde_json::to_string(status).unwrap();
            let deserialized: DiagnosticStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(*status, deserialized);
        }
    }

    #[test]
    fn test_diagnostic_overall_status_aggregation() {
        // All ok → overall ok
        let checks = [
            DiagnosticCheck {
                name: "a".into(),
                status: DiagnosticStatus::Ok,
                message: "fine".into(),
                details: None,
            },
            DiagnosticCheck {
                name: "b".into(),
                status: DiagnosticStatus::Ok,
                message: "fine".into(),
                details: None,
            },
        ];
        let overall = if checks.iter().any(|c| c.status == DiagnosticStatus::Error) {
            DiagnosticStatus::Error
        } else if checks.iter().any(|c| c.status == DiagnosticStatus::Warning) {
            DiagnosticStatus::Warning
        } else {
            DiagnosticStatus::Ok
        };
        assert_eq!(overall, DiagnosticStatus::Ok);

        // Any warning → overall warning
        let checks2 = [
            DiagnosticCheck {
                name: "a".into(),
                status: DiagnosticStatus::Ok,
                message: "fine".into(),
                details: None,
            },
            DiagnosticCheck {
                name: "b".into(),
                status: DiagnosticStatus::Warning,
                message: "warn".into(),
                details: None,
            },
        ];
        let overall2 = if checks2.iter().any(|c| c.status == DiagnosticStatus::Error) {
            DiagnosticStatus::Error
        } else if checks2
            .iter()
            .any(|c| c.status == DiagnosticStatus::Warning)
        {
            DiagnosticStatus::Warning
        } else {
            DiagnosticStatus::Ok
        };
        assert_eq!(overall2, DiagnosticStatus::Warning);

        // Any error → overall error
        let checks3 = [
            DiagnosticCheck {
                name: "a".into(),
                status: DiagnosticStatus::Warning,
                message: "warn".into(),
                details: None,
            },
            DiagnosticCheck {
                name: "b".into(),
                status: DiagnosticStatus::Error,
                message: "err".into(),
                details: None,
            },
        ];
        let overall3 = if checks3.iter().any(|c| c.status == DiagnosticStatus::Error) {
            DiagnosticStatus::Error
        } else if checks3
            .iter()
            .any(|c| c.status == DiagnosticStatus::Warning)
        {
            DiagnosticStatus::Warning
        } else {
            DiagnosticStatus::Ok
        };
        assert_eq!(overall3, DiagnosticStatus::Error);
    }

    // ── F5-2: Version compatibility tests ───────────────────────────────────

    #[test]
    fn test_version_compatibility_same() {
        let compat = check_version_compatibility(Some(PROTOCOL_VERSION));
        assert!(compat.compatible);
        assert_eq!(compat.local_version, PROTOCOL_VERSION);
        assert_eq!(compat.remote_version.as_deref(), Some(PROTOCOL_VERSION));
        assert!(compat.message.contains("match"));
    }

    #[test]
    fn test_version_compatibility_accepts_v_prefixed_tag() {
        let tag = format!("v{PROTOCOL_VERSION}");
        let compat = check_version_compatibility(Some(&tag));
        assert!(compat.compatible);
        assert_eq!(compat.remote_version.as_deref(), Some(tag.as_str()));
        assert!(compat.message.contains("match"));
    }

    #[test]
    fn test_version_compatibility_minor_diff() {
        // Use a version with a different minor: local major same, minor+10
        let local_parts: Vec<u64> = PROTOCOL_VERSION
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect();
        let local_major = local_parts.first().copied().unwrap_or(0);
        let local_minor = local_parts.get(1).copied().unwrap_or(0);
        let diff_minor = format!("{}.{}.99", local_major, local_minor + 10);
        let compat = check_version_compatibility(Some(&diff_minor));
        assert!(compat.compatible);
        assert!(compat.message.contains("Minor version difference"));
    }

    #[test]
    fn test_version_compatibility_major_diff() {
        // Use a version with a different major: major+10
        let local_parts: Vec<u64> = PROTOCOL_VERSION
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect();
        let local_major = local_parts.first().copied().unwrap_or(0);
        let diff_major = format!("{}.0.0", local_major + 10);
        let compat = check_version_compatibility(Some(&diff_major));
        assert!(!compat.compatible);
        assert!(compat.message.contains("Major version mismatch"));
    }

    #[test]
    fn test_version_compatibility_unknown() {
        let compat = check_version_compatibility(None);
        assert!(compat.compatible);
        assert_eq!(compat.remote_version, None);
        assert!(compat.message.contains("Unable to determine"));
    }

    #[test]
    fn test_version_compatibility_rejects_malformed_version() {
        let compat = check_version_compatibility(Some("not-a-version"));
        assert!(!compat.compatible);
        assert!(compat.message.contains("Invalid remote version"));
    }

    #[test]
    fn test_version_compatibility_patch_diff() {
        // Same major.minor, different patch → still compatible, message contains "match"
        let local_parts: Vec<u64> = PROTOCOL_VERSION
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect();
        let local_major = local_parts.first().copied().unwrap_or(0);
        let local_minor = local_parts.get(1).copied().unwrap_or(0);
        let patch_diff = format!("{}.{}.99", local_major, local_minor);
        let compat = check_version_compatibility(Some(&patch_diff));
        assert!(compat.compatible);
        // Patch-only difference still results in "match" since major and minor are same
        assert!(compat.message.contains("Versions match"));
    }

    // ── F5-4: Unified view tests ────────────────────────────────────────────

    #[test]
    fn test_daemon_unified_view_serialization() {
        let view = super::DaemonUnifiedView {
            daemons: vec![
                super::DaemonViewEntry {
                    alias: "localhost".to_string(),
                    url: "http://127.0.0.1:7722".to_string(),
                    connected: true,
                    host_count: Some(5),
                    health: Some(super::DaemonHealthSummary {
                        version: Some("0.1.0".to_string()),
                        uptime_secs: Some(3600),
                        pid: Some(12345),
                    }),
                    metrics: Some(super::DaemonMetricsSummary {
                        request_count: Some(100),
                        exec_count: Some(50),
                        exec_blocked_count: Some(2),
                        approval_count: Some(10),
                    }),
                },
                super::DaemonViewEntry {
                    alias: "prod".to_string(),
                    url: "https://daemon.prod.example.com:7722".to_string(),
                    connected: false,
                    host_count: None,
                    health: None,
                    metrics: None,
                },
            ],
            total_hosts: 5,
            total_connected: 1,
        };
        let json = serde_json::to_string(&view).unwrap();
        let deserialized: super::DaemonUnifiedView = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.daemons.len(), 2);
        assert_eq!(deserialized.total_hosts, 5);
        assert_eq!(deserialized.total_connected, 1);
        assert_eq!(deserialized.daemons[0].alias, "localhost");
        assert!(deserialized.daemons[0].connected);
        assert_eq!(deserialized.daemons[0].host_count, Some(5));
        assert_eq!(deserialized.daemons[1].alias, "prod");
        assert!(!deserialized.daemons[1].connected);
    }

    #[test]
    fn test_daemon_view_entry_connected() {
        let entry = super::DaemonViewEntry {
            alias: "test-daemon".to_string(),
            url: "http://localhost:7722".to_string(),
            connected: true,
            host_count: Some(3),
            health: Some(super::DaemonHealthSummary {
                version: Some("0.1.0".to_string()),
                uptime_secs: Some(7200),
                pid: Some(999),
            }),
            metrics: Some(super::DaemonMetricsSummary {
                request_count: Some(200),
                exec_count: Some(100),
                exec_blocked_count: Some(5),
                approval_count: Some(20),
            }),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let de: super::DaemonViewEntry = serde_json::from_str(&json).unwrap();
        assert!(de.connected);
        assert_eq!(
            de.health.as_ref().unwrap().version.as_deref(),
            Some("0.1.0")
        );
        assert_eq!(de.health.as_ref().unwrap().uptime_secs, Some(7200));
        assert_eq!(de.health.as_ref().unwrap().pid, Some(999));
        assert_eq!(de.metrics.as_ref().unwrap().exec_count, Some(100));
        assert_eq!(de.metrics.as_ref().unwrap().exec_blocked_count, Some(5));
    }

    // ── F5-3: Daemon Scope tests ─────────────────────────────────────────────

    #[test]
    fn test_daemon_scope_no_scope_allows_all() {
        // None scope allows everything
        assert!(check_daemon_scope(&None, "any-host", &[], "any command").is_ok());
        assert!(check_daemon_scope(&None, "prod", &["web".to_string()], "sudo rm -rf /").is_ok());
    }

    #[test]
    fn test_daemon_scope_denied_command() {
        let scope = Some(DaemonScope {
            denied_commands: vec!["rm -rf *".to_string(), "mkfs *".to_string()],
            ..DaemonScope::default()
        });
        let tags = vec!["web".to_string()];

        // Denied commands should be blocked
        assert!(check_daemon_scope(&scope, "prod", &tags, "rm -rf /").is_err());
        assert!(check_daemon_scope(&scope, "prod", &tags, "mkfs /dev/sda").is_err());

        // Non-denied commands should pass
        assert!(check_daemon_scope(&scope, "prod", &tags, "ls -la").is_ok());
        assert!(check_daemon_scope(&scope, "prod", &tags, "sudo apt update").is_ok());
    }

    #[test]
    fn test_daemon_scope_allowed_hosts() {
        let scope = Some(DaemonScope {
            allowed_hosts: vec!["prod-web-1".to_string(), "prod-web-2".to_string()],
            ..DaemonScope::default()
        });

        // Allowed hosts should pass
        assert!(check_daemon_scope(&scope, "prod-web-1", &[], "ls").is_ok());
        assert!(check_daemon_scope(&scope, "prod-web-2", &[], "ls").is_ok());

        // Non-allowed hosts should fail
        assert!(check_daemon_scope(&scope, "dev-box", &[], "ls").is_err());
        assert!(check_daemon_scope(&scope, "staging", &[], "ls").is_err());
    }

    #[test]
    fn test_daemon_scope_allowed_tags() {
        let scope = Some(DaemonScope {
            allowed_tags: vec!["production".to_string()],
            ..DaemonScope::default()
        });

        // Host with matching tag should pass
        let prod_tags = vec!["production".to_string(), "web".to_string()];
        assert!(check_daemon_scope(&scope, "prod-web-1", &prod_tags, "ls").is_ok());

        // Host with no matching tag should fail
        let dev_tags = vec!["development".to_string(), "web".to_string()];
        assert!(check_daemon_scope(&scope, "dev-box", &dev_tags, "ls").is_err());

        // Host with no tags should fail
        assert!(check_daemon_scope(&scope, "bare-host", &[], "ls").is_err());
    }

    #[test]
    fn test_daemon_scope_allowed_commands() {
        let scope = Some(DaemonScope {
            allowed_commands: vec![
                "ls *".to_string(),
                "cat *".to_string(),
                "uptime".to_string(),
            ],
            ..DaemonScope::default()
        });

        // Allowed commands should pass
        assert!(check_daemon_scope(&scope, "any-host", &[], "ls -la").is_ok());
        assert!(check_daemon_scope(&scope, "any-host", &[], "cat /etc/hosts").is_ok());
        assert!(check_daemon_scope(&scope, "any-host", &[], "uptime").is_ok());

        // Non-allowed commands should fail
        assert!(check_daemon_scope(&scope, "any-host", &[], "sudo reboot").is_err());
        assert!(check_daemon_scope(&scope, "any-host", &[], "rm -rf /").is_err());
    }

    #[test]
    fn test_daemon_scope_denied_before_allowed() {
        // denied_commands is checked BEFORE allowed_commands
        let scope = Some(DaemonScope {
            allowed_commands: vec!["sudo *".to_string()],
            denied_commands: vec!["sudo rm *".to_string()],
            ..DaemonScope::default()
        });

        // "sudo apt update" matches allowed "sudo *" but not denied -> allowed
        assert!(check_daemon_scope(&scope, "host", &[], "sudo apt update").is_ok());

        // "sudo rm -rf /" matches both allowed and denied, but denied wins -> denied
        assert!(check_daemon_scope(&scope, "host", &[], "sudo rm -rf /").is_err());
    }

    #[test]
    fn test_daemon_scope_empty_lists_allow_all() {
        // All empty lists = allow everything (same as None scope)
        let scope = Some(DaemonScope::default());
        assert!(check_daemon_scope(&scope, "any-host", &[], "any command").is_ok());
        assert!(
            check_daemon_scope(&scope, "any-host", &["any-tag".to_string()], "rm -rf /").is_ok()
        );
    }

    #[test]
    fn test_daemon_scope_case_insensitive_hosts() {
        let scope = Some(DaemonScope {
            allowed_hosts: vec!["Prod-Web-1".to_string()],
            ..DaemonScope::default()
        });
        assert!(check_daemon_scope(&scope, "prod-web-1", &[], "ls").is_ok());
        assert!(check_daemon_scope(&scope, "PROD-WEB-1", &[], "ls").is_ok());
    }

    #[test]
    fn test_daemon_scope_serialization() {
        let scope = DaemonScope {
            allowed_hosts: vec!["prod".to_string()],
            allowed_tags: vec!["web".to_string()],
            allowed_commands: vec!["ls *".to_string()],
            denied_commands: vec!["rm *".to_string()],
        };
        let json = serde_json::to_string(&scope).unwrap();
        let deserialized: DaemonScope = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.allowed_hosts, vec!["prod"]);
        assert_eq!(deserialized.allowed_tags, vec!["web"]);
        assert_eq!(deserialized.allowed_commands, vec!["ls *"]);
        assert_eq!(deserialized.denied_commands, vec!["rm *"]);
    }

    #[test]
    fn test_remote_daemon_with_scope() {
        let toml_str = r#"
[[remotes]]
alias = "prod"
url = "https://daemon.prod.example.com:7722"
token_env = "PROD_TOKEN"

[remotes.scope]
allowed_hosts = ["web-1", "web-2"]
allowed_tags = ["production"]
allowed_commands = ["ls *", "cat *"]
denied_commands = ["rm -rf *"]
"#;
        let file: RemotesFile = toml::from_str(toml_str).unwrap();
        assert_eq!(file.remotes.len(), 1);
        let remote = &file.remotes[0];
        assert_eq!(remote.alias, "prod");
        let scope = remote.scope.as_ref().unwrap();
        assert_eq!(scope.allowed_hosts, vec!["web-1", "web-2"]);
        assert_eq!(scope.allowed_tags, vec!["production"]);
        assert_eq!(scope.allowed_commands, vec!["ls *", "cat *"]);
        assert_eq!(scope.denied_commands, vec!["rm -rf *"]);
    }

    #[test]
    fn test_scoped_daemon_token_toml_roundtrip() {
        let toml_str = r#"
[[tokens]]
name = "readonly"
token_env = "READONLY_TOKEN"

[tokens.scope]
allowed_hosts = ["prod-web-1"]
allowed_tags = ["production"]
allowed_commands = ["ls *", "cat *"]
denied_commands = ["rm -rf *"]
"#;
        let file: DaemonTokensFile = toml::from_str(toml_str).unwrap();
        assert_eq!(file.tokens.len(), 1);
        validate_scoped_daemon_tokens(&file.tokens).unwrap();
        let token = &file.tokens[0];
        assert_eq!(token.name.as_deref(), Some("readonly"));
        assert_eq!(token.token_env.as_deref(), Some("READONLY_TOKEN"));
        let scope = token.scope.as_ref().unwrap();
        assert_eq!(scope.allowed_hosts, vec!["prod-web-1"]);
        assert_eq!(scope.allowed_tags, vec!["production"]);
        assert_eq!(scope.allowed_commands, vec!["ls *", "cat *"]);
        assert_eq!(scope.denied_commands, vec!["rm -rf *"]);
    }

    #[test]
    fn validate_scoped_daemon_tokens_requires_scope() {
        let token = ScopedDaemonToken {
            name: Some("missing-scope".to_string()),
            token: Some("secret".to_string()),
            token_env: None,
            scope: None,
        };
        assert!(validate_scoped_daemon_tokens(&[token]).is_err());
    }

    #[tokio::test]
    async fn remote_scope_tags_use_fallback_when_tag_scope_is_empty() {
        let scope = Some(DaemonScope {
            allowed_hosts: vec!["prod".to_string()],
            allowed_tags: Vec::new(),
            allowed_commands: Vec::new(),
            denied_commands: Vec::new(),
        });
        let tags = tags_for_remote_scope_check(
            &scope,
            "http://127.0.0.1:1",
            "unused",
            "prod",
            vec!["local-tag".to_string()],
        )
        .await
        .unwrap();
        assert_eq!(tags, vec!["local-tag"]);
    }
}
