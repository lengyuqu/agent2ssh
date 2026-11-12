#![cfg(feature = "daemon")]

//! End-to-end tests for stopping the real daemon: the graceful route, the
//! `stop_daemon_process` wait, and the admin-only rule on `POST /shutdown`.
//!
//! Like `daemon_local_exec_e2e.rs` this spawns the actual `agent2ssh-daemon`
//! binary. Unlike it, these tests call library functions (`daemon_control`,
//! `daemon_activity`) that find the daemon through the *process* environment —
//! the config directory and `AGENT2SSH_DAEMON_ADDR` — so each test is
//! `#[serial]` and restores both on drop.
//!
//! The test process deliberately never reaps the child before asserting. That is
//! the desktop app's situation: it spawns the daemon, drops the handle, and a
//! daemon that has exited is a zombie to it until the app itself ends.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agent2ssh::daemon_activity::{local_daemon_request, local_daemon_request_with_body};
#[cfg(not(target_os = "macos"))]
use agent2ssh::daemon_control::request_graceful_shutdown;
use agent2ssh::daemon_control::{process_is_alive, stop_daemon_process};
use agent2ssh::store::CONFIG_DIR_ENV;

mod common;

const ADDR_ENV: &str = "AGENT2SSH_DAEMON_ADDR";
const STARTUP_BUDGET: Duration = Duration::from_secs(30);

// macOS Unix sockets have a short pathname limit; these test config dirs need
// not embed the descriptive test name, which can exceed that limit.
fn fixture_root() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/tmp")
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::temp_dir()
    }
}

struct RunningDaemon {
    child: Child,
    dir: PathBuf,
    token: String,
}

impl RunningDaemon {
    fn start(label: &str, extra_files: &[(&str, &str)]) -> Self {
        Self::start_with_name(label, extra_files, false)
    }

    fn start_with_name(_label: &str, extra_files: &[(&str, &str)], renamed: bool) -> Self {
        let dir = fixture_root().join(format!(
            "a2s-{}-{:x}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock before the epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create the throwaway config dir");
        let token = format!("stop-e2e-token-{}", std::process::id());
        std::fs::write(dir.join("daemon.token"), &token).expect("seed daemon.token");
        for (name, body) in extra_files {
            std::fs::write(dir.join(name), body).expect("seed config file");
        }

        let addr = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
            listener.local_addr().expect("local addr").to_string()
        };
        // The library functions under test read these from this process.
        std::env::set_var(CONFIG_DIR_ENV, &dir);
        std::env::set_var(ADDR_ENV, &addr);

        let original = std::path::Path::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"));
        let binary = if renamed {
            let copy = dir.join(format!("renamed-daemon{}", std::env::consts::EXE_SUFFIX));
            std::fs::copy(original, &copy).expect("copy daemon under a different name");
            copy
        } else {
            original.to_path_buf()
        };
        let child = Self::spawn(&binary, &dir);
        let mut daemon = Self { child, dir, token };
        for attempt in 0..5 {
            match daemon.wait_until_serving() {
                Ok(()) => return daemon,
                Err((true, detail)) if attempt < 4 => {
                    eprintln!("retrying daemon after port collision: {detail}");
                    let listener =
                        TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
                    let addr = listener.local_addr().unwrap().to_string();
                    drop(listener);
                    std::env::set_var(ADDR_ENV, addr);
                    daemon.child = Self::spawn(&binary, &daemon.dir);
                }
                Err((_, detail)) => panic!("{detail}"),
            }
        }
        unreachable!()
    }

    fn spawn(binary: &std::path::Path, dir: &std::path::Path) -> Child {
        let stderr =
            std::fs::File::create(dir.join("daemon.stderr")).expect("create daemon stderr");
        let mut command = Command::new(binary);
        // These existing tests exercise the explicitly enabled legacy route;
        // daemon_macos_conservative_e2e separately checks the default refusal.
        #[cfg(target_os = "macos")]
        command.env("AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN", "1");
        command
            .env(CONFIG_DIR_ENV, dir)
            .env(ADDR_ENV, std::env::var(ADDR_ENV).expect("daemon address"))
            .env("AGENT2SSH_LOG", "warn")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn agent2ssh-daemon")
    }

    fn wait_until_serving(&mut self) -> Result<(), (bool, String)> {
        let deadline = Instant::now() + STARTUP_BUDGET;
        while Instant::now() < deadline {
            if let Ok(response) =
                local_daemon_request("GET", "/health", None, Duration::from_millis(500))
            {
                if response.status == 200 {
                    if let Ok(body) = serde_json::from_str::<serde_json::Value>(&response.body) {
                        if body["pid"].as_u64() == Some(u64::from(self.child.id())) {
                            return Ok(());
                        }
                    }
                }
            }
            if let Some(status) = self.child.try_wait().expect("check daemon child") {
                let stderr =
                    std::fs::read_to_string(self.dir.join("daemon.stderr")).unwrap_or_default();
                let collision = ["os error 48", "os error 98", "os error 10048"]
                    .iter()
                    .any(|code| stderr.contains(code));
                return Err((
                    collision,
                    format!("daemon exited before health check: {status}; stderr: {stderr}"),
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err((
            false,
            format!(
                "daemon did not answer /health within {STARTUP_BUDGET:?}; child: {:?}; stderr: {}",
                self.child.try_wait(),
                std::fs::read_to_string(self.dir.join("daemon.stderr")).unwrap_or_default()
            ),
        ))
    }

    fn pid_file(&self) -> PathBuf {
        self.dir.join("daemon.pid")
    }
}

/// Exercise daemon draining with an explicit compatibility request on macOS;
/// this helper is not an implementation of the production stop contract.
fn request_drain_for_test(pid: u32) -> Result<(), String> {
    #[cfg(not(target_os = "macos"))]
    {
        stop_daemon_process(pid).map_err(|error| error.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        agent2ssh::daemon_control::request_unbound_graceful_shutdown(pid)
            .map_err(|error| error.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if !process_is_alive(pid) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err("daemon did not exit after explicit test request".into())
    }
}

impl Drop for RunningDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
        common::restore_default_config_env();
        std::env::remove_var(ADDR_ENV);
    }
}

/// Kills and reaps a child when the test leaves the scope, so a failed assertion
/// cannot leave a daemon holding a port for the rest of the run.
struct ChildGuard<'a>(&'a mut Child);

impl Drop for ChildGuard<'_> {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl ChildGuard<'_> {
    /// Stop and reap the child now, for a path that cannot use the daemon's own
    /// shutdown route.
    fn drop_child(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// An ephemeral loopback address nothing is listening on yet.
fn free_address() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    listener.local_addr().expect("local addr").to_string()
}

fn wait_until(what: &str, within: Duration, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if done() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

/// Stopping returns only once the daemon is gone, and the daemon got to clean up
/// after itself: the pid file is removed by the daemon's own drain, not by the
/// caller, so its absence is the evidence that the stop was graceful rather than
/// a kill.
#[test]
#[serial_test::serial]
fn stop_daemon_process_waits_for_a_graceful_exit() {
    let mut daemon = RunningDaemon::start("graceful", &[]);
    let pid = daemon.child.id();
    assert!(daemon.pid_file().exists(), "the daemon writes its pid file");
    assert!(process_is_alive(pid));

    let started = Instant::now();
    request_drain_for_test(pid).expect("stop the daemon");

    assert!(
        started.elapsed() < Duration::from_secs(7),
        "a healthy daemon should drain well inside the grace period, took {:?}",
        started.elapsed()
    );
    assert!(
        !process_is_alive(pid),
        "an exited-but-unreaped daemon must not be reported alive"
    );
    assert!(
        !daemon.pid_file().exists(),
        "the daemon's own shutdown removes its pid file; a kill would have left it"
    );
    // Query the retained Child only after the stop returns: doing so earlier
    // could hide failures in stop's handling of an unreaped Windows process.
    let status = daemon
        .child
        .try_wait()
        .expect("inspect the retained daemon child")
        .expect("stop returned while the daemon child was still running");
    assert!(
        status.success(),
        "daemon did not exit successfully: {status}"
    );
}

#[test]
#[serial_test::serial]
fn stop_closes_an_active_event_stream_and_exits() {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let daemon = RunningDaemon::start("stop-with-events", &[]);
    let pid = daemon.child.id();
    let mut stream = TcpStream::connect(std::env::var(ADDR_ENV).unwrap()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET /events/stream HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\n\r\n",
        daemon.token
    )
    .unwrap();
    let mut received = Vec::new();
    let mut chunk = [0u8; 1024];
    while !received
        .windows(b"stream_connected".len())
        .any(|w| w == b"stream_connected")
    {
        let n = stream.read(&mut chunk).expect("SSE initial event");
        assert!(n > 0, "stream closed before initial event");
        received.extend_from_slice(&chunk[..n]);
        assert!(received.len() < 65536, "initial SSE response too large");
    }
    assert!(received.starts_with(b"HTTP/1.1 200"));
    let started = Instant::now();
    request_drain_for_test(pid).expect("stop with an active SSE subscriber");
    assert!(started.elapsed() < Duration::from_secs(8));
    assert!(!process_is_alive(pid));
    assert!(!daemon.pid_file().exists());
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
            Err(error) => panic!("SSE stream did not close: {error}"),
        }
    }
    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut restarted = ChildGuard(RunningDaemon::spawn(
        std::path::Path::new(env!("CARGO_BIN_EXE_agent2ssh-daemon")),
        &daemon.dir,
    ));
    let restarted_pid = restarted.0.id();
    wait_until("restarted daemon", STARTUP_BUDGET, || {
        agent2ssh::daemon_control::daemon_health_pid() == Some(restarted_pid)
            || restarted.0.try_wait().unwrap().is_some()
    });
    assert!(
        restarted.0.try_wait().unwrap().is_none(),
        "restart must remain alive"
    );
    request_drain_for_test(restarted_pid).expect("stop restarted daemon");
}

#[test]
#[serial_test::serial]
fn stop_exits_with_an_idle_exec_websocket() {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let daemon = RunningDaemon::start("stop-with-idle-ws", &[]);
    let mut stream = TcpStream::connect(std::env::var(ADDR_ENV).unwrap()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(stream, "GET /exec/stream HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n", daemon.token).unwrap();
    let mut response = Vec::new();
    let mut chunk = [0u8; 1024];
    while !response.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut chunk).unwrap();
        assert!(n > 0);
        response.extend_from_slice(&chunk[..n]);
        assert!(response.len() < 8192);
    }
    assert!(
        response.starts_with(b"HTTP/1.1 101"),
        "{}",
        String::from_utf8_lossy(&response)
    );
    // Deliberately keep the upgraded socket open without sending a request.
    request_drain_for_test(daemon.child.id()).expect("stop with idle WebSocket");
    assert!(!daemon.pid_file().exists());
}

// Real network backpressure: opt in with the existing isolated SSH fixture.
#[cfg(unix)]
#[test]
#[serial_test::serial]
fn stop_exits_when_exec_websocket_peer_stops_reading() {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::os::fd::AsRawFd;
    let Some(port) = std::env::var("AGENT2SSH_TEST_SSH_PORT").ok() else {
        assert_ne!(
            std::env::var("AGENT2SSH_TEST_REQUIRE_SSH").as_deref(),
            Ok("1"),
            "SSH fixture required"
        );
        eprintln!("skipping: SSH fixture not configured");
        return;
    };
    let port: u16 = port.parse().unwrap();
    let mut host = serde_json::json!({"name":"fixture", "host":std::env::var("AGENT2SSH_TEST_SSH_HOST").unwrap_or_else(|_| "127.0.0.1".into()), "port":port, "user":std::env::var("AGENT2SSH_TEST_SSH_USER").unwrap_or_else(|_| "root".into())});
    if let Ok(key) = std::env::var("AGENT2SSH_TEST_SSH_KEY") {
        host["key_path"] = serde_json::json!(key);
    } else {
        host["password"] =
            serde_json::json!(std::env::var("AGENT2SSH_TEST_SSH_PASSWORD")
                .unwrap_or_else(|_| "a2s-fixture".into()));
    }
    let hosts = serde_json::json!({"schema_version":1,"hosts":[host]}).to_string();
    let daemon = RunningDaemon::start("stop-ws-backpressure", &[("hosts.json", &hosts)]);
    let mut stream = TcpStream::connect(std::env::var(ADDR_ENV).unwrap()).unwrap();
    let size: libc::c_int = 4096;
    let result = unsafe {
        libc::setsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            &size as *const _ as *const _,
            std::mem::size_of_val(&size) as libc::socklen_t,
        )
    };
    assert_eq!(result, 0, "set receive buffer");
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    write!(stream, "GET /exec/stream HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n", daemon.token).unwrap();
    let mut response = Vec::new();
    let mut byte = [0u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        response.push(byte[0]);
        assert!(response.len() < 8192);
    }
    assert!(response.starts_with(b"HTTP/1.1 101"));
    let message = br#"{"host":"fixture","command":"yes aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","force":true,"timeout_secs":120}"#;
    let mask = [1u8, 2, 3, 4];
    let mut frame = vec![0x81, 0x80 | 126];
    frame.extend_from_slice(&(message.len() as u16).to_be_bytes());
    frame.extend_from_slice(&mask);
    frame.extend(message.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    stream.write_all(&frame).unwrap();
    let mut header = [0u8; 2];
    stream
        .read_exact(&mut header)
        .expect("first WebSocket frame");
    let length = match header[1] & 0x7f {
        126 => {
            let mut b = [0; 2];
            stream.read_exact(&mut b).unwrap();
            u16::from_be_bytes(b) as usize
        }
        127 => {
            let mut b = [0; 8];
            stream.read_exact(&mut b).unwrap();
            usize::try_from(u64::from_be_bytes(b)).unwrap()
        }
        n => n as usize,
    };
    assert!(length < 1_000_000);
    let mut payload = vec![0u8; length];
    stream.read_exact(&mut payload).unwrap();
    let event: serde_json::Value = serde_json::from_slice(&payload).expect("stream event JSON");
    assert_eq!(event["type"], "stdout", "SSH stream did not start: {event}");
    // The peer stops reading while the remote command floods output.
    std::thread::sleep(Duration::from_secs(2));
    let started = Instant::now();
    request_drain_for_test(daemon.child.id()).expect("stop with backpressured WebSocket");
    assert!(started.elapsed() < Duration::from_secs(7));
    assert!(!daemon.pid_file().exists());
    let diagnostics = std::fs::read_to_string(daemon.dir.join("app.log")).unwrap();
    assert!(
        diagnostics.contains("WebSocket drain deadline expired"),
        "the stalled send did not reach the deadline"
    );
    let audit = std::fs::read_to_string(daemon.dir.join("audit.jsonl")).unwrap();
    assert!(audit.contains("daemon shutdown interrupted stream; remote outcome unknown"));
}

#[test]
#[serial_test::serial]
fn shutdown_interrupts_an_active_ssh_request() {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let Some(port) = std::env::var("AGENT2SSH_TEST_SSH_PORT").ok() else {
        assert_ne!(
            std::env::var("AGENT2SSH_TEST_REQUIRE_SSH").as_deref(),
            Ok("1"),
            "SSH fixture required"
        );
        eprintln!("skipping: SSH fixture not configured");
        return;
    };
    let mut host = serde_json::json!({"name":"fixture", "host":std::env::var("AGENT2SSH_TEST_SSH_HOST").unwrap_or_else(|_| "127.0.0.1".into()), "port":port.parse::<u16>().unwrap(), "user":std::env::var("AGENT2SSH_TEST_SSH_USER").unwrap_or_else(|_| "root".into())});
    if let Ok(key) = std::env::var("AGENT2SSH_TEST_SSH_KEY") {
        host["key_path"] = serde_json::json!(key);
    } else {
        host["password"] =
            serde_json::json!(std::env::var("AGENT2SSH_TEST_SSH_PASSWORD")
                .unwrap_or_else(|_| "a2s-fixture".into()));
    }
    let hosts = serde_json::json!({"schema_version":1,"hosts":[host]}).to_string();
    let daemon = RunningDaemon::start("stop-ssh-exec", &[("hosts.json", &hosts)]);
    let marker = format!("/tmp/a2s-stop-{}", uuid::Uuid::new_v4());
    let command = format!("printf ready > {marker}; sleep 30");
    let body =
        serde_json::json!({"host":"fixture","command":command,"force":true,"timeout_secs":60,"reason":"test change","side_effect":"test effect","change_id":"test-42"})
            .to_string();
    let mut request = TcpStream::connect(std::env::var(ADDR_ENV).unwrap()).unwrap();
    request
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(request, "POST /exec HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", daemon.token, body.len(), body).unwrap();
    let probe = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let url = format!("http://{}/exec", std::env::var(ADDR_ENV).unwrap());
    wait_until("remote command marker", Duration::from_secs(20), || {
        probe.post(&url).bearer_auth(&daemon.token)
            .json(&serde_json::json!({"host":"fixture","command":format!("test -f {marker}"),"force":true,"timeout_secs":3}))
            .send().ok().and_then(|response| response.json::<serde_json::Value>().ok())
            .is_some_and(|body| body["exit_code"] == 0)
    });
    request_drain_for_test(daemon.child.id()).expect("stop while SSH command is running");
    let mut response = String::new();
    request.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 503"), "{response}");
    assert!(
        response.contains("remote command outcome unknown"),
        "{response}"
    );
    let audit = std::fs::read_to_string(daemon.dir.join("audit.jsonl")).unwrap();
    let interrupted = audit
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|entry| entry["change_id"] == "test-42")
        .expect("interrupted command audit");
    assert!(interrupted["reason"]
        .as_str()
        .unwrap()
        .contains("remote command outcome unknown"));
    assert!(interrupted["reason"]
        .as_str()
        .unwrap()
        .contains("test change"));
    assert_eq!(interrupted["side_effect"], "test effect");
    assert!(!daemon.pid_file().exists());
}

#[test]
#[serial_test::serial]
fn shutdown_cancels_a_request_waiting_for_approval() {
    use std::io::Write;
    use std::net::TcpStream;
    let policy = "[[policies]]\nname = 'test'\nhosts = ['local']\nrequires_approval = true\nttl_secs = 120\n";
    let daemon = RunningDaemon::start(
        "stop-pending-approval",
        &[("approval_policies.toml", policy)],
    );
    let mut request = TcpStream::connect(std::env::var(ADDR_ENV).unwrap()).unwrap();
    let body = r#"{"host":"local","command":"echo approval-pending-marker","force":false}"#;
    write!(request, "POST /local-exec HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", daemon.token, body.len(), body).unwrap();
    wait_until("pending approval", Duration::from_secs(10), || {
        local_daemon_request(
            "GET",
            "/approvals",
            Some(&daemon.token),
            Duration::from_secs(1),
        )
        .ok()
        .and_then(|response| serde_json::from_str::<serde_json::Value>(&response.body).ok())
        .and_then(|items| {
            items
                .as_array()
                .map(|items| items.iter().any(|item| item["status"] == "pending"))
        })
        .unwrap_or(false)
    });
    request_drain_for_test(daemon.child.id()).expect("stop while approval is pending");
    assert!(!daemon.pid_file().exists());
    let records = std::fs::read_to_string(daemon.dir.join("approvals.jsonl")).unwrap();
    let final_record = records
        .lines()
        .last()
        .and_then(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .unwrap();
    assert_eq!(final_record["status"], "rejected");
    let audit = std::fs::read_to_string(daemon.dir.join("audit.jsonl")).unwrap();
    assert!(audit.contains("daemon shutting down; approval wait cancelled"));
    // The audit line proves the execution was refused; this proves the daemon
    // said so in its own diagnostics too, which is what the module comment on
    // `daemon_activity` promises. Nothing else pinned that entry.
    let diagnostics = std::fs::read_to_string(daemon.dir.join("app.log")).unwrap();
    assert!(
        diagnostics.contains("approval wait cancelled during shutdown"),
        "the cancelled approval left no diagnostics entry: {diagnostics}"
    );
}

#[cfg(unix)]
#[test]
#[serial_test::serial]
fn shutdown_reaps_a_running_local_command_and_its_child() {
    use std::io::Write;
    use std::net::TcpStream;
    let daemon = RunningDaemon::start("stop-local-command", &[]);
    let marker = daemon.dir.join("running-pids");
    let command = format!(
        "sleep 60 & child=$!; printf '%s %s' $$ \"$child\" > '{}'; wait \"$child\"",
        marker.display()
    );
    let body =
        serde_json::json!({"host":"local", "command":command, "force":true, "timeout_secs":120})
            .to_string();
    let mut request = TcpStream::connect(std::env::var(ADDR_ENV).unwrap()).unwrap();
    write!(request, "POST /local-exec HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", daemon.token, body.len(), body).unwrap();
    wait_until("local process started", Duration::from_secs(10), || {
        marker.exists()
    });
    let pids = std::fs::read_to_string(marker)
        .unwrap()
        .split_whitespace()
        .map(|value| value.parse::<i32>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(pids.len(), 2);
    let stopped = request_drain_for_test(daemon.child.id());
    if stopped.is_err() {
        // Only the process group spawned by this test is targeted.
        unsafe {
            libc::kill(-pids[0], libc::SIGKILL);
        }
    }
    stopped.expect("stop during a local command");
    let alive = |pid: i32| {
        let mut system = sysinfo::System::new();
        let sys_pid = sysinfo::Pid::from_u32(pid as u32);
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sys_pid]), true);
        system
            .process(sys_pid)
            .is_some_and(|p| p.status() != sysinfo::ProcessStatus::Zombie)
    };
    wait_until("local process tree to exit", Duration::from_secs(5), || {
        pids.iter().all(|pid| !alive(*pid))
    });
    let audit = std::fs::read_to_string(daemon.dir.join("audit.jsonl")).unwrap();
    assert!(audit.contains("local command stopped during daemon shutdown"));
    assert!(!daemon.pid_file().exists());
}

#[test]
#[serial_test::serial]
fn exiting_old_daemon_preserves_replacement_pid_in_the_same_config() {
    let old = RunningDaemon::start("pid-replacement", &[]);
    let old_pid = old.child.id();
    let old_addr = std::env::var(ADDR_ENV).unwrap();
    let mut replacement = RunningDaemon::start("pid-replacement-other", &[]);
    let replacement_pid = replacement.child.id();
    // Simulate a legacy or external writer which does not follow the lock
    // protocol. A new daemon no longer overwrites an existing live PID.
    std::env::set_var(CONFIG_DIR_ENV, &old.dir);
    std::fs::write(old.pid_file(), replacement_pid.to_string()).unwrap();
    agent2ssh::daemon_control::mark_daemon_owned(replacement_pid).expect("mark replacement owned");
    std::env::set_var(ADDR_ENV, old_addr);
    #[cfg(not(target_os = "macos"))]
    request_drain_for_test(old_pid).expect("stop old daemon");
    #[cfg(target_os = "macos")]
    {
        // The record was deliberately replaced, so the unbound client must
        // refuse. Stop this test-owned child out of band to exercise its own
        // conditional cleanup of the replacement's record.
        assert!(agent2ssh::daemon_control::request_unbound_graceful_shutdown(old_pid).is_err());
        assert_eq!(
            unsafe { libc::kill(old_pid as libc::pid_t, libc::SIGTERM) },
            0
        );
        wait_until("old test daemon exit", Duration::from_secs(10), || {
            !process_is_alive(old_pid)
        });
    }
    #[cfg(not(target_os = "macos"))]
    assert!(!agent2ssh::daemon_control::remove_daemon_pid_file_if(old_pid).unwrap());
    #[cfg(target_os = "macos")]
    assert!(agent2ssh::daemon_control::remove_daemon_pid_file_if(old_pid).is_err());
    assert_eq!(
        std::fs::read_to_string(old.pid_file()).unwrap(),
        replacement_pid.to_string()
    );
    assert!(replacement.child.try_wait().unwrap().is_none());
    assert!(
        agent2ssh::daemon_control::daemon_is_owned(replacement_pid),
        "stopping the old daemon must preserve the replacement's owner marker"
    );
}

#[test]
#[serial_test::serial]
fn direct_daemon_with_another_port_cannot_replace_a_live_pid() {
    let mut old = RunningDaemon::start("same-config-live", &[]);
    let old_pid = old.child.id();
    let old_addr = std::env::var(ADDR_ENV).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let replacement_addr = listener.local_addr().unwrap().to_string();
    drop(listener);
    std::env::set_var(ADDR_ENV, replacement_addr);
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"));
    let mut replacement = RunningDaemon::spawn(binary, &old.dir);
    wait_until("competing daemon exit", STARTUP_BUDGET, || {
        replacement.try_wait().unwrap().is_some()
    });
    let status = replacement.wait().unwrap();
    let stderr = std::fs::read_to_string(old.dir.join("daemon.stderr")).unwrap();
    std::env::set_var(ADDR_ENV, old_addr);
    assert!(!status.success());
    assert!(
        stderr.contains(if cfg!(target_os = "macos") {
            "already exists; retained"
        } else {
            "live or cannot be verified"
        }),
        "{stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(old.pid_file()).unwrap(),
        old_pid.to_string()
    );
    assert!(old.child.try_wait().unwrap().is_none());
}

#[cfg_attr(
    target_os = "macos",
    ignore = "legacy automatic stale-record replacement; macOS conservative replacement is covered by daemon_macos_conservative_e2e"
)]
#[test]
#[serial_test::serial]
fn direct_daemon_recovers_a_definitely_absent_pid() {
    let daemon = RunningDaemon::start("direct-stale", &[("daemon.pid", "4294967294")]);
    assert_eq!(
        std::fs::read_to_string(daemon.pid_file()).unwrap(),
        daemon.child.id().to_string()
    );
    assert!(process_is_alive(daemon.child.id()));
}

// A stale PID can point to any process. Mentioning the daemon in an argument
// must not make that process a candidate for the stop path.
#[cfg(unix)]
#[test]
#[serial_test::serial]
fn stop_does_not_signal_a_process_with_daemon_name_in_an_argument() {
    let mut child = Command::new("sh")
        .args(["-c", "read ignored", "agent2ssh-daemon"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn unrelated shell");
    let pid = child.id();
    assert!(
        !process_is_alive(pid),
        "an argument is not the executable name"
    );
    let error = stop_daemon_process(pid).expect_err("unrelated process is not safely identified");
    assert!(error.to_string().contains(if cfg!(target_os = "macos") {
        "not bound"
    } else {
        "cannot be safely identified"
    }));
    let still_running = child.try_wait().expect("check unrelated process").is_none();
    let _ = child.kill();
    let _ = child.wait();
    assert!(still_running, "stop must not signal the unrelated process");
}

#[cfg(unix)]
#[test]
#[serial_test::serial]
fn stop_refuses_a_daemon_named_process_without_authenticated_health() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("agent2ssh-daemon");
    symlink("/bin/sleep", &executable).unwrap();
    let mut child = Command::new(executable).arg("30").spawn().unwrap();
    let pid = child.id();
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    std::env::set_var(CONFIG_DIR_ENV, dir.path());
    std::env::set_var(ADDR_ENV, silent.local_addr().unwrap().to_string());
    std::fs::write(dir.path().join("daemon.pid"), pid.to_string()).unwrap();
    let result = stop_daemon_process(pid);
    let still_running = child.try_wait().unwrap().is_none();
    let pid_retained = dir.path().join("daemon.pid").exists();
    let _ = child.kill();
    let _ = child.wait();
    common::restore_default_config_env();
    std::env::remove_var(ADDR_ENV);
    assert!(result
        .unwrap_err()
        .to_string()
        .contains(if cfg!(target_os = "macos") {
            "not bound"
        } else {
            "could not be authenticated"
        }));
    assert!(still_running && pid_retained);
}

#[test]
#[serial_test::serial]
fn renamed_daemon_is_reported_healthy_but_not_unsafely_stopped() {
    use agent2ssh::daemon_control::{daemon_pid_state, DaemonPidState};
    let daemon = RunningDaemon::start_with_name("renamed", &[], true);
    let pid = daemon.child.id();
    assert!(!process_is_alive(pid));
    assert_eq!(daemon_pid_state(pid), DaemonPidState::Healthy);
    // K14 says the diagnostic report contradicts itself in one JSON: the
    // process-name gate answers "not a daemon of ours" while the config/token
    // identity answers "healthy". Neither half is covered anywhere else, so pin
    // both here — a change to either judgement should have to come here and
    // decide which one is wrong, not drift apart silently.
    let report = agent2ssh::diagnostics::generate_system_report().expect("system report");
    assert_eq!(report["daemon"]["process_alive"], serde_json::json!(false));
    assert_eq!(report["daemon"]["health_ok"], serde_json::json!(true));
    assert_eq!(report["daemon"]["pid"], serde_json::json!(pid));
    #[cfg(feature = "tauri")]
    {
        let status = agent2ssh::tauri_commands::daemon_status().expect("desktop status");
        #[cfg(not(target_os = "macos"))]
        assert!(status.running);
        #[cfg(target_os = "macos")]
        assert!(!status.running);
        assert_eq!(status.pid, Some(pid));
        assert!(agent2ssh::tauri_commands::daemon_stop().is_err());
        assert_eq!(
            std::fs::read_to_string(daemon.pid_file()).unwrap(),
            pid.to_string()
        );
    }
    let cli = |action| {
        Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
            .args(["daemon", action])
            .env(CONFIG_DIR_ENV, &daemon.dir)
            .output()
            .expect("run CLI")
    };
    let status = cli("status");
    assert!(status.status.success());
    // macOS status does not disclose a token to an unbound endpoint, so the
    // same self-reported health can only be reported as unverified.
    assert!(
        String::from_utf8_lossy(&status.stdout).contains(if cfg!(target_os = "macos") {
            "unverified"
        } else {
            "healthy"
        })
    );
    let start = cli("start");
    assert!(start.status.success());
    assert!(
        String::from_utf8_lossy(&start.stdout).contains(if cfg!(target_os = "macos") {
            "identity is unbound"
        } else {
            "already running"
        })
    );
    let stop = cli("stop");
    assert!(!stop.status.success());
    assert!(
        String::from_utf8_lossy(&stop.stderr).contains(if cfg!(target_os = "macos") {
            "not bound"
        } else {
            "cannot be safely identified"
        })
    );
    assert_eq!(
        std::fs::read_to_string(daemon.pid_file()).unwrap(),
        pid.to_string()
    );
    assert_eq!(daemon_pid_state(pid), DaemonPidState::Healthy);

    // A matching PID alone is not enough if this config has a different token.
    std::fs::write(daemon.dir.join("daemon.token"), "wrong-token").unwrap();
    assert_eq!(daemon_pid_state(pid), DaemonPidState::Unverified);
    #[cfg(feature = "tauri")]
    assert!(!agent2ssh::tauri_commands::daemon_status().unwrap().running);
    assert!(!cli("start").status.success());
    assert!(!cli("stop").status.success());
    assert_eq!(
        std::fs::read_to_string(daemon.pid_file()).unwrap(),
        pid.to_string()
    );
    std::fs::write(daemon.dir.join("daemon.token"), &daemon.token).unwrap();
    assert_eq!(daemon_pid_state(pid), DaemonPidState::Healthy);

    // A live renamed daemon with an unavailable health endpoint is uncertain,
    // not stale. No desktop or CLI path may silently discard its PID.
    let previous_addr = std::env::var(ADDR_ENV).unwrap();
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    std::env::set_var(ADDR_ENV, silent.local_addr().unwrap().to_string());
    assert_eq!(daemon_pid_state(pid), DaemonPidState::Unverified);
    #[cfg(feature = "tauri")]
    {
        let status = agent2ssh::tauri_commands::daemon_status().expect("desktop status");
        assert!(!status.running);
        assert!(status.message.contains("cannot be verified"));
        assert!(agent2ssh::tauri_commands::daemon_stop().is_err());
    }
    assert!(!cli("start").status.success());
    assert!(!cli("stop").status.success());
    assert_eq!(
        std::fs::read_to_string(daemon.pid_file()).unwrap(),
        pid.to_string()
    );
    std::env::set_var(ADDR_ENV, previous_addr);
}

#[test]
#[serial_test::serial]
fn wrong_admin_token_never_signals_the_real_daemon() {
    let mut daemon = RunningDaemon::start("wrong-token", &[]);
    let pid = daemon.child.id();
    std::fs::write(daemon.dir.join("daemon.token"), "wrong-token").unwrap();
    assert_eq!(
        agent2ssh::daemon_control::daemon_pid_state(pid),
        agent2ssh::daemon_control::DaemonPidState::Unverified
    );
    let result = stop_daemon_process(pid);
    let alive = daemon.child.try_wait().unwrap().is_none();
    let pid_retained = daemon.pid_file().exists();
    let message = result.unwrap_err().to_string();
    // Either refusal is correct and both are inert; which one fires depends on
    // whether the stop even gets to send the token. What must never happen is
    // the daemon going away, or the reason being anything else.
    assert!(
        message.contains("did not accept this config's admin token")
            || message.contains("could not be authenticated for shutdown")
            || (cfg!(target_os = "macos") && message.contains("not bound")),
        "{message}"
    );
    assert!(alive && pid_retained);
}

#[test]
#[serial_test::serial]
fn start_refuses_to_claim_an_existing_health_endpoint_without_a_pid_file() {
    use agent2ssh::daemon_control::start_daemon_background;
    let daemon = RunningDaemon::start("existing-health", &[]);
    std::fs::remove_file(daemon.pid_file()).unwrap();
    let cli = |action| {
        Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
            .args(["daemon", action])
            .env(CONFIG_DIR_ENV, &daemon.dir)
            .output()
            .unwrap()
    };
    let status = cli("status");
    assert!(String::from_utf8_lossy(&status.stdout).contains("no PID record"));
    assert!(!cli("stop").status.success());
    #[cfg(feature = "tauri")]
    {
        let status = agent2ssh::tauri_commands::daemon_status().unwrap();
        assert!(!status.running && status.message.contains("no PID record"));
        assert!(agent2ssh::tauri_commands::daemon_stop().is_err());
    }
    let error =
        start_daemon_background(std::path::Path::new(env!("CARGO_BIN_EXE_agent2ssh-daemon")))
            .expect_err("must not start over an existing endpoint");
    assert!(
        error.to_string().contains("already served by pid"),
        "{error}"
    );
    assert!(process_is_alive(daemon.child.id()));
}

#[test]
#[serial_test::serial]
fn competing_daemon_cannot_overwrite_the_running_daemon_pid() {
    let daemon = RunningDaemon::start("competing", &[]);
    let pid = daemon.child.id();
    let output = Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))
        .env(CONFIG_DIR_ENV, &daemon.dir)
        .env(ADDR_ENV, std::env::var(ADDR_ENV).unwrap())
        .output()
        .expect("launch competing daemon");
    assert!(
        !output.status.success(),
        "competing daemon must fail to bind"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        ["os error 48", "os error 98", "os error 10048"]
            .iter()
            .any(|code| stderr.contains(code)),
        "expected an address-in-use error, got: {stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(daemon.pid_file()).unwrap(),
        pid.to_string()
    );
    assert!(process_is_alive(pid));
}

#[cfg(unix)]
#[test]
#[serial_test::serial]
fn recognized_process_with_a_different_health_pid_is_unverified() {
    use agent2ssh::daemon_control::{daemon_pid_state, DaemonPidState};
    use std::os::unix::fs::symlink;
    let daemon = RunningDaemon::start("different-health-pid", &[]);
    let executable = daemon.dir.join("agent2ssh-daemon");
    symlink("/bin/sleep", &executable).unwrap();
    let mut child = Command::new(executable).arg("30").spawn().unwrap();
    let pid = child.id();
    let recognized = process_is_alive(pid);
    let state = daemon_pid_state(pid);
    let result = stop_daemon_process(pid);
    let still_running = child.try_wait().unwrap().is_none();
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        recognized,
        "test process must match the daemon executable name"
    );
    assert_eq!(state, DaemonPidState::Unverified);
    assert!(result
        .unwrap_err()
        .to_string()
        .contains(if cfg!(target_os = "macos") {
            "not bound"
        } else {
            "could not be authenticated"
        }));
    assert!(still_running);
    assert!(process_is_alive(daemon.child.id()));
}

#[test]
#[serial_test::serial]
fn other_daemons_health_is_not_reported_as_our_pid() {
    use agent2ssh::daemon_control::{daemon_pid_state, DaemonPidState};
    let daemon = RunningDaemon::start("health-mismatch", &[]);
    let actual_pid = daemon.child.id();
    let other_pid = u32::MAX - 1;
    std::fs::write(daemon.pid_file(), other_pid.to_string()).unwrap();
    assert_eq!(daemon_pid_state(other_pid), DaemonPidState::Unverified);
    let report = agent2ssh::diagnostics::generate_system_report().expect("diagnostics");
    assert_eq!(report["daemon"]["health_ok"], false);
    assert_eq!(report["daemon"]["health_endpoint_pid"], actual_pid);
    #[cfg(feature = "tauri")]
    {
        let status = agent2ssh::tauri_commands::daemon_status().unwrap();
        assert!(!status.running);
        assert!(status.message.contains("cannot be verified"));
    }
    assert_eq!(
        std::fs::read_to_string(daemon.pid_file()).unwrap(),
        other_pid.to_string()
    );
    std::fs::write(daemon.pid_file(), actual_pid.to_string()).unwrap();
}

#[test]
#[serial_test::serial]
fn a_forged_endpoint_cannot_report_an_absent_pid_as_running() {
    use agent2ssh::daemon_control::{daemon_matches_config, daemon_pid_state, DaemonPidState};
    use std::io::{Read, Write};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    let dir = tempfile::tempdir().unwrap();
    let pid = u32::MAX - 1;
    std::fs::write(dir.path().join("daemon.pid"), pid.to_string()).unwrap();
    std::fs::write(dir.path().join("daemon.token"), "test-only-token").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    std::env::set_var(CONFIG_DIR_ENV, dir.path());
    std::env::set_var(ADDR_ENV, listener.local_addr().unwrap().to_string());
    let done = Arc::new(AtomicBool::new(false));
    let stopping = done.clone();
    let server = std::thread::spawn(move || {
        while !stopping.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut socket, _)) => {
                    socket
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut request = [0; 4096];
                    let _ = socket.read(&mut request);
                    let body = format!(r#"{{"ok":true,"pid":{pid}}}"#);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(response.as_bytes());
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(e) => panic!("accept fake daemon: {e}"),
            }
        }
    });
    let state = daemon_pid_state(pid);
    let matches = daemon_matches_config(pid);
    #[cfg(feature = "tauri")]
    let desktop_status = agent2ssh::tauri_commands::daemon_status().unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
        .env(CONFIG_DIR_ENV, dir.path())
        .env(ADDR_ENV, std::env::var(ADDR_ENV).unwrap())
        .args(["daemon", "status"])
        .output()
        .unwrap();
    done.store(true, Ordering::Relaxed);
    server.join().unwrap();
    common::restore_default_config_env();
    std::env::remove_var(ADDR_ENV);
    assert_eq!(state, DaemonPidState::Unverified);
    assert!(!matches);
    #[cfg(feature = "tauri")]
    {
        assert!(!desktop_status.running);
        assert!(desktop_status.message.contains("cannot be verified"));
    }
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("unverified"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("daemon.pid")).unwrap(),
        pid.to_string()
    );
}

#[cfg_attr(
    target_os = "macos",
    ignore = "legacy automatic stale-record replacement; macOS conservative replacement is covered by daemon_macos_conservative_e2e"
)]
#[test]
#[serial_test::serial]
fn background_start_verifies_and_stops_its_own_daemon() {
    use agent2ssh::daemon_control::{daemon_pid_state, start_daemon_background, DaemonPidState};
    let dir = tempfile::tempdir().unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    std::env::set_var(CONFIG_DIR_ENV, dir.path());
    std::env::set_var(ADDR_ENV, port.to_string());
    // A definitely absent PID is still cleaned up so restart remains usable.
    std::fs::write(dir.path().join("daemon.pid"), (u32::MAX - 1).to_string()).unwrap();
    let started =
        start_daemon_background(std::path::Path::new(env!("CARGO_BIN_EXE_agent2ssh-daemon")))
            .expect("start real daemon");
    let state = daemon_pid_state(started.pid);
    let stopped = stop_daemon_process(started.pid);
    common::restore_default_config_env();
    std::env::remove_var(ADDR_ENV);
    assert_eq!(state, DaemonPidState::Healthy);
    stopped.expect("stop the started daemon");
    assert!(!dir.path().join("daemon.pid").exists());
}

#[cfg(unix)]
#[test]
#[serial_test::serial]
fn failed_start_reaps_its_unrecognized_child_and_clears_its_pid() {
    use std::os::unix::fs::PermissionsExt;
    for publish_pid in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("renamed-child");
        let publish = if publish_pid {
            "printf '%s' \"$$\" > \"$AGENT2SSH_CONFIG_DIR/daemon.pid\"\n"
        } else {
            ""
        };
        let script = format!("#!/bin/sh\nprintf '%s' \"$$\" > \"$AGENT2SSH_CONFIG_DIR/child.pid\"\n{publish}exec /bin/sleep 15\n");
        std::fs::write(&stub, script).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o700)).unwrap();
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        std::env::set_var(CONFIG_DIR_ENV, dir.path());
        std::env::set_var(ADDR_ENV, port.to_string());
        let result = agent2ssh::daemon_control::start_daemon_background(&stub);
        let pid = std::fs::read_to_string(dir.path().join("child.pid"))
            .expect("stub recorded child PID")
            .parse::<i32>()
            .unwrap();
        let pid_retained = dir.path().join("daemon.pid").exists();
        let child_alive = unsafe { libc::kill(pid, 0) } == 0;
        if child_alive {
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
        }
        common::restore_default_config_env();
        std::env::remove_var(ADDR_ENV);
        assert!(result.unwrap_err().to_string().contains("was stopped"));
        assert!(
            !child_alive,
            "failed startup must not leave its child running (published={publish_pid})"
        );
        assert!(
            !pid_retained,
            "the stopped child's PID must be removed (published={publish_pid})"
        );
    }
}

#[test]
#[serial_test::serial]
fn startup_does_not_accept_another_process_health_response() {
    use std::io::{Read, Write};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    std::env::set_var(CONFIG_DIR_ENV, dir.path());
    std::env::set_var(ADDR_ENV, listener.local_addr().unwrap().to_string());
    let done = Arc::new(AtomicBool::new(false));
    let stop = done.clone();
    let other_pid = std::process::id();
    let server = std::thread::spawn(move || {
        let mut requests = 0;
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut socket, _)) => {
                    let mut buf = [0; 2048];
                    let _ = socket.set_read_timeout(Some(Duration::from_secs(1)));
                    let _ = socket.read(&mut buf);
                    requests += 1;
                    // The address becomes healthy only after startup's preflight.
                    let body = if requests == 1 {
                        "{}".to_string()
                    } else {
                        format!(r#"{{"ok":true,"pid":{other_pid}}}"#)
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(response.as_bytes());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("accept fake health: {error}"),
            }
        }
    });
    let result = agent2ssh::daemon_control::start_daemon_background(std::path::Path::new(env!(
        "CARGO_BIN_EXE_agent2ssh-daemon"
    )));
    done.store(true, Ordering::Relaxed);
    server.join().unwrap();
    common::restore_default_config_env();
    std::env::remove_var(ADDR_ENV);
    assert!(
        result.is_err(),
        "another process's health must not confirm startup"
    );
    assert!(!dir.path().join("daemon.pid").exists());
}

/// The route the Windows stop uses, exercised here on every platform: the daemon
/// accepts the request and then shuts down by itself, cleaning up.
#[test]
#[serial_test::serial]
fn the_shutdown_route_stops_the_daemon_gracefully() {
    let mut daemon = RunningDaemon::start("route", &[]);
    #[cfg(not(target_os = "macos"))]
    assert!(request_graceful_shutdown(), "the admin token is accepted");
    #[cfg(target_os = "macos")]
    request_drain_for_test(daemon.child.id()).expect("explicit test compatibility request");

    wait_until("the daemon to exit", Duration::from_secs(10), || {
        daemon.child.try_wait().ok().flatten().is_some()
    });
    let status = daemon.child.try_wait().unwrap().unwrap();
    assert!(
        status.success(),
        "a requested shutdown exits cleanly: {status:?}"
    );
    assert!(!daemon.pid_file().exists(), "and removes its pid file");
}

#[test]
#[serial_test::serial]
fn malformed_shutdown_bodies_are_rejected_without_stopping_the_daemon() {
    use std::io::{Read, Write};
    let mut daemon = RunningDaemon::start("malformed-shutdown", &[]);
    let addr = std::env::var(ADDR_ENV).expect("daemon address");
    for (content_type, body) in [
        (Some("application/json"), r#"{"expect_pid":"1234"}"#),
        (Some("application/json"), r#"{"expect_pid":-1}"#),
        (Some("application/json"), r#"{"expect_pid":4294967296}"#),
        (Some("application/json"), r#"{"expect_pid":"#),
        (Some("application/json"), r#"{"expect_pid":null}"#),
        (Some("application/json"), "{}"),
        (Some("application/json"), "null"),
        (Some("application/json"), " "),
        (Some("text/plain"), r#"{"expect_pid":1234}"#),
        (None, r#"{"expect_pid":1234}"#),
    ] {
        let mut stream = std::net::TcpStream::connect(&addr).expect("connect to daemon");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let content_type = content_type
            .map(|value| format!("Content-Type: {value}\r\n"))
            .unwrap_or_default();
        write!(stream, "POST /shutdown HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}", daemon.token, content_type, body.len(), body).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(
            response.starts_with("HTTP/1.1 400 "),
            "body {body:?}: {response}"
        );
        let health = local_daemon_request("GET", "/health", None, Duration::from_secs(2))
            .expect("daemon remains healthy");
        assert_eq!(health.status, 200, "body {body:?}");
        assert!(daemon.child.try_wait().unwrap().is_none(), "body {body:?}");
    }

    // JSON media-type parameters and +json variants are valid; a numeric zero
    // reaches the PID comparison instead of being rejected as malformed.
    for content_type in [
        "application/json; charset=utf-8",
        "application/problem+json",
    ] {
        let mut stream = std::net::TcpStream::connect(&addr).expect("connect to daemon");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let body = r#"{"expect_pid":0}"#;
        write!(stream, "POST /shutdown HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", daemon.token, content_type, body.len(), body).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(
            response.starts_with("HTTP/1.1 409 "),
            "content type {content_type}: {response}"
        );
        assert!(daemon.child.try_wait().unwrap().is_none());
    }
}

#[test]
#[serial_test::serial]
fn the_shutdown_route_is_closed_to_anonymous_and_scoped_callers() {
    let daemon = RunningDaemon::start(
        "denied",
        &[(
            "daemon_tokens.toml",
            "[[tokens]]\nname = \"scoped\"\ntoken = \"scoped-token\"\n[tokens.scope]\nallowed_hosts = [\"nowhere\"]\n",
        )],
    );

    let anonymous = local_daemon_request("POST", "/shutdown", None, Duration::from_secs(2))
        .expect("anonymous request");
    assert_eq!(anonymous.status, 401, "{}", anonymous.body);

    let scoped = local_daemon_request(
        "POST",
        "/shutdown",
        Some("scoped-token"),
        Duration::from_secs(2),
    )
    .expect("scoped request");
    assert_eq!(scoped.status, 403, "{}", scoped.body);

    let scoped_bad_body = local_daemon_request_with_body(
        "POST",
        "/shutdown",
        Some("scoped-token"),
        &serde_json::json!({ "expect_pid": "not-a-pid" }),
        Duration::from_secs(2),
    )
    .expect("scoped malformed request");
    assert_eq!(scoped_bad_body.status, 403, "{}", scoped_bad_body.body);

    // Neither refusal may have taken the daemon down.
    let health = local_daemon_request("GET", "/health", None, Duration::from_secs(2))
        .expect("health after refusals");
    assert_eq!(health.status, 200);
    assert!(
        daemon.token.starts_with("stop-e2e"),
        "admin token untouched"
    );
}

// ── The listener moves between the health check and the shutdown ─────────
//
// The stop path asks `/health` which PID is listening, decides, and then POSTs
// `/shutdown`. Those are two connections, and a restart or a respawn in between
// hands the listening socket to a *different* daemon that accepts the same admin
// token, because the token is a file both read from the same config directory.
// Without the expected-PID check the second daemon would take the first one's
// stop. These tests hold that race still.

/// A throwaway config dir with a token already written.
fn race_config_dir(_label: &str) -> PathBuf {
    let dir = fixture_root().join(format!(
        "a2s-{}-{:x}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before the epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create the throwaway config dir");
    std::fs::write(
        dir.join("daemon.token"),
        format!("stop-race-token-{}", std::process::id()),
    )
    .expect("seed daemon.token");
    dir
}

fn free_addr() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    listener.local_addr().expect("local addr").to_string()
}

/// Whether a port is free *right now*, asked by binding to it.
///
/// [`free_addr`] says a port was free a moment ago; an ephemeral port can be
/// handed out again by the kernel, so anything that must actually own one
/// re-checks. A neighbour test binding and releasing has no other symptom than
/// this, which is why a bound port is retried rather than reported.
fn port_is_free(addr: &str) -> bool {
    TcpListener::bind(addr).is_ok()
}

/// Spawn a daemon on an explicit address.
fn spawn_daemon_at(dir: &std::path::Path, addr: &str) -> Child {
    let stderr = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("daemon.stderr"))
        .expect("open daemon stderr");
    Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))
        .env("AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN", "1")
        .env(CONFIG_DIR_ENV, dir)
        .env(ADDR_ENV, addr)
        .env("AGENT2SSH_LOG", "info")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("spawn agent2ssh-daemon")
}

/// Start a daemon on `addr` and wait until it answers `/health` as itself,
/// without going through the proxy (which may still be pointing at the old one).
fn start_listener_at(dir: &std::path::Path, addr: &str) -> Child {
    for _ in 0..10 {
        // The port may have been taken by a neighbour since it was chosen; a
        // daemon that cannot bind exits, which is what the retry is for.
        if !port_is_free(addr) {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        }
        let mut child = spawn_daemon_at(dir, addr);
        let deadline = Instant::now() + STARTUP_BUDGET;
        while Instant::now() < deadline {
            if let Ok(response) =
                agent2ssh::daemon_activity::direct_daemon_request(addr, "GET", "/health", None)
            {
                if response.status == 200
                    && serde_json::from_str::<serde_json::Value>(&response.body)
                        .ok()
                        .and_then(|body| body["pid"].as_u64())
                        == Some(u64::from(child.id()))
                {
                    return child;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    // Distinguish "the port was taken" from "nothing can start here": the second
    // is a real failure, the first is a neighbour winning a free-port race.
    let detail = std::fs::read_to_string(dir.join("daemon.stderr")).unwrap_or_default();
    assert!(
        detail.contains("Address already in use") || !port_is_free(addr),
        "no daemon could be started on {addr}; stderr: {detail}"
    );
    panic!("port {addr} stayed taken by another process for the whole retry window");
}

/// Forward every connection arriving on `front` to whatever `back` is *at that
/// moment*, one short-lived connection at a time, counting them.
///
/// The destination is re-read per connection, which is what lets one proxy
/// address host two different daemons in turn: the client's second request
/// reaches a listener its first request did not. Deliberately dumb otherwise —
/// a buffering proxy would hide the very thing this holds still.
fn proxy(front: TcpListener, back: Arc<std::sync::Mutex<String>>) -> Arc<AtomicUsize> {
    let counted = Arc::new(AtomicUsize::new(0));
    let counter = counted.clone();
    std::thread::spawn(move || {
        for stream in front.incoming() {
            let Ok(mut client) = stream else { continue };
            counter.fetch_add(1, Ordering::SeqCst);
            let destination = back.lock().expect("proxy destination").clone();
            let Ok(mut upstream) = std::net::TcpStream::connect(&destination) else {
                continue;
            };
            let (Ok(mut client_reader), Ok(mut upstream_writer)) =
                (client.try_clone(), upstream.try_clone())
            else {
                continue;
            };
            let lift = std::thread::spawn(move || {
                let _ = std::io::copy(&mut client_reader, &mut upstream_writer);
                let _ = upstream_writer.shutdown(std::net::Shutdown::Write);
            });
            let _ = std::io::copy(&mut upstream, &mut client);
            let _ = client.shutdown(std::net::Shutdown::Write);
            let _ = lift.join();
        }
    });
    counted
}

/// The local transport is shared by the stop health check. This tests the
/// transport's actual response parsing, not whether a stop sends a shutdown.
#[cfg(unix)]
#[test]
fn local_health_response_parser_does_not_enforce_content_length() {
    use std::io::{Read, Write};

    for (response, expect_json) in [
        (
            "HTTP/1.1 200 OK\r\nX-Health: {\"ok\":true,\"pid\":42}\r\nContent-Length: 0\r\n\r\n",
            false,
        ),
        (
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n\r\n{\"ok\":true,\"pid\":42}",
            true,
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut chunk).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&chunk[..n]);
            }
            stream.write_all(response.as_bytes()).unwrap();
        });
        let result =
            agent2ssh::daemon_activity::direct_daemon_request(&address, "GET", "/health", None)
                .unwrap();
        server.join().unwrap();
        assert_eq!(result.status, 200);
        let body = serde_json::from_str::<serde_json::Value>(&result.body).ok();
        assert_eq!(
            body.as_ref().and_then(|json| json["pid"].as_u64()),
            expect_json.then_some(42)
        );
    }
}

/// A daemon that predates `expect_pid` ignores it and accepts the stop — which
/// is the previous behaviour, and the reason the client does not claim the
/// check happened. What must hold here: the stop still completes after the
/// stub's 200, the request was sent exactly once, and it is the force-kill stage
/// that ends the process.
///
/// Nothing here reads `app.log`, so the warning the client writes on this path is
/// pinned by no test at all (K14); the other half — a current daemon's stop
/// reporting the PID it checked — cannot be asserted until the daemon sends a
/// receipt.
///
/// The "old daemon" is a stub speaking the old protocol (200 to any body) on the
/// A real process whose executable name is `agent2ssh-daemon`, used where a
/// test needs something the identity checks will name but which is not a daemon.
///
/// Unix symlinks `/bin/sleep`; Windows copies `ping.exe` under that name and
/// runs it in a loop, because a symlink needs a privilege the test account may
/// not hold and `Process::name()` reads the executable's file name on both.
#[cfg(unix)]
fn stand_in_named_daemon(dir: &std::path::Path) -> Child {
    let named = dir.join("agent2ssh-daemon");
    let _ = std::fs::remove_file(&named);
    std::os::unix::fs::symlink("/bin/sleep", &named).expect("a stand-in process");
    Command::new(&named)
        .arg("30")
        .spawn()
        .expect("a process to name")
}

#[cfg(windows)]
fn stand_in_named_daemon(dir: &std::path::Path) -> Child {
    let system_root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let named = dir.join("agent2ssh-daemon.exe");
    std::fs::copy(system_root.join("System32").join("ping.exe"), &named)
        .expect("copy a stand-in process");
    Command::new(&named)
        .args(["-t", "127.0.0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("a process to name")
}

/// A matching executable name is not proof of identity: an unrelated process
/// can reuse the PID. This stub answers the old protocol — `200` to any body,
/// including one carrying `expect_pid` — so the client cannot tell whether the
/// daemon compared it.
///
/// The old daemon in this test is a stub speaking that protocol on the
/// address this config points at, with a PID its `/health` also reports, so the
/// identity checks pass and only the missing field check differs.
///
/// On Windows this is also the case that pins "the stop path really binds":
/// the stand-in never exits, so the stop reaches the forced-termination branch,
/// and replacing `BoundProcess::open` with `None` turns it red (the stop then
/// refuses to signal instead of succeeding).
#[test]
#[serial_test::serial]
fn a_daemon_that_ignores_the_expected_pid_stops_and_says_so() {
    use std::io::{Read, Write};
    let dir = race_config_dir("legacy-daemon");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind the stub");
    let addr = listener.local_addr().unwrap().to_string();
    std::env::set_var(CONFIG_DIR_ENV, &dir);
    std::env::set_var(ADDR_ENV, &addr);
    let token = std::fs::read_to_string(dir.join("daemon.token")).unwrap();

    // A real process, so the identity checks that run before the request have
    // something to name; the stub merely claims to be it.
    let mut host_process = stand_in_named_daemon(&dir);
    let pid = host_process.id();
    // Give the kernel a moment to publish its start time.
    std::thread::sleep(Duration::from_millis(200));
    let served_dir = std::fs::canonicalize(&dir)
        .expect("canonical config dir")
        .display()
        .to_string();
    let posted = Arc::new(AtomicUsize::new(0));
    let counted = posted.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let Ok(read) = stream.read(&mut chunk) else {
                    break;
                };
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
            }
            let is_post = request.starts_with(b"POST /shutdown ");
            if is_post {
                counted.fetch_add(1, Ordering::SeqCst);
            }
            let body = if is_post {
                // The old daemon knows nothing of `expect_pid`.
                r#"{"ok":true}"#.to_string()
            } else {
                // The config directory has to be escaped: on Windows it is a
                // path full of backslashes, and pasting it into a JSON string
                // produces `\U` — invalid JSON, which would make the client see
                // no health answer at all rather than the PID it expected.
                let served_json =
                    serde_json::to_string(&served_dir).expect("encode the config dir");
                format!(r#"{{"ok":true,"pid":{pid},"config_dir":{served_json}}}"#)
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });

    // The daemon answered 200 without checking, so the client treats the stop as
    // accepted — and then waits for the process to exit. This stand-in does not,
    // so the stop ends the way a hung daemon's does: killed after the drain
    // window. The point is the *first* half: the request went out once, and the
    // client did not mistake the old daemon's 200 for the checked path.
    let _ = token;
    // A stand-in that does not exit on request is killed after the drain window,
    // which the stop reports as success — so reaching Ok is the honest outcome.
    let stopped = stop_daemon_process(pid);
    if cfg!(target_os = "macos") {
        assert!(stopped.unwrap_err().to_string().contains("not bound"));
        assert_eq!(posted.load(Ordering::SeqCst), 0);
        assert!(host_process.try_wait().unwrap().is_none());
        let _ = host_process.kill();
        let _ = host_process.wait();
        let _ = std::fs::remove_dir_all(&dir);
        common::restore_default_config_env();
        std::env::remove_var(ADDR_ENV);
        return;
    }
    assert_eq!(
        posted.load(Ordering::SeqCst),
        1,
        "the old daemon was asked exactly once: {stopped:?}"
    );
    let still_running = host_process
        .try_wait()
        .expect("check the named process")
        .is_none();
    if still_running {
        let _ = host_process.kill();
        let _ = host_process.wait();
    }
    // The disjunction accepted `Err` as long as the process was gone, so a stop
    // that failed after the force signal still passed. Here the request was sent
    // (the stub answered POST /shutdown above, and `posted == 1` says so) and the
    // process only goes away after the force-kill, so `Ok` is the honest
    // outcome; `Err` would mean the kill stage or the owner cleanup failed.
    assert!(
        stopped.is_ok(),
        "the stop completes once the old daemon's 200 has been force-killed: {stopped:?}"
    );
    // The warning may say that the receipt is missing; it may not claim to know
    // the daemon's version, because a current daemon that compared and matched
    // answers the same 200 (#20).
    let log = std::fs::read_to_string(dir.join("app.log")).unwrap_or_default();
    assert!(
        log.contains("without a verifiable expect_pid receipt"),
        "the missing receipt should be logged: {log}"
    );
    assert!(
        !log.contains("predates"),
        "the log must not attribute the missing receipt to a version: {log}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    common::restore_default_config_env();
    std::env::remove_var(ADDR_ENV);
}

/// The body limit is the third refusal these two routes can answer, and the
/// only one no handler writes.
///
/// `POST /shutdown` and `POST /approvals/{id}/approve` take
/// `axum::body::Bytes`, which buffers the whole body before the handler runs.
/// A body past axum's default 2 MiB limit is therefore refused as `413` from
/// inside the extractor, with a plain-text message rather than the daemon's
/// `Error` object. Both are asserted against the real binary: the mirrored
/// router in `daemon_integration` mounts neither route, so it cannot answer for
/// the limit, and the doc-side check there only proves the status is listed.
///
/// The body is one byte past the limit and carries nothing either handler would
/// read, so a 400 here would mean the handler ran — which is the distinction
/// this pins.
#[test]
#[serial_test::serial]
fn an_oversized_body_is_refused_with_a_plain_text_413_before_the_handler() {
    let daemon = RunningDaemon::start("oversized-body", &[]);
    let oversized = serde_json::json!({ "pad": "x".repeat(2 * 1024 * 1024) });
    for path in [
        "/shutdown",
        "/approvals/00000000-0000-0000-0000-000000000000/approve",
    ] {
        let response = local_daemon_request_with_body(
            "POST",
            path,
            Some(&daemon.token),
            &oversized,
            Duration::from_secs(5),
        )
        .unwrap_or_else(|error| panic!("{path} answers an oversized body: {error}"));
        assert_eq!(response.status, 413, "{path}: {}", response.body);
        assert!(
            serde_json::from_str::<serde_json::Value>(&response.body).is_err(),
            "{path} answers the body limit in plain text, not as the Error object: {}",
            response.body
        );
    }
    // A refused body is inert: the daemon is still serving, so nothing read the
    // body and nothing acted on it.
    let health = local_daemon_request("GET", "/health", None, Duration::from_secs(2))
        .expect("the daemon still answers after two refused bodies");
    assert_eq!(health.status, 200, "{}", health.body);
}

/// The listener that answers may be a daemon serving a *different* config
/// directory — started from a shell that exported another `AGENT2SSH_CONFIG_DIR`.
/// It accepts this caller's token only if the tokens happen to be equal; the
/// config directory it reports is what says it is not our daemon, and the stop
/// must refuse on that alone.
#[test]
#[serial_test::serial]
fn a_daemon_serving_another_config_directory_is_not_stopped() {
    let mut daemon = RunningDaemon::start("other-config", &[]);
    let pid = daemon.child.id();
    let report = agent2ssh::daemon_activity::local_daemon_request(
        "GET",
        "/health",
        None,
        Duration::from_secs(2),
    )
    .expect("health");
    let body: serde_json::Value = serde_json::from_str(&report.body).expect("health body");
    let served = body["config_dir"]
        .as_str()
        .expect("a daemon reports the config directory it serves")
        .to_string();
    assert_eq!(
        served,
        std::fs::canonicalize(&daemon.dir)
            .unwrap_or_else(|_| daemon.dir.clone())
            .display()
            .to_string(),
        "the daemon names the directory it was given"
    );

    // Point the caller at a directory that is not the one being served, with the
    // same token: identity now rests on `config_dir`, not on the token.
    let other = race_config_dir("other-config-caller");
    std::fs::write(other.join("daemon.token"), &daemon.token).unwrap();
    std::env::set_var(CONFIG_DIR_ENV, &other);
    let error = stop_daemon_process(pid).expect_err("that is not this config's daemon");
    let message = error.to_string();
    assert!(
        message.contains(if cfg!(target_os = "macos") {
            "not bound"
        } else {
            "serves config directory"
        }),
        "{message}"
    );
    assert!(
        daemon.child.try_wait().unwrap().is_none(),
        "the daemon is still running"
    );
    assert!(daemon.pid_file().exists(), "its PID file is untouched");

    let _ = std::fs::remove_dir_all(&other);
    common::restore_default_config_env();
}

/// A daemon that is handed another process's PID refuses and keeps running.
///
/// This is the decision the daemon makes, isolated from how the client got there:
/// the admin token is *its own* and correct, and the only thing wrong is the PID
/// in the body.
#[test]
#[serial_test::serial]
fn a_daemon_refuses_a_shutdown_that_names_another_process() {
    let mut daemon = RunningDaemon::start("expect-pid", &[]);
    let own_pid = daemon.child.id();
    // A real process, so the assertion is about which PID it is rather than
    // about a number that merely fails to parse.
    let stranger = Command::new("sleep").arg("30").spawn();
    let (stranger_pid, mut stranger_child) = match stranger {
        Ok(child) => (child.id(), Some(child)),
        // No `sleep` on PATH: any PID that is not ours exercises the same branch.
        Err(_) => (own_pid.wrapping_add(1), None),
    };
    assert_ne!(stranger_pid, own_pid);

    let refused = local_daemon_request_with_body(
        "POST",
        "/shutdown",
        Some(&daemon.token),
        &serde_json::json!({ "expect_pid": stranger_pid }),
        Duration::from_secs(2),
    )
    .expect("the daemon answers a refusal");
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert!(
        refused.body.contains(&own_pid.to_string()),
        "the refusal names the daemon that answered: {}",
        refused.body
    );

    // A refusal is inert: the daemon is still serving, and its own PID is
    // accepted, so the 409 was about identity and not about the token.
    let health = local_daemon_request("GET", "/health", None, Duration::from_secs(2))
        .expect("health after the refusal");
    assert_eq!(health.status, 200);
    let accepted = local_daemon_request_with_body(
        "POST",
        "/shutdown",
        Some(&daemon.token),
        &serde_json::json!({ "expect_pid": own_pid }),
        Duration::from_secs(2),
    )
    .expect("the daemon answers its own stop");
    assert_eq!(accepted.status, 200, "{}", accepted.body);
    wait_until(
        "the daemon to exit after its own stop",
        Duration::from_secs(10),
        || daemon.child.try_wait().ok().flatten().is_some(),
    );
    if let Some(child) = stranger_child.as_mut() {
        let still_there = child.try_wait().expect("check the stranger").is_none();
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            still_there,
            "the stranger named in a refused request is not signalled either"
        );
    }
}

/// The race, end to end: `/health` reaches A through a proxy, A exits and a
/// daemon with the same admin token takes the same address, and the shutdown
/// request that was meant for A arrives at B — A's PID in the body, B on the
/// socket.
///
/// The second request is the one the old code sent (it had no field to send):
/// nothing in it said which process it meant, so whichever daemon held the
/// socket took it. With the check, B refuses and stays up.
///
/// Both daemons are the real binary on the real platform, so this is the same
/// race on Windows and on Unix; the proxy and the port probe are plain
/// `TcpListener` code with nothing platform-specific in them.
#[test]
#[serial_test::serial]
fn a_stop_that_reached_the_wrong_daemon_stops_nothing() {
    let dir = race_config_dir("race");
    let token = std::fs::read_to_string(dir.join("daemon.token")).expect("read daemon.token");
    let proxy_addr = free_addr();
    let first_addr = free_addr();
    std::env::set_var(CONFIG_DIR_ENV, &dir);
    std::env::set_var(ADDR_ENV, &proxy_addr);

    // A is the daemon the client finds through the proxy.
    let mut a = start_listener_at(&dir, &first_addr);
    let a_pid = a.id();
    #[cfg(target_os = "macos")]
    let a_instance: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("daemon.instance.json")).unwrap()).unwrap();
    let destination = Arc::new(std::sync::Mutex::new(first_addr.clone()));
    let front = TcpListener::bind(&proxy_addr).expect("bind the proxy");
    let connections = proxy(front, destination.clone());
    assert_eq!(
        agent2ssh::daemon_control::daemon_health_pid(),
        Some(a_pid),
        "the client's health check reaches A through the proxy"
    );

    // A exits — a restart, from the client's point of view — and B takes the same
    // address with the same admin token, so the only thing that changes in the
    // client's view is which process answers.
    let _ = a.kill();
    let _ = a.wait();
    #[cfg(target_os = "macos")]
    agent2ssh::daemon_identity::macos::recover(
        &dir,
        a_pid,
        a_instance["instance_id"].as_str().unwrap(),
    )
    .expect("explicitly recover A's exited identity records");
    let mut b = start_listener_at(&dir, &first_addr);
    let b_pid = b.id();
    assert_ne!(a_pid, b_pid);
    *destination.lock().expect("proxy destination") = first_addr.clone();
    let through_proxy = local_daemon_request("GET", "/health", None, Duration::from_secs(2))
        .expect("health through the proxy");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&through_proxy.body)
            .ok()
            .and_then(|body| body["pid"].as_u64()),
        Some(u64::from(b_pid)),
        "the same address now belongs to B"
    );

    // The stop the client had queued for A arrives at B, still naming A.
    let queued_for_a = local_daemon_request_with_body(
        "POST",
        "/shutdown",
        Some(&token),
        &serde_json::json!({ "expect_pid": a_pid }),
        Duration::from_secs(2),
    )
    .expect("B answers");
    assert_eq!(queued_for_a.status, 409, "{}", queued_for_a.body);
    assert!(
        queued_for_a.body.contains(&b_pid.to_string()),
        "the refusal names B: {}",
        queued_for_a.body
    );
    assert!(
        b.try_wait().unwrap().is_none(),
        "B must still be running: the stop was never B's to take"
    );

    // The same request naming B is accepted, so the 409 was about identity and
    // not about the token, the route or the proxy.
    let for_b = local_daemon_request_with_body(
        "POST",
        "/shutdown",
        Some(&token),
        &serde_json::json!({ "expect_pid": b_pid }),
        Duration::from_secs(2),
    )
    .expect("B answers its own stop");
    assert_eq!(for_b.status, 200, "{}", for_b.body);
    wait_until(
        "B to exit after its own stop",
        Duration::from_secs(10),
        || b.try_wait().ok().flatten().is_some(),
    );
    assert!(
        connections.load(Ordering::SeqCst) >= 3,
        "the proxy carried the health check and both shutdown requests"
    );

    let _ = std::fs::remove_dir_all(&dir);
    common::restore_default_config_env();
    std::env::remove_var(ADDR_ENV);
}

/// What the quit confirmation reads: a real daemon, its real list routes, the
/// admin token from the config directory.
#[test]
#[serial_test::serial]
fn activity_reads_an_idle_daemon_as_idle() {
    let _daemon = RunningDaemon::start("activity", &[]);
    let activity = agent2ssh::daemon_activity::fetch_daemon_activity()
        .expect("a healthy daemon answers all four list routes");
    assert!(activity.is_idle(), "{activity:?}");
}

/// The other half of the confirmation: a daemon that *is* holding something is
/// read as busy, and the count is the one the dialog will print.
#[test]
#[serial_test::serial]
fn activity_counts_a_resident_session() {
    let daemon = RunningDaemon::start("activity-busy", &[]);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("http client");
    let addr = std::env::var(ADDR_ENV).unwrap();

    let opened = client
        .post(format!("http://{addr}/resident-sessions"))
        .bearer_auth(&daemon.token)
        .json(&serde_json::json!({ "host": "local", "force": true }))
        .send()
        .expect("open a resident session");
    assert_eq!(
        opened.status(),
        200,
        "{}",
        opened.text().unwrap_or_default()
    );

    let activity = agent2ssh::daemon_activity::fetch_daemon_activity().expect("activity");
    assert_eq!(activity.resident_sessions, 1, "{activity:?}");
    assert!(!activity.is_idle());
}

// ── A relative AGENT2SSH_CONFIG_DIR ─────────────────────────────────────────
//
// The setting is read by whichever process needs it, resolved against *that*
// process's working directory. So there are two facts to pin, and only one of
// them is good news: in one working directory the daemon and the tool that stops
// it mean the same folder and everything above works; across two working
// directories the same value means two folders and no stop can happen. The
// second is documented (`docs/KNOWN-ISSUES.md` K16) rather than papered over,
// because making it "work" would mean changing what a relative path means.

/// Both sides working from the same directory: `./cfg` is one folder, the paths
/// this module reports are absolute, and no extra level appears under it.
#[test]
#[serial_test::serial]
fn a_relative_config_dir_in_one_working_directory_names_one_daemon() {
    let root = std::fs::canonicalize(fixture_root())
        .expect("canonical temp dir")
        .join(format!(
            "a2sr-{}-{:x}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock before the epoch")
                .as_nanos()
        ));
    let config = root.join("cfg");
    std::fs::create_dir_all(&config).expect("create the relative config dir");
    let token = format!("relative-token-{}", std::process::id());
    std::fs::write(config.join("daemon.token"), &token).expect("seed daemon.token");
    let addr = free_address();
    let previous = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&root).expect("stand in the working directory");
    std::env::set_var(CONFIG_DIR_ENV, "cfg");
    std::env::set_var(ADDR_ENV, &addr);

    // Asked of the library, not of the filesystem. The daemon resolves the same
    // value from the same directory, so this is the one place the two readings
    // meet — and `cfg/daemon.pid` existing proves nothing, because the daemon's
    // own file is there whether or not this process agrees where "cfg" is.
    assert_eq!(
        agent2ssh::daemon_control::daemon_pid_path().ok(),
        Some(config.join("daemon.pid")),
        "one level: cfg/daemon.pid, not cfg/cfg/daemon.pid"
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))
        .env("AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN", "1")
        .env(CONFIG_DIR_ENV, "cfg")
        .env(ADDR_ENV, &addr)
        .env("AGENT2SSH_LOG", "warn")
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the daemon from the working directory");
    let pid = child.id();
    // A panic here would otherwise leave a daemon running on the configured
    // address for the rest of the suite — and with `AGENT2SSH_DAEMON_ADDR` gone
    // it would be squatting the default port where every other test looks.
    let guard = ChildGuard(&mut child);
    wait_until("the daemon to serve", STARTUP_BUDGET, || {
        local_daemon_request("GET", "/health", None, Duration::from_millis(200))
            .is_ok_and(|response| response.status == 200)
    });

    assert!(
        !config.join("cfg").exists(),
        "a relative path must not nest"
    );
    assert_eq!(
        agent2ssh::daemon_control::daemon_pid_path()
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|raw| raw.trim().parse::<u32>().ok()),
        Some(pid),
        "the pid file this process reads is the one the daemon wrote"
    );
    assert_eq!(
        agent2ssh::daemon_control::daemon_pid_state(pid),
        agent2ssh::daemon_control::DaemonPidState::Healthy
    );
    request_drain_for_test(pid).expect("stop it");
    drop(guard);

    std::env::set_current_dir(previous).expect("restore the working directory");
    let _ = std::fs::remove_dir_all(&root);
}

/// The same relative value from a different working directory is a different
/// folder: the daemon is answering but this process has no pid file for it, so
/// the honest answer is "cannot be verified" — never a stop, and never a
/// deletion of whatever pid file *this* folder's `cfg` would hold.
#[test]
#[serial_test::serial]
fn a_relative_config_dir_from_another_working_directory_is_a_different_daemon() {
    let root = std::fs::canonicalize(fixture_root())
        .expect("canonical temp dir")
        .join(format!(
            "a2ss-{}-{:x}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock before the epoch")
                .as_nanos()
        ));
    let daemon_dir = root.join("home");
    let elsewhere = root.join("elsewhere");
    std::fs::create_dir_all(daemon_dir.join("cfg")).expect("daemon config dir");
    std::fs::create_dir_all(&elsewhere).expect("other working directory");
    std::fs::write(
        daemon_dir.join("cfg").join("daemon.token"),
        "relative-token",
    )
    .expect("seed daemon.token");
    let addr = free_address();
    // The child gets these on its command; this process needs them too before it
    // probes anything, or it looks for the daemon on the default port while the
    // daemon it just started listens somewhere else.
    std::env::set_var(CONFIG_DIR_ENV, "cfg");
    std::env::set_var(ADDR_ENV, &addr);

    let mut child = Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))
        .env(CONFIG_DIR_ENV, "cfg")
        .env(ADDR_ENV, &addr)
        .env("AGENT2SSH_LOG", "warn")
        .current_dir(&daemon_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the daemon from its own working directory");
    let pid = child.id();
    let mut guard = ChildGuard(&mut child);
    // `wait_for_daemon_pid`, not a bare `/health` probe: with this config
    // directory the test cannot use `RunningDaemon::wait_until_serving`, and the
    // probe has to name *this* child — the previous test's daemon may still be
    // answering on another port while /health here is not up yet.
    wait_until("the daemon to report its pid", STARTUP_BUDGET, || {
        agent2ssh::daemon_control::daemon_health_pid() == Some(pid)
    });

    // This is the test's whole point: the daemon is up and answering, so
    // anything that goes wrong from here must not be read as "not started".
    assert_eq!(
        agent2ssh::daemon_control::daemon_health_pid(),
        Some(pid),
        "the daemon is answering before the working directory changes"
    );

    let previous = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&elsewhere).expect("stand in the other directory");

    assert_eq!(
        agent2ssh::daemon_control::daemon_pid_path().ok(),
        Some(elsewhere.join("cfg").join("daemon.pid")),
        "the same value names a different file here"
    );
    assert!(
        agent2ssh::daemon_control::read_daemon_pid()
            .expect("read")
            .is_none(),
        "and nothing has written it — the daemon's file is in the other directory"
    );
    assert_eq!(
        agent2ssh::daemon_control::daemon_health_pid(),
        Some(pid),
        "while the daemon itself is still the one answering"
    );
    assert!(
        agent2ssh::daemon_control::daemon_pid_state(pid)
            != agent2ssh::daemon_control::DaemonPidState::Healthy,
        "so this directory cannot call it healthy: there is no pid file to agree with"
    );

    // Deliberately no stop: `stop_daemon_process` is not reachable from here
    // without a pid file, and the child is this process's own, so clean it up
    // directly. A daemon left behind would keep the port alive past the test.
    guard.drop_child();

    std::env::set_current_dir(previous).expect("restore the working directory");
    let _ = std::fs::remove_dir_all(&root);
}
