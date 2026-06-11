use anyhow::{anyhow, Context, Result};
use std::{
    collections::HashMap,
    sync::OnceLock,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
    sync::Mutex,
};
use uuid::Uuid;

use crate::{
    store::{ensure_config_dir, load_config},
    types::HostProfile,
};

pub struct SessionHandle {
    pub id: Uuid,
    pub host: String,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    // kept alive to prevent process exit; killed on session_close
    _child: Child,
}

// Process-local session store. Meaningful in long-running processes (MCP server).
static SESSIONS: OnceLock<Mutex<HashMap<Uuid, SessionHandle>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<Uuid, SessionHandle>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn resolve_host(name: &str) -> Result<HostProfile> {
    load_config()?
        .hosts
        .into_iter()
        .find(|h| h.name == name)
        .ok_or_else(|| anyhow!("unknown host profile: {name}"))
}

fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).display().to_string();
        }
    }
    path.to_string()
}

pub async fn session_open_core(host_name: &str) -> Result<Uuid> {
    ensure_config_dir()?;
    let host = resolve_host(host_name)?;

    let target = match &host.user {
        Some(u) if !u.trim().is_empty() => format!("{}@{}", u, host.host),
        _ => host.host.clone(),
    };

    let mut cmd = tokio::process::Command::new("ssh");
    cmd.arg("-tt")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new")
        .arg("-o")
        .arg("TERM=dumb")
        .arg("-p")
        .arg(host.port.unwrap_or(22).to_string());

    if let Some(key_path) = &host.key_path {
        if !key_path.trim().is_empty() {
            cmd.arg("-i").arg(expand_tilde(key_path));
        }
    }

    cmd.arg(&target)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().context("failed to spawn ssh session")?;

    let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin on child"))?;
    let stdout_raw = child.stdout.take().ok_or_else(|| anyhow!("no stdout on child"))?;
    let stdout = BufReader::new(stdout_raw);

    let id = Uuid::new_v4();
    let handle = SessionHandle {
        id,
        host: host_name.to_string(),
        stdin,
        stdout,
        _child: child,
    };

    sessions().lock().await.insert(id, handle);
    Ok(id)
}

pub async fn session_write_core(id: Uuid, input: &str) -> Result<()> {
    let mut store = sessions().lock().await;
    let handle = store.get_mut(&id).ok_or_else(|| anyhow!("unknown session: {id}"))?;
    handle
        .stdin
        .write_all(input.as_bytes())
        .await
        .context("failed to write to session stdin")?;
    // Flush to ensure the command is sent
    handle.stdin.flush().await.context("failed to flush session stdin")?;
    Ok(())
}

pub async fn session_read_core(id: Uuid, timeout_ms: u64) -> Result<String> {
    let mut store = sessions().lock().await;
    let handle = store.get_mut(&id).ok_or_else(|| anyhow!("unknown session: {id}"))?;

    let mut output = Vec::new();
    let mut chunk = [0u8; 4096];
    // Wait up to `timeout_ms` for first data, then use 200ms quiet period
    let mut remaining_ms = timeout_ms;

    loop {
        let deadline = Duration::from_millis(remaining_ms);
        match tokio::time::timeout(deadline, handle.stdout.read(&mut chunk)).await {
            Ok(Ok(0)) => break, // EOF
            Ok(Ok(n)) => {
                output.extend_from_slice(&chunk[..n]);
                // Use short timeout for subsequent reads (wait for quiet)
                remaining_ms = 200;
            }
            Ok(Err(e)) => return Err(anyhow!("session read error: {e}")),
            Err(_) => break, // timeout — no more data right now
        }
    }

    Ok(String::from_utf8_lossy(&output).into_owned())
}

pub async fn session_close_core(id: Uuid) -> Result<()> {
    let mut store = sessions().lock().await;
    if store.remove(&id).is_none() {
        return Err(anyhow!("unknown session: {id}"));
    }
    // SessionHandle drop kills _child via tokio Child Drop impl
    Ok(())
}

pub async fn session_list_core() -> Vec<(Uuid, String)> {
    sessions()
        .lock()
        .await
        .values()
        .map(|h| (h.id, h.host.clone()))
        .collect()
}
