use anyhow::{anyhow, Context, Result};
use std::{
    collections::HashMap,
    sync::{mpsc, OnceLock},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    embedded_ssh::{spawn_terminal, TerminalCommand, TerminalEvent},
    store::{ensure_config_dir, load_config},
    types::HostProfile,
};

const DEFAULT_SESSION_COLS: u32 = 80;
const DEFAULT_SESSION_ROWS: u32 = 24;
const SESSION_OPEN_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const SESSION_READ_QUIET_PERIOD: Duration = Duration::from_millis(200);

pub struct SessionHandle {
    pub id: Uuid,
    pub host: String,
    tx: mpsc::Sender<TerminalCommand>,
    rx: mpsc::Receiver<TerminalEvent>,
    pending_output: Vec<u8>,
    connected: bool,
    closed: bool,
}

// Process-local session store. Meaningful in long-running processes (daemon/MCP/Tauri).
static SESSIONS: OnceLock<Mutex<HashMap<Uuid, SessionHandle>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<Uuid, SessionHandle>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn resolve_host(name: &str) -> Result<HostProfile> {
    load_config()?
        .hosts
        .into_iter()
        .find(|h| h.name == name)
        .ok_or_else(|| anyhow!("unknown host profile: {name}"))
}

fn apply_session_event(handle: &mut SessionHandle, event: TerminalEvent) -> Result<()> {
    match event {
        TerminalEvent::Connected(_) => {
            handle.connected = true;
            Ok(())
        }
        TerminalEvent::Output(data) => {
            handle.pending_output.extend_from_slice(&data);
            Ok(())
        }
        TerminalEvent::Error(error) => {
            handle.closed = true;
            Err(anyhow!("session error: {error}"))
        }
        TerminalEvent::Closed => {
            handle.closed = true;
            Ok(())
        }
    }
}

fn probe_session_open(handle: &mut SessionHandle) -> Result<()> {
    let deadline = Instant::now() + SESSION_OPEN_PROBE_TIMEOUT;
    while Instant::now() < deadline && !handle.connected && !handle.closed {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match handle.rx.recv_timeout(remaining) {
            Ok(event) => apply_session_event(handle, event)?,
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                handle.closed = true;
                return Err(anyhow!("session worker disconnected during open"));
            }
        }
    }

    while let Ok(event) = handle.rx.try_recv() {
        apply_session_event(handle, event)?;
    }

    if handle.closed && !handle.connected {
        return Err(anyhow!("session closed during open"));
    }

    Ok(())
}

pub async fn session_open_core(host_name: &str) -> Result<Uuid> {
    ensure_config_dir()?;
    let host = resolve_host(host_name)?;
    let (tx, rx) = spawn_terminal(host, DEFAULT_SESSION_COLS, DEFAULT_SESSION_ROWS);
    let id = Uuid::new_v4();
    let mut handle = SessionHandle {
        id,
        host: host_name.to_string(),
        tx,
        rx,
        pending_output: Vec::new(),
        connected: false,
        closed: false,
    };

    probe_session_open(&mut handle)?;
    sessions().lock().await.insert(id, handle);
    Ok(id)
}

pub async fn session_write_core(id: Uuid, input: &str) -> Result<()> {
    let store = sessions().lock().await;
    let handle = store
        .get(&id)
        .ok_or_else(|| anyhow!("unknown session: {id}"))?;
    if handle.closed {
        return Err(anyhow!("session is closed: {id}"));
    }
    handle
        .tx
        .send(TerminalCommand::Input(input.as_bytes().to_vec()))
        .context("failed to write to embedded SSH session")?;
    Ok(())
}

pub async fn session_read_core(id: Uuid, timeout_ms: u64) -> Result<String> {
    let mut store = sessions().lock().await;
    let handle = store
        .get_mut(&id)
        .ok_or_else(|| anyhow!("unknown session: {id}"))?;

    let mut output = std::mem::take(&mut handle.pending_output);
    let mut wait = if output.is_empty() {
        Duration::from_millis(timeout_ms)
    } else {
        SESSION_READ_QUIET_PERIOD
    };

    loop {
        match handle.rx.recv_timeout(wait) {
            Ok(event) => {
                let had_output = matches!(event, TerminalEvent::Output(_));
                apply_session_event(handle, event)?;
                if had_output {
                    output.extend_from_slice(&handle.pending_output);
                    handle.pending_output.clear();
                    wait = SESSION_READ_QUIET_PERIOD;
                } else if handle.closed {
                    output.extend_from_slice(&handle.pending_output);
                    handle.pending_output.clear();
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                handle.closed = true;
                break;
            }
        }
    }

    Ok(String::from_utf8_lossy(&output).into_owned())
}

pub async fn session_close_core(id: Uuid) -> Result<()> {
    let mut store = sessions().lock().await;
    let handle = store
        .remove(&id)
        .ok_or_else(|| anyhow!("unknown session: {id}"))?;
    let _ = handle.tx.send(TerminalCommand::Close);
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
