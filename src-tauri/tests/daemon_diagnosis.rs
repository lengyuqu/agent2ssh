//! Issue #21 — read-only daemon diagnosis and conservative cleanup.
//!
//! Every case runs the real CLI against its own config directory and its own
//! `AGENT2SSH_DAEMON_ADDR`, so a real daemon on the machine cannot answer and
//! turn a measurement into an accident. Synthetic values only.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn cli(dir: &Path, addr: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
        .args(args)
        .env("AGENT2SSH_CONFIG_DIR", dir)
        .env("AGENT2SSH_DAEMON_ADDR", addr)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

fn status_json(dir: &Path, addr: &str) -> serde_json::Value {
    let output = cli(dir, addr, &["daemon", "status", "--json"]);
    assert!(
        output.status.success(),
        "daemon status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "status was not JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// A port nothing listens on: bind, take the number, drop the listener.
fn dead_addr() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    drop(listener);
    addr
}

/// A minimal HTTP server with an explicit answer per route, so "forged health"
/// and "rejected token" can be produced without a real daemon.
struct FakeDaemon {
    addr: String,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
    #[cfg(target_os = "macos")]
    requests: Arc<std::sync::Mutex<Vec<String>>>,
}

impl FakeDaemon {
    fn start(health: Option<serde_json::Value>, gate_status: u16) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        #[cfg(target_os = "macos")]
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        #[cfg(target_os = "macos")]
        let captured = requests.clone();
        let health_body = health.map(|value| value.to_string());
        let handle = std::thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buffer = [0u8; 2048];
                        let read = stream.read(&mut buffer).unwrap_or(0);
                        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                        #[cfg(target_os = "macos")]
                        captured.lock().unwrap().push(request.clone());
                        let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                        let (status, body) = if path.starts_with("/health") {
                            match &health_body {
                                Some(body) => (200u16, body.clone()),
                                None => (404u16, "{}".to_string()),
                            }
                        } else if path.starts_with("/gate") {
                            (gate_status, "{}".to_string())
                        } else {
                            (404u16, "{}".to_string())
                        };
                        let reason = match status {
                            200 => "OK",
                            401 => "Unauthorized",
                            _ => "Not Found",
                        };
                        let response = format!(
                            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            addr,
            stop,
            handle: Some(handle),
            #[cfg(target_os = "macos")]
            requests,
        }
    }
}

impl Drop for FakeDaemon {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn health_body(pid: u32, config_dir: &Path) -> serde_json::Value {
    let canonical = std::fs::canonicalize(config_dir).unwrap_or_else(|_| config_dir.to_path_buf());
    serde_json::json!({
        "ok": true,
        "pid": pid,
        "config_dir": canonical.display().to_string(),
        "version": "test",
    })
}

#[test]
fn pid_zero_is_absent_not_denied() {
    // The OS rejects PID 0 as an invalid parameter, which is "no such process",
    // not "access denied". Conflating the two is what turns a live daemon into
    // a stale record someone deletes.
    assert!(matches!(
        agent2ssh::daemon_control::observe_pid(0),
        agent2ssh::daemon_control::PidObservation::Absent
    ));
}

#[test]
fn no_record_and_no_endpoint_reports_not_running() {
    let dir = tempfile::tempdir().unwrap();
    let report = status_json(dir.path(), &dead_addr());
    #[cfg(not(target_os = "macos"))]
    assert_eq!(report["verdict"], "not-running");
    #[cfg(target_os = "macos")]
    assert_eq!(report["verdict"], "unverified");
    assert_eq!(report["record"]["state"], "missing");
    assert_eq!(report["health"]["state"], "inactive");
    assert_eq!(report["pid_lock"], "free");
    #[cfg(not(target_os = "macos"))]
    assert!(report["next_step"]
        .as_str()
        .unwrap()
        .contains("daemon start"));
    #[cfg(target_os = "macos")]
    assert!(report["next_step"]
        .as_str()
        .unwrap()
        .contains("state is unknown"));
    let name = dir
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        report["config_dir"].as_str().unwrap().contains(&name),
        "config_dir should be the directory the report read: {}",
        report["config_dir"]
    );
}

#[cfg(target_os = "macos")]
#[test]
fn leftover_instance_does_not_report_not_running_or_suggest_unsafe_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("daemon.instance.json"), "partial").unwrap();
    let report = status_json(dir.path(), &dead_addr());
    assert_eq!(report["verdict"], "unverified");
    assert_eq!(report["token"], "not-checked");
    assert!(report["next_step"]
        .as_str()
        .unwrap()
        .contains("--expect-instance"));
    assert!(dir.path().join("daemon.instance.json").exists());
}

#[test]
fn malformed_record_is_damaged_and_clear_stale_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    let pid_path = dir.path().join("daemon.pid");
    std::fs::write(&pid_path, b"not-a-pid\x01").unwrap();
    let report = status_json(dir.path(), &dead_addr());
    assert_eq!(report["verdict"], "damaged");
    assert_eq!(report["record"]["state"], "malformed");
    assert!(report["record"]["raw"]
        .as_str()
        .unwrap()
        .contains("not-a-pid"));

    let addr = dead_addr();
    let output = cli(
        dir.path(),
        &addr,
        &["daemon", "clear-stale", "--expect-pid", "4242"],
    );
    assert!(!output.status.success());
    assert!(
        pid_path.exists(),
        "clear-stale must keep a malformed record"
    );
}

#[test]
fn live_unrelated_process_is_unverified_and_clear_stale_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let own_pid = std::process::id();
    let pid_path = dir.path().join("daemon.pid");
    std::fs::write(&pid_path, own_pid.to_string()).unwrap();
    let report = status_json(dir.path(), &dead_addr());
    assert_eq!(report["verdict"], "running-unhealthy");
    assert_eq!(report["observation"]["state"], "live");

    let addr = dead_addr();
    let output = cli(
        dir.path(),
        &addr,
        &[
            "daemon",
            "clear-stale",
            "--expect-pid",
            &own_pid.to_string(),
        ],
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("exists or cannot be observed"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(pid_path.exists());
}

#[test]
fn absent_pid_is_stale_and_clear_stale_removes_the_record() {
    let dir = tempfile::tempdir().unwrap();
    let absent = u32::MAX - 1; // not a real PID on any supported platform
    let pid_path = dir.path().join("daemon.pid");
    std::fs::write(&pid_path, absent.to_string()).unwrap();
    let report = status_json(dir.path(), &dead_addr());
    assert_eq!(report["verdict"], "stale");
    assert_eq!(report["observation"]["state"], "absent");
    assert!(report["next_step"]
        .as_str()
        .unwrap()
        .contains("clear-stale"));

    let addr = dead_addr();
    let output = cli(
        dir.path(),
        &addr,
        &["daemon", "clear-stale", "--expect-pid", &absent.to_string()],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!pid_path.exists(), "the record should be gone");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("no process was stopped"),
        "cleanup must say no process was signalled"
    );
}

#[test]
fn concurrent_replacement_is_refused() {
    use fs2::FileExt;
    let dir = tempfile::tempdir().unwrap();
    let pid_path = dir.path().join("daemon.pid");
    let lock_path = dir.path().join(".daemon-pid.lock");
    std::fs::write(&pid_path, (u32::MAX - 1).to_string()).unwrap();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    lock.lock_exclusive().unwrap();

    let addr = dead_addr();
    let child = Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
        .args([
            "daemon",
            "clear-stale",
            "--expect-pid",
            &(u32::MAX - 1).to_string(),
        ])
        .env("AGENT2SSH_CONFIG_DIR", dir.path())
        .env("AGENT2SSH_DAEMON_ADDR", &addr)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // Give the child time to read the snapshot and block on the lock, then
    // replace the record behind its back.
    std::thread::sleep(std::time::Duration::from_millis(400));
    std::fs::write(&pid_path, (u32::MAX - 2).to_string()).unwrap();
    FileExt::unlock(&lock).unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success(), "a replaced record was removed");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("changed") || stderr.contains("expected"),
        "unexpected stderr: {stderr}"
    );
    let kept = std::fs::read_to_string(&pid_path).unwrap();
    assert_eq!(kept.trim(), (u32::MAX - 2).to_string());
}

#[test]
fn forged_health_with_a_different_pid_is_unverified() {
    let dir = tempfile::tempdir().unwrap();
    let recorded = u32::MAX - 1;
    std::fs::write(dir.path().join("daemon.pid"), recorded.to_string()).unwrap();
    let fake = FakeDaemon::start(Some(health_body(1234, dir.path())), 200);
    let report = status_json(dir.path(), &fake.addr);
    assert_eq!(report["health"]["state"], "reporting");
    assert_eq!(report["health"]["same_pid"], false);
    assert_eq!(report["verdict"], "unverified");
    assert!(
        report["next_step"].as_str().unwrap().contains("1234"),
        "the advice must name the endpoint's PID"
    );
}

#[test]
fn rejected_token_is_reported_and_the_token_is_not_printed() {
    let dir = tempfile::tempdir().unwrap();
    let own_pid = std::process::id();
    std::fs::write(dir.path().join("daemon.pid"), own_pid.to_string()).unwrap();
    let token = "synthetic-daemon-token-7f3c";
    std::fs::write(dir.path().join("daemon.token"), token).unwrap();
    let fake = FakeDaemon::start(Some(health_body(own_pid, dir.path())), 401);
    let output = cli(dir.path(), &fake.addr, &["daemon", "status", "--json"]);
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    #[cfg(not(target_os = "macos"))]
    assert_eq!(report["token"], "rejected");
    #[cfg(target_os = "macos")]
    {
        assert_eq!(report["token"], "not-checked");
        let requests = fake.requests.lock().unwrap();
        assert!(!requests.is_empty());
        assert!(requests.iter().all(|request| !request.contains(token)));
        assert!(requests
            .iter()
            .all(|request| !request.to_ascii_lowercase().contains("authorization:")));
    }
    assert_eq!(report["verdict"], "unverified");
    for stream in [&output.stdout, &output.stderr] {
        assert!(
            !String::from_utf8_lossy(stream).contains(token),
            "the admin token leaked into the diagnosis"
        );
    }
}

#[test]
fn held_pid_lock_is_reported_without_blocking() {
    use fs2::FileExt;
    let dir = tempfile::tempdir().unwrap();
    let lock_path = dir.path().join(".daemon-pid.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    lock.lock_exclusive().unwrap();

    let started = std::time::Instant::now();
    let report = status_json(dir.path(), &dead_addr());
    let elapsed = started.elapsed();
    assert_eq!(report["pid_lock"], "held");
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "status blocked on the writers' lock for {elapsed:?}"
    );
    FileExt::unlock(&lock).unwrap();
}
