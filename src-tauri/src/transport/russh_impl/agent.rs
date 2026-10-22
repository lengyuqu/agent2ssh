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
use russh::keys::PublicKey as RhPublicKey;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::{block_on, other, Session};
use crate::transport::types::Error;

/// The agent transports, behind one type.
///
/// `Send` matters as much as the I/O traits: the future that uses this holds
/// it across an `.await` and `block_on` requires `Send`.
enum AgentStream {
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
    key: RhPublicKey,
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
        // A certificate identity signs with the key inside the certificate, so
        // `public_key()` is the one to keep either way.
        let key = identity.public_key().into_owned();
        let blob = key.to_bytes().unwrap_or_default();
        Self {
            key,
            blob,
            comment: identity.comment().to_string(),
        }
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
        let client = block_on(connect_stream())?;
        *self.client.try_lock().expect("no other caller holds this") = Some(client);
        Ok(())
    }

    /// Fetch the agent's identity list. Separate from [`Self::identities`]
    /// because the facade mirrors libssh2's two-step shape.
    pub fn list_identities(&mut self) -> Result<(), Error> {
        let fetched = block_on(async {
            let mut guard = self.client.lock().await;
            let client = guard
                .as_mut()
                .ok_or_else(|| other("the SSH agent is not connected"))?;
            client.request_identities().await.map_err(|error| {
                other(format!("the SSH agent refused to list identities: {error}"))
            })
        })?;
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
        let key = identity.key.clone();
        let session = self.session.clone();

        let result = block_on(async {
            let mut client_guard = self.client.lock().await;
            if client_guard.is_none() {
                return Err(other("the SSH agent is not connected"));
            }
            let mut handle_guard = session.inner.handle.lock().await;
            let Some(handle) = handle_guard.as_mut() else {
                return Err(other("handshake has not run"));
            };
            // Same nest as the file-based path: outer "the server said
            // nothing", inner "any hash".
            let hash_alg = super::rsa_hash_alg(handle, key.algorithm().is_rsa()).await?;
            handle
                .authenticate_publickey_with(
                    username,
                    key,
                    hash_alg,
                    client_guard
                        .as_mut()
                        .expect("checked above that the agent is connected"),
                )
                .await
                .map_err(|error| other(format!("the SSH agent could not sign: {error}")))
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
    fn a_certificate_identity_maps_through_its_embedded_key() {
        // Same shape a certificate-backed identity takes: `public_key()`
        // unwraps to the certified key, which is what the server knows.
        let certificate_identity = identity("cert");
        let mapped = PublicKey::from_identity(&certificate_identity);
        assert_eq!(
            mapped.key.algorithm(),
            RhPublicKey::from_openssh(TEST_KEY).unwrap().algorithm()
        );
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
