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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agent2ssh::daemon_activity::local_daemon_request;
use agent2ssh::daemon_control::{process_is_alive, request_graceful_shutdown, stop_daemon_process};
use agent2ssh::store::CONFIG_DIR_ENV;

mod common;

const ADDR_ENV: &str = "AGENT2SSH_DAEMON_ADDR";
const STARTUP_BUDGET: Duration = Duration::from_secs(30);

struct RunningDaemon {
    child: Child,
    dir: PathBuf,
    token: String,
}

impl RunningDaemon {
    fn start(label: &str, extra_files: &[(&str, &str)]) -> Self {
        Self::start_with_name(label, extra_files, false)
    }

    fn start_with_name(label: &str, extra_files: &[(&str, &str)], renamed: bool) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "a2s-daemon-stop-{label}-{}-{}",
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
        Command::new(binary)
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

impl Drop for RunningDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
        common::restore_default_config_env();
        std::env::remove_var(ADDR_ENV);
    }
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
    let daemon = RunningDaemon::start("graceful", &[]);
    let pid = daemon.child.id();
    assert!(daemon.pid_file().exists(), "the daemon writes its pid file");
    assert!(process_is_alive(pid));

    let started = Instant::now();
    stop_daemon_process(pid).expect("stop the daemon");

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
    stop_daemon_process(pid).expect("stop with an active SSE subscriber");
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
    stop_daemon_process(restarted_pid).expect("stop restarted daemon");
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
    stop_daemon_process(daemon.child.id()).expect("stop with idle WebSocket");
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
    stop_daemon_process(daemon.child.id()).expect("stop with backpressured WebSocket");
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
    stop_daemon_process(daemon.child.id()).expect("stop while SSH command is running");
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
    stop_daemon_process(daemon.child.id()).expect("stop while approval is pending");
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
    let stopped = stop_daemon_process(daemon.child.id());
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
    stop_daemon_process(old_pid).expect("stop old daemon");
    assert!(!agent2ssh::daemon_control::remove_daemon_pid_file_if(old_pid).unwrap());
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
    assert!(stderr.contains("live or cannot be verified"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(old.pid_file()).unwrap(),
        old_pid.to_string()
    );
    assert!(old.child.try_wait().unwrap().is_none());
}

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
    assert!(error.to_string().contains("cannot be safely identified"));
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
        .contains("could not be authenticated"));
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
    #[cfg(feature = "tauri")]
    {
        let status = agent2ssh::tauri_commands::daemon_status().expect("desktop status");
        assert!(status.running);
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
    assert!(String::from_utf8_lossy(&status.stdout).contains("Daemon is running"));
    let start = cli("start");
    assert!(start.status.success());
    assert!(String::from_utf8_lossy(&start.stdout).contains("already running"));
    let stop = cli("stop");
    assert!(!stop.status.success());
    assert!(String::from_utf8_lossy(&stop.stderr).contains("cannot be safely identified"));
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
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("could not be authenticated"));
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
    assert!(String::from_utf8_lossy(&status.stdout).contains("PID file is missing"));
    assert!(!cli("stop").status.success());
    #[cfg(feature = "tauri")]
    {
        let status = agent2ssh::tauri_commands::daemon_status().unwrap();
        assert!(!status.running && status.message.contains("PID file is missing"));
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
        .contains("could not be authenticated"));
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
                    let body = format!(r#"{{"ok":true,"pid":{pid},"shutdown_pid_guard":true}}"#);
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
    assert!(String::from_utf8_lossy(&status.stdout).contains("identity cannot be verified"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("daemon.pid")).unwrap(),
        pid.to_string()
    );
}

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
    assert!(request_graceful_shutdown(), "the admin token is accepted");

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

    let anonymous_guarded = agent2ssh::daemon_activity::local_daemon_request_for_pid(
        "POST",
        "/shutdown/guarded",
        None,
        Duration::from_secs(2),
        daemon.child.id(),
    )
    .expect("anonymous guarded shutdown");
    assert_eq!(anonymous_guarded.status, 401, "{}", anonymous_guarded.body);

    let missing = local_daemon_request(
        "POST",
        "/shutdown/guarded",
        Some(&daemon.token),
        Duration::from_secs(2),
    )
    .expect("missing target PID");
    assert_eq!(
        missing.status, 400,
        "guarded route must require the PID header"
    );

    let client = reqwest::blocking::Client::new();
    let address = std::env::var(ADDR_ENV).unwrap();
    for invalid in ["not-a-pid", "0"] {
        let response = client
            .post(format!("http://{address}/shutdown/guarded"))
            .bearer_auth(&daemon.token)
            .header(agent2ssh::daemon_activity::EXPECTED_PID_HEADER, invalid)
            .send()
            .expect("invalid target PID");
        assert_eq!(response.status(), 400, "invalid target: {invalid}");
    }

    let scoped_mismatch = agent2ssh::daemon_activity::local_daemon_request_for_pid(
        "POST",
        "/shutdown/guarded",
        Some("scoped-token"),
        Duration::from_secs(2),
        daemon.child.id().wrapping_add(1),
    )
    .expect("scoped guarded mismatch");
    assert_eq!(
        scoped_mismatch.status, 403,
        "authentication must precede PID checks"
    );

    let mismatch = agent2ssh::daemon_activity::local_daemon_request_for_pid(
        "POST",
        "/shutdown/guarded",
        Some(&daemon.token),
        Duration::from_secs(2),
        daemon.child.id().wrapping_add(1),
    )
    .expect("guarded mismatch");
    assert_eq!(mismatch.status, 409, "{}", mismatch.body);
    assert!(mismatch.body.contains("target PID"), "{}", mismatch.body);

    // Neither refusal may have taken the daemon down.
    let health = local_daemon_request("GET", "/health", None, Duration::from_secs(2))
        .expect("health after refusals");
    assert_eq!(health.status, 200);
    assert!(
        daemon.token.starts_with("stop-e2e"),
        "admin token untouched"
    );
}

// An endpoint can claim to have accepted a shutdown while the actual daemon
// remains alive; the client must not turn that response into a PID-based kill.
#[test]
#[serial_test::serial]
fn an_accepted_shutdown_that_does_not_stop_the_daemon_never_signals_it() {
    use std::io::{Read, Write};
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };

    let mut daemon = RunningDaemon::start("unresponsive-stop", &[]);
    let pid = daemon.child.id();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    std::env::set_var(ADDR_ENV, listener.local_addr().unwrap().to_string());
    let done = Arc::new(AtomicBool::new(false));
    let posts = Arc::new(AtomicUsize::new(0));
    let stop = done.clone();
    let observed_posts = posts.clone();
    let server = std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut socket, _)) => {
                    socket
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut request = [0; 4096];
                    let n = socket.read(&mut request).unwrap_or(0);
                    if request[..n].starts_with(b"POST /shutdown/guarded ") {
                        observed_posts.fetch_add(1, Ordering::Relaxed);
                    }
                    let body = format!(r#"{{"ok":true,"pid":{pid},"shutdown_pid_guard":true}}"#);
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
                Err(error) => panic!("accept shutdown stub: {error}"),
            }
        }
    });
    let result = stop_daemon_process(pid);
    done.store(true, Ordering::Relaxed);
    server.join().unwrap();
    assert!(result.unwrap_err().to_string().contains("no signal sent"));
    assert_eq!(posts.load(Ordering::Relaxed), 1);
    assert!(
        daemon.child.try_wait().unwrap().is_none(),
        "the real daemon must remain alive"
    );
    assert!(daemon.pid_file().exists(), "the PID file must be retained");
}

#[test]
#[serial_test::serial]
// Legacy daemons do not enforce the target header. A new client must not
// silently send an unguarded stop, even when their health PID looks correct.
fn legacy_daemon_without_pid_guard_is_not_automatically_stopped() {
    use std::io::{Read, Write};
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };
    let mut daemon = RunningDaemon::start("legacy-guard", &[]);
    let pid = daemon.child.id();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    std::env::set_var(ADDR_ENV, listener.local_addr().unwrap().to_string());
    let done = Arc::new(AtomicBool::new(false));
    let posts = Arc::new(AtomicUsize::new(0));
    let stop = done.clone();
    let observed_posts = posts.clone();
    let server = std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut socket, _)) => {
                    let _ = socket.set_read_timeout(Some(Duration::from_secs(1)));
                    let mut request = [0; 2048];
                    let n = socket.read(&mut request).unwrap_or(0);
                    if request[..n].starts_with(b"POST ") {
                        observed_posts.fetch_add(1, Ordering::Relaxed);
                    }
                    let body = format!(r#"{{"ok":true,"pid":{pid}}}"#);
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
                Err(error) => panic!("accept legacy stub: {error}"),
            }
        }
    });
    let result = stop_daemon_process(pid);
    done.store(true, Ordering::Relaxed);
    server.join().unwrap();
    let alive = daemon.child.try_wait().unwrap().is_none();
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("does not support guarded shutdown"));
    assert_eq!(
        posts.load(Ordering::Relaxed),
        0,
        "legacy endpoint must not receive POST"
    );
    assert!(alive && daemon.pid_file().exists());
}

// Simulate the listener changing between health and shutdown requests. Both
// daemons intentionally share a token; the replacement must reject the request.
#[test]
#[serial_test::serial]
fn shutdown_does_not_reach_a_replacement_daemon_with_the_same_token() {
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpStream};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let mut target = RunningDaemon::start("race-target", &[]);
    let target_addr = std::env::var(ADDR_ENV).unwrap();
    let mut replacement = RunningDaemon::start("race-replacement", &[]);
    let replacement_addr = std::env::var(ADDR_ENV).unwrap();
    assert_eq!(target.token, replacement.token);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    std::env::set_var(CONFIG_DIR_ENV, &target.dir);
    std::env::set_var(ADDR_ENV, listener.local_addr().unwrap().to_string());
    let done = Arc::new(AtomicBool::new(false));
    let stop = done.clone();
    let guarded_conflict = Arc::new(AtomicBool::new(false));
    let observed_conflict = guarded_conflict.clone();
    let target_pid = target.child.id();
    let proxy = std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut inbound, _)) => {
                    inbound
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = Vec::new();
                    let mut chunk = [0; 2048];
                    loop {
                        let n = inbound.read(&mut chunk).unwrap();
                        if n == 0 {
                            break;
                        }
                        request.extend_from_slice(&chunk[..n]);
                        if request.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let is_guarded_post = request.starts_with(b"POST /shutdown/guarded ");
                    let addr = if request.starts_with(b"POST /shutdown") {
                        &replacement_addr
                    } else {
                        &target_addr
                    };
                    let mut outbound = TcpStream::connect(addr).unwrap();
                    outbound
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    outbound.write_all(&request).unwrap();
                    let mut response = Vec::new();
                    outbound.read_to_end(&mut response).unwrap();
                    let expected = format!("x-agent2ssh-expected-pid: {target_pid}");
                    if is_guarded_post
                        && String::from_utf8_lossy(&request)
                            .to_ascii_lowercase()
                            .contains(&expected)
                        && response.starts_with(b"HTTP/1.1 409")
                    {
                        observed_conflict.store(true, Ordering::Relaxed);
                    }
                    let _ = inbound.write_all(&response);
                    let _ = inbound.shutdown(Shutdown::Both);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("accept proxy: {error}"),
            }
        }
    });
    let result = stop_daemon_process(target.child.id());
    done.store(true, Ordering::Relaxed);
    proxy.join().unwrap();
    let target_alive = target.child.try_wait().unwrap().is_none();
    let replacement_alive = replacement.child.try_wait().unwrap().is_none();
    assert!(result.is_err(), "replacement shutdown must be rejected");
    assert!(
        guarded_conflict.load(Ordering::Relaxed),
        "replacement must receive the guarded PID and reject it with 409"
    );
    assert!(
        target_alive && replacement_alive,
        "neither process may be stopped"
    );
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
