//! The russh backend for the transport facade.
//!
//! The default backend since S2: the same synchronous facade as the libssh2 one
//! that preceded it, so nothing above `src/transport/` changed. Selecting
//! `transport-ssh2` swaps this module out for `legacy_ssh2`.
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
//! - `agent` — the Windows Pageant branch cannot be verified on this machine,
//!   and writing it blind is worse than naming it. See the Windows checklist in
//!   `docs/plans/russh-migration.md` §10; the work is a `Signer` wrapper because
//!   russh 0.60 ships none.
//! - `auth_methods` — russh does not expose the server's advertised list, so the
//!   "do not attempt a method the server never offered" short-circuit is gone.
//!   `embedded_ssh` prints "[unknown]" and tries anyway, which fails closed at
//!   the server.
//! - `set_keepalive` — only effective before `handshake()`; russh fixes the
//!   interval at connect time. `keepalive_send()` still works afterwards.

mod channel;
mod forward;
mod sftp;

use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use russh::client::{Config, Handle};
use russh::keys::PublicKey as RhPublicKey;
use russh::Disconnect;

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

    async fn check_server_key(&mut self, key: &RhPublicKey) -> Result<bool, Self::Error> {
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
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
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

        let handle = block_on(async move {
            stream.set_nonblocking(true)?;
            let stream = tokio::net::TcpStream::from_std(stream)?;
            russh::client::connect_stream(Arc::new(config), stream, handler).await
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
        _pubkey: Option<&std::path::Path>,
        privatekey: &std::path::Path,
        passphrase: Option<&str>,
    ) -> Result<(), Error> {
        self.ensure_connected()?;
        // russh derives the public half from the private key, so a mismatched
        // separate `.pub` file is ignored rather than rejected.
        let key = std::sync::Arc::new(russh::keys::load_secret_key(privatekey, passphrase)?);

        // `authenticate_publickey` rather than the `_with` variant: russh 0.60
        // ships no `Signer` implementation at all, so the signer-taking API
        // cannot be called without writing one. This path picks the RSA hash
        // itself.
        let result = block_on(async {
            let mut guard = self.inner.handle.lock().await;
            let Some(handle) = guard.as_mut() else {
                return Err(other("handshake has not run"));
            };
            // `best_supported_rsa_hash` nests once more than it looks: the outer
            // option is "the server said nothing", the inner one is "any hash".
            let hash_alg = handle.best_supported_rsa_hash().await?.flatten();
            let signer = russh::keys::PrivateKeyWithHashAlg::new(key, hash_alg);
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
        Err(unsupported("the SSH agent"))
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

/// Not implemented; see the module-level gap list.
pub struct Agent;

impl Agent {
    pub fn connect(&mut self) -> Result<(), Error> {
        Err(unsupported("the SSH agent"))
    }

    pub fn list_identities(&mut self) -> Result<(), Error> {
        Err(unsupported("the SSH agent"))
    }

    pub fn identities(&self) -> Result<Vec<PublicKey>, Error> {
        Err(unsupported("the SSH agent"))
    }

    pub fn userauth(&self, _username: &str, _identity: &PublicKey) -> Result<(), Error> {
        Err(unsupported("the SSH agent"))
    }
}

/// One agent identity. Only used by `Agent`, which is not implemented here.
pub struct PublicKey;

impl PublicKey {
    pub fn blob(&self) -> &[u8] {
        &[]
    }

    pub fn comment(&self) -> &str {
        ""
    }
}
