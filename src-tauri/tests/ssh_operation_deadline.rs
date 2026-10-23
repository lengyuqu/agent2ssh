//! Real SSH protocol regressions for post-handshake deadlines and channel
//! request replies. Each peer binds its own loopback socket and uses a fixed
//! throwaway key; no sshd, agent, user credentials, or external fixture is used.

mod common;

use std::borrow::Cow;
use std::io::Read;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use agent2ssh::transport::{Error, KeyboardInteractivePrompt, Prompt, Session};
use russh::server::{Auth, ChannelOpenHandle, Handler};

const OPERATION_BUDGET_MS: u32 = 250;
const WATCHDOG: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Behavior {
    PasswordStall,
    PublicKeyStall,
    CertificateStall,
    KeyboardStartStall,
    KeyboardResponseStall,
    SessionOpenStall,
    DirectOpenStall,
    ForwardStall,
    RejectRequests,
    SilentRequests,
    CloseRequests,
    AcceptRequests,
}

struct Server(Behavior);

impl Handler for Server {
    type Error = russh::Error;

    async fn auth_password(&mut self, _: &str, _: &str) -> Result<Auth, Self::Error> {
        if self.0 == Behavior::PasswordStall {
            std::future::pending::<()>().await;
        }
        Ok(Auth::Accept)
    }

    async fn auth_publickey(
        &mut self,
        _: &str,
        _: &russh::keys::PublicKey,
    ) -> Result<Auth, Self::Error> {
        if self.0 == Behavior::PublicKeyStall {
            std::future::pending::<()>().await;
        }
        Ok(Auth::Accept)
    }

    async fn auth_openssh_certificate(
        &mut self,
        _: &str,
        _: &russh::keys::Certificate,
    ) -> Result<Auth, Self::Error> {
        if self.0 == Behavior::CertificateStall {
            std::future::pending::<()>().await;
        }
        Ok(Auth::Accept)
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        _: &str,
        _: &str,
        response: Option<russh::server::Response<'a>>,
    ) -> Result<Auth, Self::Error> {
        if self.0 == Behavior::KeyboardStartStall
            || self.0 == Behavior::KeyboardResponseStall && response.is_some()
        {
            std::future::pending::<()>().await;
        }
        Ok(Auth::Partial {
            name: Cow::Borrowed("probe"),
            instructions: Cow::Borrowed("enter the test answer"),
            prompts: Cow::Owned(vec![(Cow::Borrowed("code"), false)]),
        })
    }

    async fn channel_open_session(
        &mut self,
        _: russh::Channel<russh::server::Msg>,
        reply: ChannelOpenHandle,
        _: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        if self.0 == Behavior::SessionOpenStall {
            std::future::pending::<()>().await;
        }
        reply.accept().await;
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        _: russh::Channel<russh::server::Msg>,
        _: &str,
        _: u32,
        _: &str,
        _: u32,
        reply: ChannelOpenHandle,
        _: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        if self.0 == Behavior::DirectOpenStall {
            std::future::pending::<()>().await;
        }
        reply.accept().await;
        Ok(())
    }

    async fn tcpip_forward(
        &mut self,
        _: &str,
        _: &mut u32,
        _: &mut russh::server::Session,
    ) -> Result<bool, Self::Error> {
        if self.0 == Behavior::ForwardStall {
            std::future::pending::<()>().await;
        }
        Ok(true)
    }

    async fn pty_request(
        &mut self,
        channel: russh::ChannelId,
        _: &str,
        _: u32,
        _: u32,
        _: u32,
        _: u32,
        _: &[(russh::Pty, u32)],
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        self.reply(channel, session)
    }

    async fn shell_request(
        &mut self,
        channel: russh::ChannelId,
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        self.reply(channel, session)
    }

    async fn exec_request(
        &mut self,
        channel: russh::ChannelId,
        _: &[u8],
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        self.reply(channel, session)?;
        if self.0 == Behavior::AcceptRequests {
            session.data(channel, b"accepted output".as_slice())?;
            session.exit_status_request(channel, 0)?;
            session.eof(channel)?;
            session.close(channel)?;
        }
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel: russh::ChannelId,
        _: &str,
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        self.reply(channel, session)
    }

    async fn data(
        &mut self,
        channel: russh::ChannelId,
        data: &[u8],
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        if self.0 == Behavior::AcceptRequests && data.get(4) == Some(&1) {
            // Minimal SFTP v3 VERSION response to the client's INIT.
            session.data(channel, [0, 0, 0, 5, 2, 0, 0, 0, 3].as_slice())?;
        }
        Ok(())
    }
}

impl Server {
    fn reply(
        &self,
        channel: russh::ChannelId,
        session: &mut russh::server::Session,
    ) -> Result<(), russh::Error> {
        match self.0 {
            Behavior::RejectRequests => session.channel_failure(channel),
            Behavior::SilentRequests => Ok(()),
            Behavior::CloseRequests => session.close(channel),
            _ => session.channel_success(channel),
        }
    }
}

fn private_key(seed: u8) -> russh::keys::PrivateKey {
    russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[seed; 32]).into()
}

struct Peer {
    address: SocketAddr,
    closed: mpsc::Receiver<()>,
    runtime: Option<tokio::runtime::Runtime>,
}

impl Peer {
    fn start(behavior: Behavior) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let (closed_tx, closed) = mpsc::channel();
        runtime.spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let (socket, _) = listener.accept().await.unwrap();
            let (proxy, server_stream) = tokio::io::duplex(64 * 1024);
            let (mut socket_read, mut socket_write) = socket.into_split();
            let (mut proxy_read, mut proxy_write) = tokio::io::split(proxy);
            // Observe EOF independently of the deliberately stalled SSH handler.
            // Merely returning an outer timeout would leave this pump waiting.
            tokio::spawn(async move {
                let _ = tokio::io::copy(&mut socket_read, &mut proxy_write).await;
                let _ = closed_tx.send(());
            });
            tokio::spawn(async move {
                let _ = tokio::io::copy(&mut proxy_read, &mut socket_write).await;
            });
            let config = russh::server::Config {
                keys: vec![private_key(17)],
                auth_rejection_time: Duration::ZERO,
                auth_rejection_time_initial: Some(Duration::ZERO),
                ..Default::default()
            };
            if let Ok(server) =
                russh::server::run_stream(Arc::new(config), server_stream, Server(behavior)).await
            {
                let _ = server.await;
            }
        });
        Self {
            address,
            closed,
            runtime: Some(runtime),
        }
    }

    fn session(&self) -> Session {
        let mut session = Session::new().unwrap();
        session.set_timeout(2_000);
        session.set_tcp_stream(TcpStream::connect(self.address).unwrap());
        session.handshake().unwrap();
        session
    }

    fn assert_closed(&self) {
        self.closed
            .recv_timeout(WATCHDOG)
            .expect("timed-out operation retained its SSH socket");
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        // Also makes a regression fail promptly instead of hanging test teardown.
        self.runtime.take().unwrap().shutdown_background();
    }
}

fn bounded_call<T: Send + 'static>(operation: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = tx.send(operation());
    });
    let result = rx
        .recv_timeout(WATCHDOG)
        .expect("SSH operation leaked a blocking worker beyond its configured deadline");
    worker.join().unwrap();
    result
}

fn assert_timeout(result: Result<(), Error>, started: Instant, operation: &str) {
    let message = result.unwrap_err().to_string();
    assert!(message.contains(operation), "wrong operation: {message}");
    assert!(message.contains("timed out"), "wrong failure: {message}");
    assert!(started.elapsed() < Duration::from_secs(3));
}

struct Answer;
impl KeyboardInteractivePrompt for Answer {
    fn prompt(&mut self, _: &str, _: &str, prompts: &[Prompt<'_>]) -> Vec<String> {
        vec!["test answer".to_string(); prompts.len()]
    }
}

#[test]
fn post_handshake_authentication_deadlines_release_the_socket_and_worker() {
    for (behavior, expected) in [
        (Behavior::PasswordStall, "password authentication"),
        (Behavior::PublicKeyStall, "public-key authentication"),
        (Behavior::CertificateStall, "certificate authentication"),
        (
            Behavior::KeyboardStartStall,
            "keyboard-interactive authentication",
        ),
        (
            Behavior::KeyboardResponseStall,
            "keyboard-interactive authentication response",
        ),
    ] {
        let peer = Peer::start(behavior);
        let session = peer.session();
        session.set_timeout(OPERATION_BUDGET_MS);
        let started = Instant::now();
        let result = bounded_call(move || {
            let directory = tempfile::tempdir().unwrap();
            let key_path = directory.path().join("test-key");
            let key = private_key(18);
            key.write_openssh_file(&key_path, Default::default())
                .unwrap();
            let result = match behavior {
                Behavior::PasswordStall => session.userauth_password("testuser", "test password"),
                Behavior::PublicKeyStall => {
                    session.userauth_pubkey_file("testuser", None, &key_path, None)
                }
                Behavior::CertificateStall => {
                    let mut builder = russh::keys::ssh_key::certificate::Builder::new(
                        vec![19; 32],
                        key.public_key().key_data().clone(),
                        0,
                        u64::MAX,
                    )
                    .unwrap();
                    builder.valid_principal("testuser").unwrap();
                    let certificate = builder.sign(&private_key(20)).unwrap();
                    let cert_path = directory.path().join("test-key-cert.pub");
                    std::fs::write(&cert_path, certificate.to_openssh().unwrap()).unwrap();
                    session.userauth_pubkey_file("testuser", Some(&cert_path), &key_path, None)
                }
                _ => session.userauth_keyboard_interactive("testuser", &mut Answer),
            };
            assert!(!session.authenticated());
            result
        });
        assert_timeout(result, started, expected);
        peer.assert_closed();
    }
}

#[test]
fn channel_and_forward_setup_deadlines_release_the_socket_and_worker() {
    for (behavior, expected) in [
        (Behavior::SessionOpenStall, "session channel open"),
        (Behavior::DirectOpenStall, "direct-tcpip channel open"),
        (Behavior::ForwardStall, "remote forwarding request"),
    ] {
        let peer = Peer::start(behavior);
        let session = peer.session();
        session
            .userauth_password("testuser", "test password")
            .unwrap();
        session.set_timeout(OPERATION_BUDGET_MS);
        let started = Instant::now();
        let result = bounded_call(move || match behavior {
            Behavior::SessionOpenStall => session.channel_session().map(|_| ()),
            Behavior::DirectOpenStall => session
                .channel_direct_tcpip("127.0.0.1", 80, None)
                .map(|_| ()),
            _ => session
                .channel_forward_listen(9000, Some("127.0.0.1"), None)
                .map(|_| ()),
        });
        assert_timeout(result, started, expected);
        peer.assert_closed();
    }
}

#[test]
fn exec_shell_and_pty_report_server_rejection_without_waiting_for_timeout() {
    let peer = Peer::start(Behavior::RejectRequests);
    let session = peer.session();
    session
        .userauth_password("testuser", "test password")
        .unwrap();
    let mut channel = session.channel_session().unwrap();
    bounded_call(move || {
        let started = Instant::now();
        for (request, result) in [
            ("PTY", channel.request_pty("xterm", None, None)),
            ("shell", channel.shell()),
            ("exec", channel.exec("echo probe")),
        ] {
            let message = result.unwrap_err().to_string();
            assert!(
                message.contains(&format!("rejected the {request} request")),
                "{message}"
            );
        }
        assert!(started.elapsed() < Duration::from_secs(1));
        session.disconnect(None, "test complete", None).unwrap();
    });
}

#[test]
fn accepted_pty_shell_and_exec_replies_leave_output_intact() {
    let peer = Peer::start(Behavior::AcceptRequests);
    let session = peer.session();
    session
        .userauth_password("testuser", "test password")
        .unwrap();
    let mut channel = session.channel_session().unwrap();
    bounded_call(move || {
        channel.request_pty("xterm", None, None).unwrap();
        channel.shell().unwrap();
        channel.exec("echo probe").unwrap();
        let mut output = String::new();
        channel.read_to_string(&mut output).unwrap();
        channel.wait_close().unwrap();
        assert_eq!(output, "accepted output");
        assert_eq!(channel.exit_status().unwrap(), 0);
        session.disconnect(None, "test complete", None).unwrap();
    });
}

#[test]
fn missing_request_reply_times_out_and_closes_the_connection() {
    let peer = Peer::start(Behavior::SilentRequests);
    let session = peer.session();
    session
        .userauth_password("testuser", "test password")
        .unwrap();
    let mut channel = session.channel_session().unwrap();
    session.set_timeout(OPERATION_BUDGET_MS);
    let started = Instant::now();
    let result = bounded_call(move || channel.exec("echo probe"));
    assert_timeout(result, started, "exec request");
    peer.assert_closed();
}

#[test]
fn close_before_request_reply_is_reported_immediately() {
    let peer = Peer::start(Behavior::CloseRequests);
    let session = peer.session();
    session
        .userauth_password("testuser", "test password")
        .unwrap();
    let mut channel = session.channel_session().unwrap();
    let error = bounded_call(move || channel.exec("echo probe")).unwrap_err();
    assert!(error
        .to_string()
        .contains("closed before the server replied to the exec request"));
}

#[test]
fn sftp_subsystem_acknowledgements_are_checked_before_initialization() {
    for behavior in [
        Behavior::RejectRequests,
        Behavior::CloseRequests,
        Behavior::SilentRequests,
        Behavior::AcceptRequests,
    ] {
        let peer = Peer::start(behavior);
        let session = peer.session();
        session
            .userauth_password("testuser", "test password")
            .unwrap();
        session.set_timeout(OPERATION_BUDGET_MS);
        let started = Instant::now();
        let result = bounded_call(move || session.sftp().map(|_| ()));
        match behavior {
            Behavior::AcceptRequests => result.unwrap(),
            Behavior::RejectRequests => assert!(result
                .unwrap_err()
                .to_string()
                .contains("rejected the SFTP subsystem")),
            Behavior::CloseRequests => {
                assert!(result.unwrap_err().to_string().contains("closed before"))
            }
            _ => {
                assert_timeout(result, started, "SFTP session open");
                peer.assert_closed();
            }
        }
    }
}
