use anyhow::{anyhow, Result};
use ssh2::Session;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

use crate::{
    embedded_ssh::connect_embedded_ssh,
    store::load_config,
    types::{ConnectionStatus, HostProfile},
};

/// How often the supervisor probes each retained connection's liveness (K5).
const PROBE_INTERVAL: Duration = Duration::from_secs(30);
/// libssh2-level keepalive interval requested on each session, so the protocol
/// injects its own keepalives between our probes.
const SSH_KEEPALIVE_SECS: u32 = 15;
/// Reconnect backoff bounds (K5): first retry after BASE, doubling up to MAX.
const RECONNECT_BACKOFF_BASE: Duration = Duration::from_secs(5);
const RECONNECT_BACKOFF_MAX: Duration = Duration::from_secs(300);
/// Timeout (seconds) for a reconnect attempt.
const RECONNECT_TIMEOUT_SECS: u64 = 30;

/// Health/liveness bookkeeping for one retained connection (K5).
#[derive(Default)]
struct ConnectionHealth {
    healthy: bool,
    reconnecting: bool,
    last_error: Option<String>,
    consecutive_failures: u32,
    /// Earliest instant at which the next reconnect attempt may run (backoff).
    next_attempt: Option<Instant>,
}

/// A retained embedded SSH connection plus its health state. The session lives
/// behind a `std` mutex so the blocking supervisor (keepalive probe / reconnect)
/// can touch it inside `spawn_blocking` without holding a tokio lock across an
/// `.await`.
struct RetainedConnection {
    session: Arc<StdMutex<Option<Session>>>,
    health: Arc<StdMutex<ConnectionHealth>>,
}

type ConnectionHandleSnapshot = (
    String,
    Arc<StdMutex<Option<Session>>>,
    Arc<StdMutex<ConnectionHealth>>,
);

static CONNECTIONS: OnceLock<Mutex<HashMap<String, RetainedConnection>>> = OnceLock::new();
static HOST_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
static SUPERVISOR_STARTED: AtomicBool = AtomicBool::new(false);

fn connections() -> &'static Mutex<HashMap<String, RetainedConnection>> {
    CONNECTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn host_locks() -> &'static Mutex<HashMap<String, Arc<Mutex<()>>>> {
    HOST_LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn per_host_lock(host_name: &str) -> Arc<Mutex<()>> {
    let mut map = host_locks().lock().await;
    map.entry(host_name.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn resolve_host(host_name: &str) -> Result<HostProfile> {
    load_config()?
        .hosts
        .into_iter()
        .find(|host| host.name == host_name)
        .ok_or_else(|| anyhow!("unknown host profile: {host_name}"))
}

/// Apply libssh2 keepalive settings to a freshly established session.
fn configure_keepalive(session: &Session) {
    // want_reply=true asks the server to acknowledge, so a dead peer surfaces
    // sooner via the next `keepalive_send`.
    session.set_keepalive(true, SSH_KEEPALIVE_SECS);
}

/// List configured hosts and whether Agent2SSH currently holds an embedded SSH
/// session for them, including liveness/reconnect state (K5).
pub async fn list_active_connections() -> Vec<ConnectionStatus> {
    let config = match load_config() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let store = connections().lock().await;
    config
        .hosts
        .iter()
        .map(|host| {
            let (connected, healthy, reconnecting, last_error) = match store.get(&host.name) {
                Some(conn) => {
                    let session_present = conn.session.lock().map(|s| s.is_some()).unwrap_or(false);
                    let health = conn.health.lock().ok();
                    let healthy = health.as_ref().map(|h| h.healthy).unwrap_or(false);
                    let reconnecting = health.as_ref().map(|h| h.reconnecting).unwrap_or(false);
                    let last_error = health.as_ref().and_then(|h| h.last_error.clone());
                    (session_present, healthy, reconnecting, last_error)
                }
                None => (false, false, false, None),
            };
            ConnectionStatus {
                host: host.name.clone(),
                connected,
                socket_path: None,
                healthy,
                reconnecting,
                last_error,
            }
        })
        .collect()
}

/// Manually establish and retain an embedded SSH connection to a specific host.
pub async fn connect_host(host_name: &str) -> Result<()> {
    let host = resolve_host(host_name)?;
    let lock = per_host_lock(&host.name).await;
    let _guard = lock.lock().await;

    if connections().lock().await.contains_key(&host.name) {
        return Ok(());
    }

    let host_for_task = host.clone();
    let session = tokio::task::spawn_blocking(move || connect_embedded_ssh(&host_for_task, 60))
        .await
        .map_err(|e| anyhow!("embedded connection task failed: {e}"))??;
    configure_keepalive(&session);

    connections().lock().await.insert(
        host.name.clone(),
        RetainedConnection {
            session: Arc::new(StdMutex::new(Some(session))),
            health: Arc::new(StdMutex::new(ConnectionHealth {
                healthy: true,
                ..Default::default()
            })),
        },
    );

    ensure_supervisor_running();
    Ok(())
}

/// Close a retained embedded SSH connection.
pub async fn disconnect_host(host_name: &str) -> Result<()> {
    resolve_host(host_name)?;
    connections().lock().await.remove(host_name);
    Ok(())
}

/// Spawn the background supervisor once. It periodically probes every retained
/// connection and reconnects dropped ones with exponential backoff (K5).
fn ensure_supervisor_running() {
    if SUPERVISOR_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(PROBE_INTERVAL).await;
            supervise_all().await;
        }
    });
}

/// One supervision pass over all retained connections.
async fn supervise_all() {
    // Snapshot host names + handles so we don't hold the map lock across awaits.
    let handles: Vec<ConnectionHandleSnapshot> = {
        let store = connections().lock().await;
        store
            .iter()
            .map(|(name, conn)| (name.clone(), conn.session.clone(), conn.health.clone()))
            .collect()
    };

    for (name, session, health) in handles {
        supervise_one(&name, session, health).await;
    }
}

async fn supervise_one(
    name: &str,
    session: Arc<StdMutex<Option<Session>>>,
    health: Arc<StdMutex<ConnectionHealth>>,
) {
    // Probe liveness with a keepalive in a blocking task.
    let probe_session = session.clone();
    let probe = tokio::task::spawn_blocking(move || {
        let guard = probe_session
            .lock()
            .map_err(|_| "session lock poisoned".to_string())?;
        match guard.as_ref() {
            Some(s) => s.keepalive_send().map(|_| ()).map_err(|e| e.to_string()),
            None => Err("no live session".to_string()),
        }
    })
    .await;

    let probe_ok = matches!(probe, Ok(Ok(())));
    if probe_ok {
        if let Ok(mut h) = health.lock() {
            h.healthy = true;
            h.reconnecting = false;
            h.last_error = None;
            h.consecutive_failures = 0;
            h.next_attempt = None;
        }
        return;
    }

    // Record the failure and decide whether a reconnect attempt is due.
    let err_msg = match probe {
        Ok(Err(e)) => e,
        Err(e) => format!("probe task failed: {e}"),
        Ok(Ok(())) => unreachable!(),
    };
    let attempt_due = {
        let Ok(mut h) = health.lock() else { return };
        h.healthy = false;
        h.last_error = Some(err_msg);
        match h.next_attempt {
            Some(at) if Instant::now() < at => false,
            _ => {
                h.reconnecting = true;
                true
            }
        }
    };
    if !attempt_due {
        return;
    }

    // Attempt one reconnect (blocking) using the current host profile.
    let host = match resolve_host(name) {
        Ok(h) => h,
        Err(e) => {
            finish_reconnect(&health, Err(format!("host profile gone: {e}")));
            return;
        }
    };
    let new_session =
        tokio::task::spawn_blocking(move || connect_embedded_ssh(&host, RECONNECT_TIMEOUT_SECS))
            .await;

    match new_session {
        Ok(Ok(s)) => {
            configure_keepalive(&s);
            if let Ok(mut guard) = session.lock() {
                *guard = Some(s);
            }
            finish_reconnect(&health, Ok(()));
        }
        Ok(Err(e)) => finish_reconnect(&health, Err(e.to_string())),
        Err(e) => finish_reconnect(&health, Err(format!("reconnect task failed: {e}"))),
    }
}

/// Update health after a reconnect attempt: clear state on success, or schedule
/// the next attempt with exponential backoff on failure.
fn finish_reconnect(health: &Arc<StdMutex<ConnectionHealth>>, result: Result<(), String>) {
    let Ok(mut h) = health.lock() else { return };
    h.reconnecting = false;
    match result {
        Ok(()) => {
            h.healthy = true;
            h.last_error = None;
            h.consecutive_failures = 0;
            h.next_attempt = None;
        }
        Err(e) => {
            h.healthy = false;
            h.last_error = Some(e);
            h.consecutive_failures = h.consecutive_failures.saturating_add(1);
            h.next_attempt = Some(Instant::now() + backoff_delay(h.consecutive_failures));
        }
    }
}

/// Exponential backoff: BASE * 2^(failures-1), capped at MAX.
fn backoff_delay(failures: u32) -> Duration {
    if failures == 0 {
        return RECONNECT_BACKOFF_BASE;
    }
    let shift = failures.saturating_sub(1).min(16);
    let scaled = RECONNECT_BACKOFF_BASE
        .checked_mul(1u32 << shift)
        .unwrap_or(RECONNECT_BACKOFF_MAX);
    scaled.min(RECONNECT_BACKOFF_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_caps() {
        assert_eq!(backoff_delay(0), RECONNECT_BACKOFF_BASE);
        assert_eq!(backoff_delay(1), RECONNECT_BACKOFF_BASE);
        assert_eq!(backoff_delay(2), RECONNECT_BACKOFF_BASE * 2);
        assert_eq!(backoff_delay(3), RECONNECT_BACKOFF_BASE * 4);
        // Far out, it saturates at the max rather than overflowing.
        assert_eq!(backoff_delay(60), RECONNECT_BACKOFF_MAX);
    }
}
