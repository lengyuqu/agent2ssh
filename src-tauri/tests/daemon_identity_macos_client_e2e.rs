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
    assert!(server.join().unwrap().is_empty());
    let record = dir.path().join("daemon.instance.json");
    let replacement = dir.path().join("same-bytes-replacement");
    std::fs::write(&replacement, std::fs::read(&record).unwrap()).unwrap();
    std::fs::rename(replacement, record).unwrap();
    assert!(observation.revalidate().is_err());
    drop(publication);
}
