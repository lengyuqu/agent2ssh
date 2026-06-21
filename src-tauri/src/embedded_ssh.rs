use anyhow::{anyhow, Context, Result};
use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
    Engine as _,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ssh2::{HashType, KeyboardInteractivePrompt, Prompt, Session};
use std::{
    collections::HashMap,
    io::{ErrorKind, Read, Write},
    net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs},
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    store::{config_dir, ensure_config_dir, load_config, restrict_file_to_owner},
    types::{HostProfile, ProxyProfile, ProxyProtocol},
};

#[derive(Debug, Clone)]
pub struct EmbeddedSshConnectionInfo {
    pub host: String,
    pub address: String,
    pub username: String,
    pub fingerprint_sha256: Option<String>,
    pub host_key_algorithm: Option<String>,
    pub auth_method: String,
    pub server_banner: Option<String>,
}

#[derive(Debug)]
pub enum TerminalCommand {
    Input(Vec<u8>),
    Resize { cols: u32, rows: u32 },
    Close,
}

#[derive(Debug)]
pub enum TerminalEvent {
    Connected(EmbeddedSshConnectionInfo),
    Output(Vec<u8>),
    Error(String),
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrustedHostFingerprint {
    host: String,
    address: String,
    host_key_algorithm: String,
    fingerprint_sha256: String,
    first_seen_unix: u64,
    last_seen_unix: u64,
}

fn expand_tilde(path: &str) -> String {
    if path == "~" {
        return dirs::home_dir()
            .map(|home| home.display().to_string())
            .unwrap_or_else(|| path.to_string());
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).display().to_string();
        }
    }
    path.to_string()
}

fn host_key_algorithm(kind: ssh2::HostKeyType) -> &'static str {
    match kind {
        ssh2::HostKeyType::Rsa => "ssh-rsa",
        ssh2::HostKeyType::Dss => "ssh-dss",
        ssh2::HostKeyType::Ecdsa256 => "ecdsa-sha2-nistp256",
        ssh2::HostKeyType::Ecdsa384 => "ecdsa-sha2-nistp384",
        ssh2::HostKeyType::Ecdsa521 => "ecdsa-sha2-nistp521",
        ssh2::HostKeyType::Ed25519 => "ssh-ed25519",
        ssh2::HostKeyType::Unknown => "unknown",
    }
}

fn fingerprint_sha256(session: &Session) -> Option<String> {
    if let Some(hash) = session.host_key_hash(HashType::Sha256) {
        return Some(format!("SHA256:{}", STANDARD_NO_PAD.encode(hash)));
    }
    let (key, _) = session.host_key()?;
    let digest = Sha256::digest(key);
    Some(format!("SHA256:{}", STANDARD_NO_PAD.encode(digest)))
}

fn known_hosts_path() -> Result<std::path::PathBuf> {
    Ok(config_dir()?.join("known_hosts.json"))
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn known_host_identity(host: &HostProfile) -> String {
    format!("{}:{}", host.host.trim(), host.port.unwrap_or(22))
}

fn load_known_host_fingerprints_unlocked() -> Result<HashMap<String, TrustedHostFingerprint>> {
    let path = known_hosts_path()?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read known host fingerprints {}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(HashMap::new());
    }
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse known host fingerprints {}", path.display()))
}

fn save_known_host_fingerprints_unlocked(
    fingerprints: &HashMap<String, TrustedHostFingerprint>,
) -> Result<()> {
    ensure_config_dir()?;
    let path = known_hosts_path()?;
    let raw = serde_json::to_string_pretty(fingerprints)?;
    std::fs::write(&path, raw)
        .with_context(|| format!("failed to write known host fingerprints {}", path.display()))?;
    restrict_file_to_owner(&path)?;
    Ok(())
}

fn trust_or_verify_host_fingerprint(
    host: &HostProfile,
    address: &str,
    host_key_algorithm: &str,
    fingerprint_sha256: &str,
) -> Result<()> {
    ensure_config_dir()?;
    let _guard = crate::store::lock_config_file(".known_hosts.lock")?;
    let mut trusted = load_known_host_fingerprints_unlocked()?;
    let identity = known_host_identity(host);
    let now = now_unix_secs();

    if let Some(existing) = trusted.get_mut(&identity) {
        if existing.fingerprint_sha256 != fingerprint_sha256
            || existing.host_key_algorithm != host_key_algorithm
        {
            return Err(anyhow!(
                "SSH host fingerprint changed for {} ({}): expected {} {}, got {} {}",
                host.name,
                identity,
                existing.host_key_algorithm,
                existing.fingerprint_sha256,
                host_key_algorithm,
                fingerprint_sha256
            ));
        }
        existing.host = host.name.clone();
        existing.address = address.to_string();
        existing.last_seen_unix = now;
        save_known_host_fingerprints_unlocked(&trusted)?;
        return Ok(());
    }

    trusted.insert(
        identity,
        TrustedHostFingerprint {
            host: host.name.clone(),
            address: address.to_string(),
            host_key_algorithm: host_key_algorithm.to_string(),
            fingerprint_sha256: fingerprint_sha256.to_string(),
            first_seen_unix: now,
            last_seen_unix: now,
        },
    );
    save_known_host_fingerprints_unlocked(&trusted)
}

fn default_username() -> String {
    std::env::var("USER").unwrap_or_else(|_| "root".to_string())
}

pub fn resolved_username(host: &HostProfile) -> String {
    host.user
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(default_username)
}

fn connect_tcp_address(host: &str, port: u16, timeout_secs: u64) -> Result<TcpStream> {
    let address = format!("{host}:{port}");
    let socket_addr = address
        .to_socket_addrs()
        .with_context(|| format!("failed to resolve {address}"))?
        .next()
        .ok_or_else(|| anyhow!("failed to resolve {address}"))?;
    let tcp = TcpStream::connect_timeout(&socket_addr, Duration::from_secs(timeout_secs.min(30)))?;
    tcp.set_read_timeout(Some(Duration::from_secs(timeout_secs)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(timeout_secs)))?;
    Ok(tcp)
}

fn connect_direct_tcp(host: &HostProfile, timeout_secs: u64) -> Result<TcpStream> {
    connect_tcp_address(&host.host, host.port.unwrap_or(22), timeout_secs)
}

fn resolve_jump_host(name: &str) -> Result<HostProfile> {
    load_config()?
        .hosts
        .into_iter()
        .find(|host| host.name == name)
        .ok_or_else(|| anyhow!("unknown jump host profile: {name}"))
}

fn resolve_proxy(id: &str) -> Result<ProxyProfile> {
    load_config()?
        .proxies
        .into_iter()
        .find(|proxy| proxy.id == id)
        .ok_or_else(|| anyhow!("unknown proxy profile: {id}"))
}

fn auth_method_for_host(host: &HostProfile) -> &'static str {
    if host
        .password
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        "password"
    } else if host
        .key_path
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        "publickey_file"
    } else {
        "agent"
    }
}

fn proxy_for_host(host: &HostProfile) -> Result<Option<ProxyProfile>> {
    host.proxy_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(resolve_proxy)
        .transpose()
}

fn connect_via_jump_to(
    host: &HostProfile,
    target_host: &str,
    target_port: u16,
    timeout_secs: u64,
    depth: usize,
) -> Result<TcpStream> {
    let jump_name = host
        .jump_host
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("missing jump host profile for '{}'", host.name))?;
    let jump = resolve_jump_host(jump_name)?;
    let jump_session = connect_embedded_ssh_inner(&jump, timeout_secs, depth + 1)
        .with_context(|| format!("failed to connect jump host '{jump_name}'"))?;
    jump_session.set_blocking(true);
    let channel = jump_session
        .channel_direct_tcpip(target_host, target_port, None)
        .with_context(|| {
            format!(
                "failed to open direct-tcpip channel via '{jump_name}' to {}:{}",
                target_host, target_port
            )
        })?;
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let local_addr = listener.local_addr()?;
    let client = TcpStream::connect(local_addr)?;
    let (server, _) = listener.accept()?;
    jump_session.set_blocking(false);
    thread::spawn(move || {
        let _jump_session = jump_session;
        if let Err(error) = bridge_tcp_and_channel(server, channel) {
            let _ = crate::diagnostics::append_diagnostic_log(
                "warn",
                "embedded_ssh",
                "jump host proxy channel closed with error",
                Some(serde_json::json!({ "error": error.to_string() })),
            );
        }
    });
    Ok(client)
}

fn connect_via_jump(host: &HostProfile, timeout_secs: u64, depth: usize) -> Result<TcpStream> {
    connect_via_jump_to(
        host,
        &host.host,
        host.port.unwrap_or(22),
        timeout_secs,
        depth,
    )
}

fn connect_proxy_endpoint(
    host: &HostProfile,
    proxy: &ProxyProfile,
    timeout_secs: u64,
    depth: usize,
) -> Result<TcpStream> {
    let mut stream = if host
        .jump_host
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
    {
        connect_via_jump_to(host, &proxy.host, proxy.port, timeout_secs, depth)?
    } else {
        connect_tcp_address(&proxy.host, proxy.port, timeout_secs)?
    };
    match proxy.protocol {
        ProxyProtocol::Http => connect_http_proxy(&mut stream, proxy, host)?,
        ProxyProtocol::Socks5 => connect_socks5_proxy(&mut stream, proxy, host)?,
    }
    Ok(stream)
}

fn connect_transport(host: &HostProfile, timeout_secs: u64, depth: usize) -> Result<TcpStream> {
    if let Some(proxy) = proxy_for_host(host)? {
        return connect_proxy_endpoint(host, &proxy, timeout_secs, depth).with_context(|| {
            format!(
                "failed to connect '{}' through {} proxy '{}'",
                host.name, proxy.protocol, proxy.name
            )
        });
    }
    if host
        .jump_host
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
    {
        connect_via_jump(host, timeout_secs, depth)
    } else {
        connect_direct_tcp(host, timeout_secs)
    }
}

fn connect_http_proxy(
    stream: &mut TcpStream,
    proxy: &ProxyProfile,
    host: &HostProfile,
) -> Result<()> {
    let target = format!("{}:{}", host.host, host.port.unwrap_or(22));
    let mut request =
        format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\nProxy-Connection: Keep-Alive\r\n");
    if let Some(username) = proxy
        .username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let password = proxy.password.as_deref().unwrap_or("");
        let credentials = STANDARD.encode(format!("{username}:{password}"));
        request.push_str(&format!("Proxy-Authorization: Basic {credentials}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let response = read_http_proxy_response(stream)?;
    let status_line = response
        .lines()
        .next()
        .ok_or_else(|| anyhow!("empty HTTP proxy response"))?;
    let mut parts = status_line.split_whitespace();
    let _version = parts.next();
    let code = parts
        .next()
        .ok_or_else(|| anyhow!("malformed HTTP proxy response: {status_line}"))?;
    if code != "200" {
        return Err(anyhow!("HTTP proxy CONNECT failed: {status_line}"));
    }
    Ok(())
}

fn read_http_proxy_response(stream: &mut TcpStream) -> Result<String> {
    let mut response = Vec::new();
    let mut buf = [0u8; 1];
    while response.len() < 16 * 1024 {
        stream.read_exact(&mut buf)?;
        response.push(buf[0]);
        if response.ends_with(b"\r\n\r\n") {
            return String::from_utf8(response).context("HTTP proxy response is not UTF-8");
        }
    }
    Err(anyhow!("HTTP proxy response header exceeded 16 KiB"))
}

fn connect_socks5_proxy(
    stream: &mut TcpStream,
    proxy: &ProxyProfile,
    host: &HostProfile,
) -> Result<()> {
    let has_auth = proxy
        .username
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    if has_auth {
        stream.write_all(&[0x05, 0x02, 0x00, 0x02])?;
    } else {
        stream.write_all(&[0x05, 0x01, 0x00])?;
    }
    let mut method = [0u8; 2];
    stream.read_exact(&mut method)?;
    if method[0] != 0x05 {
        return Err(anyhow!("SOCKS5 proxy returned unsupported version"));
    }
    match method[1] {
        0x00 => {}
        0x02 => authenticate_socks5_proxy(stream, proxy)?,
        0xff => return Err(anyhow!("SOCKS5 proxy rejected authentication methods")),
        value => {
            return Err(anyhow!(
                "SOCKS5 proxy selected unsupported method {value:#x}"
            ))
        }
    }

    let target_host = host.host.as_bytes();
    if target_host.len() > u8::MAX as usize {
        return Err(anyhow!("SOCKS5 target host is too long"));
    }
    let target_port = host.port.unwrap_or(22).to_be_bytes();
    let mut request = Vec::with_capacity(7 + target_host.len());
    request.extend_from_slice(&[0x05, 0x01, 0x00, 0x03, target_host.len() as u8]);
    request.extend_from_slice(target_host);
    request.extend_from_slice(&target_port);
    stream.write_all(&request)?;

    let mut header = [0u8; 4];
    stream.read_exact(&mut header)?;
    if header[0] != 0x05 {
        return Err(anyhow!("SOCKS5 connect response has unsupported version"));
    }
    if header[1] != 0x00 {
        return Err(anyhow!(
            "SOCKS5 proxy connect failed with status {:#x}",
            header[1]
        ));
    }
    match header[3] {
        0x01 => read_and_discard(stream, 4)?,
        0x03 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len)?;
            read_and_discard(stream, len[0] as usize)?;
        }
        0x04 => read_and_discard(stream, 16)?,
        value => {
            return Err(anyhow!(
                "SOCKS5 response has unsupported address type {value:#x}"
            ))
        }
    }
    read_and_discard(stream, 2)?;
    Ok(())
}

fn authenticate_socks5_proxy(stream: &mut TcpStream, proxy: &ProxyProfile) -> Result<()> {
    let username = proxy
        .username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("SOCKS5 proxy requested username/password authentication"))?;
    let password = proxy.password.as_deref().unwrap_or("");
    if username.len() > u8::MAX as usize || password.len() > u8::MAX as usize {
        return Err(anyhow!("SOCKS5 proxy credentials are too long"));
    }
    let mut request = Vec::with_capacity(3 + username.len() + password.len());
    request.push(0x01);
    request.push(username.len() as u8);
    request.extend_from_slice(username.as_bytes());
    request.push(password.len() as u8);
    request.extend_from_slice(password.as_bytes());
    stream.write_all(&request)?;

    let mut response = [0u8; 2];
    stream.read_exact(&mut response)?;
    if response != [0x01, 0x00] {
        return Err(anyhow!("SOCKS5 proxy authentication failed"));
    }
    Ok(())
}

fn read_and_discard(stream: &mut TcpStream, len: usize) -> Result<()> {
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(())
}

fn bridge_tcp_and_channel(mut stream: TcpStream, mut channel: ssh2::Channel) -> Result<()> {
    stream.set_nonblocking(true)?;
    let mut tcp_closed = false;
    let mut channel_closed = false;
    let mut tcp_buf = [0u8; 8192];
    let mut channel_buf = [0u8; 8192];

    while !tcp_closed || !channel_closed {
        match stream.read(&mut tcp_buf) {
            Ok(0) => {
                tcp_closed = true;
                let _ = channel.send_eof();
            }
            Ok(n) => write_all_channel(&mut channel, &tcp_buf[..n])?,
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(error) => return Err(error.into()),
        }

        match channel.read(&mut channel_buf) {
            Ok(0) => {
                if channel.eof() {
                    channel_closed = true;
                    let _ = stream.shutdown(Shutdown::Write);
                }
            }
            Ok(n) => write_all_tcp(&mut stream, &channel_buf[..n])?,
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(error) => return Err(error.into()),
        }

        if tcp_closed && channel_closed {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }

    let _ = channel.close();
    let _ = channel.wait_close();
    Ok(())
}

fn write_all_tcp(stream: &mut TcpStream, mut data: &[u8]) -> Result<()> {
    while !data.is_empty() {
        match stream.write(data) {
            Ok(0) => return Err(anyhow!("tcp stream closed while writing")),
            Ok(n) => data = &data[n..],
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn write_all_channel(channel: &mut ssh2::Channel, mut data: &[u8]) -> Result<()> {
    while !data.is_empty() {
        match channel.write(data) {
            Ok(0) => return Err(anyhow!("ssh channel closed while writing")),
            Ok(n) => data = &data[n..],
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error.into()),
        }
    }
    let _ = channel.flush();
    Ok(())
}

fn connect_embedded_ssh_inner(
    host: &HostProfile,
    timeout_secs: u64,
    depth: usize,
) -> Result<Session> {
    if depth > 4 {
        return Err(anyhow!(
            "too many nested jump hosts while connecting '{}'",
            host.name
        ));
    }

    let via_jump = host
        .jump_host
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    let mut session = Session::new()?;
    let tcp = connect_transport(host, timeout_secs, depth)?;
    tcp.set_read_timeout(Some(Duration::from_secs(timeout_secs)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(timeout_secs)))?;
    session.set_tcp_stream(tcp);
    session.handshake()?;

    let host_address = format!("{}:{}", host.host, host.port.unwrap_or(22));
    let (host_key_algorithm, host_key_fingerprint) = match session.host_key() {
        Some((_, kind)) => (
            host_key_algorithm(kind).to_string(),
            fingerprint_sha256(&session)
                .ok_or_else(|| anyhow!("failed to calculate SSH host fingerprint"))?,
        ),
        None => return Err(anyhow!("SSH server did not provide a host key")),
    };
    trust_or_verify_host_fingerprint(
        host,
        &host_address,
        &host_key_algorithm,
        &host_key_fingerprint,
    )?;

    let username = resolved_username(host);
    let auth_method = authenticate(&session, host, &username)?;

    if !session.authenticated() {
        return Err(anyhow!("SSH authentication failed for '{}'", host.name));
    }

    let _ = crate::diagnostics::append_diagnostic_log(
        "info",
        "embedded_ssh",
        "ssh connection authenticated",
        Some(serde_json::json!({
            "host": host.name,
            "address": host_address,
            "username": username,
            "auth_method": auth_method,
            "fingerprint_sha256": host_key_fingerprint,
            "host_key_algorithm": host_key_algorithm,
            "server_banner": session.banner().map(ToOwned::to_owned),
            "jump_host": via_jump,
        })),
    );

    Ok(session)
}

pub fn connect_embedded_ssh(host: &HostProfile, timeout_secs: u64) -> Result<Session> {
    connect_embedded_ssh_inner(host, timeout_secs, 0)
}

struct PasswordPrompter<'a> {
    password: &'a str,
}

impl KeyboardInteractivePrompt for PasswordPrompter<'_> {
    fn prompt<'a>(
        &mut self,
        _username: &str,
        _instructions: &str,
        prompts: &[Prompt<'a>],
    ) -> Vec<String> {
        prompts.iter().map(|_| self.password.to_string()).collect()
    }
}

fn authenticate(session: &Session, host: &HostProfile, username: &str) -> Result<String> {
    let auth_methods = session.auth_methods(username).unwrap_or("").to_string();
    let _ = crate::diagnostics::append_diagnostic_log(
        "info",
        "embedded_ssh",
        "ssh auth methods advertised",
        Some(serde_json::json!({
            "host": host.name,
            "username": username,
            "auth_methods": auth_methods,
        })),
    );

    if let Some(marker) = host
        .password
        .as_deref()
        .filter(|value| crate::secrets::is_secret_ref(value))
    {
        let guidance = if crate::secrets::is_legacy_keyring_ref(marker) {
            "legacy OS keyring reference; re-enter the password so it can be stored in secrets.enc"
        } else {
            "locked app-managed secret reference; unlock the credential store with the master password"
        };
        return Err(anyhow!("SSH password for '{}' is a {guidance}", host.name));
    }

    if let Some(password) = host
        .password
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let methods = auth_methods
            .split(',')
            .map(str::trim)
            .collect::<std::collections::HashSet<_>>();
        let mut failures = Vec::new();
        let password_advertised = auth_methods.is_empty() || methods.contains("password");

        if password_advertised {
            match session.userauth_password(username, password) {
                Ok(()) if session.authenticated() => return Ok("password".into()),
                Ok(()) => failures.push("password returned without authenticating".to_string()),
                Err(error) => failures.push(format!("password: {error}")),
            }
        }

        if !session.authenticated() {
            let mut prompter = PasswordPrompter { password };
            match session.userauth_keyboard_interactive(username, &mut prompter) {
                Ok(()) if session.authenticated() => return Ok("keyboard-interactive".into()),
                Ok(()) => failures
                    .push("keyboard-interactive returned without authenticating".to_string()),
                Err(error) => {
                    let label = if methods.contains("keyboard-interactive") {
                        "keyboard-interactive"
                    } else {
                        "keyboard-interactive fallback"
                    };
                    failures.push(format!("{label}: {error}"));
                }
            }
        }

        if !failures.is_empty() {
            return Err(anyhow!(
                "SSH password authentication failed for '{}' using advertised methods [{}]: {}",
                host.name,
                if auth_methods.is_empty() {
                    "unknown"
                } else {
                    auth_methods.as_str()
                },
                failures.join("; ")
            ));
        }
    }

    if let Some(key_path) = host
        .key_path
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let path = expand_tilde(key_path);
        session.userauth_pubkey_file(username, None, Path::new(&path), None)?;
        return Ok("publickey_file".into());
    }

    let mut agent = session.agent()?;
    agent.connect()?;
    agent.list_identities()?;
    let identity = agent
        .identities()?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no SSH key_path, password, or ssh-agent identity available"))?;
    agent.userauth(username, &identity)?;
    Ok("agent".into())
}

fn connection_info(
    host: &HostProfile,
    session: &Session,
    auth_method: &str,
) -> EmbeddedSshConnectionInfo {
    let (host_key_algorithm, fingerprint_sha256) = match session.host_key() {
        Some((_, kind)) => (
            Some(host_key_algorithm(kind).to_string()),
            fingerprint_sha256(session),
        ),
        None => (None, None),
    };

    EmbeddedSshConnectionInfo {
        host: host.name.clone(),
        address: format!("{}:{}", host.host, host.port.unwrap_or(22)),
        username: resolved_username(host),
        fingerprint_sha256,
        host_key_algorithm,
        auth_method: auth_method.to_string(),
        server_banner: session.banner().map(ToOwned::to_owned),
    }
}

pub fn spawn_terminal(
    host: HostProfile,
    initial_cols: u32,
    initial_rows: u32,
) -> (mpsc::Sender<TerminalCommand>, mpsc::Receiver<TerminalEvent>) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<TerminalCommand>();
    let (event_tx, event_rx) = mpsc::channel::<TerminalEvent>();

    thread::spawn(move || {
        if let Err(error) = run_terminal(host, initial_cols, initial_rows, &cmd_rx, &event_tx) {
            let _ = event_tx.send(TerminalEvent::Error(error.to_string()));
        }
        let _ = event_tx.send(TerminalEvent::Closed);
    });

    (cmd_tx, event_rx)
}

fn run_terminal(
    host: HostProfile,
    initial_cols: u32,
    initial_rows: u32,
    cmd_rx: &mpsc::Receiver<TerminalCommand>,
    event_tx: &mpsc::Sender<TerminalEvent>,
) -> Result<()> {
    let session = connect_embedded_ssh(&host, 60)?;
    let username = resolved_username(&host);
    let auth_method = auth_method_for_host(&host);
    let info = connection_info(&host, &session, auth_method);
    let _ = event_tx.send(TerminalEvent::Connected(info.clone()));
    let _ = crate::diagnostics::append_diagnostic_log(
        "info",
        "embedded_ssh_terminal",
        "terminal connected",
        Some(serde_json::json!({
            "host": info.host,
            "address": info.address,
            "username": username,
            "fingerprint_sha256": info.fingerprint_sha256,
            "host_key_algorithm": info.host_key_algorithm,
            "auth_method": info.auth_method,
            "server_banner": info.server_banner,
        })),
    );

    let mut channel = session.channel_session()?;
    channel.request_pty(
        "xterm-256color",
        None,
        Some((initial_cols.max(1), initial_rows.max(1), 0, 0)),
    )?;
    channel.shell()?;
    session.set_blocking(false);

    let mut buffer = [0u8; 8192];
    loop {
        while let Ok(command) = cmd_rx.try_recv() {
            match command {
                TerminalCommand::Input(data) => {
                    if let Err(error) = channel.write_all(&data) {
                        if error.kind() != ErrorKind::WouldBlock {
                            return Err(error.into());
                        }
                    }
                    let _ = channel.flush();
                }
                TerminalCommand::Resize { cols, rows } => {
                    channel.request_pty_size(cols.max(1), rows.max(1), None, None)?;
                }
                TerminalCommand::Close => {
                    let _ = channel.close();
                    let _ = channel.wait_close();
                    return Ok(());
                }
            }
        }

        match channel.read(&mut buffer) {
            Ok(0) => {
                if channel.eof() {
                    break;
                }
            }
            Ok(n) => {
                if event_tx
                    .send(TerminalEvent::Output(buffer[..n].to_vec()))
                    .is_err()
                {
                    break;
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }

        if channel.eof() {
            break;
        }
    }

    let _ = channel.wait_close();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_host() -> HostProfile {
        HostProfile {
            name: "target".into(),
            host: "example.internal".into(),
            user: None,
            port: Some(22),
            key_path: None,
            password: None,
            jump_host: None,
            proxy_id: None,
            risk_override: None,
            tags: vec![],
            group: crate::types::default_host_group(),
            env: None,
            role: None,
            owner: None,
        }
    }

    fn test_proxy(protocol: ProxyProtocol) -> ProxyProfile {
        ProxyProfile {
            id: "proxy".into(),
            name: "Proxy".into(),
            protocol,
            host: "127.0.0.1".into(),
            port: 8080,
            username: None,
            password: None,
        }
    }

    #[test]
    #[serial_test::serial]
    fn host_fingerprint_is_auto_trusted_then_verified() {
        let config_dir =
            std::env::temp_dir().join(format!("agent2ssh-known-hosts-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&config_dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &config_dir);
        let host = test_host();

        trust_or_verify_host_fingerprint(
            &host,
            "example.internal:22",
            "ssh-ed25519",
            "SHA256:first",
        )
        .unwrap();
        let trusted = load_known_host_fingerprints_unlocked().unwrap();
        let entry = trusted.get("example.internal:22").unwrap();
        assert_eq!(entry.fingerprint_sha256, "SHA256:first");
        assert_eq!(entry.host_key_algorithm, "ssh-ed25519");

        trust_or_verify_host_fingerprint(
            &host,
            "example.internal:22",
            "ssh-ed25519",
            "SHA256:first",
        )
        .unwrap();

        let mismatch = trust_or_verify_host_fingerprint(
            &host,
            "example.internal:22",
            "ssh-ed25519",
            "SHA256:changed",
        )
        .unwrap_err()
        .to_string();
        assert!(
            mismatch.contains("SSH host fingerprint changed"),
            "unexpected mismatch error: {mismatch}"
        );

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&config_dir);
    }

    #[test]
    fn http_proxy_connect_handshake_succeeds() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            let (mut server, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                server.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("CONNECT example.internal:22 HTTP/1.1"));
            server
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .unwrap();
            let mut payload = [0u8; 4];
            server.read_exact(&mut payload).unwrap();
            assert_eq!(&payload, b"ping");
            done_tx.send(()).unwrap();
        });

        let mut stream = TcpStream::connect(addr).unwrap();
        connect_http_proxy(&mut stream, &test_proxy(ProxyProtocol::Http), &test_host()).unwrap();
        stream.write_all(b"ping").unwrap();
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn socks5_proxy_connect_handshake_succeeds() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            let (mut server, _) = listener.accept().unwrap();
            let mut greeting = [0u8; 3];
            server.read_exact(&mut greeting).unwrap();
            assert_eq!(greeting, [0x05, 0x01, 0x00]);
            server.write_all(&[0x05, 0x00]).unwrap();

            let mut header = [0u8; 5];
            server.read_exact(&mut header).unwrap();
            assert_eq!(&header[..4], &[0x05, 0x01, 0x00, 0x03]);
            let host_len = header[4] as usize;
            let mut host = vec![0u8; host_len];
            server.read_exact(&mut host).unwrap();
            assert_eq!(String::from_utf8(host).unwrap(), "example.internal");
            let mut port = [0u8; 2];
            server.read_exact(&mut port).unwrap();
            assert_eq!(u16::from_be_bytes(port), 22);
            server
                .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0x1f, 0x90])
                .unwrap();
            let mut payload = [0u8; 4];
            server.read_exact(&mut payload).unwrap();
            assert_eq!(&payload, b"ping");
            done_tx.send(()).unwrap();
        });

        let mut stream = TcpStream::connect(addr).unwrap();
        connect_socks5_proxy(
            &mut stream,
            &test_proxy(ProxyProtocol::Socks5),
            &test_host(),
        )
        .unwrap();
        stream.write_all(b"ping").unwrap();
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }
}
