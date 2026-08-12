use anyhow::{anyhow, Result};
use ssh2::Session;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::{watch, Mutex};
use uuid::Uuid;

use crate::{
    app_state::app_state,
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

/// T2-11: Connection drop signal.
/// When a connection is dropped, the watch channel sends `false` to all
/// subscribers, allowing blocking tasks (exec, SFTP, terminal) to cancel
/// their pending operations on that connection.
pub type DropSignal = watch::Sender<bool>;
pub type DropReceiver = watch::Receiver<bool>;

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
pub struct RetainedConnection {
    session: Arc<StdMutex<Option<Session>>>,
    health: Arc<StdMutex<ConnectionHealth>>,
    /// T2-11: Drop signal — sends `false` when connection drops.
    /// Blocking tasks subscribe to this to cancel their pending operations.
    drop_tx: DropSignal,
}

type ConnectionHandleSnapshot = (
    String,
    Arc<StdMutex<Option<Session>>>,
    Arc<StdMutex<ConnectionHealth>>,
    DropSignal,
);

/// T2-11: Get a drop receiver for a specific host.
/// Returns `None` if no retained connection exists.
/// The receiver yields `false` when the connection drops, allowing
/// blocking tasks to cancel their pending operations.
pub async fn subscribe_drop(host_name: &str) -> Option<DropReceiver> {
    let store = connections().lock().await;
    store.get(host_name).map(|conn| conn.drop_tx.subscribe())
}

// Process-local connection stores, delegated to AppState (P2 #5).
static SUPERVISOR_STARTED: AtomicBool = AtomicBool::new(false);

fn connections() -> &'static Mutex<HashMap<String, RetainedConnection>> {
    &app_state().connections
}

fn host_locks() -> &'static Mutex<HashMap<String, Arc<Mutex<()>>>> {
    &app_state().host_locks
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

    // Reserve a lifecycle entry for this connection.
    let lifecycle = crate::app_state::lifecycle();
    let reservation = crate::lifecycle::LifecycleRegistry::reserve(
        &lifecycle,
        &host.name,
        crate::app_state::ResourceKind::Connection,
        crate::app_state::ResourceOwner::Headless(Uuid::new_v4()),
    )
    .map_err(|e| anyhow!("lifecycle reserve failed: {e}"))?;

    let host_for_task = host.clone();
    let session = tokio::task::spawn_blocking(move || connect_embedded_ssh(&host_for_task, 60))
        .await
        .map_err(|e| anyhow!("embedded connection task failed: {e}"))??;
    configure_keepalive(&session);

    // Connection is ready — activate the lifecycle entry.
    reservation
        .activate()
        .map_err(|e| anyhow!("lifecycle activate failed: {e}"))?;

    // T2-11: Create drop signal channel (true = connected, false = dropped)
    let (drop_tx, _drop_rx) = watch::channel(true);

    connections().lock().await.insert(
        host.name.clone(),
        RetainedConnection {
            session: Arc::new(StdMutex::new(Some(session))),
            health: Arc::new(StdMutex::new(ConnectionHealth {
                healthy: true,
                ..Default::default()
            })),
            drop_tx,
        },
    );

    ensure_supervisor_running();
    Ok(())
}

/// Close a retained embedded SSH connection.
pub async fn disconnect_host(host_name: &str) -> Result<()> {
    resolve_host(host_name)?;
    connections().lock().await.remove(host_name);
    // Mark the lifecycle entry as Closed.
    let _ = crate::app_state::lifecycle()
        .lock()
        .unwrap()
        .close(host_name, None);
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
            .map(|(name, conn)| {
                (
                    name.clone(),
                    conn.session.clone(),
                    conn.health.clone(),
                    conn.drop_tx.clone(),
                )
            })
            .collect()
    };

    for (name, session, health, drop_tx) in handles {
        supervise_one(&name, session, health, drop_tx).await;
    }
}

async fn supervise_one(
    name: &str,
    session: Arc<StdMutex<Option<Session>>>,
    health: Arc<StdMutex<ConnectionHealth>>,
    drop_tx: DropSignal,
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
    // T2-11: Broadcast connection drop to all subscribers
    let _ = drop_tx.send(false);
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
            // T2-11: Broadcast connection restored
            let _ = drop_tx.send(true);
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

    #[tokio::test]
    async fn t2_11_drop_signal_propagates() {
        // T2-11: Verify that a watch channel can broadcast drop and restore
        let (tx, rx) = watch::channel(true);
        assert!(*rx.borrow(), "initial state should be connected");

        // Simulate connection drop
        let _ = tx.send(false);
        assert!(!*rx.borrow(), "should reflect drop after send(false)");

        // Simulate reconnection
        let _ = tx.send(true);
        assert!(*rx.borrow(), "should reflect restore after send(true)");
    }

    #[tokio::test]
    async fn t2_11_subscribe_drop_returns_none_for_unknown_host() {
        let receiver = subscribe_drop("nonexistent-host-xyz").await;
        assert!(receiver.is_none(), "should return None for unknown host");
    }
}
