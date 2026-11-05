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

// Legacy daemons do not enforce the target header. A new client must not
// silently send an unguarded stop, even when their health PID looks correct.
#[test]
#[serial_test::serial]
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
