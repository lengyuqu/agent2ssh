//! Real macOS Unix socket handshake; this does not claim process binding.
#![cfg(target_os = "macos")]

use agent2ssh::daemon_identity::{
    check_hello_reply, macos_peer_pid, read_hello, write_hello, HelloFrame,
};
use std::{
    io::Read,
    os::unix::net::{UnixListener, UnixStream},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const ID: &str = "0123456789abcdef0123456789abcdef";

// Invoked in a separate test process by the parent. It is inert in normal runs.
#[test]
fn ipc_child() {
    let Ok(path) = std::env::var("A2S_IDENTITY_TEST_SOCKET") else {
        return;
    };
    let listener = UnixListener::bind(path).unwrap();
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let hello = read_hello(&mut stream).unwrap();
    assert_eq!(
        hello,
        HelloFrame::Hello {
            version: 1,
            client_nonce: ID.into()
        }
    );
    let reply = HelloFrame::HelloOk {
        version: 1,
        client_nonce: ID.into(),
        instance_id: ID.into(),
        pid: std::process::id(),
    };
    write_hello(&mut stream, &reply).unwrap();
    // The parent closes without sending any token or stop frame.
    let mut extra = [0u8; 1];
    assert_eq!(stream.read(&mut extra).unwrap(), 0);
}

#[test]
fn real_peer_pid_and_hello_remain_unbound() {
    let dir = tempfile::Builder::new()
        .prefix("a2s-ipc-")
        .tempdir_in("/tmp")
        .unwrap();
    let path = dir.path().join("control.sock");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "ipc_child", "--nocapture"])
        .env("A2S_IDENTITY_TEST_SOCKET", &path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut stream = loop {
        match UnixStream::connect(&path) {
            Ok(stream) => break stream,
            Err(e) if Instant::now() < deadline => {
                let _ = e;
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("socket did not appear: {e}");
            }
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let peer = macos_peer_pid(&stream).unwrap();
    assert_eq!(peer, child.id());
    write_hello(
        &mut stream,
        &HelloFrame::Hello {
            version: 1,
            client_nonce: ID.into(),
        },
    )
    .unwrap();
    let reply = read_hello(&mut stream).unwrap();
    assert!(check_hello_reply(&reply, ID, ID, child.id(), peer).is_ok());
    assert!(check_hello_reply(&reply, ID, ID, child.id(), peer.wrapping_add(1)).is_err());
    drop(stream);
    assert!(child.wait().unwrap().success());
}
