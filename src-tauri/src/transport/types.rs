//! Backend-neutral types for the SSH transport facade.
//!
//! Nothing here mentions `ssh2`, `libssh2` or `russh`. That is the point: the
//! rest of the crate talks to these types, so swapping the backend is a change
//! inside [`super::legacy_ssh2`] (S0) or a new backend module (S1) rather than a
//! change in 9.5k lines of session code.
//!
//! When a type here mirrors one from `ssh2`, the shape is deliberate: identical
//! field names and enum spellings, so S0 can be a pure seam and the compiler can
//! prove that no behaviour moved.

use std::fmt;

/// Which broad category a transport failure falls into.
///
/// Replaces libssh2's numeric `ErrorCode::Session(-N)` codes. The four named
/// variants are the ones call sites actually discriminate on; everything else is
/// [`ErrorKind::Other`], which keeps [`Error::message`] as the only thing a user
/// sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Key exchange failed: no common KEX, cipher or MAC algorithm.
    /// libssh2 `-5`.
    KeyExchange,
    /// The server rejected the offered public key.
    /// libssh2 `-18` and `-19`.
    PublicKeyRefused,
    /// Non-blocking call with no data available yet. libssh2 `-37`.
    WouldBlock,
    /// A failure the backend could not describe, so the message is generic.
    /// libssh2 `-1`. Under russh this can never occur — every failure is typed —
    /// which is also why the key-format hint built on it stops firing there.
    Opaque,
    /// Anything the facade does not need to name.
    Other,
}

/// A transport error: a category plus the backend's own description.
///
/// The message is kept verbatim (for libssh2 that is `"[Session(-5)] <text>"`,
/// the same string the `ssh2` crate's `Display` produced) so user-facing text
/// does not shift underneath this refactor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    message: String,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// The host key algorithm the server presented.
///
/// The spelling of each variant mirrors `ssh2::HostKeyType`. The *strings* are
/// what matter downstream: `embedded_ssh::host_key_algorithm` turns these into
/// the exact names written to `known_hosts.json`, so they must not change with
/// the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyKind {
    Rsa,
    Dss,
    Ecdsa256,
    Ecdsa384,
    Ecdsa521,
    Ed25519,
    Unknown,
}

/// Which host-key hash to compute. Mirrors `ssh2::HashType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashType {
    Md5,
    Sha1,
    Sha256,
}

/// A negotiable method family. Mirrors `ssh2::MethodType`, minus the numeric
/// discriminants — the backend maps them.
///
/// `LangCs`/`LangSc` are never configured (no `SshAlgoPrefs` field) but are kept
/// so `supported_algs` can answer for them the way libssh2 does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodType {
    Kex,
    HostKey,
    CryptCs,
    CryptSc,
    MacCs,
    MacSc,
    CompCs,
    CompSc,
    LangCs,
    LangSc,
    SignAlgo,
}

/// File metadata, as SFTP reports it.
///
/// Field-for-field the same as `ssh2::FileStat`, including `atime` — the
/// long-format listing prints it as the access column.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileStat {
    pub size: Option<u64>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub perm: Option<u32>,
    pub atime: Option<u64>,
    pub mtime: Option<u64>,
}

/// SFTP open flags.
///
/// A hand-rolled bitset rather than the `bitflags` crate: S0 may not add a
/// dependency, and the crate only ever combines `WRITE | APPEND`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OpenFlags(u32);

impl OpenFlags {
    pub const READ: Self = Self(1 << 0);
    pub const WRITE: Self = Self(1 << 1);
    pub const APPEND: Self = Self(1 << 2);
    pub const CREATE: Self = Self(1 << 3);
    pub const TRUNCATE: Self = Self(1 << 4);
    pub const EXCLUSIVE: Self = Self(1 << 5);

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for OpenFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for OpenFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Whether an SFTP open targets a file or a directory. Mirrors `ssh2::OpenType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenType {
    File,
    Dir,
}

/// The reason code sent with a disconnect. Values are the SSH_MSG_DISCONNECT
/// codes from RFC 4253 §11.1.
///
/// No call site passes anything but `None` today; the enum exists so the
/// signature stays faithful and a future caller has somewhere to go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisconnectCode {
    HostNotAllowedToConnect = 1,
    ProtocolError = 2,
    KeyExchangeFailed = 3,
    Reserved = 4,
    MacError = 5,
    CompressionError = 6,
    ServiceNotAvailable = 7,
    ProtocolVersionNotSupported = 8,
    HostKeyNotVerifiable = 9,
    ConnectionLost = 10,
    ByApplication = 11,
    TooManyConnections = 12,
    AuthCancelledByUser = 13,
    NoMoreAuthMethodsAvailable = 14,
    IllegalUserName = 15,
}

impl DisconnectCode {
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// One keyboard-interactive challenge.
///
/// Identical to `ssh2::Prompt`: `text` is what to show, `echo` decides whether
/// the answer is displayed as it is typed.
pub struct Prompt<'a> {
    pub text: std::borrow::Cow<'a, str>,
    pub echo: bool,
}

/// The callback keyboard-interactive authentication drives.
///
/// Mirrors `ssh2::KeyboardInteractivePrompt`. The signature is `&[Prompt]` and a
/// `Vec<String>` answer — one string per prompt, in order.
pub trait KeyboardInteractivePrompt {
    fn prompt<'a>(
        &mut self,
        username: &str,
        instructions: &str,
        prompts: &[Prompt<'a>],
    ) -> Vec<String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_flags_compose_and_report() {
        let flags = OpenFlags::WRITE | OpenFlags::APPEND;
        assert!(flags.contains(OpenFlags::WRITE));
        assert!(flags.contains(OpenFlags::APPEND));
        assert!(!flags.contains(OpenFlags::READ));
        assert!(!flags.is_empty());
        assert_eq!(flags.bits(), 0b110);
        assert!(OpenFlags::default().is_empty());
    }

    #[test]
    fn error_keeps_kind_and_verbatim_message() {
        let error = Error::new(ErrorKind::WouldBlock, "[Session(-37)] would block");
        assert_eq!(error.kind(), ErrorKind::WouldBlock);
        assert_eq!(error.to_string(), "[Session(-37)] would block");
        assert_eq!(error.message(), "[Session(-37)] would block");
    }

    #[test]
    fn disconnect_codes_are_the_rfc_values() {
        assert_eq!(DisconnectCode::ByApplication.code(), 11);
        assert_eq!(DisconnectCode::KeyExchangeFailed.code(), 3);
    }
}
