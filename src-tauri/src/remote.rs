use anyhow::{anyhow, Result};
use std::collections::HashSet;
use serde::{Deserialize, Serialize};

use crate::store::config_dir;

/// A remote daemon entry configured in ~/.agent2ssh/remotes.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteDaemon {
    pub alias: String,
    pub url: String,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub token_env: Option<String>,
}

/// The full remotes.toml file structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemotesFile {
    #[serde(default)]
    pub remotes: Vec<RemoteDaemon>,
}

/// Daemon info returned to clients (CLI, MCP, Tauri, Web)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub alias: String,
    pub url: String,
    pub connected: bool,
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

/// List all configured daemons (localhost + remotes).
///
/// Connectivity is probed by hitting each daemon's /health endpoint
/// with a short timeout. Failures are reported as `connected: false`
/// rather than returning an error, so a single unreachable remote
/// never breaks the whole listing.
pub fn list_daemons_core() -> Result<Vec<DaemonInfo>> {
    let mut daemons = Vec::new();

    // Always include localhost as first entry
    let local_url = "http://127.0.0.1:7722".to_string();
    let local_connected = check_health_blocking(&local_url);
    daemons.push(DaemonInfo {
        alias: "localhost".to_string(),
        url: local_url,
        connected: local_connected,
    });

    // Add all configured remotes
    let remotes = load_remotes()?;
    for remote in remotes {
        let connected = check_health_blocking(&remote.url);
        daemons.push(DaemonInfo {
            alias: remote.alias,
            url: remote.url,
            connected,
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

/// Get daemon URL and token by alias.
///
/// * `"localhost"` -> local daemon URL + token from `~/.agent2ssh/daemon.token`
/// * Anything else -> looked up in the remotes list
pub fn get_daemon(alias: &str) -> Result<(String, Option<String>)> {
    if alias == "localhost" {
        let token = read_local_token();
        return Ok(("http://127.0.0.1:7722".to_string(), token));
    }
    let remotes = load_remotes()?;
    for remote in &remotes {
        if remote.alias == alias {
            return Ok((remote.url.clone(), resolve_token(remote)));
        }
    }
    Err(anyhow!("daemon '{}' not found in remotes", alias))
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
                    // Fallback: try resolving as-is
                    "127.0.0.1:7722".parse().unwrap()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(alias: &str) -> RemoteDaemon {
        RemoteDaemon {
            alias: alias.to_string(),
            url: "https://daemon.example.com:7722".to_string(),
            token: None,
            token_env: Some("AGENT2SSH_TOKEN".to_string()),
        }
    }

    #[test]
    fn validate_remotes_accepts_valid_remote() {
        assert!(validate_remotes(&[remote("prod")]).is_ok());
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
}
