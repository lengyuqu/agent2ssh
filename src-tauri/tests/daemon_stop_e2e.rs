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

        let child = Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))
            .env(CONFIG_DIR_ENV, &dir)
            .env(ADDR_ENV, &addr)
            .env("AGENT2SSH_LOG", "warn")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn agent2ssh-daemon");
        let daemon = Self { child, dir, token };
        daemon.wait_until_serving();
        daemon
    }

    fn wait_until_serving(&self) {
        let deadline = Instant::now() + STARTUP_BUDGET;
        while Instant::now() < deadline {
            if local_daemon_request("GET", "/health", None, Duration::from_millis(500))
                .is_ok_and(|response| response.status == 200)
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("the daemon did not answer /health within {STARTUP_BUDGET:?}");
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
    stop_daemon_process(pid).expect("unrelated process is ignored");
    let still_running = child.try_wait().expect("check unrelated process").is_none();
    let _ = child.kill();
    let _ = child.wait();
    assert!(still_running, "stop must not signal the unrelated process");
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
