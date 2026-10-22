//! The `ssh2` / libssh2 backend for the transport facade.
//!
//! This is the only file in the crate allowed to name `ssh2`. Everything it
//! exposes is expressed in [`super::types`], so the call sites compile against
//! the facade and a later backend (russh) can be dropped in without touching
//! them.
//!
//! S0 keeps behaviour exactly where it was: every method forwards to the same
//! `ssh2` call, with the same argument shape and the same error text. The one
//! thing that changes is *where* the libssh2 numeric codes are interpreted —
//! [`Error::from`] is now the single place that knows `-5`, `-18`, `-19`, `-37`
//! and `-1`.

use std::io::{Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::path::Path;

use super::types::{
    DisconnectCode, Error, ErrorKind, FileStat, HashType, HostKeyKind, KeyboardInteractivePrompt,
    MethodType, OpenFlags, OpenType, Prompt,
};

impl From<ssh2::Error> for Error {
    fn from(error: ssh2::Error) -> Self {
        let kind = match error.code() {
            ssh2::ErrorCode::Session(-5) => ErrorKind::KeyExchange,
            ssh2::ErrorCode::Session(-18) | ssh2::ErrorCode::Session(-19) => {
                ErrorKind::PublicKeyRefused
            }
            ssh2::ErrorCode::Session(-37) => ErrorKind::WouldBlock,
            ssh2::ErrorCode::Session(-1) => ErrorKind::Opaque,
            _ => ErrorKind::Other,
        };
        Error::new(kind, error.to_string())
    }
}

fn host_key_kind(kind: ssh2::HostKeyType) -> HostKeyKind {
    match kind {
        ssh2::HostKeyType::Rsa => HostKeyKind::Rsa,
        ssh2::HostKeyType::Dss => HostKeyKind::Dss,
        ssh2::HostKeyType::Ecdsa256 => HostKeyKind::Ecdsa256,
        ssh2::HostKeyType::Ecdsa384 => HostKeyKind::Ecdsa384,
        ssh2::HostKeyType::Ecdsa521 => HostKeyKind::Ecdsa521,
        ssh2::HostKeyType::Ed25519 => HostKeyKind::Ed25519,
        ssh2::HostKeyType::Unknown => HostKeyKind::Unknown,
    }
}

fn hash_type(hash: HashType) -> ssh2::HashType {
    match hash {
        HashType::Md5 => ssh2::HashType::Md5,
        HashType::Sha1 => ssh2::HashType::Sha1,
        HashType::Sha256 => ssh2::HashType::Sha256,
    }
}

fn method_type(method: MethodType) -> ssh2::MethodType {
    match method {
        MethodType::Kex => ssh2::MethodType::Kex,
        MethodType::HostKey => ssh2::MethodType::HostKey,
        MethodType::CryptCs => ssh2::MethodType::CryptCs,
        MethodType::CryptSc => ssh2::MethodType::CryptSc,
        MethodType::MacCs => ssh2::MethodType::MacCs,
        MethodType::MacSc => ssh2::MethodType::MacSc,
        MethodType::CompCs => ssh2::MethodType::CompCs,
        MethodType::CompSc => ssh2::MethodType::CompSc,
        MethodType::LangCs => ssh2::MethodType::LangCs,
        MethodType::LangSc => ssh2::MethodType::LangSc,
        MethodType::SignAlgo => ssh2::MethodType::SignAlgo,
    }
}

fn disconnect_code(code: DisconnectCode) -> ssh2::DisconnectCode {
    use ssh2::DisconnectCode as S;
    match code {
        DisconnectCode::HostNotAllowedToConnect => S::HostNotAllowedToConnect,
        DisconnectCode::ProtocolError => S::ProtocolError,
        DisconnectCode::KeyExchangeFailed => S::KeyExchangeFailed,
        DisconnectCode::Reserved => S::Reserved,
        DisconnectCode::MacError => S::MacError,
        DisconnectCode::CompressionError => S::CompressionError,
        DisconnectCode::ServiceNotAvailable => S::ServiceNotAvailable,
        DisconnectCode::ProtocolVersionNotSupported => S::ProtocolVersionNotSupported,
        DisconnectCode::HostKeyNotVerifiable => S::HostKeyNotVerifiable,
        DisconnectCode::ConnectionLost => S::ConnectionLost,
        DisconnectCode::ByApplication => S::ByApplication,
        DisconnectCode::TooManyConnections => S::TooManyConnections,
        DisconnectCode::AuthCancelledByUser => S::AuthCancelledByUser,
        DisconnectCode::NoMoreAuthMethodsAvailable => S::NoMoreAuthMethodsAvailable,
        DisconnectCode::IllegalUserName => S::IllegalUserName,
    }
}

fn open_flags(flags: OpenFlags) -> ssh2::OpenFlags {
    // `TRUNCATE` and `EXCLUSIVE` imply `CREATE` in libssh2 as well; the facade's
    // bitset keeps them independent so the mapping is explicit here.
    let mut out = ssh2::OpenFlags::empty();
    if flags.contains(OpenFlags::READ) {
        out |= ssh2::OpenFlags::READ;
    }
    if flags.contains(OpenFlags::WRITE) {
        out |= ssh2::OpenFlags::WRITE;
    }
    if flags.contains(OpenFlags::APPEND) {
        out |= ssh2::OpenFlags::APPEND;
    }
    if flags.contains(OpenFlags::CREATE) {
        out |= ssh2::OpenFlags::CREATE;
    }
    if flags.contains(OpenFlags::TRUNCATE) {
        out |= ssh2::OpenFlags::TRUNCATE;
    }
    if flags.contains(OpenFlags::EXCLUSIVE) {
        out |= ssh2::OpenFlags::EXCLUSIVE;
    }
    out
}

fn ssh2_open_type(open_type: OpenType) -> ssh2::OpenType {
    match open_type {
        OpenType::File => ssh2::OpenType::File,
        OpenType::Dir => ssh2::OpenType::Dir,
    }
}

fn file_stat(stat: ssh2::FileStat) -> FileStat {
    FileStat {
        size: stat.size,
        uid: stat.uid,
        gid: stat.gid,
        perm: stat.perm,
        atime: stat.atime,
        mtime: stat.mtime,
    }
}

/// A single SSH connection.
///
/// `Clone` is shallow — it shares the underlying libssh2 session, exactly as
/// `ssh2::Session` does. `forward.rs` relies on that: it keeps one handle in an
/// `Arc<Mutex<Option<Session>>>` for `Drop` and hands clones to workers.
#[derive(Clone)]
pub struct Session {
    inner: ssh2::Session,
}

impl Session {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            inner: ssh2::Session::new()?,
        })
    }

    /// Hand the session an already-connected socket.
    ///
    /// Takes a concrete `TcpStream` rather than `AsRawFd`/`AsRawSocket`: the two
    /// call sites both pass one, and the portable bound keeps this signature
    /// identical on every platform.
    pub fn set_tcp_stream(&mut self, stream: TcpStream) {
        self.inner.set_tcp_stream(stream);
    }

    pub fn handshake(&mut self) -> Result<(), Error> {
        Ok(self.inner.handshake()?)
    }

    /// Blocking-mode budget for every libssh2 call. Re-settable at any time —
    /// `forward.rs` lowers it to 500 ms just before a disconnect, and the exec
    /// path raises it after the drain for the teardown guard.
    pub fn set_timeout(&self, timeout_ms: u32) {
        self.inner.set_timeout(timeout_ms);
    }

    /// Non-blocking mode. This is *not* the same knob as the timeout: it is what
    /// makes reads return `WouldBlock` immediately instead of waiting.
    pub fn set_blocking(&self, blocking: bool) {
        self.inner.set_blocking(blocking);
    }

    pub fn authenticated(&self) -> bool {
        self.inner.authenticated()
    }

    pub fn userauth_pubkey_file(
        &self,
        username: &str,
        pubkey: Option<&Path>,
        privatekey: &Path,
        passphrase: Option<&str>,
    ) -> Result<(), Error> {
        Ok(self
            .inner
            .userauth_pubkey_file(username, pubkey, privatekey, passphrase)?)
    }

    pub fn userauth_password(&self, username: &str, password: &str) -> Result<(), Error> {
        Ok(self.inner.userauth_password(username, password)?)
    }

    pub fn userauth_keyboard_interactive<P: KeyboardInteractivePrompt>(
        &self,
        username: &str,
        prompter: &mut P,
    ) -> Result<(), Error> {
        let mut adapter = PromptAdapter(prompter);
        Ok(self
            .inner
            .userauth_keyboard_interactive(username, &mut adapter)?)
    }

    /// The SSH agent, including Windows Pageant via ssh2's agent connector.
    pub fn agent(&self) -> Result<Agent, Error> {
        Ok(Agent {
            inner: self.inner.agent()?,
        })
    }

    /// The methods the server advertises for `username`, comma-separated.
    ///
    /// Empty when the server declines to answer. Callers use this to skip a
    /// password attempt the server would refuse anyway, so an empty string must
    /// keep meaning "ask anyway".
    pub fn auth_methods(&self, username: &str) -> Result<&str, Error> {
        Ok(self.inner.auth_methods(username)?)
    }

    pub fn banner(&self) -> Option<&str> {
        self.inner.banner()
    }

    pub fn host_key(&self) -> Option<(&[u8], HostKeyKind)> {
        self.inner
            .host_key()
            .map(|(key, kind)| (key, host_key_kind(kind)))
    }

    pub fn host_key_hash(&self, hash: HashType) -> Option<&[u8]> {
        self.inner.host_key_hash(hash_type(hash))
    }

    pub fn disconnect(
        &self,
        reason: Option<DisconnectCode>,
        description: &str,
        lang: Option<&str>,
    ) -> Result<(), Error> {
        Ok(self
            .inner
            .disconnect(reason.map(disconnect_code), description, lang)?)
    }

    pub fn sftp(&self) -> Result<Sftp, Error> {
        Ok(Sftp {
            inner: self.inner.sftp()?,
        })
    }

    pub fn channel_session(&self) -> Result<Channel, Error> {
        Ok(Channel {
            inner: self.inner.channel_session()?,
        })
    }

    pub fn channel_direct_tcpip(
        &self,
        host: &str,
        port: u16,
        src: Option<(&str, u16)>,
    ) -> Result<Channel, Error> {
        Ok(Channel {
            inner: self.inner.channel_direct_tcpip(host, port, src)?,
        })
    }

    pub fn channel_forward_listen(
        &self,
        remote_port: u16,
        host: Option<&str>,
        queue_maxsize: Option<u32>,
    ) -> Result<(Listener, u16), Error> {
        let (listener, bound) =
            self.inner
                .channel_forward_listen(remote_port, host, queue_maxsize)?;
        Ok((Listener { inner: listener }, bound))
    }

    pub fn set_keepalive(&self, want_reply: bool, interval: u32) {
        self.inner.set_keepalive(want_reply, interval);
    }

    pub fn keepalive_send(&self) -> Result<u32, Error> {
        Ok(self.inner.keepalive_send()?)
    }

    pub fn supported_algs(&self, method: MethodType) -> Result<Vec<&'static str>, Error> {
        Ok(self.inner.supported_algs(method_type(method))?)
    }

    pub fn method_pref(&self, method: MethodType, prefs: &str) -> Result<(), Error> {
        Ok(self.inner.method_pref(method_type(method), prefs)?)
    }
}

/// Adapts the facade's prompt trait to the shape `ssh2` expects.
struct PromptAdapter<'p, P: KeyboardInteractivePrompt>(&'p mut P);

impl<P: KeyboardInteractivePrompt> ssh2::KeyboardInteractivePrompt for PromptAdapter<'_, P> {
    fn prompt<'a>(
        &mut self,
        username: &str,
        instructions: &str,
        prompts: &[ssh2::Prompt<'a>],
    ) -> Vec<String> {
        let converted: Vec<Prompt<'a>> = prompts
            .iter()
            .map(|prompt| Prompt {
                text: prompt.text.clone(),
                echo: prompt.echo,
            })
            .collect();
        self.0.prompt(username, instructions, &converted)
    }
}

/// A connected SSH agent.
pub struct Agent {
    inner: ssh2::Agent,
}

impl Agent {
    pub fn connect(&mut self) -> Result<(), Error> {
        Ok(self.inner.connect()?)
    }

    pub fn list_identities(&mut self) -> Result<(), Error> {
        Ok(self.inner.list_identities()?)
    }

    pub fn identities(&self) -> Result<Vec<PublicKey>, Error> {
        Ok(self
            .inner
            .identities()?
            .into_iter()
            .map(|inner| PublicKey { inner })
            .collect())
    }

    pub fn userauth(&self, username: &str, identity: &PublicKey) -> Result<(), Error> {
        Ok(self.inner.userauth(username, &identity.inner)?)
    }
}

/// One identity held by the agent.
pub struct PublicKey {
    inner: ssh2::PublicKey,
}

impl PublicKey {
    pub fn blob(&self) -> &[u8] {
        self.inner.blob()
    }

    pub fn comment(&self) -> &str {
        self.inner.comment()
    }
}

/// An SSH channel: command, shell or forwarded TCP stream.
pub struct Channel {
    inner: ssh2::Channel,
}

impl Read for Channel {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for Channel {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl Channel {
    /// The channel's stderr stream.
    ///
    /// Takes `&mut self` even though libssh2 only needs `&self`: stdout and
    /// stderr are two independent reads off the same underlying connection, and
    /// the backend that replaces this one demultiplexes them out of a single
    /// message stream, which does need mutable access.
    pub fn stderr(&mut self) -> Stderr {
        Stderr {
            inner: self.inner.stderr(),
        }
    }

    pub fn eof(&self) -> bool {
        self.inner.eof()
    }

    pub fn exit_status(&self) -> Result<i32, Error> {
        Ok(self.inner.exit_status()?)
    }

    pub fn wait_close(&mut self) -> Result<(), Error> {
        Ok(self.inner.wait_close()?)
    }

    pub fn close(&mut self) -> Result<(), Error> {
        Ok(self.inner.close()?)
    }

    pub fn send_eof(&mut self) -> Result<(), Error> {
        Ok(self.inner.send_eof()?)
    }

    pub fn exec(&mut self, command: &str) -> Result<(), Error> {
        Ok(self.inner.exec(command)?)
    }

    pub fn shell(&mut self) -> Result<(), Error> {
        Ok(self.inner.shell()?)
    }

    /// Request a PTY.
    ///
    /// `mode` is accepted for signature compatibility and must stay `None`: the
    /// crate has no caller that sets terminal mode strings, and forwarding an
    /// unsupported value silently would be worse than refusing it.
    pub fn request_pty(
        &mut self,
        term: &str,
        mode: Option<&str>,
        dim: Option<(u32, u32, u32, u32)>,
    ) -> Result<(), Error> {
        if mode.is_some() {
            return Err(Error::new(
                ErrorKind::Other,
                "PTY mode strings are not supported by this transport",
            ));
        }
        Ok(self.inner.request_pty(term, None, dim)?)
    }

    pub fn request_pty_size(
        &mut self,
        width: u32,
        height: u32,
        width_px: Option<u32>,
        height_px: Option<u32>,
    ) -> Result<(), Error> {
        Ok(self
            .inner
            .request_pty_size(width, height, width_px, height_px)?)
    }
}

/// The stderr half of a [`Channel`].
pub struct Stderr {
    inner: ssh2::Stream,
}

impl Read for Stderr {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

/// An accepted channel from a remote forward.
pub struct Listener {
    inner: ssh2::Listener,
}

impl Listener {
    pub fn accept(&mut self) -> Result<Channel, Error> {
        Ok(Channel {
            inner: self.inner.accept()?,
        })
    }
}

/// An SFTP session.
pub struct Sftp {
    inner: ssh2::Sftp,
}

impl Sftp {
    pub fn open(&self, path: &Path) -> Result<File, Error> {
        Ok(File {
            inner: self.inner.open(path)?,
        })
    }

    pub fn create(&self, path: &Path) -> Result<File, Error> {
        Ok(File {
            inner: self.inner.create(path)?,
        })
    }

    pub fn open_mode(
        &self,
        path: &Path,
        flags: OpenFlags,
        mode: i32,
        open_type: OpenType,
    ) -> Result<File, Error> {
        Ok(File {
            inner: self.inner.open_mode(
                path,
                open_flags(flags),
                mode,
                ssh2_open_type(open_type),
            )?,
        })
    }

    pub fn stat(&self, path: &Path) -> Result<FileStat, Error> {
        Ok(file_stat(self.inner.stat(path)?))
    }

    pub fn readdir(&self, path: &Path) -> Result<Vec<(std::path::PathBuf, FileStat)>, Error> {
        Ok(self
            .inner
            .readdir(path)?
            .into_iter()
            .map(|(path, stat)| (path, file_stat(stat)))
            .collect())
    }

    pub fn mkdir(&self, path: &Path, mode: i32) -> Result<(), Error> {
        Ok(self.inner.mkdir(path, mode)?)
    }

    pub fn rmdir(&self, path: &Path) -> Result<(), Error> {
        Ok(self.inner.rmdir(path)?)
    }

    pub fn rename(&self, src: &Path, dst: &Path, overwrite: bool) -> Result<(), Error> {
        let flags = if overwrite {
            Some(ssh2::RenameFlags::OVERWRITE)
        } else {
            None
        };
        Ok(self.inner.rename(src, dst, flags)?)
    }

    pub fn unlink(&self, path: &Path) -> Result<(), Error> {
        Ok(self.inner.unlink(path)?)
    }
}

/// An open SFTP file handle.
pub struct File {
    inner: ssh2::File,
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for File {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl Seek for File {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

impl File {
    pub fn stat(&mut self) -> Result<FileStat, Error> {
        Ok(file_stat(self.inner.stat()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numeric-to-category mapping is the one thing that moved into this
    /// file, so it is the one thing worth pinning: it used to live at four
    /// separate call sites and each of them keyed off a bare integer.
    #[test]
    fn libssh2_codes_map_to_the_four_named_kinds() {
        let cases = [
            (-5, ErrorKind::KeyExchange),
            (-18, ErrorKind::PublicKeyRefused),
            (-19, ErrorKind::PublicKeyRefused),
            (-37, ErrorKind::WouldBlock),
            (-1, ErrorKind::Opaque),
            (-31, ErrorKind::Other),
        ];

        for (code, expected) in cases {
            let error = Error::from(ssh2::Error::new(
                ssh2::ErrorCode::Session(code),
                "backend message",
            ));
            assert_eq!(error.kind(), expected, "code {code}");
            assert!(error.message().contains("backend message"));
        }
    }

    /// `-18` carries libssh2's own explanation, so it must not be classified as
    /// opaque — the key-format hint keys off exactly this distinction.
    #[test]
    fn described_failures_are_not_opaque() {
        let described = Error::from(ssh2::Error::new(
            ssh2::ErrorCode::Session(-18),
            "Username/PublicKey combination invalid",
        ));
        assert_ne!(described.kind(), ErrorKind::Opaque);
    }

    #[test]
    fn open_flags_map_through_to_libssh2() {
        let flags = open_flags(OpenFlags::WRITE | OpenFlags::APPEND);
        assert!(flags.contains(ssh2::OpenFlags::WRITE));
        assert!(flags.contains(ssh2::OpenFlags::APPEND));
        assert!(!flags.contains(ssh2::OpenFlags::READ));
        assert!(open_flags(OpenFlags::default()).is_empty());
    }
}
