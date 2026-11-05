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
    /// The test stopped the child itself; do not touch it again.
    fn forget(self) {
        std::mem::forget(self);
    }

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

    // Neither refusal may have taken the daemon down.
    let health = local_daemon_request("GET", "/health", None, Duration::from_secs(2))
        .expect("health after refusals");
    assert_eq!(health.status, 200);
    assert!(
        daemon.token.starts_with("stop-e2e"),
        "admin token untouched"
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

// ── A relative AGENT2SSH_CONFIG_DIR ─────────────────────────────────────────
//
// The setting is read by whichever process needs it, resolved against *that*
// process's working directory. So there are two facts to pin, and only one of
// them is good news: in one working directory the daemon and the tool that stops
// it mean the same folder and everything above works; across two working
// directories the same value means two folders and no stop can happen. The
// second is documented (`docs/KNOWN-ISSUES.md` K15) rather than papered over,
// because making it "work" would mean changing what a relative path means.

/// Both sides working from the same directory: `./cfg` is one folder, the paths
/// this module reports are absolute, and no extra level appears under it.
#[test]
#[serial_test::serial]
fn a_relative_config_dir_in_one_working_directory_names_one_daemon() {
    let root = std::env::temp_dir().join(format!(
        "a2s-relative-{}-{}",
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
    stop_daemon_process(pid).expect("stop it");
    guard.forget();

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
    let root = std::env::temp_dir().join(format!(
        "a2s-relative-split-{}-{}",
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
