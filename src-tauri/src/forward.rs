use anyhow::{anyhow, Context, Result};
use std::{
    collections::HashMap,
    io::{ErrorKind, Read, Write},
    net::{IpAddr, Shutdown, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
use tokio::sync::Mutex as TokioMutex;
use uuid::Uuid;

use crate::{
    app_state::app_state,
    embedded_ssh::connect_embedded_ssh,
    store::load_config,
    types::{ForwardDirection, ForwardRule, HostProfile},
};

/// Duration to wait for the worker thread to finish after signaling stop.
/// If the worker doesn't join within this period, we proceed with forced
/// cleanup to avoid hanging the caller.
const DROP_GRACE_PERIOD: Duration = Duration::from_secs(2);

pub struct ForwardHandle {
    rule: ForwardRule,
    stop: Arc<AtomicBool>,
    /// For remote forwards, the worker holds a long-lived SSH session.
    /// Storing it here allows Drop to call `session.disconnect()` before
    /// joining the worker, sending SSH_MSG_DISCONNECT to the server so
    /// it can clean up immediately rather than leaking a half-open session.
    session: Arc<Mutex<Option<ssh2::Session>>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for ForwardHandle {
    fn drop(&mut self) {
        // 1. Signal the worker to stop accepting new connections.
        self.stop.store(true, Ordering::SeqCst);

        // 2. Attempt graceful SSH disconnect before joining.
        //    This sends SSH_MSG_DISCONNECT to the server, allowing it to
        //    clean up the session immediately instead of waiting for TCP
        //    keepalive timeout (which can be minutes).
        if let Ok(mut guard) = self.session.lock() {
            if let Some(session) = guard.take() {
                // Set a short timeout for the disconnect call so we don't
                // block indefinitely if the network is unreachable.
                session.set_timeout(500);
                let _ = session.disconnect(None, "forward closed", None);
            }
        }

        // 3. Join the worker thread with a grace period.
        //    If the worker doesn't finish within DROP_GRACE_PERIOD, we
        //    proceed anyway — the thread will eventually exit on its own
        //    when it notices the stop flag.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join_timeout(DROP_GRACE_PERIOD);
        }
    }
}

/// Extension trait to join a thread with a timeout.
/// std::thread::JoinHandle doesn't have a built-in join_timeout, so we
/// implement it by parking a helper thread.
trait JoinHandleExt {
    fn join_timeout(self, timeout: Duration);
}

impl JoinHandleExt for thread::JoinHandle<()> {
    fn join_timeout(self, timeout: Duration) {
        // Use a channel to implement the timeout. If the worker doesn't
        // finish within the timeout, we just drop the handle — the thread
        // will continue running in the background and eventually exit when
        // it checks the stop flag.
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = self.join();
            let _ = tx.send(());
        });
        let _ = rx.recv_timeout(timeout);
    }
}

// Process-local forward store, delegated to AppState (P2 #5).
fn forwards() -> &'static TokioMutex<HashMap<Uuid, ForwardHandle>> {
    &app_state().forwards
}

fn resolve_host(name: &str) -> Result<HostProfile> {
    load_config()?
        .hosts
        .into_iter()
        .find(|h| h.name == name)
        .ok_or_else(|| anyhow!("unknown host profile: {name}"))
}

fn remote_forward_target_allowed(target_host: &str) -> bool {
    let normalized = target_host.trim().trim_matches(['[', ']']);
    normalized.eq_ignore_ascii_case("localhost")
        || normalized
            .parse::<IpAddr>()
            .map(|addr| addr.is_loopback())
            .unwrap_or(false)
}

pub async fn forward_add_core(
    host_name: &str,
    direction: ForwardDirection,
    bind_port: u16,
    target_host: &str,
    target_port: u16,
) -> Result<ForwardRule> {
    let host = resolve_host(host_name)?;
    if direction == ForwardDirection::Remote && !remote_forward_target_allowed(target_host) {
        return Err(anyhow!(
            "remote forward target_host must be loopback; got '{}'",
            target_host
        ));
    }
    let rule = ForwardRule {
        id: Uuid::new_v4(),
        host: host_name.to_string(),
        direction,
        bind_port,
        target_host: target_host.to_string(),
        target_port,
    };

    // Reserve a lifecycle entry for this forward.
    let lifecycle = crate::app_state::lifecycle();
    let reservation = crate::lifecycle::LifecycleRegistry::reserve(
        &lifecycle,
        &rule.id.to_string(),
        crate::app_state::ResourceKind::Forward,
        crate::app_state::ResourceOwner::Headless(uuid::Uuid::new_v4()),
    )
    .map_err(|e| anyhow!("lifecycle reserve failed: {e}"))?;

    let stop = Arc::new(AtomicBool::new(false));
    let session_slot: Arc<Mutex<Option<ssh2::Session>>> = Arc::new(Mutex::new(None));
    let worker_stop = stop.clone();
    let worker_session_slot = session_slot.clone();
    let worker_rule = rule.clone();
    let worker = thread::spawn(move || {
        let result = match worker_rule.direction {
            ForwardDirection::Local => run_local_forward(host, worker_rule, worker_stop),
            ForwardDirection::Remote => {
                run_remote_forward(host, worker_rule, worker_stop, worker_session_slot)
            }
        };
        if let Err(error) = result {
            let _ = crate::diagnostics::append_diagnostic_log(
                "error",
                "embedded_ssh_forward",
                "forward worker stopped with error",
                Some(serde_json::json!({ "error": error.to_string() })),
            );
        }
    });

    // Forward is ready — activate the lifecycle entry.
    reservation
        .activate()
        .map_err(|e| anyhow!("lifecycle activate failed: {e}"))?;

    forwards().lock().await.insert(
        rule.id,
        ForwardHandle {
            rule: rule.clone(),
            stop,
            session: session_slot,
            worker: Some(worker),
        },
    );
    Ok(rule)
}

fn run_local_forward(host: HostProfile, rule: ForwardRule, stop: Arc<AtomicBool>) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", rule.bind_port))
        .with_context(|| format!("failed to bind local forward port {}", rule.bind_port))?;
    listener.set_nonblocking(true)?;
    let _ = crate::diagnostics::append_diagnostic_log(
        "info",
        "embedded_ssh_forward",
        "local forward listening",
        Some(serde_json::json!({
            "host": rule.host,
            "bind_port": rule.bind_port,
            "target_host": rule.target_host,
            "target_port": rule.target_port,
        })),
    );

    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let host = host.clone();
                let target_host = rule.target_host.clone();
                let target_port = rule.target_port;
                thread::spawn(move || {
                    if let Err(error) =
                        handle_local_connection(host, stream, target_host, target_port)
                    {
                        let _ = crate::diagnostics::append_diagnostic_log(
                            "warn",
                            "embedded_ssh_forward",
                            "local forward connection failed",
                            Some(serde_json::json!({ "error": error.to_string() })),
                        );
                    }
                });
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn handle_local_connection(
    host: HostProfile,
    stream: TcpStream,
    target_host: String,
    target_port: u16,
) -> Result<()> {
    let session = connect_embedded_ssh(&host, 60)?;
    let channel = session.channel_direct_tcpip(&target_host, target_port, None)?;
    session.set_blocking(false);
    bridge_tcp_and_channel(stream, channel)
}

fn run_remote_forward(
    host: HostProfile,
    rule: ForwardRule,
    stop: Arc<AtomicBool>,
    session_slot: Arc<Mutex<Option<ssh2::Session>>>,
) -> Result<()> {
    let session = connect_embedded_ssh(&host, 60)?;
    session.set_blocking(false);
    // Store the session so Drop can call disconnect() on it.
    if let Ok(mut guard) = session_slot.lock() {
        *guard = Some(session.clone());
    }
    let (mut listener, bound_port) =
        session.channel_forward_listen(rule.bind_port, None, Some(16))?;
    let _ = crate::diagnostics::append_diagnostic_log(
        "info",
        "embedded_ssh_forward",
        "remote forward listening",
        Some(serde_json::json!({
            "host": rule.host,
            "bind_port": bound_port,
            "target_host": rule.target_host,
            "target_port": rule.target_port,
        })),
    );

    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok(channel) => {
                let target_host = rule.target_host.clone();
                let target_port = rule.target_port;
                thread::spawn(move || {
                    match TcpStream::connect((target_host.as_str(), target_port)) {
                        Ok(stream) => {
                            if let Err(error) = bridge_tcp_and_channel(stream, channel) {
                                let _ = crate::diagnostics::append_diagnostic_log(
                                    "warn",
                                    "embedded_ssh_forward",
                                    "remote forward connection failed",
                                    Some(serde_json::json!({ "error": error.to_string() })),
                                );
                            }
                        }
                        Err(error) => {
                            let _ = crate::diagnostics::append_diagnostic_log(
                                "warn",
                                "embedded_ssh_forward",
                                "remote forward target connection failed",
                                Some(serde_json::json!({ "error": error.to_string() })),
                            );
                        }
                    }
                });
            }
            Err(error) if ssh_error_is_would_block(&error) => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn ssh_error_is_would_block(error: &ssh2::Error) -> bool {
    matches!(error.code(), ssh2::ErrorCode::Session(-37))
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
            Ok(n) => {
                write_all_channel(&mut channel, &tcp_buf[..n])?;
            }
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
            Ok(n) => {
                write_all_tcp(&mut stream, &channel_buf[..n])?;
            }
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

pub async fn forward_list_core() -> Vec<ForwardRule> {
    forwards()
        .lock()
        .await
        .values()
        .map(|h| h.rule.clone())
        .collect()
}

pub async fn forward_remove_core(id: Uuid) -> Result<()> {
    let mut store = forwards().lock().await;
    if store.remove(&id).is_none() {
        return Err(anyhow!("unknown forward: {id}"));
    }
    // Mark the lifecycle entry as Closed.
    let _ = crate::app_state::lifecycle()
        .lock()
        .unwrap()
        .close(&id.to_string(), None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::remote_forward_target_allowed;

    #[test]
    fn remote_forward_target_allows_only_loopback() {
        assert!(remote_forward_target_allowed("localhost"));
        assert!(remote_forward_target_allowed("127.0.0.1"));
        assert!(remote_forward_target_allowed("::1"));
        assert!(remote_forward_target_allowed("[::1]"));

        assert!(!remote_forward_target_allowed("10.0.0.5"));
        assert!(!remote_forward_target_allowed("metadata.google.internal"));
        assert!(!remote_forward_target_allowed("example.com"));
    }
}
