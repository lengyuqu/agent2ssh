#![cfg(all(target_os = "macos", feature = "daemon"))]
//! Isolated macOS start/reuse checks: a forged HTTP endpoint must never see
//! the synthetic admin token. No real user configuration is used.
use agent2ssh::daemon_control::{
    daemon_matches_config, daemon_pid_state, start_daemon_background, DaemonPidState,
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    os::unix::net::UnixStream,
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
const TOKEN: &str = "synthetic-start-token-8f1a";
const NONCE: &str = "0123456789abcdef0123456789abcdef";
struct EnvRestore {
    dir: Option<std::ffi::OsString>,
    addr: Option<std::ffi::OsString>,
}
impl EnvRestore {
    fn set(dir: &Path, addr: &str) -> Self {
        let old = Self {
            dir: std::env::var_os("AGENT2SSH_CONFIG_DIR"),
            addr: std::env::var_os("AGENT2SSH_DAEMON_ADDR"),
        };
        std::env::set_var("AGENT2SSH_CONFIG_DIR", dir);
        std::env::set_var("AGENT2SSH_DAEMON_ADDR", addr);
        old
    }
}
impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in [
            ("AGENT2SSH_CONFIG_DIR", self.dir.take()),
            ("AGENT2SSH_DAEMON_ADDR", self.addr.take()),
        ] {
            if let Some(v) = value {
                std::env::set_var(key, v)
            } else {
                std::env::remove_var(key)
            }
        }
    }
}
struct Fake {
    addr: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Fake {
    fn start(pid: u32, config: &Path) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let config = std::fs::canonicalize(config).unwrap().display().to_string();
        let thread = std::thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut s, _)) => {
                        s.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                        let mut buf = [0u8; 4096];
                        let n = s.read(&mut buf).unwrap_or(0);
                        captured
                            .lock()
                            .unwrap()
                            .push(String::from_utf8_lossy(&buf[..n]).to_string());
                        let body = serde_json::json!({"ok":true,"pid":pid,"config_dir":config})
                            .to_string();
                        let _ = write!(
                            s,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("fake listener: {e}"),
                }
            }
        });
        Self {
            addr,
            requests,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Fake {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            t.join().unwrap();
        }
    }
}
#[test]
#[serial_test::serial]
fn forged_live_pid_cannot_receive_token_or_be_reused() {
    let dir = tempfile::Builder::new()
        .prefix("a2s-start-")
        .tempdir_in("/tmp")
        .unwrap();
    let pid = std::process::id();
    std::fs::write(dir.path().join("daemon.pid"), pid.to_string()).unwrap();
    std::fs::write(dir.path().join("daemon.token"), TOKEN).unwrap();
    let fake = Fake::start(pid, dir.path());
    let _env = EnvRestore::set(dir.path(), &fake.addr);
    assert!(!daemon_matches_config(pid));
    assert_eq!(daemon_pid_state(pid), DaemonPidState::Unverified);
    let result = start_daemon_background(Path::new(env!("CARGO_BIN_EXE_agent2ssh-daemon")));
    assert!(result.is_err());
    let cli = Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
        .args(["daemon", "start"])
        .env("AGENT2SSH_CONFIG_DIR", dir.path())
        .env("AGENT2SSH_DAEMON_ADDR", &fake.addr)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!cli.status.success());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("daemon.pid")).unwrap(),
        pid.to_string()
    );
    let requests = fake.requests.lock().unwrap();
    assert!(!requests.is_empty());
    for request in requests.iter() {
        assert!(!request.contains(TOKEN));
        assert!(!request.to_ascii_lowercase().contains("authorization:"));
        assert!(request.starts_with("GET /health "));
    }
}
fn send(s: &mut UnixStream, value: serde_json::Value) {
    let b = serde_json::to_vec(&value).unwrap();
    s.write_all(&(b.len() as u32).to_be_bytes()).unwrap();
    s.write_all(&b).unwrap();
}
fn read(s: &mut UnixStream) -> serde_json::Value {
    let mut h = [0; 4];
    s.read_exact(&mut h).unwrap();
    let mut b = vec![0; u32::from_be_bytes(h) as usize];
    s.read_exact(&mut b).unwrap();
    serde_json::from_slice(&b).unwrap()
}
struct TestDaemonCleanup {
    dir: std::path::PathBuf,
    pid: u32,
}
impl Drop for TestDaemonCleanup {
    fn drop(&mut self) {
        // On failure, stop only the synthetic daemon over its isolated socket;
        // do not fall back to a numeric PID signal.
        let _ = (|| -> std::io::Result<()> {
            let record: serde_json::Value =
                serde_json::from_slice(&std::fs::read(self.dir.join("daemon.instance.json"))?)?;
            let mut stream = UnixStream::connect(self.dir.join(".daemon-ipc/s"))?;
            stream.set_read_timeout(Some(Duration::from_secs(1)))?;
            stream.set_write_timeout(Some(Duration::from_secs(1)))?;
            let exchange =
                |stream: &mut UnixStream, value: serde_json::Value| -> std::io::Result<()> {
                    let bytes = serde_json::to_vec(&value)?;
                    stream.write_all(&(bytes.len() as u32).to_be_bytes())?;
                    stream.write_all(&bytes)?;
                    let mut header = [0; 4];
                    stream.read_exact(&mut header)?;
                    let len = u32::from_be_bytes(header) as usize;
                    if len > 16 * 1024 {
                        return Err(std::io::ErrorKind::InvalidData.into());
                    }
                    let mut body = vec![0; len];
                    stream.read_exact(&mut body)?;
                    Ok(())
                };
            exchange(
                &mut stream,
                serde_json::json!({"type":"hello","version":1,"client_nonce":NONCE}),
            )?;
            exchange(
                &mut stream,
                serde_json::json!({"type":"authorize","version":1,"admin_token":TOKEN}),
            )?;
            exchange(
                &mut stream,
                serde_json::json!({"type":"stop","version":1,"instance_id":record["instance_id"],"expect_pid":self.pid,"request_id":NONCE}),
            )
        })();
    }
}
#[test]
#[serial_test::serial]
fn real_spawn_reports_unbound_and_can_be_cleaned_up_over_test_ipc() {
    let dir = tempfile::Builder::new()
        .prefix("a2s-start-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::write(dir.path().join("daemon.token"), TOKEN).unwrap();
    let addr = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    let _env = EnvRestore::set(dir.path(), &addr);
    let started =
        start_daemon_background(Path::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))).unwrap();
    let _cleanup = TestDaemonCleanup {
        dir: dir.path().to_path_buf(),
        pid: started.pid,
    };
    assert!(!started.endpoint_authenticated);
    assert_eq!(daemon_pid_state(started.pid), DaemonPidState::Unverified);
    let instance: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("daemon.instance.json")).unwrap())
            .unwrap();
    let mut stream = UnixStream::connect(dir.path().join(".daemon-ipc/s")).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    send(
        &mut stream,
        serde_json::json!({"type":"hello","version":1,"client_nonce":NONCE}),
    );
    assert_eq!(read(&mut stream)["pid"], started.pid);
    send(
        &mut stream,
        serde_json::json!({"type":"authorize","version":1,"admin_token":TOKEN}),
    );
    assert_eq!(read(&mut stream)["result"], "ok");
    send(
        &mut stream,
        serde_json::json!({"type":"stop","version":1,"instance_id":instance["instance_id"],"expect_pid":started.pid,"request_id":NONCE}),
    );
    assert_eq!(read(&mut stream)["result"], "accepted");
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while dir.path().join("daemon.pid").exists() && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(!dir.path().join("daemon.pid").exists());
}

#[test]
#[serial_test::serial]
fn forged_child_health_without_published_instance_never_receives_token() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::Builder::new()
        .prefix("a2s-start-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::write(dir.path().join("daemon.token"), TOKEN).unwrap();
    let stub = dir.path().join("fake-child");
    std::fs::write(&stub, "#!/bin/sh\nprintf '%s' \"$$\" > \"$AGENT2SSH_CONFIG_DIR/child.pid\"\nprintf '%s' \"$$\" > \"$AGENT2SSH_CONFIG_DIR/daemon.pid\"\nexec /bin/sleep 15\n").unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o700)).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    listener.set_nonblocking(true).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let done = stop.clone();
    let child_pid_path = dir.path().join("child.pid");
    let config = std::fs::canonicalize(dir.path())
        .unwrap()
        .display()
        .to_string();
    let thread = std::thread::spawn(move || {
        while !done.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut s, _)) => {
                    s.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                    let mut buf = [0u8; 4096];
                    let n = s.read(&mut buf).unwrap_or(0);
                    captured
                        .lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&buf[..n]).to_string());
                    let body = std::fs::read_to_string(&child_pid_path)
                        .ok()
                        .and_then(|p| p.parse::<u32>().ok())
                        .map(|pid| {
                            serde_json::json!({"ok":true,"pid":pid,"config_dir":config}).to_string()
                        })
                        .unwrap_or_else(|| "{}".to_string());
                    let _ = write!(
                        s,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(e) => panic!("fake listener: {e}"),
            }
        }
    });
    let _env = EnvRestore::set(dir.path(), &addr);
    let result = start_daemon_background(&stub);
    stop.store(true, Ordering::SeqCst);
    thread.join().unwrap();
    assert!(result.is_err());
    assert!(!dir.path().join("daemon.pid").exists());
    let requests = requests.lock().unwrap();
    assert!(requests.len() > 1);
    assert!(requests
        .iter()
        .all(|r| !r.contains(TOKEN) && !r.to_ascii_lowercase().contains("authorization:")));
}

#[test]
#[serial_test::serial]
fn cli_starts_a_child_but_refuses_unbound_reuse() {
    let dir = tempfile::Builder::new()
        .prefix("a2s-start-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::write(dir.path().join("daemon.token"), TOKEN).unwrap();
    let addr = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
            .args(["daemon", "start"])
            .env("AGENT2SSH_CONFIG_DIR", dir.path())
            .env("AGENT2SSH_DAEMON_ADDR", &addr)
            .env_remove("AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let first = run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let pid = std::fs::read_to_string(dir.path().join("daemon.pid"))
        .unwrap()
        .parse::<u32>()
        .unwrap();
    let cleanup = TestDaemonCleanup {
        dir: dir.path().to_path_buf(),
        pid,
    };
    assert!(String::from_utf8_lossy(&first.stdout).contains("unbound and not authenticated"));
    let again = run();
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("not reused"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("daemon.pid")).unwrap(),
        pid.to_string()
    );
    drop(cleanup);
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while dir.path().join("daemon.pid").exists() && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(!dir.path().join("daemon.pid").exists());
}
