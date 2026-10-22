//! The russh backend for the transport facade.
//!
//! The only backend. It replaced a libssh2 implementation behind the same
//! synchronous facade, which is why nothing above `src/transport/` had to
//! change when it did.
//!
//! # Shape
//!
//! russh is async and this crate is not, so every method here wraps one
//! `block_on` on a **dedicated** runtime. The async-ness never escapes this
//! module; see the rule in `transport::mod`.
//!
//! Connecting is two-phase because the facade is: `new()` builds state,
//! `set_tcp_stream()` supplies the socket, `handshake()` connects. That order is
//! what lets `ssh_algo::apply_algo_prefs` set preferences before the handshake,
//! exactly as it did against libssh2.
//!
//! # Deliberate gaps in this slice
//!
//! These return a named error rather than pretending to work; they are the
//! remaining S1 work, listed in `docs/plans/russh-migration.md`:
//!
//! - `auth_methods` — russh does not expose the server's advertised list, so
//!   the "do not attempt a method the server never offered" short-circuit is
//!   gone. `embedded_ssh` prints "[unknown]" and tries anyway, which fails
//!   closed at the server.
//! - `set_keepalive` — only effective before `handshake()`; russh fixes the
//!   interval at connect time. `keepalive_send()` still works afterwards.
//!
//! `agent` is no longer one of them: see `agent.rs`, including why the note
//! that called for a wrapper `Signer` was reading a grep artifact.

mod agent;
mod channel;
mod forward;
mod sftp;

use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use russh::client::{Config, Handle};
use russh::Disconnect;

pub use agent::{Agent, PublicKey};
pub use channel::{Channel, Stderr};
pub use forward::Listener;
pub use sftp::{File, Sftp};

use super::types::{
    DisconnectCode, Error, ErrorKind, HashType, HostKeyKind, KeyboardInteractivePrompt, MethodType,
    Prompt,
};

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

pub(super) fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("agent2ssh-transport")
            .build()
            .expect("the transport runtime could not be built")
    })
}

/// Run `future` on the transport runtime and block until it finishes.
///
/// Called from inside another runtime this would panic (`block_on` refuses to
/// nest), so that case moves the work to a scoped thread. Callers in async
/// contexts are supposed to wrap the facade call in `spawn_blocking`; this is
/// the guard for the ones that have not been converted.
pub(super) fn block_on<F>(future: F) -> F::Output
where
    F: std::future::Future + Send,
    F::Output: Send,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::scope(|scope| {
            scope
                .spawn(|| runtime().block_on(future))
                .join()
                .expect("the transport runtime panicked")
        })
    } else {
        runtime().block_on(future)
    }
}

fn other(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Other, message)
}

fn unsupported(what: &str) -> Error {
    other(format!(
        "the russh transport backend does not implement {what} yet"
    ))
}

/// A private key on disk, used as a `Signer` with the hash the caller chooses.
///
/// Needed because russh's own certificate path signs through
/// `signature::Signer::try_sign`, which for an RSA key means the legacy
/// `ssh-rsa` (SHA-1) signature and cannot be told otherwise. OpenSSH has refused
/// SHA-1 RSA since 8.8, so an RSA certificate has to be signed with
/// rsa-sha2-256 or -512. `authenticate_openssh_cert` takes no hash;
/// `authenticate_certificate_with` takes one and a `Signer`, and this is that
/// signer. For non-RSA keys the hash is ignored, which is why the same type
/// serves ed25519 and ECDSA certificates too.
struct LocalKeySigner {
    key: std::sync::Arc<russh::keys::PrivateKey>,
}

/// What a [`LocalKeySigner`] can fail with.
///
/// `russh::auth::Signer` requires `From<russh::SendError>`; the others are
/// reported through the same string because nothing above the transport
/// distinguishes them.
#[derive(Debug)]
struct SignerFailed(String);

impl std::fmt::Display for SignerFailed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SignerFailed {}

impl From<russh::SendError> for SignerFailed {
    fn from(_: russh::SendError) -> Self {
        Self("the session ended while the key was signing".to_string())
    }
}

impl russh::Signer for LocalKeySigner {
    type Error = SignerFailed;

    fn auth_sign(
        &mut self,
        _identity: &russh::keys::agent::AgentIdentity,
        hash_alg: Option<russh::keys::HashAlg>,
        to_sign: Vec<u8>,
    ) -> impl std::future::Future<Output = Result<Vec<u8>, Self::Error>> + Send {
        use signature::Signer as _;

        let key = std::sync::Arc::clone(&self.key);
        async move {
            use russh::keys::ssh_key::encoding::Encode;

            let signature = match key.key_data() {
                russh::keys::ssh_key::private::KeypairData::Rsa(keypair) => {
                    (keypair, hash_alg).try_sign(&to_sign)
                }
                // Every other key type ignores `hash_alg`, exactly as
                // `PrivateKeyWithHashAlg::new` does.
                other => other.try_sign(&to_sign),
            }
            .map_err(|error| SignerFailed(format!("the key could not sign: {error}")))?;

            // The caller appends this buffer to the packet verbatim, so the
            // signature has to arrive in its wire form: one SSH `string`
            // holding `string algorithm || string signature`.
            let mut body = Vec::new();
            signature.encode(&mut body).map_err(|error| {
                SignerFailed(format!("the signature could not be encoded: {error}"))
            })?;
            let length = u32::try_from(body.len())
                .map_err(|_| SignerFailed("the signature is too large to encode".to_string()))?;

            let mut signed = to_sign;
            signed.extend_from_slice(&length.to_be_bytes());
            signed.extend_from_slice(&body);
            Ok(signed)
        }
    }
}

/// Load a private key, decrypting a legacy-encrypted PEM first.
///
/// The transport's loader decrypts `Proc-Type: 4,ENCRYPTED` itself but then
/// parses whatever it got as PKCS#1, so an encrypted *EC* key comes back as
/// `Pkcs1: PKCS#1 ASN.1 error`. `legacy_pem` decrypts that container here and
/// hands back a plaintext PEM, which the same loader parses for every key type —
/// so this is a pre-step, not a second code path.
fn load_private_key(
    path: &std::path::Path,
    passphrase: Option<&str>,
) -> Result<std::sync::Arc<russh::keys::PrivateKey>, Error> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        other(format!(
            "the private key at {} could not be read: {error}",
            path.display()
        ))
    })?;
    let decrypted = crate::legacy_pem::decrypt(&text, passphrase).map_err(other)?;
    match decrypted {
        Some(plain) => Ok(std::sync::Arc::new(russh::keys::decode_secret_key(
            &plain, None,
        )?)),
        None => Ok(std::sync::Arc::new(russh::keys::decode_secret_key(
            &text, passphrase,
        )?)),
    }
}

/// Read an OpenSSH user certificate (`<key>-cert.pub`).
///
/// `Certificate::from_openssh` wants one line, and the file `ssh-keygen -s`
/// writes is exactly that; trailing newlines are tolerated by the parser.
fn read_certificate(path: &std::path::Path) -> Result<russh::keys::Certificate, Error> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        other(format!(
            "the certificate at {} could not be read: {error}",
            path.display()
        ))
    })?;
    russh::keys::Certificate::from_openssh(text.trim()).map_err(|error| {
        other(format!(
            "{} is not a user certificate: {error}",
            path.display()
        ))
    })
}

/// The RSA signature hash to use for this server.
///
/// `best_supported_rsa_hash` returns `Option<Option<HashAlg>>`, and the two
/// levels mean different things: the outer one is "did the server advertise
/// `server-sig-algs` at all", the inner one "is a SHA-2 variant in the list".
///
/// Flattening the two — which the first version of this code did — turns
/// *unknown* into *SHA-1*, and `PrivateKeyWithHashAlg::new(key, None)` really
/// does mean the legacy `ssh-rsa`: its own documentation says so, and
/// `userauth_pubkey: signature algorithm ssh-rsa not in PubkeyAcceptedAlgorithms`
/// in an OpenSSH 10.3 server's log is what that produces. OpenSSH has refused
/// SHA-1 RSA by default since 8.8, so the unknown case has to resolve to SHA-2.
///
/// `None` survives in one place only: a server that advertised the extension
/// *without* any SHA-2 entry, which is a server old enough to need `ssh-rsa`.
async fn rsa_hash_alg(
    handle: &mut Handle<ClientHandler>,
    is_rsa: bool,
) -> Result<Option<russh::keys::HashAlg>, Error> {
    if !is_rsa {
        return Ok(None);
    }
    Ok(match handle.best_supported_rsa_hash().await? {
        Some(Some(alg)) => Some(alg),
        Some(None) => None,
        None => Some(russh::keys::HashAlg::Sha256),
    })
}

impl From<russh::Error> for Error {
    fn from(error: russh::Error) -> Self {
        let kind = match &error {
            // "No shared algorithm" is libssh2's -5, and it is what the
            // user-facing key-exchange hint keys off.
            russh::Error::NoCommonAlgo { .. } | russh::Error::KexInit | russh::Error::Kex => {
                ErrorKind::KeyExchange
            }
            _ => ErrorKind::Other,
        };
        Error::new(kind, error.to_string())
    }
}

impl From<russh::keys::Error> for Error {
    fn from(error: russh::keys::Error) -> Self {
        Error::new(ErrorKind::Other, error.to_string())
    }
}

/// The host key captured during the handshake.
///
/// It has to be owned: libssh2 could hand out a borrow into the session, but
/// russh only exposes the key inside the handshake callback, so the facade keeps
/// a copy and `host_key()` reads from it.
struct CapturedKey {
    blob: Vec<u8>,
    kind: HostKeyKind,
}

/// `check_server_key` accepts every key and records it.
///
/// Not a regression: libssh2 never verified either. This crate's trust model is
/// TOFU and runs after the handshake — `connect_embedded_ssh_inner` reads the
/// key, fingerprints it and calls `trust_or_verify_host_fingerprint`, which is
/// what actually refuses a changed host. Refusing here would move that decision
/// before the caller can compare it.
struct ClientHandler {
    host_key: Arc<Mutex<Option<CapturedKey>>>,
    banner: Arc<Mutex<Option<String>>>,
    forwards: Arc<forward::Registry>,
}

impl russh::client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        // A server may present a host certificate rather than a bare key. What
        // the fingerprint on record names is the key the certificate vouches
        // for, so that is what gets captured either way.
        let key = match key {
            russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            russh::keys::PublicKeyOrCertificate::Certificate(cert) => {
                cert.public_key().clone().into()
            }
        };
        if let Ok(blob) = key.to_bytes() {
            let kind = match key.algorithm() {
                russh::keys::Algorithm::Ed25519 => HostKeyKind::Ed25519,
                russh::keys::Algorithm::Rsa { .. } => HostKeyKind::Rsa,
                russh::keys::Algorithm::Dsa => HostKeyKind::Dss,
                russh::keys::Algorithm::Ecdsa { curve } => match curve {
                    russh::keys::EcdsaCurve::NistP256 => HostKeyKind::Ecdsa256,
                    russh::keys::EcdsaCurve::NistP384 => HostKeyKind::Ecdsa384,
                    russh::keys::EcdsaCurve::NistP521 => HostKeyKind::Ecdsa521,
                },
                _ => HostKeyKind::Unknown,
            };
            *self.host_key.lock().unwrap() = Some(CapturedKey { blob, kind });
        }
        Ok(true)
    }

    /// The server's pre-authentication banner (RFC 4252 §5.4).
    ///
    /// Captured rather than ignored: `core.rs` feeds it to the SFTP chunk-size
    /// heuristic, which sizes transfers by the server implementation.
    async fn auth_banner(
        &mut self,
        banner: &str,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        *self.banner.lock().unwrap() = Some(banner.to_string());
        Ok(())
    }

    /// An inbound connection for a forward we asked for.
    ///
    /// Keeping the channel *is* the accept: dropping it would tell the server
    /// the connection was refused. The consumer pulls it out of the registry
    /// from `Listener::accept`.
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        _connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: russh::client::ChannelOpenHandle,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        // The open must be answered explicitly: dropping this handle sends an
        // administrative rejection, which would tell the server the forward is
        // closed while the registry holds the other end.
        reply.accept().await;
        self.forwards.push(connected_port, channel);
        Ok(())
    }
}

/// State that exists before the socket is connected.
#[derive(Default)]
struct Pending {
    stream: Option<TcpStream>,
    prefs: Vec<(MethodType, String)>,
    keepalive: Option<u32>,
}

struct Inner {
    settings: Arc<channel::Settings>,
    host_key: Arc<Mutex<Option<CapturedKey>>>,
    banner: Arc<Mutex<Option<String>>>,
    authenticated: AtomicBool,
    pending: Mutex<Pending>,
    forwards: Arc<forward::Registry>,
    /// `Handle` is not `Clone`, and authentication needs `&mut`, so this is a
    /// tokio mutex: its guard is `Send` and can be held across an await.
    handle: tokio::sync::Mutex<Option<Handle<ClientHandler>>>,
    /// Set once the handshake has run, so the mutex is not taken on every call.
    connected: AtomicBool,
}

/// A single SSH connection.
#[derive(Clone)]
pub struct Session {
    inner: Arc<Inner>,
}

impl Session {
    pub fn new() -> Result<Self, Error> {
        let settings = Arc::new(channel::Settings::new());
        Ok(Self {
            inner: Arc::new(Inner {
                forwards: Arc::new(forward::Registry::new(Arc::clone(&settings))),
                settings,
                host_key: Arc::new(Mutex::new(None)),
                banner: Arc::new(Mutex::new(None)),
                authenticated: AtomicBool::new(false),
                pending: Mutex::new(Pending::default()),
                handle: tokio::sync::Mutex::new(None),
                connected: AtomicBool::new(false),
            }),
        })
    }

    pub fn set_tcp_stream(&mut self, stream: TcpStream) {
        self.inner.pending.lock().unwrap().stream = Some(stream);
    }

    pub fn handshake(&mut self) -> Result<(), Error> {
        let stream = self
            .inner
            .pending
            .lock()
            .unwrap()
            .stream
            .take()
            .ok_or_else(|| other("no TCP stream was provided"))?;

        let mut config = Config::default();
        {
            let pending = self.inner.pending.lock().unwrap();
            apply_prefs(&mut config, &pending.prefs)?;
            if let Some(interval) = pending.keepalive {
                config.keepalive_interval =
                    Some(std::time::Duration::from_secs(u64::from(interval)));
            }
        }

        let handler = ClientHandler {
            host_key: Arc::clone(&self.inner.host_key),
            banner: Arc::clone(&self.inner.banner),
            forwards: Arc::clone(&self.inner.forwards),
        };

        // The deadline has to bound the handshake itself, not just channel I/O.
        // A peer that accepts TCP and then says nothing leaves `connect_stream`
        // awaiting the banner forever; the callers that wrap a connect in
        // `tokio::time::timeout` (`ping_hosts_core`, `collect_health_snapshot`)
        // then return on schedule while the `spawn_blocking` worker stays parked
        // on this future, holding both a thread and the socket — once per host,
        // per poll. Dropping the future drops the stream, which is what closes
        // it and releases the worker; `connect_deadline.rs` asserts exactly that
        // by watching for EOF on the peer side.
        let deadline = self.inner.settings.timeout();

        let handle = block_on(async move {
            // `?` cannot convert these two: the block's error type is the
            // facade's, and only `russh::Error` has a `From` impl for it.
            stream.set_nonblocking(true).map_err(|error| {
                other(format!(
                    "the socket could not be made non-blocking: {error}"
                ))
            })?;
            let stream = tokio::net::TcpStream::from_std(stream).map_err(|error| {
                other(format!("the socket could not be handed to tokio: {error}"))
            })?;
            let connect = russh::client::connect_stream(Arc::new(config), stream, handler);
            let connected: Result<Handle<ClientHandler>, russh::Error> = match deadline {
                Some(limit) => match tokio::time::timeout(limit, connect).await {
                    Ok(result) => result,
                    Err(_) => {
                        return Err(other(format!(
                            "the SSH handshake did not finish within {} ms; the peer accepted \
                             the TCP connection but never completed the exchange",
                            limit.as_millis()
                        )))
                    }
                },
                None => connect.await,
            };
            connected.map_err(Error::from)
        })?;

        *self.inner.handle.blocking_lock() = Some(handle);
        self.inner.connected.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn set_timeout(&self, timeout_ms: u32) {
        self.inner.settings.set_timeout(timeout_ms);
    }

    pub fn set_blocking(&self, blocking: bool) {
        self.inner.settings.set_blocking(blocking);
    }

    pub fn authenticated(&self) -> bool {
        self.inner.authenticated.load(Ordering::SeqCst)
    }

    fn ensure_connected(&self) -> Result<(), Error> {
        if self.inner.connected.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(other("handshake has not run"))
        }
    }
}

fn apply_prefs(config: &mut Config, prefs: &[(MethodType, String)]) -> Result<(), Error> {
    // `Preferred` lives in a private module, so the fields are assigned in
    // place instead of naming the type.
    for (method, wanted) in prefs {
        let names: Vec<&str> = wanted
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .collect();
        if names.is_empty() {
            continue;
        }
        // Each category has its own name type with a fallible `TryFrom<&str>`,
        // which is what gives us libssh2's fail-closed behaviour for a typo
        // instead of silently ignored preferences.
        match method {
            MethodType::Kex => {
                let parsed = names
                    .iter()
                    .map(|name| russh::kex::Name::try_from(*name))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| other(format!("unsupported kex algorithm in '{wanted}'")))?;
                config.preferred.kex = parsed.into();
            }
            MethodType::HostKey => {
                let parsed = names
                    .iter()
                    .map(|name| name.parse::<russh::keys::Algorithm>())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| other(format!("unsupported host key algorithm in '{wanted}'")))?;
                config.preferred.key = parsed.into();
            }
            MethodType::CryptCs | MethodType::CryptSc => {
                let parsed = names
                    .iter()
                    .map(|name| russh::cipher::Name::try_from(*name))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| other(format!("unsupported cipher in '{wanted}'")))?;
                config.preferred.cipher = parsed.into();
            }
            MethodType::MacCs | MethodType::MacSc => {
                let parsed = names
                    .iter()
                    .map(|name| russh::mac::Name::try_from(*name))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| other(format!("unsupported mac in '{wanted}'")))?;
                config.preferred.mac = parsed.into();
            }
            MethodType::CompCs | MethodType::CompSc => {
                let parsed = names
                    .iter()
                    .map(|name| russh::compression::Name::try_from(*name))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| other(format!("unsupported compression in '{wanted}'")))?;
                config.preferred.compression = parsed.into();
            }
            // Not negotiated as a list: signature algorithms are chosen per
            // authentication attempt, languages are never configured.
            MethodType::SignAlgo | MethodType::LangCs | MethodType::LangSc => {}
        }
    }
    Ok(())
}

impl Session {
    fn auth_result(
        result: russh::client::AuthResult,
        authenticated: &AtomicBool,
    ) -> Result<(), Error> {
        match result {
            russh::client::AuthResult::Success => {
                authenticated.store(true, Ordering::SeqCst);
                Ok(())
            }
            russh::client::AuthResult::Failure { .. } => Err(Error::new(
                ErrorKind::PublicKeyRefused,
                "the server rejected these credentials",
            )),
        }
    }

    pub fn userauth_pubkey_file(
        &self,
        username: &str,
        pubkey: Option<&std::path::Path>,
        privatekey: &std::path::Path,
        passphrase: Option<&str>,
    ) -> Result<(), Error> {
        self.ensure_connected()?;
        // russh derives the public half from the private key, so a mismatched
        // separate `.pub` file is ignored rather than rejected.
        let key = load_private_key(privatekey, passphrase)?;

        // `pubkey` is the `-cert.pub` sibling when the app found one
        // (`embedded_ssh::user_certificate_for`). It is a *certificate*, not a
        // plain public key, and the server only accepts the certificate form
        // when the key itself is not authorized — which is exactly the
        // `TrustedUserCAKeys` setup this exists for.
        if let Some(cert_path) = pubkey {
            let cert = read_certificate(cert_path)?;
            let is_rsa = key.algorithm().is_rsa();
            let mut signer = LocalKeySigner {
                key: std::sync::Arc::clone(&key),
            };
            let result = block_on(async {
                let mut guard = self.inner.handle.lock().await;
                let Some(handle) = guard.as_mut() else {
                    return Err(other("handshake has not run"));
                };
                // A certificate is never signed with SHA-1. OpenSSH 8.8+ does not
                // offer the legacy `ssh-rsa-cert-v01` type at all, so an RSA
                // certificate gets SHA-2 even when the server advertised
                // nothing; `rsa_hash_alg`'s `None` means exactly that legacy
                // signature, which is right for a bare key and wrong here.
                let hash_alg = match rsa_hash_alg(handle, is_rsa).await? {
                    Some(algorithm) => Some(algorithm),
                    None if is_rsa => Some(russh::keys::HashAlg::Sha512),
                    None => None,
                };
                handle
                    .authenticate_certificate_with(username, cert, hash_alg, &mut signer)
                    .await
                    .map_err(|error| {
                        other(format!("the key could not sign the certificate: {error}"))
                    })
            })?;
            return Self::auth_result(result, &self.inner.authenticated);
        }

        // `authenticate_publickey` takes a `PrivateKeyWithHashAlg`, which is the
        // signer here; the `_with` variant would need a caller-supplied `Signer`
        // and buys nothing for a key that is on disk.
        let result = block_on(async {
            let mut guard = self.inner.handle.lock().await;
            let Some(handle) = guard.as_mut() else {
                return Err(other("handshake has not run"));
            };
            let hash_alg = rsa_hash_alg(handle, key.algorithm().is_rsa()).await?;
            let signer = russh::keys::PrivateKeyWithHashAlg::new(key.clone(), hash_alg);
            let first = handle.authenticate_publickey(username, signer).await?;
            if first.success() || !key.algorithm().is_rsa() || hash_alg.is_none() {
                return Ok(first);
            }
            // A server old enough that `server-sig-algs` lists only `ssh-rsa`,
            // or one that never sent the extension, may refuse the SHA-2
            // attempt. One legacy retry is what a pre-7.2 server needs, and it
            // is attempted only after the modern signature was refused.
            let signer = russh::keys::PrivateKeyWithHashAlg::new(key.clone(), None);
            Ok(handle.authenticate_publickey(username, signer).await?)
        })?;

        Self::auth_result(result, &self.inner.authenticated)
    }

    pub fn userauth_password(&self, username: &str, password: &str) -> Result<(), Error> {
        self.ensure_connected()?;
        let result = block_on(async {
            let mut guard = self.inner.handle.lock().await;
            let handle = guard
                .as_mut()
                .ok_or_else(|| other("handshake has not run"))?;
            Ok::<_, Error>(handle.authenticate_password(username, password).await?)
        })?;
        Self::auth_result(result, &self.inner.authenticated)
    }

    pub fn userauth_keyboard_interactive<P: KeyboardInteractivePrompt>(
        &self,
        username: &str,
        prompter: &mut P,
    ) -> Result<(), Error> {
        use russh::client::KeyboardInteractiveAuthResponse as Response;

        self.ensure_connected()?;
        let mut response = block_on(async {
            let mut guard = self.inner.handle.lock().await;
            let handle = guard
                .as_mut()
                .ok_or_else(|| other("handshake has not run"))?;
            Ok::<_, Error>(
                handle
                    .authenticate_keyboard_interactive_start(username, None::<String>)
                    .await?,
            )
        })?;

        loop {
            match response {
                Response::Success => {
                    self.inner.authenticated.store(true, Ordering::SeqCst);
                    return Ok(());
                }
                Response::Failure { .. } => {
                    return Err(Error::new(
                        ErrorKind::PublicKeyRefused,
                        "keyboard-interactive authentication was refused",
                    ))
                }
                Response::InfoRequest {
                    name,
                    instructions,
                    prompts,
                } => {
                    let converted: Vec<Prompt<'_>> = prompts
                        .iter()
                        .map(|prompt| Prompt {
                            text: std::borrow::Cow::Owned(prompt.prompt.clone()),
                            echo: prompt.echo,
                        })
                        .collect();
                    let answers = prompter.prompt(&name, &instructions, &converted);
                    response = block_on(async {
                        let mut guard = self.inner.handle.lock().await;
                        let handle = guard
                            .as_mut()
                            .ok_or_else(|| other("handshake has not run"))?;
                        Ok::<_, Error>(
                            handle
                                .authenticate_keyboard_interactive_respond(answers)
                                .await?,
                        )
                    })?;
                }
            }
        }
    }

    pub fn agent(&self) -> Result<Agent, Error> {
        // No fallible step here: the connection to the agent happens in
        // `Agent::connect`, because libssh2's two-step shape is what the
        // caller in `embedded_ssh` follows.
        Ok(Agent::new(self.clone()))
    }

    pub fn auth_methods(&self, _username: &str) -> Result<&str, Error> {
        Err(unsupported("auth_methods"))
    }

    pub fn banner(&self) -> Option<String> {
        self.inner.banner.lock().unwrap().clone()
    }

    pub fn host_key(&self) -> Option<(Vec<u8>, HostKeyKind)> {
        let captured = self.inner.host_key.lock().unwrap();
        captured
            .as_ref()
            .map(|captured| (captured.blob.clone(), captured.kind))
    }

    pub fn host_key_hash(&self, hash: HashType) -> Option<Vec<u8>> {
        use sha2::Digest as _;
        let captured = self.inner.host_key.lock().unwrap();
        let captured = captured.as_ref()?;
        match hash {
            // Only SHA-256 is used by this crate (`fingerprint_sha256`). The
            // other two would need extra digest crates for no caller.
            HashType::Sha256 => Some(sha2::Sha256::digest(&captured.blob).to_vec()),
            HashType::Md5 | HashType::Sha1 => None,
        }
    }

    pub fn disconnect(
        &self,
        _reason: Option<DisconnectCode>,
        description: &str,
        _lang: Option<&str>,
    ) -> Result<(), Error> {
        if !self.inner.connected.load(Ordering::SeqCst) {
            return Ok(());
        }
        block_on(async {
            let guard = self.inner.handle.lock().await;
            let Some(handle) = guard.as_ref() else {
                return Err(other("handshake has not run"));
            };
            handle
                .disconnect(Disconnect::ByApplication, description, "")
                .await?;
            Ok(())
        })
    }

    pub fn sftp(&self) -> Result<Sftp, Error> {
        self.ensure_connected()?;
        let session = block_on(async {
            let guard = self.inner.handle.lock().await;
            let Some(handle) = guard.as_ref() else {
                return Err(other("handshake has not run"));
            };
            let channel = handle.channel_open_session().await?;
            // `into_stream` hands over raw bytes; the subsystem request is what
            // turns the channel into an SFTP conversation. Without it sshd starts
            // a shell, the INIT packet is read as shell input, and every request
            // ends in russh-sftp's ten-second timeout.
            channel.request_subsystem(false, "sftp").await?;
            Ok::<_, Error>(russh_sftp::client::SftpSession::new(channel.into_stream()).await?)
        })?;
        Ok(Sftp::new(session))
    }

    pub fn channel_session(&self) -> Result<Channel, Error> {
        self.ensure_connected()?;
        let channel = block_on(async {
            let guard = self.inner.handle.lock().await;
            let Some(handle) = guard.as_ref() else {
                return Err(other("handshake has not run"));
            };
            Ok::<_, Error>(handle.channel_open_session().await?)
        })?;
        Ok(channel::open(channel, Arc::clone(&self.inner.settings)))
    }

    pub fn channel_direct_tcpip(
        &self,
        host: &str,
        port: u16,
        _src: Option<(&str, u16)>,
    ) -> Result<Channel, Error> {
        self.ensure_connected()?;
        let channel = block_on(async {
            let guard = self.inner.handle.lock().await;
            let Some(handle) = guard.as_ref() else {
                return Err(other("handshake has not run"));
            };
            Ok::<_, Error>(
                handle
                    .channel_open_direct_tcpip(host, u32::from(port), "127.0.0.1", 0)
                    .await?,
            )
        })?;
        Ok(channel::open(channel, Arc::clone(&self.inner.settings)))
    }

    pub fn channel_forward_listen(
        &self,
        remote_port: u16,
        host: Option<&str>,
        _queue_maxsize: Option<u32>,
    ) -> Result<(Listener, u16), Error> {
        self.ensure_connected()?;
        let address = host.unwrap_or("0.0.0.0").to_string();
        let bound = block_on(async {
            let guard = self.inner.handle.lock().await;
            let Some(handle) = guard.as_ref() else {
                return Err(other("handshake has not run"));
            };
            Ok::<_, Error>(
                handle
                    .tcpip_forward(address, u32::from(remote_port))
                    .await?,
            )
        })?;
        // russh's `tcpip_forward` reports the server's port **only when we asked
        // for 0** (dynamic allocation); for an explicit port it returns 0. Taking
        // that literally binds the listener to port 0, so every arriving channel
        // is filed under 0 and `accept()` never sees it -- the connection hangs
        // open with no data. Prefer a non-zero answer, otherwise trust the port
        // we asked for.
        let effective = if bound != 0 {
            bound
        } else if remote_port != 0 {
            u32::from(remote_port)
        } else {
            return Err(other("the server did not report a bound port"));
        };
        let port = u16::try_from(effective).map_err(|_| {
            other(format!(
                "the server reported an unrepresentable bound port {effective}"
            ))
        })?;
        Ok((
            Listener::new(u32::from(port), Arc::clone(&self.inner.forwards)),
            port,
        ))
    }

    pub fn set_keepalive(&self, _want_reply: bool, interval: u32) {
        let mut pending = self.inner.pending.lock().unwrap();
        if !self.inner.connected.load(Ordering::SeqCst) {
            pending.keepalive = Some(interval);
        }
        // After the handshake russh's interval is fixed; `keepalive_send` is the
        // path the liveness checks use.
    }

    pub fn keepalive_send(&self) -> Result<u32, Error> {
        self.ensure_connected()?;
        block_on(async {
            let guard = self.inner.handle.lock().await;
            let handle = guard
                .as_ref()
                .ok_or_else(|| other("handshake has not run"))?;
            handle.send_keepalive(true).await?;
            Ok::<u32, Error>(0)
        })
    }

    pub fn supported_algs(&self, method: MethodType) -> Result<Vec<&'static str>, Error> {
        Ok(supported_names(method))
    }

    pub fn method_pref(&self, method: MethodType, prefs: &str) -> Result<(), Error> {
        let mut pending = self.inner.pending.lock().unwrap();
        if self.inner.connected.load(Ordering::SeqCst) {
            return Err(other(
                "algorithm preferences must be set before the handshake",
            ));
        }
        pending.prefs.push((method, prefs.to_string()));
        Ok(())
    }
}

/// The names this backend accepts, for `ssh_algo::narrow` to filter against.
///
/// Deliberately **permissive**: a name listed here but unknown to russh still
/// fails closed in `apply_prefs`, while a name missing here would be dropped
/// silently — so the error is on the generous side. It is a snapshot, not a
/// query; russh exposes no runtime capability list.
fn supported_names(method: MethodType) -> Vec<&'static str> {
    const KEX: &[&str] = &[
        "curve25519-sha256",
        "curve25519-sha256@libssh.org",
        "ecdh-sha2-nistp256",
        "ecdh-sha2-nistp384",
        "ecdh-sha2-nistp521",
        "diffie-hellman-group16-sha512",
        "diffie-hellman-group18-sha512",
        "diffie-hellman-group14-sha256",
    ];
    const HOSTKEY: &[&str] = &[
        "ssh-ed25519",
        "ecdsa-sha2-nistp256",
        "ecdsa-sha2-nistp384",
        "ecdsa-sha2-nistp521",
        "rsa-sha2-512",
        "rsa-sha2-256",
        "ssh-rsa",
    ];
    const CIPHER: &[&str] = &[
        "chacha20-poly1305@openssh.com",
        "aes256-gcm@openssh.com",
        "aes128-gcm@openssh.com",
        "aes256-ctr",
        "aes192-ctr",
        "aes128-ctr",
    ];
    const MAC: &[&str] = &[
        "hmac-sha2-256-etm@openssh.com",
        "hmac-sha2-512-etm@openssh.com",
        "hmac-sha2-256",
        "hmac-sha2-512",
    ];
    // russh is built without the `flate2` feature here, so compression is `none`.
    const COMP: &[&str] = &["none"];

    match method {
        MethodType::Kex => KEX.to_vec(),
        MethodType::HostKey => HOSTKEY.to_vec(),
        MethodType::CryptCs | MethodType::CryptSc => CIPHER.to_vec(),
        MethodType::MacCs | MethodType::MacSc => MAC.to_vec(),
        MethodType::CompCs | MethodType::CompSc => COMP.to_vec(),
        MethodType::LangCs | MethodType::LangSc | MethodType::SignAlgo => Vec::new(),
    }
}
