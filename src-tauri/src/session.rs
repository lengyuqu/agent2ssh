use anyhow::{anyhow, Context, Result};
use std::{
    collections::HashMap,
    sync::{mpsc, Arc, Mutex as StdMutex},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    app_state::app_state,
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
    pub tx: mpsc::Sender<TerminalCommand>,
    pub rx: mpsc::Receiver<TerminalEvent>,
    pub pending_output: Vec<u8>,
    pub connected: bool,
    pub closed: bool,
}

// Process-local session store, now delegated to the centralized AppState
// (P2 #5). The accessor function remains for backward compatibility — it
// returns a reference to the Mutex inside AppState, which is stable for the
// process lifetime since AppState is held in a OnceLock.
fn sessions() -> &'static Mutex<HashMap<Uuid, Arc<StdMutex<SessionHandle>>>> {
    &app_state().sessions
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

    // Reserve a lifecycle entry before creating the resource. If
    // anything fails before activation, the reservation's Drop impl
    // marks it Closed, preventing orphaned Pending entries.
    let lifecycle = crate::app_state::lifecycle();
    let reservation = crate::lifecycle::LifecycleRegistry::reserve(
        &lifecycle,
        &id.to_string(),
        crate::app_state::ResourceKind::SshSession,
        crate::app_state::ResourceOwner::Headless(uuid::Uuid::new_v4()),
    )
    .map_err(|e| anyhow!("lifecycle reserve failed: {e}"))?;

    let mut handle = SessionHandle {
        id,
        host: host_name.to_string(),
        tx,
        rx,
        pending_output: Vec::new(),
        connected: false,
        closed: false,
    };

    probe_session_open(&mut handle).inspect_err(|_| {
        // probe failed — reservation will be dropped, marking the
        // lifecycle entry as Closed automatically.
        let _ = lifecycle.lock().unwrap().close(&id.to_string(), None);
    })?;

    // Session is ready — activate the lifecycle entry.
    reservation
        .activate()
        .map_err(|e| anyhow!("lifecycle activate failed: {e}"))?;

    sessions()
        .lock()
        .await
        .insert(id, Arc::new(StdMutex::new(handle)));
    Ok(id)
}

pub async fn session_write_core(id: Uuid, input: &str) -> Result<()> {
    let handle = sessions()
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| anyhow!("unknown session: {id}"))?;
    let handle = handle
        .lock()
        .map_err(|_| anyhow!("session lock poisoned: {id}"))?;
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
    let handle = sessions()
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| anyhow!("unknown session: {id}"))?;

    tokio::task::spawn_blocking(move || read_session_handle(id, handle, timeout_ms))
        .await
        .map_err(|e| anyhow!("session read task failed: {e}"))?
}

fn read_session_handle(
    id: Uuid,
    handle: Arc<StdMutex<SessionHandle>>,
    timeout_ms: u64,
) -> Result<String> {
    let mut handle = handle
        .lock()
        .map_err(|_| anyhow!("session lock poisoned: {id}"))?;

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
                apply_session_event(&mut handle, event)?;
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
    let handle = handle
        .lock()
        .map_err(|_| anyhow!("session lock poisoned: {id}"))?;
    let _ = handle.tx.send(TerminalCommand::Close);
    // Mark the lifecycle entry as Closed.
    let _ = crate::app_state::lifecycle()
        .lock()
        .unwrap()
        .close(&id.to_string(), None);
    Ok(())
}

pub async fn session_list_core() -> Vec<(Uuid, String)> {
    sessions()
        .lock()
        .await
        .values()
        .filter_map(|h| h.lock().ok().map(|handle| (handle.id, handle.host.clone())))
        .collect()
}
