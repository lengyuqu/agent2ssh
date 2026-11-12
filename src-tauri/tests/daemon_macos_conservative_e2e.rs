#![cfg(all(feature = "daemon", target_os = "macos"))]
//! macOS conservative stop/recovery against real isolated processes.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

const TOKEN: &str = "synthetic-macos-test-token";
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn cli(dir: &Path, addr: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
        .args(args)
        .env("AGENT2SSH_CONFIG_DIR", dir)
        .env("AGENT2SSH_DAEMON_ADDR", addr)
        .env_remove("AGENT2SSH_MASTER_PASSWORD")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}
fn body(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_shutdown_trigger(dir: &Path, trigger: &str) {
    let raw = std::fs::read_to_string(dir.join("daemon-output.log")).unwrap();
    assert!(
        raw.lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .any(|entry| entry["fields"]["message"]
                == "shutdown signal received, draining WS connections..."
                && entry["fields"]["trigger"] == trigger),
        "missing shutdown trigger {trigger} in test diagnostics"
    );
}
fn request(addr: &str, token: &str) -> String {
    request_body(addr, token, "")
}
fn request_body(addr: &str, token: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(stream, "POST /shutdown HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}
fn daemon(dir: &Path, addr: &str, compatibility: bool) -> ChildGuard {
    std::fs::write(dir.join("daemon.token"), TOKEN).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"));
    let log = std::fs::File::create(dir.join("daemon-output.log")).unwrap();
    cmd.env("AGENT2SSH_CONFIG_DIR", dir)
        .env("AGENT2SSH_DAEMON_ADDR", addr)
        .env_remove("AGENT2SSH_MASTER_PASSWORD")
        .env_remove("AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN")
        .env("AGENT2SSH_LOG", "info")
        .env("AGENT2SSH_LOG_FORMAT", "json")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::null());
    if compatibility {
        cmd.env("AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN", "1");
    }
    let child = ChildGuard(cmd.spawn().unwrap());
    let until = Instant::now() + Duration::from_secs(20);
    while Instant::now() < until {
        if dir.join("daemon.pid").exists()
            && agent2ssh::daemon_activity::direct_daemon_request(addr, "GET", "/health", None)
                .is_ok_and(|response| {
                    response.status == 200
                        && serde_json::from_str::<serde_json::Value>(&response.body)
                            .ok()
                            .and_then(|body| body["pid"].as_u64())
                            == Some(u64::from(child.0.id()))
                })
        {
            return child;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    panic!("daemon did not become ready");
}
fn address() -> String {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string()
}

#[test]
fn missing_pid_record_is_reported_as_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let addr = address();
    let stop = cli(dir.path(), &addr, &["daemon", "stop"]);
    assert!(!stop.status.success());
    assert!(body(&stop).contains("state are unknown"), "{}", body(&stop));
    let status = cli(dir.path(), &addr, &["daemon", "status"]);
    assert!(status.status.success());
    assert!(
        body(&status).contains("state is unknown"),
        "{}",
        body(&status)
    );
}

#[test]
fn default_refuses_http_cli_stop_and_restart_without_touching_process() {
    let dir = tempfile::tempdir().unwrap();
    let addr = address();
    let mut child = daemon(dir.path(), &addr, false);
    let pid = child.0.id();
    let before = std::fs::read(dir.path().join("daemon.pid")).unwrap();
    let unauthorized = request(&addr, "wrong-synthetic-token");
    assert!(unauthorized.starts_with("HTTP/1.1 401"), "{unauthorized}");
    let response = request(&addr, TOKEN);
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    assert!(response.contains("unbound HTTP shutdown is disabled"));
    let opt_in_client = cli(
        dir.path(),
        &addr,
        &["daemon", "stop", "--allow-unbound-graceful"],
    );
    assert!(!opt_in_client.status.success());
    assert!(body(&opt_in_client).contains("HTTP 403"));
    assert!(!body(&opt_in_client).contains(TOKEN));
    assert!(child.0.try_wait().unwrap().is_none());
    assert_eq!(
        std::fs::read(dir.path().join("daemon.pid")).unwrap(),
        before
    );
    for args in [&["daemon", "stop"][..], &["daemon", "restart"][..]] {
        let out = cli(dir.path(), &addr, args);
        assert!(!out.status.success());
        assert!(
            body(&out).contains("no shutdown or signal was sent"),
            "{}",
            body(&out)
        );
        assert!(child.0.try_wait().unwrap().is_none());
        assert_eq!(
            std::fs::read(dir.path().join("daemon.pid")).unwrap(),
            before
        );
    }
    assert_eq!(
        std::str::from_utf8(&before).unwrap().trim(),
        pid.to_string()
    );
}

#[test]
fn explicit_cleanup_refuses_live_mismatch_and_all_automatic_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let addr = address();
    let record = dir.path().join("daemon.pid");
    let live = std::process::id();
    std::fs::write(&record, live.to_string()).unwrap();
    let actual = live.to_string();
    let out = cli(
        dir.path(),
        &addr,
        &["daemon", "clear-stale", "--expect-pid", &actual],
    );
    assert!(!out.status.success());
    assert!(body(&out).contains("exists or cannot be observed"));
    let mismatch = cli(
        dir.path(),
        &addr,
        &["daemon", "clear-stale", "--expect-pid", "2147483647"],
    );
    assert!(!mismatch.status.success());
    assert!(body(&mismatch).contains("PID file changed"));
    assert_eq!(std::fs::read_to_string(&record).unwrap(), actual);
    std::fs::write(&record, "1").unwrap();
    let protected = cli(
        dir.path(),
        &addr,
        &["daemon", "clear-stale", "--expect-pid", "1"],
    );
    assert!(!protected.status.success());
    assert!(body(&protected).contains("exists or cannot be observed"));
    assert_eq!(std::fs::read_to_string(&record).unwrap(), "1");
    let absent = "2147483647";
    std::fs::write(&record, absent).unwrap();
    let out = cli(dir.path(), &addr, &["daemon", "rotate-token"]);
    assert!(!out.status.success());
    assert!(body(&out).contains("token not rotated"));
    let out = cli(dir.path(), &addr, &["daemon", "start"]);
    assert!(!out.status.success());
    assert!(body(&out).contains("PID file retained"));
    assert_eq!(std::fs::read_to_string(&record).unwrap(), absent);
    let direct = Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))
        .env("AGENT2SSH_CONFIG_DIR", dir.path())
        .env("AGENT2SSH_DAEMON_ADDR", &addr)
        .env_remove("AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN")
        .output()
        .unwrap();
    assert!(!direct.status.success());
    assert!(body(&direct).contains("automatic replacement is disabled"));
    assert_eq!(std::fs::read_to_string(&record).unwrap(), absent);
    let out = cli(
        dir.path(),
        &addr,
        &["daemon", "clear-stale", "--expect-pid", absent],
    );
    assert!(out.status.success(), "{}", body(&out));
    assert!(!record.exists());
    let target = dir.path().join("external-record");
    std::fs::write(&target, absent).unwrap();
    symlink(&target, &record).unwrap();
    let out = cli(
        dir.path(),
        &addr,
        &["daemon", "clear-stale", "--expect-pid", absent],
    );
    assert!(!out.status.success());
    assert!(body(&out).contains("not a regular file"), "{}", body(&out));
    assert!(std::fs::symlink_metadata(&record)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), absent);
}

#[test]
fn explicit_unbound_attempt_does_not_signal_or_clean_a_forged_process() {
    let dir = tempfile::tempdir().unwrap();
    let named = dir.path().join("agent2ssh-daemon");
    symlink("/bin/sleep", &named).unwrap();
    let mut target = ChildGuard(Command::new(named).arg("30").spawn().unwrap());
    let pid = target.0.id();
    std::fs::write(dir.path().join("daemon.pid"), pid.to_string()).unwrap();
    std::fs::write(dir.path().join("daemon.token"), TOKEN).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let canonical = dir.path().canonicalize().unwrap().display().to_string();
    let posts = Arc::new(AtomicUsize::new(0));
    let seen = posts.clone();
    let connections = Arc::new(AtomicUsize::new(0));
    let seen_connections = connections.clone();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let deadline = Instant::now() + Duration::from_secs(10);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(value) => break value,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(error) => panic!("waiting for stub request: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            seen_connections.fetch_add(1, Ordering::SeqCst);
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 1024];
            while !request.windows(4).any(|b| b == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
            }
            let post = request.starts_with(b"POST /shutdown ");
            if post {
                seen.fetch_add(1, Ordering::SeqCst);
            }
            let payload = if post {
                "{\"ok\":true}".to_string()
            } else {
                serde_json::json!({"ok":true,"pid":pid,"config_dir":canonical}).to_string()
            };
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                payload.len(),
                payload
            )
            .unwrap();
        }
    });
    let default = cli(dir.path(), &addr, &["daemon", "stop"]);
    assert!(!default.status.success());
    assert!(body(&default).contains("no shutdown or signal was sent"));
    assert_eq!(connections.load(Ordering::SeqCst), 0);
    let out = cli(
        dir.path(),
        &addr,
        &["daemon", "stop", "--allow-unbound-graceful"],
    );
    server.join().unwrap();
    assert!(
        !out.status.success(),
        "an unverified exit must not report success: {}",
        body(&out)
    );
    assert!(
        body(&out).contains("Process exit is not verified")
            || body(&out).contains("process exit is not verified")
    );
    assert!(body(&out).contains("wrong daemon"));
    assert_eq!(posts.load(Ordering::SeqCst), 1);
    assert!(target.0.try_wait().unwrap().is_none());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("daemon.pid")).unwrap(),
        pid.to_string()
    );
}

#[test]
fn real_daemon_legacy_opt_in_accepts_only_when_explicitly_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let addr = address();
    let mut child = daemon(dir.path(), &addr, true);
    let wrong = request_body(
        &addr,
        TOKEN,
        &format!("{{\"expect_pid\":{}}}", child.0.id().wrapping_add(1)),
    );
    assert!(wrong.starts_with("HTTP/1.1 409"), "{wrong}");
    let malformed = request_body(&addr, TOKEN, r#"{"expect_pid":null}"#);
    assert!(malformed.starts_with("HTTP/1.1 400"), "{malformed}");
    assert!(child.0.try_wait().unwrap().is_none());
    let out = cli(
        dir.path(),
        &addr,
        &["daemon", "stop", "--allow-unbound-graceful"],
    );
    assert!(
        !out.status.success(),
        "an unverified exit must not report success: {}",
        body(&out)
    );
    assert!(body(&out).contains("not verified"));
    let until = Instant::now() + Duration::from_secs(10);
    let mut exited = false;
    while Instant::now() < until {
        if child.0.try_wait().unwrap().is_some() {
            exited = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    assert!(
        exited,
        "daemon did not exit after accepting explicit request"
    );
    assert_shutdown_trigger(dir.path(), "http");
}

#[test]
fn external_sigterm_is_recorded_as_the_shutdown_trigger() {
    let dir = tempfile::tempdir().unwrap();
    let addr = address();
    let mut child = daemon(dir.path(), &addr, false);
    assert_eq!(
        unsafe { libc::kill(child.0.id() as libc::pid_t, libc::SIGTERM) },
        0
    );
    let until = Instant::now() + Duration::from_secs(10);
    let mut exited = false;
    while Instant::now() < until {
        if child.0.try_wait().unwrap().is_some() {
            exited = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    assert!(exited, "daemon did not exit after SIGTERM");
    assert_shutdown_trigger(dir.path(), "sigterm");
}

#[test]
fn explicit_attempt_fails_when_listener_switches_from_a_to_b() {
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let a_addr = address();
    let b_addr = address();
    let mut a = daemon(a_dir.path(), &a_addr, true);
    let mut b = daemon(b_dir.path(), &b_addr, true);
    let a_pid = a.0.id();
    let b_pid = b.0.id();
    assert_ne!(a_pid, b_pid);
    let front = TcpListener::bind("127.0.0.1:0").unwrap();
    front.set_nonblocking(true).unwrap();
    let proxy_addr = front.local_addr().unwrap().to_string();
    let proxy = std::thread::spawn(move || {
        for target in [a_addr, b_addr] {
            let deadline = Instant::now() + Duration::from_secs(10);
            let (mut client, _) = loop {
                match front.accept() {
                    Ok(value) => break value,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(error) => panic!("proxy accept: {error}"),
                }
            };
            client.set_nonblocking(false).unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 2048];
            let header_end = loop {
                let n = client.read(&mut buf).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buf[..n]);
                if let Some(i) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
            };
            let headers = std::str::from_utf8(&request[..header_end]).unwrap();
            let length: usize = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            while request.len() < header_end + length {
                let n = client.read(&mut buf).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buf[..n]);
            }
            let mut upstream = TcpStream::connect(target).unwrap();
            upstream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            upstream.write_all(&request).unwrap();
            let mut reply = Vec::new();
            upstream.read_to_end(&mut reply).unwrap();
            client.write_all(&reply).unwrap();
        }
    });
    let out = cli(
        a_dir.path(),
        &proxy_addr,
        &["daemon", "stop", "--allow-unbound-graceful"],
    );
    proxy.join().unwrap();
    assert!(!out.status.success());
    assert!(body(&out).contains("HTTP 409"), "{}", body(&out));
    assert!(a.0.try_wait().unwrap().is_none());
    assert!(b.0.try_wait().unwrap().is_none());
    assert_eq!(
        std::fs::read_to_string(a_dir.path().join("daemon.pid")).unwrap(),
        a_pid.to_string()
    );
    assert_eq!(
        std::fs::read_to_string(b_dir.path().join("daemon.pid")).unwrap(),
        b_pid.to_string()
    );
}

#[test]
fn compatibility_refuses_mismatched_health_and_reports_lost_or_invalid_replies() {
    for scenario in ["mismatch", "record-change", "invalid", "lost"] {
        let dir = tempfile::tempdir().unwrap();
        let named = dir.path().join("agent2ssh-daemon");
        symlink("/bin/sleep", &named).unwrap();
        let mut target = ChildGuard(Command::new(named).arg("30").spawn().unwrap());
        let pid = target.0.id();
        std::fs::write(dir.path().join("daemon.pid"), pid.to_string()).unwrap();
        std::fs::write(dir.path().join("daemon.token"), TOKEN).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let front = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = front.local_addr().unwrap().to_string();
        front.set_nonblocking(true).unwrap();
        let canonical = dir.path().canonicalize().unwrap().display().to_string();
        let pid_record = dir.path().join("daemon.pid");
        let server = std::thread::spawn(move || {
            let count = if scenario == "mismatch" || scenario == "record-change" {
                1
            } else {
                2
            };
            for index in 0..count {
                let deadline = Instant::now() + Duration::from_secs(8);
                let (mut stream, _) = loop {
                    match front.accept() {
                        Ok(value) => break value,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(10))
                        }
                        Err(error) => panic!("stub accept: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut bytes = [0; 2048];
                let header_end = loop {
                    let n = stream.read(&mut bytes).unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&bytes[..n]);
                    if let Some(i) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i + 4;
                    }
                };
                let headers = std::str::from_utf8(&request[..header_end]).unwrap();
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                while request.len() < header_end + length {
                    let n = stream.read(&mut bytes).unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&bytes[..n]);
                }
                if index == 1 && scenario == "lost" {
                    continue;
                }
                if index == 0 && scenario == "record-change" {
                    std::fs::write(&pid_record, format!("{pid}\n")).unwrap();
                }
                let payload = if index == 0 {
                    serde_json::json!({"ok":true,"pid":if scenario == "mismatch" {pid+1} else {pid},"config_dir":canonical}).to_string()
                } else {
                    "{\"ok\":false}".to_string()
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    payload.len(),
                    payload
                )
                .unwrap();
            }
        });
        let out = cli(
            dir.path(),
            &addr,
            &["daemon", "stop", "--allow-unbound-graceful"],
        );
        server.join().unwrap();
        assert!(!out.status.success(), "{scenario}: {}", body(&out));
        let expected = match scenario {
            "mismatch" => "does not match",
            "record-change" => "record changed during discovery",
            "invalid" => "not a valid acceptance",
            _ => "outcome is unknown",
        };
        assert!(body(&out).contains(expected), "{scenario}: {}", body(&out));
        assert!(target.0.try_wait().unwrap().is_none());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("daemon.pid")).unwrap(),
            if scenario == "record-change" {
                format!("{pid}\n")
            } else {
                pid.to_string()
            }
        );
    }
}
