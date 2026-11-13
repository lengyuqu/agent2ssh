#![cfg(all(target_os = "macos", feature = "daemon"))]
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    os::unix::net::UnixStream,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
const TOKEN: &str = "a2s-test-only-credential";
const ID: &str = "0123456789abcdef0123456789abcdef";
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn send(s: &mut UnixStream, value: Value) {
    let bytes = serde_json::to_vec(&value).unwrap();
    s.write_all(&(bytes.len() as u32).to_be_bytes()).unwrap();
    s.write_all(&bytes).unwrap();
}
fn receive(s: &mut UnixStream) -> Value {
    let mut h = [0; 4];
    s.read_exact(&mut h).unwrap();
    let mut b = vec![0; u32::from_be_bytes(h) as usize];
    s.read_exact(&mut b).unwrap();
    serde_json::from_slice(&b).unwrap()
}
fn daemon(dir: &Path) -> (ChildGuard, Value) {
    let addr = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    std::fs::write(dir.join("daemon.token"), TOKEN).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))
        .env("AGENT2SSH_CONFIG_DIR", dir)
        .env("AGENT2SSH_DAEMON_ADDR", addr)
        .env_remove("AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut guard = ChildGuard(child);
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(raw) = std::fs::read(dir.join("daemon.instance.json")) {
            let record: Value = serde_json::from_slice(&raw).unwrap();
            if UnixStream::connect(dir.join(".daemon-ipc/s")).is_ok() {
                return (guard, record);
            }
        }
        if let Some(status) = guard.0.try_wait().unwrap() {
            panic!("daemon exited before IPC ready: {status}");
        }
        assert!(Instant::now() < deadline, "daemon did not start");
        thread::sleep(Duration::from_millis(20));
    }
}
fn connect(dir: &Path) -> UnixStream {
    let s = UnixStream::connect(dir.join(".daemon-ipc/s")).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    s.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
    s
}
fn hello(s: &mut UnixStream, rec: &Value) {
    send(s, json!({"type":"hello","version":1,"client_nonce":ID}));
    let r = receive(s);
    assert_eq!(r["instance_id"], rec["instance_id"]);
    assert_eq!(r["pid"], rec["pid"]);
    assert_eq!(r["client_nonce"], ID);
}
fn auth(s: &mut UnixStream) {
    send(
        s,
        json!({"type":"authorize","version":1,"admin_token":TOKEN}),
    );
    assert_eq!(receive(s)["result"], "ok");
}
fn closed(s: &mut UnixStream) {
    let mut b = [0; 1];
    assert_eq!(s.read(&mut b).unwrap(), 0);
}
fn wait_exit(child: &mut ChildGuard) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success());
            return;
        }
        assert!(Instant::now() < deadline, "daemon remained alive");
        thread::sleep(Duration::from_millis(25));
    }
}
#[test]
#[serial_test::serial]
fn daemon_requires_order_token_and_instance_before_accepting_one_stop() {
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-")
        .tempdir_in("/tmp")
        .unwrap();
    let (mut child, rec) = daemon(dir.path());
    let pid = child.0.id();
    assert_eq!(rec["pid"], pid);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("daemon.pid")).unwrap(),
        pid.to_string()
    );
    // The production client may perform a no-secret hello, but cannot promote
    // a macOS peer PID into a bound object or stop the real daemon.
    let stopped = Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
        .args(["daemon", "stop"])
        .env("AGENT2SSH_CONFIG_DIR", dir.path())
        .env("AGENT2SSH_DAEMON_ADDR", "127.0.0.1:9")
        .output()
        .unwrap();
    assert!(!stopped.status.success());
    assert!(
        String::from_utf8_lossy(&stopped.stderr).contains("IPC hello matched"),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    assert!(child.0.try_wait().unwrap().is_none());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("daemon.pid")).unwrap(),
        pid.to_string()
    );
    let mut wrong_order = connect(dir.path());
    send(
        &mut wrong_order,
        json!({"type":"stop","version":1,"instance_id":rec["instance_id"],"expect_pid":pid,"request_id":ID}),
    );
    closed(&mut wrong_order);
    let mut duplicate = connect(dir.path());
    let raw = br#"{"type":"hello","version":1,"version":1,"client_nonce":"0123456789abcdef0123456789abcdef"}"#;
    duplicate
        .write_all(&(raw.len() as u32).to_be_bytes())
        .unwrap();
    duplicate.write_all(raw).unwrap();
    closed(&mut duplicate);
    let mut bad_token = connect(dir.path());
    hello(&mut bad_token, &rec);
    send(
        &mut bad_token,
        json!({"type":"authorize","version":1,"admin_token":"wrong"}),
    );
    closed(&mut bad_token);
    let mut wrong_id = connect(dir.path());
    hello(&mut wrong_id, &rec);
    auth(&mut wrong_id);
    send(
        &mut wrong_id,
        json!({"type":"stop","version":1,"instance_id":"ffffffffffffffffffffffffffffffff","expect_pid":pid,"request_id":ID}),
    );
    closed(&mut wrong_id);
    assert!(child.0.try_wait().unwrap().is_none());
    let mut good = connect(dir.path());
    assert_eq!(
        agent2ssh::daemon_identity::macos_peer_pid(&good).unwrap(),
        pid
    );
    hello(&mut good, &rec);
    auth(&mut good);
    send(
        &mut good,
        json!({"type":"stop","version":1,"instance_id":rec["instance_id"],"expect_pid":pid,"request_id":ID}),
    );
    let receipt = receive(&mut good);
    assert_eq!(receipt["result"], "accepted");
    assert_eq!(receipt["request_id"], ID);
    assert_eq!(receipt["checked_pid"], pid);
    wait_exit(&mut child);
    assert!(!dir.path().join("daemon.pid").exists());
    assert!(!dir.path().join("daemon.instance.json").exists());
    assert!(!dir.path().join(".daemon-ipc/s").exists());
}
#[test]
#[serial_test::serial]
fn crash_is_retained_and_explicit_recovery_checks_instance() {
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-")
        .tempdir_in("/tmp")
        .unwrap();
    let (mut child, rec) = daemon(dir.path());
    let pid = child.0.id().to_string();
    let instance = rec["instance_id"].as_str().unwrap();
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let run = |id: &str| {
        Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
            .args([
                "daemon",
                "clear-stale",
                "--expect-pid",
                &pid,
                "--expect-instance",
                id,
            ])
            .env("AGENT2SSH_CONFIG_DIR", dir.path())
            .output()
            .unwrap()
    };
    assert!(!run("ffffffffffffffffffffffffffffffff").status.success());
    assert!(dir.path().join("daemon.pid").exists());
    assert!(dir.path().join(".daemon-ipc/s").exists());
    let result = run(instance);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!dir.path().join("daemon.pid").exists());
    assert!(!dir.path().join("daemon.instance.json").exists());
    assert!(!dir.path().join(".daemon-ipc/s").exists());
}

#[test]
#[serial_test::serial]
fn live_instance_cannot_be_recovered_and_partial_record_is_not_cleaned() {
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-")
        .tempdir_in("/tmp")
        .unwrap();
    let (mut child, record) = daemon(dir.path());
    let pid = child.0.id().to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
        .args([
            "daemon",
            "clear-stale",
            "--expect-pid",
            &pid,
            "--expect-instance",
            record["instance_id"].as_str().unwrap(),
        ])
        .env("AGENT2SSH_CONFIG_DIR", dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(child.0.try_wait().unwrap().is_none());
    assert!(dir.path().join("daemon.instance.json").exists());
    let absent = "2147483647";
    // Keep the IPC record but replace the PID with a definitely absent value;
    // PID-only cleanup must still refuse the partial/inconsistent state.
    std::fs::write(dir.path().join("daemon.pid"), absent).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
        .args(["daemon", "clear-stale", "--expect-pid", absent])
        .env("AGENT2SSH_CONFIG_DIR", dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(dir.path().join("daemon.pid").exists());
    assert!(dir.path().join("daemon.instance.json").exists());
    assert!(dir.path().join(".daemon-ipc/s").exists());
}

#[test]
#[serial_test::serial]
fn world_writable_directory_and_symlinked_socket_directory_are_refused() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-")
        .tempdir_in("/tmp")
        .unwrap();
    let addr = || {
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .to_string()
    };
    let launch = || {
        Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))
            .env("AGENT2SSH_CONFIG_DIR", dir.path())
            .env("AGENT2SSH_DAEMON_ADDR", addr())
            .env_remove("AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN")
            .output()
            .unwrap()
    };
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(!launch().status.success());
    assert!(!dir.path().join("daemon.pid").exists());
    assert!(!dir.path().join("daemon.token").exists());
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let target = tempfile::Builder::new()
        .prefix("a2s-target-")
        .tempdir_in("/tmp")
        .unwrap();
    symlink(target.path(), dir.path().join(".daemon-ipc")).unwrap();
    assert!(!launch().status.success());
    assert!(!dir.path().join("daemon.pid").exists());
    assert!(!target.path().join("s").exists());
}

#[test]
#[serial_test::serial]
fn startup_times_out_on_held_publication_lock_without_writing_records() {
    use fs2::FileExt;
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-")
        .tempdir_in("/tmp")
        .unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.path().join(".daemon-pid.lock"))
        .unwrap();
    lock.lock_exclusive().unwrap();
    let addr = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    let started = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))
        .env("AGENT2SSH_CONFIG_DIR", dir.path())
        .env("AGENT2SSH_DAEMON_ADDR", addr)
        .env_remove("AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(started.elapsed() < Duration::from_secs(8));
    assert!(String::from_utf8_lossy(&output.stderr).contains("timed out acquiring daemon PID lock"));
    assert!(!dir.path().join("daemon.pid").exists());
    assert!(!dir.path().join("daemon.instance.json").exists());
}

#[test]
#[serial_test::serial]
fn occupied_socket_is_preserved_and_never_taken_over() {
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-")
        .tempdir_in("/tmp")
        .unwrap();
    let ipc = dir.path().join(".daemon-ipc");
    std::fs::create_dir(&ipc).unwrap();
    std::fs::set_permissions(&ipc, std::fs::Permissions::from_mode(0o700)).unwrap();
    let listener = UnixListener::bind(ipc.join("s")).unwrap();
    let addr = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))
        .env("AGENT2SSH_CONFIG_DIR", dir.path())
        .env("AGENT2SSH_DAEMON_ADDR", addr)
        .env_remove("AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!dir.path().join("daemon.pid").exists());
    assert!(!dir.path().join("daemon.instance.json").exists());
    assert!(UnixStream::connect(ipc.join("s")).is_ok());
    drop(listener);
}

#[test]
#[serial_test::serial]
fn directory_acl_grants_are_rejected() {
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-")
        .tempdir_in("/tmp")
        .unwrap();
    let status = Command::new("chmod")
        .args(["+a", "everyone allow add_file"])
        .arg(dir.path())
        .status()
        .unwrap();
    assert!(status.success());
    let addr = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))
        .env("AGENT2SSH_CONFIG_DIR", dir.path())
        .env("AGENT2SSH_DAEMON_ADDR", addr)
        .env_remove("AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("directory ACL grants"));
    assert!(!dir.path().join("daemon.pid").exists());
    assert!(!dir.path().join("daemon.token").exists());
}

#[test]
#[serial_test::serial]
fn untrusted_parent_is_rejected_before_creating_config() {
    use std::os::unix::fs::PermissionsExt;
    let parent = tempfile::Builder::new()
        .prefix("a2s-ipc-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
    let config = parent.path().join("new-config");
    let addr = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))
        .env("AGENT2SSH_CONFIG_DIR", &config)
        .env("AGENT2SSH_DAEMON_ADDR", addr)
        .env_remove("AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!config.exists());
}

#[test]
#[serial_test::serial]
fn socket_path_too_long_is_refused_without_shortening_or_publishing() {
    let root = tempfile::Builder::new()
        .prefix("a2s-ipc-")
        .tempdir_in("/tmp")
        .unwrap();
    let config = root.path().join("x".repeat(100));
    std::fs::create_dir(&config).unwrap();
    let addr = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))
        .env("AGENT2SSH_CONFIG_DIR", &config)
        .env("AGENT2SSH_DAEMON_ADDR", addr)
        .env_remove("AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("path may be too long"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!config.join("daemon.pid").exists());
    assert!(!config.join("daemon.instance.json").exists());
}
