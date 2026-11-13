#![cfg(all(target_os = "macos", feature = "daemon"))]
//! The production client may send only hello until object binding is proved.
use agent2ssh::daemon_identity::{macos, macos_client, read_hello, write_hello, HelloFrame};
use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    time::{Duration, Instant},
};

fn accept(listener: &UnixListener) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((stream, _)) => return stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(e) => panic!("accept: {e}"),
        }
    }
}
fn isolate_helper(command: &mut std::process::Command) {
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name.starts_with("DYLD_")
            || name.starts_with("AGENT2SSH_")
            || name.starts_with("A2S_EXEC_PEER_")
        {
            command.env_remove(key);
        }
    }
}
fn fake<F>(dir: &Path, action: F) -> (macos::Publication, std::thread::JoinHandle<Vec<u8>>)
where
    F: FnOnce(&mut UnixStream, &str, u32, &Path) + Send + 'static,
{
    let publication = macos::publish(dir, std::process::id()).unwrap();
    let listener = publication.listener.try_clone().unwrap();
    let id = publication.instance.instance_id.clone();
    let path = dir.to_path_buf();
    let thread = std::thread::spawn(move || {
        let mut stream = accept(&listener);
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        action(&mut stream, &id, std::process::id(), &path);
        let mut rest = Vec::new();
        stream.read_to_end(&mut rest).unwrap();
        rest
    });
    (publication, thread)
}
fn hello(stream: &mut UnixStream, id: &str, pid: u32) {
    let frame = read_hello(&mut *stream).unwrap();
    let HelloFrame::Hello { client_nonce, .. } = frame else {
        panic!("expected hello")
    };
    write_hello(
        stream,
        &HelloFrame::HelloOk {
            version: 1,
            client_nonce,
            instance_id: id.into(),
            pid,
        },
    )
    .unwrap();
}
#[test]
fn matching_peer_remains_unbound_and_receives_no_token_or_stop() {
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-client-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::write(
        dir.path().join("daemon.token"),
        b"synthetic-token-must-not-be-sent",
    )
    .unwrap();
    let (publication, server) = fake(dir.path(), |stream, id, pid, _| hello(stream, id, pid));
    let observation = macos_client::inspect_unbound(dir.path(), std::process::id()).unwrap();
    assert_eq!(observation.peer_pid, std::process::id());
    observation.revalidate().unwrap();
    drop(observation);
    assert!(server.join().unwrap().is_empty());
    assert!(publication.cleanup().unwrap());
}
#[test]
fn replaced_identical_record_is_detected_before_any_authorization() {
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-client-")
        .tempdir_in("/tmp")
        .unwrap();
    let (publication, server) = fake(dir.path(), |stream, id, pid, dir| {
        let frame = read_hello(&mut *stream).unwrap();
        let HelloFrame::Hello { client_nonce, .. } = frame else {
            panic!("expected hello")
        };
        let record = dir.join("daemon.instance.json");
        let tmp = dir.join("replacement");
        std::fs::write(&tmp, std::fs::read(&record).unwrap()).unwrap();
        std::fs::rename(tmp, record).unwrap();
        write_hello(
            stream,
            &HelloFrame::HelloOk {
                version: 1,
                client_nonce,
                instance_id: id.into(),
                pid,
            },
        )
        .unwrap();
    });
    let error = macos_client::inspect_unbound(dir.path(), std::process::id()).unwrap_err();
    assert!(error.to_string().contains("changed"), "{error:#}");
    assert!(server.join().unwrap().is_empty());
    // A changed record is intentionally retained.
    drop(publication);
}
#[test]
fn stalled_hello_has_an_overall_deadline_and_receives_no_authorization() {
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-client-")
        .tempdir_in("/tmp")
        .unwrap();
    let (publication, server) = fake(dir.path(), |stream, _, _, _| {
        assert!(matches!(
            read_hello(&mut *stream).unwrap(),
            HelloFrame::Hello { .. }
        ));
        stream.write_all(&16u32.to_be_bytes()).unwrap();
        for _ in 0..10 {
            if stream.write_all(b"x").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    });
    let start = Instant::now();
    assert!(macos_client::inspect_unbound(dir.path(), std::process::id()).is_err());
    assert!(start.elapsed() < Duration::from_secs(3));
    assert!(server.join().unwrap().is_empty());
    assert!(publication.cleanup().unwrap());
}

#[test]
fn mismatched_hello_is_rejected_without_authorization() {
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-client-")
        .tempdir_in("/tmp")
        .unwrap();
    let (publication, server) = fake(dir.path(), |stream, _, pid, _| {
        let frame = read_hello(&mut *stream).unwrap();
        let HelloFrame::Hello { client_nonce, .. } = frame else {
            panic!("expected hello")
        };
        write_hello(
            stream,
            &HelloFrame::HelloOk {
                version: 1,
                client_nonce,
                instance_id: "ffffffffffffffffffffffffffffffff".into(),
                pid,
            },
        )
        .unwrap();
    });
    let result = macos_client::inspect_unbound(dir.path(), std::process::id());
    assert!(result.is_err());
    assert!(server.join().unwrap().is_empty());
    assert!(publication.cleanup().unwrap());
}

#[test]
fn replacement_socket_is_detected_without_retry_or_authorization() {
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-client-")
        .tempdir_in("/tmp")
        .unwrap();
    let (publication, server) = fake(dir.path(), |stream, id, pid, dir| {
        let frame = read_hello(&mut *stream).unwrap();
        let HelloFrame::Hello { client_nonce, .. } = frame else {
            panic!("expected hello")
        };
        let path = dir.join(".daemon-ipc/s");
        std::fs::remove_file(&path).unwrap();
        let replacement = UnixListener::bind(&path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_hello(
            &mut *stream,
            &HelloFrame::HelloOk {
                version: 1,
                client_nonce,
                instance_id: id.into(),
                pid,
            },
        )
        .unwrap();
        // Keep the replacement alive until the original connection closes;
        // a retry would connect to it rather than the original server.
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        let mut unexpected = [0; 1];
        assert_eq!(stream.read(&mut unexpected).unwrap(), 0);
        replacement.set_nonblocking(true).unwrap();
        assert_eq!(
            replacement.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    });
    let result = macos_client::inspect_unbound(dir.path(), std::process::id());
    assert!(result.is_err());
    assert!(server.join().unwrap().is_empty());
    // The new socket is not the one owned by this publication.
    assert!(!publication.cleanup().unwrap());
}

#[test]
fn other_pid_server_helper() {
    let Ok(dir) = std::env::var("A2S_OTHER_PID_SOCKET_TEST") else {
        return;
    };
    let socket = Path::new(&dir).join(".daemon-ipc/s");
    let listener = UnixListener::bind(&socket).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    listener.set_nonblocking(true).unwrap();
    std::fs::write(Path::new(&dir).join("child-ready"), b"ready").unwrap();
    let mut stream = accept(&listener);
    stream.set_nonblocking(false).unwrap();
    let mut data = Vec::new();
    if let Err(error) = stream.read_to_end(&mut data) {
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
    }
    assert!(
        data.is_empty(),
        "peer PID mismatch must be refused before hello or authorization"
    );
}

#[test]
fn a_different_peer_pid_is_refused_before_hello() {
    use std::process::{Command, Stdio};
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-client-")
        .tempdir_in("/tmp")
        .unwrap();
    let publication = macos::publish(dir.path(), std::process::id()).unwrap();
    std::fs::remove_file(dir.path().join(".daemon-ipc/s")).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "other_pid_server_helper", "--nocapture"])
        .env("A2S_OTHER_PID_SOCKET_TEST", dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !dir.path().join("child-ready").exists() && Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("fake server exited: {status}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !dir.path().join("child-ready").exists() {
        let _ = child.kill();
        let _ = child.wait();
        panic!("fake server did not start");
    }
    let result = macos_client::inspect_unbound(dir.path(), std::process::id());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("peer PID conflicts"));
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!publication.cleanup().unwrap());
}

#[test]
fn observation_rejects_a_record_replaced_after_hello() {
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-client-")
        .tempdir_in("/tmp")
        .unwrap();
    let (publication, server) = fake(dir.path(), |stream, id, pid, _| hello(stream, id, pid));
    let observation = macos_client::inspect_unbound(dir.path(), std::process::id()).unwrap();
    let record = dir.path().join("daemon.instance.json");
    let replacement = dir.path().join("same-bytes-replacement");
    std::fs::write(&replacement, std::fs::read(&record).unwrap()).unwrap();
    std::fs::rename(replacement, record).unwrap();
    assert!(observation.revalidate().is_err());
    drop(observation);
    assert!(server.join().unwrap().is_empty());
    drop(publication);
}

#[test]
fn revalidation_refuses_unsolicited_data_or_peer_closure() {
    use std::{net::Shutdown, sync::mpsc};
    for close in [false, true] {
        let dir = tempfile::Builder::new()
            .prefix("a2s-ipc-client-")
            .tempdir_in("/tmp")
            .unwrap();
        let (go_tx, go_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let (publication, server) = fake(dir.path(), move |stream, id, pid, _| {
            hello(stream, id, pid);
            go_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            if close {
                stream.shutdown(Shutdown::Write).unwrap();
            } else {
                stream.write_all(b"x").unwrap();
            }
            done_tx.send(()).unwrap();
        });
        let observation = macos_client::inspect_unbound(dir.path(), std::process::id()).unwrap();
        go_tx.send(()).unwrap();
        done_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let error = observation
            .revalidate()
            .expect_err("changed connection must be refused");
        assert!(
            error.to_string().contains("closed or sent unexpected data"),
            "{error:#}"
        );
        drop(observation);
        assert!(server.join().unwrap().is_empty());
        assert!(publication.cleanup().unwrap());
    }
}

// These helpers exec the test binary itself, preserving only the synthetic
// accepted socket. No real daemon or user config is opened.
#[test]
fn exec_peer_before_helper() {
    use std::{os::fd::AsRawFd, os::unix::process::CommandExt, process::Command};
    let Ok(dir) = std::env::var("A2S_EXEC_PEER_DIR") else {
        return;
    };
    let publication = macos::publish(Path::new(&dir), std::process::id()).unwrap();
    let listener = publication.listener.try_clone().unwrap();
    std::fs::write(Path::new(&dir).join("exec-ready"), b"ready").unwrap();
    let mut stream = accept(&listener);
    stream.set_nonblocking(false).unwrap();
    let HelloFrame::Hello { client_nonce, .. } = read_hello(&mut stream).unwrap() else {
        panic!("expected hello")
    };
    let post_hello = std::env::var_os("A2S_EXEC_PEER_POST_HELLO").is_some();
    if post_hello {
        write_hello(
            &mut stream,
            &HelloFrame::HelloOk {
                version: 1,
                client_nonce: client_nonce.clone(),
                instance_id: publication.instance.instance_id.clone(),
                pid: std::process::id(),
            },
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !Path::new(&dir).join("exec-continue").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(Path::new(&dir).join("exec-continue").exists());
    }
    let fd = stream.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    assert!(flags >= 0);
    assert_eq!(
        unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) },
        0
    );
    let mut command = Command::new(std::env::current_exe().unwrap());
    isolate_helper(&mut command);
    command
        .args(["--exact", "exec_peer_after_helper", "--nocapture"])
        .env("A2S_EXEC_PEER_AFTER", "1")
        .env("A2S_EXEC_PEER_FD", fd.to_string())
        .env("A2S_EXEC_PEER_NONCE", client_nonce)
        .env("A2S_EXEC_PEER_INSTANCE", &publication.instance.instance_id);
    if post_hello {
        command
            .env("A2S_EXEC_PEER_POST_HELLO", "1")
            .env("A2S_EXEC_PEER_DIR", &dir);
    }
    let error = command.exec();
    panic!("exec failed: {error}");
}

#[test]
fn exec_peer_after_helper() {
    use std::os::fd::FromRawFd;
    if std::env::var_os("A2S_EXEC_PEER_AFTER").is_none() {
        return;
    }
    let fd: i32 = std::env::var("A2S_EXEC_PEER_FD").unwrap().parse().unwrap();
    // SAFETY: the companion helper intentionally inherited this owned fd.
    let mut stream = unsafe { UnixStream::from_raw_fd(fd) };
    if std::env::var_os("A2S_EXEC_PEER_POST_HELLO").is_some() {
        stream.write_all(b"x").unwrap();
        std::fs::write(
            Path::new(&std::env::var("A2S_EXEC_PEER_DIR").unwrap()).join("exec-done"),
            b"ready",
        )
        .unwrap();
    } else {
        write_hello(
            &mut stream,
            &HelloFrame::HelloOk {
                version: 1,
                client_nonce: std::env::var("A2S_EXEC_PEER_NONCE").unwrap(),
                instance_id: std::env::var("A2S_EXEC_PEER_INSTANCE").unwrap(),
                pid: std::process::id(),
            },
        )
        .unwrap();
    }
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    let mut extra = Vec::new();
    stream.read_to_end(&mut extra).unwrap();
    assert!(extra.is_empty(), "no token or stop may be sent after exec");
}

#[test]
fn same_pid_exec_during_hello_is_refused_before_authorization() {
    use std::process::{Command, Stdio};
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-exec-")
        .tempdir_in("/tmp")
        .unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    isolate_helper(&mut command);
    let mut child = command
        .args(["--exact", "exec_peer_before_helper", "--nocapture"])
        .env("A2S_EXEC_PEER_DIR", dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !dir.path().join("exec-ready").exists() && Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("helper exited: {status}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !dir.path().join("exec-ready").exists() {
        let _ = child.kill();
        let _ = child.wait();
        panic!("helper did not become ready");
    }
    let result = macos_client::inspect_unbound(dir.path(), child.id());
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let error = result.expect_err("same-PID exec must be refused");
    assert!(
        error.to_string().contains("peer audit token changed"),
        "{error:#}"
    );
}

#[test]
fn same_pid_exec_after_hello_is_refused_on_revalidation() {
    use std::process::{Command, Stdio};
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-exec-")
        .tempdir_in("/tmp")
        .unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    isolate_helper(&mut command);
    let mut child = command
        .args(["--exact", "exec_peer_before_helper", "--nocapture"])
        .env("A2S_EXEC_PEER_DIR", dir.path())
        .env("A2S_EXEC_PEER_POST_HELLO", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !dir.path().join("exec-ready").exists() && Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("helper exited: {status}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !dir.path().join("exec-ready").exists() {
        let _ = child.kill();
        let _ = child.wait();
        panic!("helper did not become ready");
    }
    let observation = macos_client::inspect_unbound(dir.path(), child.id()).unwrap();
    std::fs::write(dir.path().join("exec-continue"), b"continue").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !dir.path().join("exec-done").exists() && Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("helper exited: {status}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(dir.path().join("exec-done").exists());
    let error = observation
        .revalidate()
        .expect_err("same-PID exec after hello must be refused");
    assert!(
        error.to_string().contains("peer audit token changed"),
        "{error:#}"
    );
    drop(observation);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn same_pid_exec_blocks_explicit_legacy_http_before_token() {
    use std::{
        net::TcpListener,
        process::{Command, Stdio},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
    };
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-exec-")
        .tempdir_in("/tmp")
        .unwrap();
    let token = "synthetic-exec-test-token";
    std::fs::write(dir.path().join("daemon.token"), token).unwrap();
    // The CLI's existing conservative check recognizes this test-owned process
    // as a daemon by filename; the contents are this integration-test binary.
    let executable = dir.path().join("agent2ssh-daemon");
    std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    let mut command = Command::new(&executable);
    isolate_helper(&mut command);
    let mut child = command
        .args(["--exact", "exec_peer_before_helper", "--nocapture"])
        .env("A2S_EXEC_PEER_DIR", dir.path())
        .env_remove("AGENT2SSH_MASTER_PASSWORD")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !dir.path().join("exec-ready").exists() && Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("helper exited: {status}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !dir.path().join("exec-ready").exists() {
        let _ = child.kill();
        let _ = child.wait();
        panic!("helper did not become ready");
    }
    let pid = child.id();
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = server.local_addr().unwrap();
    server.set_nonblocking(true).unwrap();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_worker = captured.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_worker = stop.clone();
    let config = std::fs::canonicalize(dir.path())
        .unwrap()
        .display()
        .to_string();
    let thread = std::thread::spawn(move || {
        while !stop_worker.load(Ordering::SeqCst) {
            match server.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut bytes = [0u8; 4096];
                    let n = stream.read(&mut bytes).unwrap_or(0);
                    captured_worker
                        .lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&bytes[..n]).to_string());
                    let body =
                        serde_json::json!({"ok":true,"pid":pid,"config_dir":config}).to_string();
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(e) => panic!("fake HTTP listener: {e}"),
            }
        }
    });
    let mut cli = Command::new(env!("CARGO_BIN_EXE_agent2ssh"));
    isolate_helper(&mut cli);
    let result = cli
        .args(["daemon", "stop", "--allow-unbound-graceful"])
        .env("AGENT2SSH_CONFIG_DIR", dir.path())
        .env("AGENT2SSH_DAEMON_ADDR", addr.to_string())
        .env_remove("AGENT2SSH_MASTER_PASSWORD")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    stop.store(true, Ordering::SeqCst);
    thread.join().unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!result.status.success());
    let message = String::from_utf8_lossy(&result.stderr);
    assert!(
        message.contains("IPC identity evidence is inconsistent"),
        "{message}"
    );
    assert!(message.contains("peer audit token changed"), "{message}");
    let requests = captured.lock().unwrap();
    assert!(!requests.is_empty(), "the health probe must be reached");
    assert!(
        requests
            .iter()
            .all(|request| request.starts_with("GET /health ")
                && !request.to_ascii_lowercase().contains("authorization:")
                && !request.contains(token)),
        "{requests:?}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("daemon.pid")).unwrap(),
        pid.to_string()
    );
}
