//! Pins that a connect attempt against a peer which accepts TCP and then goes
//! silent gives up and releases its worker.
//!
//! `connect_embedded_ssh` set socket-level read/write timeouts but no libssh2
//! session timeout. libssh2's blocking-mode waits loop internally with no
//! deadline of their own, so `handshake` stayed blocked against such a peer.
//! `ping_hosts_core` and `collect_health_snapshot` wrap the connect in
//! `tokio::time::timeout`, so they returned on schedule — but abandoning the
//! `spawn_blocking` handle left the worker blocked forever, holding a thread and
//! the socket, once per host and per poll.
//!
//! The discriminating signal is the socket: once the connect actually gives up,
//! the session drops and the peer observes EOF. While the worker was leaked the
//! peer never did, which is how this was reproduced before the fix.

use std::io::Read;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// A peer that accepts one connection and then says nothing at all — no SSH
/// banner, no bytes. Reports when the client closes its end of the socket.
fn silent_peer() -> (u16, mpsc::Receiver<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind the silent peer");
    let port = listener.local_addr().expect("silent peer address").port();
    let (closed_tx, closed_rx) = mpsc::channel();
    std::thread::spawn(move || {
        if let Ok((mut socket, _)) = listener.accept() {
            let mut buf = [0u8; 256];
            loop {
                match socket.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
            let _ = closed_tx.send(());
        }
    });
    (port, closed_rx)
}

fn isolated_config(port: u16, label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "a2s-connect-deadline-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create config dir");
    let hosts = serde_json::json!({
        "schema_version": 1,
        "hosts": [{
            "name": "silent",
            "host": "127.0.0.1",
            "port": port,
            "user": "nobody",
            "password": "nobody",
        }],
    });
    std::fs::write(
        dir.join("hosts.json"),
        serde_json::to_vec_pretty(&hosts).unwrap(),
    )
    .expect("write hosts.json");
    std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);
    dir
}

fn release_config(dir: PathBuf) {
    std::env::remove_var("AGENT2SSH_CONFIG_DIR");
    let _ = std::fs::remove_dir_all(dir);
}

/// A leaked `spawn_blocking` worker must not stall the runtime teardown.
fn detached_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build a test runtime")
}

fn assert_peer_saw_eof(closed_rx: &mpsc::Receiver<()>, context: &str) {
    if closed_rx.recv_timeout(Duration::from_secs(5)).is_err() {
        panic!(
            "{context}: the peer never saw the socket close, so the connect is still blocked \
             and its worker was leaked"
        );
    }
}

#[test]
#[serial_test::serial]
fn ping_releases_its_worker_when_the_peer_never_speaks_ssh() {
    let (port, closed_rx) = silent_peer();
    let dir = isolated_config(port, "ping");
    let runtime = detached_runtime();

    let started = Instant::now();
    let results = runtime.block_on(agent2ssh::core::ping_hosts_core(
        vec!["silent".to_string()],
        Some(2),
    ));
    let elapsed = started.elapsed();
    runtime.shutdown_background();

    assert_eq!(results.len(), 1);
    assert!(
        !results[0].reachable,
        "a silent peer must not be reported reachable: {:?}",
        results[0]
    );
    assert!(
        elapsed < Duration::from_secs(6),
        "ping waited {elapsed:?} on a 2s budget"
    );

    assert_peer_saw_eof(&closed_rx, "ping");
    release_config(dir);
}

#[test]
#[serial_test::serial]
fn health_snapshot_releases_its_worker_when_the_peer_never_speaks_ssh() {
    let (port, closed_rx) = silent_peer();
    let dir = isolated_config(port, "health");
    let runtime = detached_runtime();

    let snapshot = runtime.block_on(agent2ssh::health::collect_health_snapshot(
        vec!["silent".to_string()],
        Some(2),
    ));
    runtime.shutdown_background();

    assert_eq!(snapshot.hosts.len(), 1);
    let host = &snapshot.hosts[0];
    assert!(
        !host.reachable,
        "a silent peer must not be reported healthy: {host:?}"
    );
    assert!(
        host.error.is_some(),
        "an unreachable host must carry an error: {host:?}"
    );

    assert_peer_saw_eof(&closed_rx, "health snapshot");
    release_config(dir);
}
