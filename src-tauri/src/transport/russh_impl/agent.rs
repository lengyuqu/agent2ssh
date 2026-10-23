//! SSH agent authentication.
//!
//! # Why this exists
//!
//! libssh2 shipped one agent connector that covered both Pageant and the
//! OpenSSH named pipe on Windows. russh splits that into separate
//! constructors — `AgentClient::connect_pageant()` and
//! `AgentClient::connect_named_pipe()` — and each returns an
//! `AgentClient<S>` with a **different** `S`, so a facade that keeps one
//! `Agent` type cannot name either of them. [`AgentStream`] is that missing
//! union: one `AsyncRead + AsyncWrite` surface over the transports, so the
//! client has a single instantiation.
//!
//! # No wrapper signer is needed
//!
//! `docs/plans/russh-migration.md` §10.3 said the transport shipped no `Signer`
//! and that an `AgentSigner` must be written. That is a grep artifact: the
//! implementation exists at `auth.rs:180`, but its header wraps —
//!
//! ```text
//! impl<R: AsyncRead + AsyncWrite + Unpin + Send + 'static> Signer
//!     for crate::keys::agent::client::AgentClient<R>
//! ```
//!
//! — so `grep "Signer for"` finds nothing while `AgentClient` does implement
//! the trait. `authenticate_publickey_with` is therefore callable directly
//! and this module forwards the agent's own signing, with no shim.
//!
//! # Ordering
//!
//! `userauth` takes the agent lock first, then the session's handle lock.
//! Nothing else in this crate takes both, so the order cannot deadlock; it is
//! written down because the reverse order in a future caller would.

use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use russh::keys::agent::client::AgentClient;
use russh::keys::agent::AgentIdentity;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::{other, Session};
use crate::transport::types::Error;

/// The agent transports, behind one type.
///
/// `Send` matters as much as the I/O traits: the future that uses this holds
/// it across an `.await` and `block_on` requires `Send`.
enum AgentStream {
    #[cfg(test)]
    Test(tokio::io::DuplexStream),
    #[cfg(windows)]
    Pageant(pageant::PageantStream),
    /// Win32-OpenSSH serves its agent on this pipe rather than on a socket.
    #[cfg(windows)]
    NamedPipe(tokio::net::windows::named_pipe::NamedPipeClient),
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
}

impl AsyncRead for AgentStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(test)]
            AgentStream::Test(stream) => Pin::new(stream).poll_read(cx, buf),
            #[cfg(windows)]
            AgentStream::Pageant(stream) => Pin::new(stream).poll_read(cx, buf),
            #[cfg(windows)]
            AgentStream::NamedPipe(stream) => Pin::new(stream).poll_read(cx, buf),
            #[cfg(unix)]
            AgentStream::Unix(stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for AgentStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            #[cfg(test)]
            AgentStream::Test(stream) => Pin::new(stream).poll_write(cx, buf),
            #[cfg(windows)]
            AgentStream::Pageant(stream) => Pin::new(stream).poll_write(cx, buf),
            #[cfg(windows)]
            AgentStream::NamedPipe(stream) => Pin::new(stream).poll_write(cx, buf),
            #[cfg(unix)]
            AgentStream::Unix(stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(test)]
            AgentStream::Test(stream) => Pin::new(stream).poll_flush(cx),
            #[cfg(windows)]
            AgentStream::Pageant(stream) => Pin::new(stream).poll_flush(cx),
            #[cfg(windows)]
            AgentStream::NamedPipe(stream) => Pin::new(stream).poll_flush(cx),
            #[cfg(unix)]
            AgentStream::Unix(stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(test)]
            AgentStream::Test(stream) => Pin::new(stream).poll_shutdown(cx),
            #[cfg(windows)]
            AgentStream::Pageant(stream) => Pin::new(stream).poll_shutdown(cx),
            #[cfg(windows)]
            AgentStream::NamedPipe(stream) => Pin::new(stream).poll_shutdown(cx),
            #[cfg(unix)]
            AgentStream::Unix(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

/// One identity the agent holds.
///
/// The wire blob is stored beside the key rather than encoded on demand,
/// because the facade's accessors return borrowed slices.
#[derive(Clone)]
pub struct PublicKey {
    identity: AgentIdentity,
    blob: Vec<u8>,
    comment: String,
}

impl PublicKey {
    pub fn blob(&self) -> &[u8] {
        &self.blob
    }

    pub fn comment(&self) -> &str {
        &self.comment
    }

    fn from_identity(identity: &AgentIdentity) -> Self {
        // The certificate, including its CA signature and principals, is the
        // identity the server and agent must see. Extracting its public key
        // here would silently turn certificate authentication into plain-key
        // authentication.
        let blob = match identity {
            AgentIdentity::PublicKey { key, .. } => key.to_bytes(),
            AgentIdentity::Certificate { certificate, .. } => certificate.to_bytes(),
        }
        .unwrap_or_default();
        Self {
            identity: identity.clone(),
            blob,
            comment: identity.comment().to_string(),
        }
    }

    async fn authenticate<S: russh::Signer>(
        &self,
        username: &str,
        handle: &mut russh::client::Handle<super::ClientHandler>,
        signer: &mut S,
    ) -> Result<russh::client::AuthResult, Error>
    where
        S::Error: std::fmt::Display,
    {
        let is_rsa = self.identity.public_key().algorithm().is_rsa();
        let hash_alg = super::rsa_hash_alg(handle, is_rsa).await?;
        match &self.identity {
            AgentIdentity::PublicKey { key, .. } => {
                handle
                    .authenticate_publickey_with(username, key.clone(), hash_alg, signer)
                    .await
            }
            AgentIdentity::Certificate { certificate, .. } => {
                handle
                    .authenticate_certificate_with(username, certificate.clone(), hash_alg, signer)
                    .await
            }
        }
        .map_err(|error| other(format!("the SSH agent could not sign: {error}")))
    }
}

/// The agent attached to one session.
pub struct Agent {
    session: Session,
    /// A tokio mutex, not a `std` one: the guard is held across the
    /// `request_identities` and `authenticate_publickey_with` awaits, and a
    /// `std` guard would make the future `!Send`.
    client: tokio::sync::Mutex<Option<AgentClient<AgentStream>>>,
    identities: Mutex<Vec<PublicKey>>,
}

impl Agent {
    pub(super) fn new(session: Session) -> Self {
        Self {
            session,
            client: tokio::sync::Mutex::new(None),
            identities: Mutex::new(Vec::new()),
        }
    }

    /// Connect to whatever agent this platform has.
    ///
    /// Failing here is not fatal at the call site: `embedded_ssh` treats an
    /// unreachable agent as "this method is unavailable" and moves on to the
    /// next authentication path.
    pub fn connect(&mut self) -> Result<(), Error> {
        let client = self
            .session
            .inner
            .settings
            .run_independent("agent connection", connect_stream())?;
        *self.client.try_lock().expect("no other caller holds this") = Some(client);
        Ok(())
    }

    /// Fetch the agent's identity list. Separate from [`Self::identities`]
    /// because the facade mirrors libssh2's two-step shape.
    pub fn list_identities(&mut self) -> Result<(), Error> {
        let fetched =
            self.session
                .inner
                .settings
                .run_independent("agent identity listing", async {
                    let mut guard = self.client.lock().await;
                    let client = guard
                        .as_mut()
                        .ok_or_else(|| other("the SSH agent is not connected"))?;
                    client.request_identities().await.map_err(|error| {
                        other(format!("the SSH agent refused to list identities: {error}"))
                    })
                });
        if fetched.is_err() {
            // An interrupted agent packet cannot be resumed on this stream.
            *self
                .client
                .try_lock()
                .expect("listing released the agent lock") = None;
        }
        let fetched = fetched?;
        *self.identities.lock().unwrap() = fetched
            .iter()
            .map(PublicKey::from_identity)
            .collect::<Vec<_>>();
        Ok(())
    }

    pub fn identities(&self) -> Result<Vec<PublicKey>, Error> {
        Ok(self.identities.lock().unwrap().clone())
    }

    /// Authenticate as `username` with one agent-held key.
    ///
    /// The agent signs; this crate never sees a private key.
    pub fn userauth(&self, username: &str, identity: &PublicKey) -> Result<(), Error> {
        self.session.ensure_connected()?;
        let session = self.session.clone();

        let result = session.run("agent authentication", async {
            let mut client_guard = self.client.lock().await;
            if client_guard.is_none() {
                return Err(other("the SSH agent is not connected"));
            }
            let mut handle_guard = session.inner.handle.lock().await;
            let Some(handle) = handle_guard.as_mut() else {
                return Err(other("handshake has not run"));
            };
            identity
                .authenticate(
                    username,
                    handle,
                    client_guard
                        .as_mut()
                        .expect("checked above that the agent is connected"),
                )
                .await
        })?;

        Session::auth_result(result, &session.inner.authenticated)
    }
}

async fn connect_stream() -> Result<AgentClient<AgentStream>, Error> {
    #[cfg(windows)]
    {
        // The OpenSSH named pipe is tried **first**, and the order is not
        // arbitrary. `pageant::PageantStream::new()` reports success even when
        // no Pageant is running — its WM_COPYDATA transport is created before
        // anything can tell — so the first real request is where it fails, with
        // `early eof`. Probing Pageant first therefore breaks the common case on
        // a modern Windows host: no Pageant, OpenSSH agent service running. The
        // pipe answers immediately by failing to open when nothing listens.
        match tokio::net::windows::named_pipe::ClientOptions::new()
            .open(r"\\.\pipe\openssh-ssh-agent")
        {
            Ok(stream) => Ok(AgentClient::connect(AgentStream::NamedPipe(stream))),
            Err(pipe_error) => match pageant::PageantStream::new().await {
                Ok(stream) => Ok(AgentClient::connect(AgentStream::Pageant(stream))),
                Err(pageant_error) => Err(other(format!(
                    "no SSH agent answered (OpenSSH pipe: {pipe_error}; Pageant: {pageant_error})"
                ))),
            },
        }
    }
    #[cfg(unix)]
    {
        let path = std::env::var("SSH_AUTH_SOCK")
            .map_err(|_| other("SSH_AUTH_SOCK is not set, so there is no SSH agent to ask"))?;
        let stream = tokio::net::UnixStream::connect(&path)
            .await
            .map_err(|error| other(format!("could not reach the SSH agent at {path}: {error}")))?;
        Ok(AgentClient::connect(AgentStream::Unix(stream)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::PublicKey as RhPublicKey;

    /// A fixed key rather than a generated one: the assertion below is about
    /// the mapping, and a random key would only add an RNG dependency to it.
    const TEST_KEY: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILZqOrxKsAQNKvmw0XX0pQrDrlBJpZj9PLfZN1RNxVFL";

    fn identity(comment: &str) -> AgentIdentity {
        AgentIdentity::PublicKey {
            key: RhPublicKey::from_openssh(TEST_KEY).unwrap(),
            comment: comment.to_string(),
        }
    }

    fn test_private_key(seed: u8) -> russh::keys::PrivateKey {
        russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[seed; 32]).into()
    }

    fn certificate_fixture() -> (
        russh::keys::PrivateKey,
        russh::keys::PrivateKey,
        russh::keys::Certificate,
    ) {
        let ca_key = test_private_key(1);
        let user_key = test_private_key(2);
        let mut builder = russh::keys::ssh_key::certificate::Builder::new(
            vec![3; 32],
            user_key.public_key().key_data().clone(),
            0,
            u64::MAX,
        )
        .unwrap();
        builder.key_id("agent-certificate-regression").unwrap();
        builder.valid_principal("testuser").unwrap();
        let certificate = builder.sign(&ca_key).unwrap();
        (ca_key, user_key, certificate)
    }

    #[tokio::test]
    async fn stalled_agent_identity_listing_obeys_the_session_timeout() {
        let session = Session::new().unwrap();
        session.set_timeout(100);
        let mut agent = Agent::new(session);
        let (stream, _silent_peer) = tokio::io::duplex(4096);
        *agent.client.lock().await = Some(AgentClient::connect(AgentStream::Test(stream)));
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            tokio::task::spawn_blocking(move || agent.list_identities()),
        )
        .await
        .expect("identity listing leaked its blocking worker")
        .unwrap()
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("agent identity listing timed out"));
    }

    #[tokio::test]
    async fn stalled_agent_listing_still_allows_ssh_authentication_fallback() {
        use std::{sync::Arc, time::Duration};
        struct Server;
        impl russh::server::Handler for Server {
            type Error = russh::Error;
            async fn auth_password(
                &mut self,
                _: &str,
                _: &str,
            ) -> Result<russh::server::Auth, Self::Error> {
                Ok(russh::server::Auth::Accept)
            }
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            let session = Session::new().unwrap();
            let (client, peer) = tokio::io::duplex(64 * 1024);
            let config = russh::server::Config {
                keys: vec![test_private_key(24)],
                ..Default::default()
            };
            let server = tokio::spawn(async move {
                if let Ok(server) = russh::server::run_stream(Arc::new(config), peer, Server).await
                {
                    let _ = server.await;
                }
            });
            let handler = super::super::ClientHandler {
                host_key: session.inner.host_key.clone(),
                banner: session.inner.banner.clone(),
                forwards: session.inner.forwards.clone(),
            };
            let handle = russh::client::connect_stream(
                Arc::new(russh::client::Config::default()),
                client,
                handler,
            )
            .await
            .unwrap();
            *session.inner.handle.lock().await = Some(handle);
            session
                .inner
                .connected
                .store(true, std::sync::atomic::Ordering::SeqCst);
            session.set_timeout(150);
            let mut agent = Agent::new(session.clone());
            let (agent_stream, _silent_agent) = tokio::io::duplex(4096);
            *agent.client.lock().await =
                Some(AgentClient::connect(AgentStream::Test(agent_stream)));
            tokio::task::spawn_blocking(move || {
                assert!(agent
                    .list_identities()
                    .unwrap_err()
                    .to_string()
                    .contains("timed out"));
                assert!(agent.client.try_lock().unwrap().is_none());
                session
                    .userauth_password("fallback-user", "fallback-password")
                    .unwrap();
                assert!(session.authenticated());
            })
            .await
            .unwrap();
            server.abort();
        })
        .await
        .expect("agent fallback did not finish");
    }

    #[tokio::test]
    async fn stalled_agent_signature_obeys_the_session_timeout() {
        use std::sync::Arc;
        use std::time::Duration;

        use tokio::io::AsyncReadExt;

        struct Server;
        impl russh::server::Handler for Server {
            type Error = russh::Error;
        }

        tokio::time::timeout(Duration::from_secs(5), async {
            let session = Session::new().unwrap();
            let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
            let config = russh::server::Config {
                keys: vec![test_private_key(21)],
                ..Default::default()
            };
            let server_task = tokio::spawn(async move {
                if let Ok(server) =
                    russh::server::run_stream(Arc::new(config), server_stream, Server).await
                {
                    let _ = server.await;
                }
            });
            let handler = super::super::ClientHandler {
                host_key: Arc::clone(&session.inner.host_key),
                banner: Arc::clone(&session.inner.banner),
                forwards: Arc::clone(&session.inner.forwards),
            };
            let handle = russh::client::connect_stream(
                Arc::new(russh::client::Config::default()),
                client_stream,
                handler,
            )
            .await
            .unwrap();
            *session.inner.handle.lock().await = Some(handle);
            session
                .inner
                .connected
                .store(true, std::sync::atomic::Ordering::SeqCst);
            session.set_timeout(300);
            let agent = Agent::new(session);
            let (agent_stream, mut silent_agent) = tokio::io::duplex(4096);
            *agent.client.lock().await =
                Some(AgentClient::connect(AgentStream::Test(agent_stream)));
            let identity = PublicKey::from_identity(&AgentIdentity::PublicKey {
                key: test_private_key(22).public_key().clone(),
                comment: "test signing identity".to_string(),
            });
            let worker = tokio::task::spawn_blocking(move || agent.userauth("testuser", &identity));
            let length = silent_agent.read_u32().await.unwrap() as usize;
            let mut request = vec![0; length];
            silent_agent.read_exact(&mut request).await.unwrap();
            assert_eq!(
                request[0], 13,
                "the fake agent must receive a signing request"
            );
            // Deliberately send no signature response.
            let error = worker.await.unwrap().unwrap_err();
            assert!(error.to_string().contains("agent authentication timed out"));
            server_task.abort();
            let _ = server_task.await;
        })
        .await
        .expect("agent authentication leaked its blocking worker");
    }

    /// Real-machine probe: what does the local agent actually hold?
    ///
    /// `#[ignore]`d because it needs an agent to exist and answers about this
    /// host, not about the code — the same reason `container_terminal_manual`
    /// is ignored. Run it when a Windows agent has to be checked by hand:
    ///
    /// ```text
    /// cargo test --no-default-features --lib pageant_lists_identities -- --ignored --nocapture
    /// ```
    ///
    /// A hang here is itself the answer: `connect_stream` reports which transport
    /// it chose, so a wedged Pageant shows up as the pipe failing and Pageant
    /// never replying.
    #[tokio::test]
    #[ignore = "needs a running agent on this machine"]
    async fn pageant_lists_identities() {
        let mut client = tokio::time::timeout(std::time::Duration::from_secs(10), connect_stream())
            .await
            .expect("connchose a transport within 10s")
            .expect("an agent answered");

        let identities = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            client.request_identities(),
        )
        .await
        .expect("the agent replied within 10s")
        .expect("identities");

        println!("identities: {}", identities.len());
        for identity in identities {
            match identity {
                AgentIdentity::PublicKey { key, comment } => {
                    println!("  {} {}", key.to_openssh().unwrap(), comment)
                }
                AgentIdentity::Certificate { certificate, .. } => {
                    println!("  certificate {:?}", certificate.algorithm())
                }
            }
        }
    }

    #[test]
    fn identity_mapping_keeps_the_wire_blob_and_comment() {
        let mapped = PublicKey::from_identity(&identity("work laptop"));
        assert_eq!(mapped.comment(), "work laptop");

        // The blob is the SSH wire encoding, so its first four bytes are the
        // length of the algorithm name that follows. Re-encoding a key this
        // mapping dropped would fail that self-description.
        assert!(mapped.blob().len() > 4);
        let declared = u32::from_be_bytes(mapped.blob()[..4].try_into().unwrap()) as usize;
        assert!(declared > 0);
        assert!(mapped.blob().len() > declared);
        assert_eq!(
            mapped.blob(),
            RhPublicKey::from_openssh(TEST_KEY)
                .unwrap()
                .to_bytes()
                .unwrap()
        );
    }

    #[test]
    fn certificate_identity_preserves_the_certificate_wire_blob_and_comment() {
        let (_, _, certificate) = certificate_fixture();
        let mapped = PublicKey::from_identity(&AgentIdentity::Certificate {
            certificate: certificate.clone(),
            comment: "CA-issued identity".to_string(),
        });
        assert_eq!(mapped.comment(), "CA-issued identity");
        assert_eq!(mapped.blob(), certificate.to_bytes().unwrap());
        assert!(matches!(mapped.identity, AgentIdentity::Certificate { .. }));
        let decoded = russh::keys::Certificate::from_bytes(mapped.blob()).unwrap();
        assert_eq!(decoded, certificate);
        assert_eq!(decoded.valid_principals(), &["testuser".to_string()]);
    }

    #[tokio::test]
    async fn agent_certificate_authenticates_against_a_ca_only_server() {
        use std::sync::Arc;
        use std::time::Duration;

        use russh::keys::ssh_key::encoding::Decode;
        use russh::Signer;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        struct CaOnlyServer {
            ca: russh::keys::ssh_key::Fingerprint,
        }

        impl russh::server::Handler for CaOnlyServer {
            type Error = russh::Error;

            // Plain public keys retain the default rejection. Only a valid
            // certificate issued by this CA for this user can authenticate.
            async fn auth_openssh_certificate(
                &mut self,
                user: &str,
                certificate: &russh::keys::Certificate,
            ) -> Result<russh::server::Auth, Self::Error> {
                let valid = certificate.validate_at(1, &[self.ca]).is_ok()
                    && certificate.valid_principals().iter().any(|p| p == user);
                Ok(if valid {
                    russh::server::Auth::Accept
                } else {
                    russh::server::Auth::reject()
                })
            }
        }

        tokio::time::timeout(Duration::from_secs(10), async {
            let (ca_key, user_key, certificate) = certificate_fixture();
            let identity = AgentIdentity::Certificate {
                certificate: certificate.clone(),
                comment: "CA-issued identity".to_string(),
            };
            let mapped = PublicKey::from_identity(&identity);
            let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
            let config = russh::server::Config {
                keys: vec![test_private_key(4)],
                auth_rejection_time: Duration::ZERO,
                auth_rejection_time_initial: Some(Duration::ZERO),
                ..Default::default()
            };
            let server_task = tokio::spawn(async move {
                russh::server::run_stream(
                    Arc::new(config),
                    server_stream,
                    CaOnlyServer {
                        ca: ca_key.fingerprint(Default::default()),
                    },
                )
                .await
                .unwrap()
                .await
                .unwrap();
            });
            let handler = super::super::ClientHandler {
                host_key: Arc::new(Mutex::new(None)),
                banner: Arc::new(Mutex::new(None)),
                forwards: Arc::new(super::super::forward::Registry::new(Arc::new(
                    super::super::channel::Settings::new(),
                ))),
            };
            let mut handle = russh::client::connect_stream(
                Arc::new(russh::client::Config::default()),
                client_stream,
                handler,
            )
            .await
            .unwrap();

            // Prove that this server does not accept the same key without its
            // CA certificate before exercising the agent-backed certificate.
            let bare_result = handle
                .authenticate_publickey(
                    "testuser",
                    russh::keys::PrivateKeyWithHashAlg::new(Arc::new(user_key.clone()), None),
                )
                .await
                .unwrap();
            assert!(!bare_result.success());

            let (agent_stream, mut agent_peer) = tokio::io::duplex(64 * 1024);
            let mut agent = AgentClient::connect(agent_stream);
            let agent_task = tokio::spawn(async move {
                let length = agent_peer.read_u32().await.unwrap() as usize;
                let mut request = vec![0; length];
                agent_peer.read_exact(&mut request).await.unwrap();
                assert_eq!(request[0], 13, "SSH2_AGENTC_SIGN_REQUEST");
                let mut fields = &request[1..];
                let blob = Vec::<u8>::decode(&mut fields).unwrap();
                assert_eq!(blob, certificate.to_bytes().unwrap());
                let data = Vec::<u8>::decode(&mut fields).unwrap();
                assert_eq!(u32::decode(&mut fields).unwrap(), 0);
                assert!(fields.is_empty());

                let mut signer = super::super::LocalKeySigner {
                    key: Arc::new(user_key),
                };
                let signed = signer
                    .auth_sign(&identity, None, data.clone())
                    .await
                    .unwrap();
                let mut response = vec![14]; // SSH2_AGENT_SIGN_RESPONSE
                response.extend_from_slice(&signed[data.len()..]);
                agent_peer.write_u32(response.len() as u32).await.unwrap();
                agent_peer.write_all(&response).await.unwrap();
            });

            let result = mapped
                .authenticate("testuser", &mut handle, &mut agent)
                .await
                .unwrap();
            assert!(
                result.success(),
                "CA-only server rejected agent certificate"
            );
            agent_task.await.unwrap();
            handle
                .disconnect(russh::Disconnect::ByApplication, "test complete", "")
                .await
                .unwrap();
            server_task.await.unwrap();
        })
        .await
        .expect("in-memory agent authentication must finish within ten seconds");
    }

    #[test]
    fn an_unconnected_agent_refuses_instead_of_pretending() {
        let session = Session::new().unwrap();
        let mut agent = Agent::new(session);
        let error = agent.list_identities().err().unwrap();
        assert!(error.to_string().contains("not connected"), "{error}");
        assert!(agent.identities().unwrap().is_empty());
    }
}
