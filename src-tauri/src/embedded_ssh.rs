use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use sha2::{Digest, Sha256};
use ssh2::{HashType, Session};
use std::{
    io::{ErrorKind, Read, Write},
    net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs},
    path::Path,
    sync::mpsc,
    thread,
    time::Duration,
};

use crate::{store::load_config, types::HostProfile};

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

fn connect_tcp(host: &HostProfile, timeout_secs: u64) -> Result<TcpStream> {
    let address = format!("{}:{}", host.host, host.port.unwrap_or(22));
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

fn resolve_jump_host(name: &str) -> Result<HostProfile> {
    load_config()?
        .hosts
        .into_iter()
        .find(|host| host.name == name)
        .ok_or_else(|| anyhow!("unknown jump host profile: {name}"))
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

fn connect_via_jump(host: &HostProfile, timeout_secs: u64, depth: usize) -> Result<TcpStream> {
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
        .channel_direct_tcpip(&host.host, host.port.unwrap_or(22), None)
        .with_context(|| {
            format!(
                "failed to open direct-tcpip channel via '{jump_name}' to {}:{}",
                host.host,
                host.port.unwrap_or(22)
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
    if via_jump.is_some() {
        let tcp = connect_via_jump(host, timeout_secs, depth)?;
        tcp.set_read_timeout(Some(Duration::from_secs(timeout_secs)))?;
        tcp.set_write_timeout(Some(Duration::from_secs(timeout_secs)))?;
        session.set_tcp_stream(tcp);
    } else {
        session.set_tcp_stream(connect_tcp(host, timeout_secs)?);
    }
    session.handshake()?;

    let username = resolved_username(host);
    let auth_method = authenticate(&session, host, &username)?;

    if !session.authenticated() {
        return Err(anyhow!("SSH authentication failed for '{}'", host.name));
    }

    let host_address = format!("{}:{}", host.host, host.port.unwrap_or(22));
    let _ = crate::diagnostics::append_diagnostic_log(
        "info",
        "embedded_ssh",
        "ssh connection authenticated",
        Some(serde_json::json!({
            "host": host.name,
            "address": host_address,
            "username": username,
            "auth_method": auth_method,
            "fingerprint_sha256": fingerprint_sha256(&session),
            "host_key_algorithm": session.host_key().map(|(_, kind)| host_key_algorithm(kind)),
            "server_banner": session.banner().map(ToOwned::to_owned),
            "jump_host": via_jump,
        })),
    );

    Ok(session)
}

pub fn connect_embedded_ssh(host: &HostProfile, timeout_secs: u64) -> Result<Session> {
    connect_embedded_ssh_inner(host, timeout_secs, 0)
}

fn authenticate(session: &Session, host: &HostProfile, username: &str) -> Result<String> {
    if let Some(password) = host
        .password
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        session.userauth_password(username, password)?;
        return Ok("password".into());
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
