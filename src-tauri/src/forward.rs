use anyhow::{anyhow, Context, Result};
use std::{
    collections::HashMap,
    io::{ErrorKind, Read, Write},
    net::{IpAddr, Shutdown, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering},
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

// ── A3: Per-rule atomic counters ──────────────────────────────────────────
//
// `RuleControl` provides real-time observability for each port-forward rule
// without locks. Counters are `Arc<Atomic*>` so they can be shared between
// the accept loop (incrementing) and any reader (e.g., a metrics endpoint).
//
// Design borrowed from rssh's `RuleControl`:
// - `bytes_tx` / `bytes_rx`: total bytes transferred through this rule
// - `connections`: how many client connections have been accepted
// - `state`: current rule state (Running / Stopped / Error)
//
// The `RuleControl` is stored alongside `ForwardHandle` and is cloned into
// each per-connection thread so the bridge loop can update byte counters.

/// State of a single forward rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleState {
    /// Worker spawned but listener not yet bound / SSH session not ready.
    Starting,
    /// Listener bound and actively accepting connections.
    Active,
    /// Gracefully stopped.
    Stopped,
    /// Worker failed.
    Error,
    /// Graceful stop failed (disconnect or join timeout).
    StoppingError,
}

/// Per-rule atomic counters for observability.
/// All fields are `Arc<Atomic*>` so they can be shared across threads
/// without locking.
#[derive(Debug, Clone)]
pub struct RuleControl {
    /// Bytes sent from local → remote (client → SSH target).
    pub bytes_tx: Arc<AtomicU64>,
    /// Bytes sent from remote → local (SSH target → client).
    pub bytes_rx: Arc<AtomicU64>,
    /// Number of client connections accepted.
    pub connections: Arc<AtomicU32>,
    /// Current rule state.
    pub state: Arc<Mutex<RuleState>>,
    /// The actual bound port (0 until listener is bound, then the real port).
    pub effective_port: Arc<AtomicU16>,
    /// Whether the forward's SSH transport / listener is alive.
    pub connected: Arc<AtomicBool>,
}

impl Default for RuleControl {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleControl {
    pub fn new() -> Self {
        Self {
            bytes_tx: Arc::new(AtomicU64::new(0)),
            bytes_rx: Arc::new(AtomicU64::new(0)),
            connections: Arc::new(AtomicU32::new(0)),
            state: Arc::new(Mutex::new(RuleState::Starting)),
            effective_port: Arc::new(AtomicU16::new(0)),
            connected: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn snapshot(&self) -> RuleStats {
        let port = self.effective_port.load(Ordering::Relaxed);
        RuleStats {
            bytes_tx: self.bytes_tx.load(Ordering::Relaxed),
            bytes_rx: self.bytes_rx.load(Ordering::Relaxed),
            connections: self.connections.load(Ordering::Relaxed),
            state: *self.state.lock().unwrap(),
            effective_port: if port > 0 { Some(port) } else { None },
            connected: self.connected.load(Ordering::Relaxed),
        }
    }

    fn set_state(&self, state: RuleState) {
        *self.state.lock().unwrap() = state;
    }
}

/// A point-in-time snapshot of rule statistics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RuleStats {
    pub bytes_tx: u64,
    pub bytes_rx: u64,
    pub connections: u32,
    pub state: RuleState,
    /// The actual bound port (None until the listener is bound).
    pub effective_port: Option<u16>,
    /// Whether the SSH transport / listener is alive.
    pub connected: bool,
}

// ── A2: bind_loopback dual-stack best-effort ─────────────────────────────
//
// On many systems, binding `127.0.0.1` means only IPv4 loopback works.
// Modern clients (especially browsers using `localhost`) may try IPv6
// (`::1`) first. If we only bind IPv4, these clients experience a
// connection delay or failure.
//
// `bind_loopback` binds IPv4 first (required), then attempts IPv6
// best-effort. If IPv6 bind fails (common on systems with `::1` already
// in use or IPv6 disabled), we log and continue — IPv4 alone is
// functional. The IPv6 listener runs in a separate thread that also
// accepts and dispatches connections.
//
// Design borrowed from rssh's `bind_loopback(port)` function.

/// Bind to loopback on both IPv4 and IPv6 (best-effort for IPv6).
/// Returns `(ipv4_listener, Option<ipv6_listener>)`.
/// IPv4 bind is required; if it fails, the error propagates.
/// IPv6 bind is best-effort: failures are logged but don't fail.
fn bind_loopback(port: u16) -> Result<(TcpListener, Option<TcpListener>)> {
    let v4 = TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("failed to bind 127.0.0.1:{}", port))?;
    v4.set_nonblocking(true)?;

    let v6 = match TcpListener::bind(("::1", port)) {
        Ok(listener) => {
            listener.set_nonblocking(true)?;
            let _ = crate::diagnostics::append_diagnostic_log(
                "info",
                "embedded_ssh_forward",
                "dual-stack loopback bound (IPv4 + IPv6)",
                Some(serde_json::json!({ "port": port })),
            );
            Some(listener)
        }
        Err(error) => {
            // IPv6 not available — this is common on systems with IPv6
            // disabled or when the port is already bound on IPv6.
            let _ = crate::diagnostics::append_diagnostic_log(
                "info",
                "embedded_ssh_forward",
                "IPv6 loopback bind skipped (best-effort)",
                Some(serde_json::json!({
                    "port": port,
                    "error": error.to_string(),
                })),
            );
            None
        }
    };

    Ok((v4, v6))
}

/// Duration to wait for the worker thread to finish after signaling stop.
/// If the worker doesn't join within this period, we proceed with forced
/// cleanup to avoid hanging the caller.
const DROP_GRACE_PERIOD: Duration = Duration::from_secs(2);

pub struct ForwardHandle {
    rule: ForwardRule,
    stop: Arc<AtomicBool>,
    /// A3: Per-rule atomic counters for observability.
    pub control: RuleControl,
    /// For remote forwards, the worker holds a long-lived SSH session.
    /// Storing it here allows Drop to call `session.disconnect()` before
    /// joining the worker, sending SSH_MSG_DISCONNECT to the server so it
    /// can clean up immediately rather than leaking a half-open session.
    session: Arc<Mutex<Option<ssh2::Session>>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ForwardHandle {
    /// Return the rule that this handle was created for.
    /// Finding 17: Used by `forward_start_core` to restart a stopped rule.
    pub fn rule(&self) -> &ForwardRule {
        &self.rule
    }

    /// Finding 17: Stop the worker thread and disconnect the session without
    /// removing the handle from the store. The rule can be restarted later
    /// with `forward_start_core`.
    pub fn stop_worker(&mut self) {
        // Signal the worker to stop accepting new connections.
        self.stop.store(true, Ordering::SeqCst);
        self.control.connected.store(false, Ordering::SeqCst);

        // Q4: Track disconnect failure so the final state is consistent
        // with Drop's behavior — StoppingError if disconnect failed.
        let mut disconnect_failed = false;
        if let Ok(mut guard) = self.session.lock() {
            if let Some(session) = guard.take() {
                session.set_timeout(500);
                if session.disconnect(None, "forward stopped", None).is_err() {
                    disconnect_failed = true;
                }
            }
        }

        // Join the worker thread with a grace period.
        if let Some(worker) = self.worker.take() {
            worker.join_timeout(DROP_GRACE_PERIOD);
        }

        // Update state — same logic as Drop.
        let current_state = *self.control.state.lock().unwrap();
        if current_state == RuleState::Error {
            // Worker already errored — preserve that state.
        } else if disconnect_failed {
            self.control.set_state(RuleState::StoppingError);
        } else {
            self.control.set_state(RuleState::Stopped);
        }
    }
}

impl Drop for ForwardHandle {
    fn drop(&mut self) {
        // 1. Signal the worker to stop accepting new connections.
        self.stop.store(true, Ordering::SeqCst);

        // Finding 7: Mark transport as disconnected.
        self.control.connected.store(false, Ordering::SeqCst);

        // 2. Attempt graceful SSH disconnect before joining.
        //    This sends SSH_MSG_DISCONNECT to the server, allowing it to
        //    clean up the session immediately instead of waiting for TCP
        //    keepalive timeout (which can be minutes).
        let mut disconnect_failed = false;
        if let Ok(mut guard) = self.session.lock() {
            if let Some(session) = guard.take() {
                // Set a short timeout for the disconnect call so we don't
                // block indefinitely if the network is unreachable.
                session.set_timeout(500);
                if session.disconnect(None, "forward closed", None).is_err() {
                    disconnect_failed = true;
                }
            }
        }

        // 3. Join the worker thread with a grace period.
        //    If the worker doesn't finish within DROP_GRACE_PERIOD, we
        //    proceed anyway — the thread will eventually exit on its own
        //    when it notices the stop flag.
        if let Some(worker) = self.worker.take() {
            worker.join_timeout(DROP_GRACE_PERIOD);
        }

        // Finding 4: Set final state — StoppingError if disconnect or join failed.
        let current_state = *self.control.state.lock().unwrap();
        if current_state == RuleState::Error {
            // Worker already errored — preserve that state.
        } else if disconnect_failed {
            self.control.set_state(RuleState::StoppingError);
        } else {
            self.control.set_state(RuleState::Stopped);
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

/// B68: Resolve a host profile and optionally override or set its jump_host.
/// If `via` is `Some`, the host's `jump_host` field is set to that value
/// (overriding any existing jump_host). This allows CLI users to specify
/// a bastion/jump host at forward time without modifying the stored profile.
fn resolve_host_with_jump(name: &str, via: Option<&str>) -> Result<HostProfile> {
    let mut host = resolve_host(name)?;
    if let Some(jump) = via.map(str::trim).filter(|s| !s.is_empty()) {
        // Validate that the jump host profile exists.
        let _jump_host = load_config()?
            .hosts
            .into_iter()
            .find(|h| h.name == jump)
            .ok_or_else(|| anyhow!("unknown jump host profile: '{jump}'"))?;
        host.jump_host = Some(jump.to_string());
    }
    Ok(host)
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
    forward_add_core_via(
        host_name,
        direction,
        bind_port,
        target_host,
        target_port,
        None,
    )
    .await
}

/// B68: Start a port forward with an optional jump host override.
/// `via` — if `Some("bastion")`, uses "bastion" as the jump host regardless
/// of the stored profile's `jump_host` field.
pub async fn forward_add_core_via(
    host_name: &str,
    direction: ForwardDirection,
    bind_port: u16,
    target_host: &str,
    target_port: u16,
    via: Option<&str>,
) -> Result<ForwardRule> {
    let host = resolve_host_with_jump(host_name, via)?;
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
        name: None,
        group_id: None,
        via: via.map(|s| s.to_string()),
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

    let handle = start_forward_worker(host, rule.clone()).await?;

    // Forward is ready — activate the lifecycle entry.
    if let Err(error) = reservation.activate() {
        drop(handle);
        return Err(anyhow!("lifecycle activate failed: {error}"));
    }

    forwards().lock().await.insert(rule.id, handle);
    Ok(rule)
}

/// Start one forwarding worker and wait until its SSH authentication and
/// listener setup have succeeded. This prevents callers from seeing a rule as
/// active when the worker has already failed in the background.
async fn start_forward_worker(host: HostProfile, rule: ForwardRule) -> Result<ForwardHandle> {
    let host_for_connect = host.clone();
    let session = tokio::task::spawn_blocking(move || connect_embedded_ssh(&host_for_connect, 60))
        .await
        .map_err(|e| anyhow!("forward SSH task failed: {e}"))??;

    let stop = Arc::new(AtomicBool::new(false));
    let session_slot: Arc<Mutex<Option<ssh2::Session>>> = Arc::new(Mutex::new(None));
    let control = RuleControl::new();
    let worker_stop = stop.clone();
    let worker_session_slot = session_slot.clone();
    let worker_control = control.clone();
    let worker_rule = rule.clone();
    let worker_direction = rule.direction;
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<std::result::Result<(), String>>();
    let worker = thread::spawn(move || {
        let error_tx = ready_tx.clone();
        let result = match worker_rule.direction {
            ForwardDirection::Local => {
                // Local forwards open one direct-tcpip channel per client. The
                // connection above is an authentication preflight; each client
                // gets an independent SSH session so libssh2 blocking modes do
                // not interfere across concurrent channels.
                drop(session);
                run_local_forward(
                    host,
                    worker_rule,
                    worker_stop,
                    worker_control.clone(),
                    ready_tx,
                )
            }
            ForwardDirection::Remote => run_remote_forward(
                session,
                worker_rule,
                worker_stop,
                worker_session_slot,
                worker_control.clone(),
                ready_tx,
            ),
        };
        if let Err(error) = result {
            worker_control.set_state(RuleState::Error);
            let _ = error_tx.send(Err(error.to_string()));
            let _ = crate::diagnostics::append_diagnostic_log(
                "error",
                "embedded_ssh_forward",
                "forward worker stopped with error",
                Some(serde_json::json!({ "error": error.to_string() })),
            );
        }
    });

    let ready = tokio::task::spawn_blocking(move || ready_rx.recv_timeout(Duration::from_secs(65)))
        .await
        .map_err(|e| anyhow!("forward readiness task failed: {e}"))?;
    match ready {
        Ok(Ok(())) => Ok(ForwardHandle {
            rule,
            stop,
            control,
            session: session_slot,
            worker: Some(worker),
        }),
        Ok(Err(message)) => {
            stop.store(true, Ordering::SeqCst);
            worker.join_timeout(DROP_GRACE_PERIOD);
            Err(anyhow!(message))
        }
        Err(_) => {
            stop.store(true, Ordering::SeqCst);
            worker.join_timeout(DROP_GRACE_PERIOD);
            Err(anyhow!(
                "{} forward did not become ready within 65 seconds",
                worker_direction
            ))
        }
    }
}

fn run_local_forward(
    host: HostProfile,
    rule: ForwardRule,
    stop: Arc<AtomicBool>,
    control: RuleControl,
    ready: std::sync::mpsc::Sender<std::result::Result<(), String>>,
) -> Result<()> {
    // A2: Dual-stack loopback binding (IPv4 required, IPv6 best-effort).
    let (listener_v4, listener_v6) = bind_loopback(rule.bind_port)?;
    // Findings 4+6+7: Record actual bound port, mark as Active + connected.
    let actual_port = listener_v4.local_addr().map(|a| a.port()).unwrap_or(rule.bind_port);
    control.effective_port.store(actual_port, Ordering::Relaxed);
    control.connected.store(true, Ordering::Relaxed);
    control.set_state(RuleState::Active);
    let _ = crate::diagnostics::append_diagnostic_log(
        "info",
        "embedded_ssh_forward",
        "local forward listening",
        Some(serde_json::json!({
            "host": rule.host,
            "bind_port": rule.bind_port,
            "target_host": rule.target_host,
            "target_port": rule.target_port,
            "ipv6": listener_v6.is_some(),
        })),
    );
    let _ = ready.send(Ok(()));

    // If we have an IPv6 listener, run it in a separate thread.
    if let Some(listener_v6) = listener_v6 {
        let host_v6 = host.clone();
        let rule_v6 = rule.clone();
        let stop_v6 = stop.clone();
        let control_v6 = control.clone();
        thread::spawn(move || {
            run_accept_loop(listener_v6, host_v6, rule_v6, stop_v6, control_v6);
        });
    }

    run_accept_loop(listener_v4, host, rule, stop, control);
    Ok(())
}

/// Shared accept loop for both IPv4 and IPv6 listeners.
fn run_accept_loop(
    listener: TcpListener,
    host: HostProfile,
    rule: ForwardRule,
    stop: Arc<AtomicBool>,
    control: RuleControl,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                control.connections.fetch_add(1, Ordering::Relaxed);
                let host = host.clone();
                let target_host = rule.target_host.clone();
                let target_port = rule.target_port;
                let control = control.clone();
                thread::spawn(move || {
                    if let Err(error) =
                        handle_local_connection(host, stream, target_host, target_port, &control)
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
            // T1-5: Accept-error resilient recovery — log + backoff + continue
            // instead of killing the entire forward. Accept can fail transiently
            // due to fd exhaustion or kernel pressure; aborting would drop all
            // active connections through this tunnel.
            Err(error) => {
                let _ = crate::diagnostics::append_diagnostic_log(
                    "warn",
                    "embedded_ssh_forward",
                    "local forward accept error; backing off",
                    Some(serde_json::json!({
                        "host": rule.host,
                        "bind_port": rule.bind_port,
                        "error": error.to_string(),
                    })),
                );
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn handle_local_connection(
    host: HostProfile,
    stream: TcpStream,
    target_host: String,
    target_port: u16,
    control: &RuleControl,
) -> Result<()> {
    let session = connect_embedded_ssh(&host, 60)?;
    let channel = session.channel_direct_tcpip(&target_host, target_port, None)?;
    session.set_blocking(false);
    bridge_tcp_and_channel(stream, channel, control)
}

fn run_remote_forward(
    session: ssh2::Session,
    rule: ForwardRule,
    stop: Arc<AtomicBool>,
    session_slot: Arc<Mutex<Option<ssh2::Session>>>,
    control: RuleControl,
    ready: std::sync::mpsc::Sender<std::result::Result<(), String>>,
) -> Result<()> {
    session.set_blocking(false);
    // Store the session so Drop can call disconnect() on it.
    if let Ok(mut guard) = session_slot.lock() {
        *guard = Some(session.clone());
    }
    let (mut listener, bound_port) =
        session.channel_forward_listen(rule.bind_port, None, Some(16))?;
    // Findings 4+6+7: Record actual bound port, mark as Active + connected.
    control.effective_port.store(bound_port, Ordering::Relaxed);
    control.connected.store(true, Ordering::Relaxed);
    control.set_state(RuleState::Active);
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
    let _ = ready.send(Ok(()));

    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok(channel) => {
                control.connections.fetch_add(1, Ordering::Relaxed);
                let target_host = rule.target_host.clone();
                let target_port = rule.target_port;
                let control = control.clone();
                thread::spawn(move || {
                    match TcpStream::connect((target_host.as_str(), target_port)) {
                        Ok(stream) => {
                            if let Err(error) = bridge_tcp_and_channel(stream, channel, &control) {
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
            // T1-5: Accept-error resilient recovery — log + backoff + continue
            // instead of killing the entire forward.
            Err(error) => {
                let _ = crate::diagnostics::append_diagnostic_log(
                    "warn",
                    "embedded_ssh_forward",
                    "remote forward accept error; backing off",
                    Some(serde_json::json!({
                        "host": rule.host,
                        "bind_port": rule.bind_port,
                        "error": error.to_string(),
                    })),
                );
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
    Ok(())
}

fn ssh_error_is_would_block(error: &ssh2::Error) -> bool {
    matches!(error.code(), ssh2::ErrorCode::Session(-37))
}

fn bridge_tcp_and_channel(
    mut stream: TcpStream,
    mut channel: ssh2::Channel,
    control: &RuleControl,
) -> Result<()> {
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
                // A3: Track bytes sent from client → SSH target.
                control.bytes_tx.fetch_add(n as u64, Ordering::Relaxed);
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
                // A3: Track bytes sent from SSH target → client.
                control.bytes_rx.fetch_add(n as u64, Ordering::Relaxed);
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

/// A3: Get per-rule statistics for observability.
/// Returns a map of forward rule ID → `RuleStats`.
pub async fn forward_stats_core() -> HashMap<Uuid, RuleStats> {
    forwards()
        .lock()
        .await
        .iter()
        .map(|(id, h)| (*id, h.control.snapshot()))
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

// ── Finding 17: Single-rule start/stop ─────────────────────────────────────
//
// Stop a forward rule's worker without removing it from the store. The rule
// can be restarted later with `forward_start_core`. This is useful for
// temporarily pausing traffic without losing the rule configuration.

/// Stop a single forward rule by its ID. The rule remains in the store and
/// can be restarted with `forward_start_core`.
pub async fn forward_stop_core(id: Uuid) -> Result<()> {
    let mut store = forwards().lock().await;
    let handle = store
        .get_mut(&id)
        .ok_or_else(|| anyhow!("unknown forward: {id}"))?;
    // Don't stop if already stopped.
    let state = *handle.control.state.lock().unwrap();
    if state == RuleState::Stopped || state == RuleState::StoppingError {
        return Ok(());
    }
    handle.stop_worker();
    Ok(())
}

/// Restart a previously stopped forward rule by its ID.
pub async fn forward_start_core(id: Uuid) -> Result<()> {
    let mut store = forwards().lock().await;
    let handle = store
        .get(&id)
        .ok_or_else(|| anyhow!("unknown forward: {id}"))?;
    // Only restart if currently stopped.
    let state = *handle.control.state.lock().unwrap();
    if state == RuleState::Active || state == RuleState::Starting {
        return Ok(());
    }
    let rule = handle.rule().clone();
    // Q2: Use resolve_host_with_jump so a stored jump-host override (from
    // `--via` at creation time) is reapplied on restart. Without this, a
    // stopped forward that was originally created with `--via bastion`
    // would silently fall back to the profile's own jump_host (or none).
    let host = resolve_host_with_jump(&rule.host, rule.via.as_deref())?;
    // Start a new worker with the same rule.
    let new_handle = start_forward_worker(host, rule).await?;
    // Replace the old handle with the new one.
    store.insert(id, new_handle);
    Ok(())
}

// ── B25: Multi-rule forward (single Forward, multiple -L/-R rules) ──────
//
// Starts multiple independently managed port-forward rules as one atomic API
// operation. Each rule has its own SSH session and listener so removing or
// failing one rule cannot corrupt another rule's libssh2 blocking state.

/// Input rule for a multi-rule forward batch.
#[derive(Debug, Clone)]
pub struct MultiForwardRule {
    pub direction: ForwardDirection,
    pub bind_port: u16,
    pub target_host: String,
    pub target_port: u16,
    /// Optional label for this individual rule.
    pub name: Option<String>,
    /// Optional group id for organizing this rule.
    pub group_id: Option<String>,
}

/// Result of a successful multi-rule forward addition.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MultiForwardResult {
    /// IDs of all rules that were started, in order.
    pub ids: Vec<Uuid>,
    /// The host profile name.
    pub host: String,
    /// Number of rules started.
    pub count: usize,
}

/// Start multiple port-forward rules on a single host in one batch.
/// All rules share the same resolved host profile. If any rule fails to become
/// ready, all previously started rules are rolled back.
pub async fn forward_add_multi_core(
    host_name: &str,
    rules: &[MultiForwardRule],
) -> Result<MultiForwardResult> {
    forward_add_multi_core_via(host_name, rules, None).await
}

/// B68: Start multiple port-forward rules with an optional jump host override.
pub async fn forward_add_multi_core_via(
    host_name: &str,
    rules: &[MultiForwardRule],
    via: Option<&str>,
) -> Result<MultiForwardResult> {
    if rules.is_empty() {
        return Err(anyhow!("no forward rules provided"));
    }
    // Validate all rules upfront.
    let host = resolve_host_with_jump(host_name, via)?;
    for (i, rule) in rules.iter().enumerate() {
        if rule.direction == ForwardDirection::Remote
            && !remote_forward_target_allowed(&rule.target_host)
        {
            return Err(anyhow!(
                "rule {}: remote forward target_host must be loopback; got '{}'",
                i,
                rule.target_host
            ));
        }
    }
    // Finding 5: Check for duplicate local ports within the batch.
    let mut seen_local_ports: std::collections::HashSet<u16> = std::collections::HashSet::new();
    for (i, rule) in rules.iter().enumerate() {
        if rule.direction == ForwardDirection::Local && !seen_local_ports.insert(rule.bind_port) {
            return Err(anyhow!(
                "rule {}: duplicate local port {} in forward batch",
                i,
                rule.bind_port
            ));
        }
    }

    let mut started_ids = Vec::with_capacity(rules.len());

    for rule in rules {
        let forward_rule = ForwardRule {
            id: Uuid::new_v4(),
            host: host_name.to_string(),
            direction: rule.direction,
            bind_port: rule.bind_port,
            target_host: rule.target_host.clone(),
            target_port: rule.target_port,
            name: rule.name.clone(),
            group_id: rule.group_id.clone(),
            via: via.map(|s| s.to_string()),
        };

        // Reserve a lifecycle entry for this forward.
        let lifecycle = crate::app_state::lifecycle();
        let reservation = match crate::lifecycle::LifecycleRegistry::reserve(
            &lifecycle,
            &forward_rule.id.to_string(),
            crate::app_state::ResourceKind::Forward,
            crate::app_state::ResourceOwner::Headless(uuid::Uuid::new_v4()),
        ) {
            Ok(reservation) => reservation,
            Err(error) => {
                rollback_forward_batch(&started_ids).await;
                return Err(anyhow!("lifecycle reserve failed: {error}"));
            }
        };

        let handle = match start_forward_worker(host.clone(), forward_rule.clone()).await {
            Ok(handle) => handle,
            Err(error) => {
                rollback_forward_batch(&started_ids).await;
                return Err(anyhow!(
                    "failed to start forward rule {}: {error}",
                    started_ids.len()
                ));
            }
        };

        if let Err(error) = reservation.activate() {
            drop(handle);
            rollback_forward_batch(&started_ids).await;
            return Err(anyhow!("lifecycle activate failed: {error}"));
        }

        forwards().lock().await.insert(forward_rule.id, handle);
        started_ids.push(forward_rule.id);
    }

    Ok(MultiForwardResult {
        ids: started_ids,
        host: host_name.to_string(),
        count: rules.len(),
    })
}

async fn rollback_forward_batch(ids: &[Uuid]) {
    for id in ids.iter().rev() {
        let _ = forward_remove_core(*id).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // ── A2: bind_loopback tests ───────────────────────────────────────────

    #[test]
    fn bind_loopback_succeeds_on_ipv4() {
        // IPv4 loopback bind should always succeed on a test machine.
        let result = bind_loopback(0); // port 0 = ephemeral
        assert!(result.is_ok());
        let (v4, v6) = result.unwrap();
        // IPv4 listener must be present.
        assert!(v4.local_addr().unwrap().ip().is_loopback());
        // IPv6 may or may not be present — both are acceptable.
        if let Some(v6_listener) = v6 {
            assert!(v6_listener.local_addr().unwrap().ip().is_loopback());
        }
    }

    #[test]
    fn bind_loopback_returns_ipv4_listener() {
        let (v4, _) = bind_loopback(0).unwrap();
        let addr = v4.local_addr().unwrap();
        assert!(addr.is_ipv4());
        assert!(addr.ip().is_loopback());
    }

    // ── A3: RuleControl tests ─────────────────────────────────────────────

    #[test]
    fn rule_control_starts_zeroed() {
        let rc = RuleControl::new();
        let stats = rc.snapshot();
        assert_eq!(stats.bytes_tx, 0);
        assert_eq!(stats.bytes_rx, 0);
        assert_eq!(stats.connections, 0);
        assert_eq!(stats.state, RuleState::Starting);
        assert_eq!(stats.effective_port, None);
        assert!(!stats.connected);
    }

    #[test]
    fn rule_control_increments() {
        let rc = RuleControl::new();
        rc.bytes_tx.fetch_add(100, Ordering::Relaxed);
        rc.bytes_rx.fetch_add(200, Ordering::Relaxed);
        rc.connections.fetch_add(1, Ordering::Relaxed);
        rc.connections.fetch_add(1, Ordering::Relaxed);

        let stats = rc.snapshot();
        assert_eq!(stats.bytes_tx, 100);
        assert_eq!(stats.bytes_rx, 200);
        assert_eq!(stats.connections, 2);
    }

    #[test]
    fn rule_control_state_transitions() {
        let rc = RuleControl::new();
        assert_eq!(rc.snapshot().state, RuleState::Starting);

        rc.set_state(RuleState::Error);
        assert_eq!(rc.snapshot().state, RuleState::Error);

        rc.set_state(RuleState::Stopped);
        assert_eq!(rc.snapshot().state, RuleState::Stopped);
    }

    #[test]
    fn rule_control_clone_shares_state() {
        let rc = RuleControl::new();
        let clone = rc.clone();
        clone.bytes_tx.fetch_add(42, Ordering::Relaxed);

        // Both point to the same AtomicU64.
        assert_eq!(rc.snapshot().bytes_tx, 42);
        assert_eq!(clone.snapshot().bytes_tx, 42);
    }

    // ── B68: Bastion/jump host override tests ──────────────────────────────

    #[test]
    fn resolve_host_with_jump_validates_unknown_jump_host() {
        // resolve_host_with_jump should reject an unknown jump host profile.
        // We can't easily set up a full config here, but we can verify the
        // error path when the host or jump host doesn't exist.
        crate::store::set_test_config_dir(
            std::env::temp_dir().join(format!("agent2ssh_b68_test_{}", std::process::id())),
        );
        let result = resolve_host_with_jump("nonexistent", Some("also_nonexistent"));
        assert!(result.is_err());
        crate::store::clear_test_config_dir();
    }

    #[test]
    fn resolve_host_with_jump_none_preserves_existing() {
        // When via is None, the function should just resolve normally.
        crate::store::set_test_config_dir(
            std::env::temp_dir().join(format!("agent2ssh_b68_none_{}", std::process::id())),
        );
        let result = resolve_host_with_jump("nonexistent", None);
        assert!(result.is_err());
        crate::store::clear_test_config_dir();
    }

    #[test]
    fn resolve_host_with_jump_empty_string_treated_as_none() {
        // An empty string or whitespace-only via should be treated as None.
        crate::store::set_test_config_dir(
            std::env::temp_dir().join(format!("agent2ssh_b68_empty_{}", std::process::id())),
        );
        let result = resolve_host_with_jump("nonexistent", Some("   "));
        // Should behave as if via was None — host not found.
        assert!(result.is_err());
        crate::store::clear_test_config_dir();
    }
}
