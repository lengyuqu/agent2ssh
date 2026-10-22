//! Private-key file formats, and whether the linked libssh2 can read them.
//!
//! ## Why this exists
//!
//! A key file that a *server* would happily accept can still be unusable here,
//! because the file format support lives in libssh2 and is selected by the
//! crypto backend `libssh2-sys` was built against — not by anything the user
//! can see. A WinCNG build's key-file reader (`_libssh2_wincng_load_private`)
//! tries exactly two PEM headers, `-----BEGIN RSA PRIVATE KEY-----` and
//! `-----BEGIN DSA PRIVATE KEY-----`, and returns a plain `-1` when neither
//! matches. The OpenSSH-container parser (`_libssh2_openssh_pem_parse`) is
//! compiled into the OpenSSL and mbedTLS backends only, so on such a build a
//! `-----BEGIN OPENSSH PRIVATE KEY-----` file — which `ssh-keygen` has written
//! *by default since OpenSSH 7.8, including for `-t rsa`* — never reaches the
//! wire.
//!
//! Windows used to be exactly that case. It is not any more: `src-tauri`
//! enables `ssh2`'s `openssl-on-win32`, so the Windows build links OpenSSL and
//! reads the container format like every other platform. What remains is the
//! diagnosis path below, which is now driven by a capability probe rather than
//! by a platform guess — see [`reads_openssh_private_keys`] — so it stays
//! correct whatever backend a future build selects.
//!
//! That `-1` is not one of libssh2's own error constants, so `ssh2` cannot
//! describe it: `Error::from_errno` has no arm for it and the caller sees
//! `[Session(-1)] unknown error`. Nothing in that message points at the key
//! file, which is why authentication used to fail with no actionable signal.
//!
//! This module turns that into a diagnosis. See [`unusable_key_hint`].

use anyhow::Result;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::OnceLock;

use crate::ssh_algo::supported_algorithms;
use crate::transport::{Error, ErrorKind, MethodType};

/// How many lines to scan for the PEM armour header.
///
/// The header is the first non-empty line of a well-formed PEM file. The small
/// allowance covers blank lines or a leading `Proc-Type`-style preamble without
/// ever reading a whole private key into memory just to look at its first line.
const HEADER_SCAN_LINES: usize = 16;

/// The on-disk encoding of a private key file, identified by its armour header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateKeyFormat {
    /// `-----BEGIN OPENSSH PRIVATE KEY-----`.
    ///
    /// The container format `ssh-keygen` writes by default since OpenSSH 7.8,
    /// for every key type. Unreadable by the WinCNG backend.
    OpensshContainer,
    /// `-----BEGIN RSA PRIVATE KEY-----` — traditional PKCS#1 PEM.
    ///
    /// The only RSA encoding the WinCNG backend can load, and therefore the
    /// one format that makes key authentication work on Windows.
    PemRsa,
    /// `-----BEGIN DSA PRIVATE KEY-----` — also readable by WinCNG, but DSA is
    /// excluded by [`crate::ssh_algo`]'s defaults.
    PemDsa,
    /// `-----BEGIN [RSA|EC] PRIVATE KEY-----` (PKCS#8) or an encrypted
    /// `-----BEGIN ENCRYPTED PRIVATE KEY-----` container.
    Pkcs8,
    /// An armour header was found, but it is not a private key form we know.
    OtherArmoured,
    /// No armour header found, or the file could not be read.
    Unknown,
}

/// Identify the encoding of the private key at `path` by its armour header.
///
/// Never fails: an unreadable or non-PEM file is [`PrivateKeyFormat::Unknown`].
/// The result is used to enrich an error that has already occurred, so a second
/// failure here must not replace the original one.
pub fn detect_private_key_format(path: &Path) -> PrivateKeyFormat {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return PrivateKeyFormat::Unknown,
    };

    for line in BufReader::new(file).lines().take(HEADER_SCAN_LINES) {
        let line = match line {
            Ok(line) => line,
            Err(_) => return PrivateKeyFormat::Unknown,
        };
        let label = match armour_label(line.trim()) {
            Some(label) => label,
            None => continue,
        };
        return match label.as_str() {
            "OPENSSH PRIVATE KEY" => PrivateKeyFormat::OpensshContainer,
            "RSA PRIVATE KEY" | "ENCRYPTED RSA PRIVATE KEY" => PrivateKeyFormat::PemRsa,
            "DSA PRIVATE KEY" | "ENCRYPTED DSA PRIVATE KEY" => PrivateKeyFormat::PemDsa,
            "PRIVATE KEY" | "ENCRYPTED PRIVATE KEY" | "EC PRIVATE KEY" => PrivateKeyFormat::Pkcs8,
            _ => PrivateKeyFormat::OtherArmoured,
        };
    }

    PrivateKeyFormat::Unknown
}

/// The label of a `-----BEGIN <label>-----` / `-----END <label>-----` line.
fn armour_label(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix("-----BEGIN ")
        .or_else(|| line.strip_prefix("-----END "))?;
    let label = rest.strip_suffix("-----")?;
    if label.is_empty() {
        return None;
    }
    Some(label.to_string())
}

/// Whether the linked libssh2 can read OpenSSH-container private keys.
///
/// The probe reads this build's **client** host-key algorithm list. That list is
/// used as a proxy for "was the OpenSSH-container parser compiled in?", and the
/// two are equivalent for every backend libssh2 ships:
///
/// - the parser is called only from `openssl.c` and `mbedtls.c`;
/// - the backends that lack it (WinCNG, libgcrypt) are also the ones with no
///   ed25519/ECDSA primitives, so they never advertise `ssh-ed25519` or
///   `ecdsa-sha2-*`;
/// - conversely a backend that advertises ed25519/ECDSA always has the parser.
///
/// Using the algorithm list rather than a `cfg!(windows)` keeps this correct
/// when the build changes: enabling `ssh2`'s `openssl-on-win32` feature gives
/// Windows OpenSSL, which supplies both the primitives and the parser, and this
/// function flips to `true` on its own.
///
/// `true` is also the conservative answer when the probe itself fails, so a
/// broken probe can never produce a misleading diagnosis.
pub fn reads_openssh_private_keys() -> bool {
    static READABLE: OnceLock<bool> = OnceLock::new();

    *READABLE.get_or_init(|| {
        supported_algorithms(MethodType::HostKey)
            .map(|algorithms| {
                algorithms.contains("ssh-ed25519")
                    || algorithms
                        .iter()
                        .any(|name| name.starts_with("ecdsa-sha2-"))
            })
            .unwrap_or(true)
    })
}

/// Whether `error` is a failure libssh2 could not describe.
///
/// libssh2 returns `-1` from several call sites that do not go through
/// `_libssh2_error()` — including the WinCNG key loader — and `-1` is not one
/// of the `LIBSSH2_ERROR_*` constants the `ssh2` crate maps, so the message is
/// the generic `unknown error`. A failure the library *could* describe (`-18`
/// "Username/PublicKey combination invalid", for instance) is deliberately
/// **not** treated as opaque: it already tells the user what went wrong.
fn is_opaque_failure(error: &Error) -> bool {
    error.kind() == ErrorKind::Opaque
}

/// The decision behind [`unusable_key_hint`], separated so every combination can
/// be tested on any platform.
///
/// Returns a diagnosis only when all three hold:
/// 1. the key really is in the OpenSSH container format,
/// 2. this build cannot read that format, and
/// 3. the failure is one libssh2 could not describe.
///
/// (1) and (2) together mean the key *cannot* work, so the hint is not a guess.
/// (3) keeps the hint away from failures that have their own explanation — a
/// rejected public key or a wrong passphrase — so it can never displace a more
/// accurate message.
fn hint_decision(format: PrivateKeyFormat, readable: bool, opaque: bool) -> Option<&'static str> {
    if format != PrivateKeyFormat::OpensshContainer || readable || !opaque {
        return None;
    }

    Some(
        "the key file uses the OpenSSH private-key container format \
         (`-----BEGIN OPENSSH PRIVATE KEY-----`), which the SSH library this build links cannot \
         read. This build's libssh2 links the WinCNG backend, whose key-file reader understands \
         only traditional PEM (`-----BEGIN RSA PRIVATE KEY-----` and `-----BEGIN DSA PRIVATE \
         KEY-----`), so this key cannot authenticate here in any form. Convert it in place with \
         `ssh-keygen -p -m PEM -f <path>` (RSA or DSA keys only) or import an RSA key that is \
         already in PEM form. Note that `ssh-keygen` has written this container format by default \
         since OpenSSH 7.8 — `-t rsa` included — and that Ed25519 keys are unusable by this build \
         even once converted",
    )
}

/// An actionable explanation for a private-key authentication failure, or `None`
/// when nothing specific can be said.
///
/// Call this only after `userauth_pubkey_file` has failed; it inspects the key
/// file on disk and this build's capabilities, never the network.
pub fn unusable_key_hint(path: &Path, error: &Error) -> Option<String> {
    hint_decision(
        detect_private_key_format(path),
        reads_openssh_private_keys(),
        is_opaque_failure(error),
    )
    .map(str::to_string)
}

/// Log-friendly name for a detected format, for diagnostic entries.
pub fn format_name(format: PrivateKeyFormat) -> &'static str {
    match format {
        PrivateKeyFormat::OpensshContainer => "openssh-container",
        PrivateKeyFormat::PemRsa => "pem-rsa",
        PrivateKeyFormat::PemDsa => "pem-dsa",
        PrivateKeyFormat::Pkcs8 => "pkcs8",
        PrivateKeyFormat::OtherArmoured => "other-armoured",
        PrivateKeyFormat::Unknown => "unknown",
    }
}

/// Whether a warning is warranted for a **newly generated** key, so the app
/// never hands the user a key this build cannot use.
///
/// Kept separate from generation itself: generating an Ed25519 key on a build
/// that cannot use it is still a legitimate thing to do (it can be exported and
/// used elsewhere), it just must not be presented as ready to authenticate.
pub fn generated_key_type_is_usable() -> bool {
    reads_openssh_private_keys()
}

/// Validate that a key file exists and report its format, for diagnostics.
pub fn inspect_key_file(path: &Path) -> Result<PrivateKeyFormat> {
    if !path.exists() {
        anyhow::bail!("key file not found: {}", path.display());
    }
    Ok(detect_private_key_format(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(name: &str, contents: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("agent2ssh-keyfmt-{}-{name}", uuid::Uuid::new_v4()));
        let mut file = std::fs::File::create(&path).expect("create temp key");
        file.write_all(contents.as_bytes()).expect("write temp key");
        path
    }

    #[test]
    fn detects_openssh_container_format() {
        let path = write_temp("openssh", "-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n");
        assert_eq!(
            detect_private_key_format(&path),
            PrivateKeyFormat::OpensshContainer
        );
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn detects_traditional_pem_forms() {
        let cases = [
            (
                "rsa",
                "-----BEGIN RSA PRIVATE KEY-----\nAAAA\n",
                PrivateKeyFormat::PemRsa,
            ),
            (
                "rsa-enc",
                "-----BEGIN ENCRYPTED RSA PRIVATE KEY-----\nAAAA\n",
                PrivateKeyFormat::PemRsa,
            ),
            (
                "dsa",
                "-----BEGIN DSA PRIVATE KEY-----\nAAAA\n",
                PrivateKeyFormat::PemDsa,
            ),
            (
                "pkcs8",
                "-----BEGIN PRIVATE KEY-----\nAAAA\n",
                PrivateKeyFormat::Pkcs8,
            ),
            (
                "pub",
                "-----BEGIN PUBLIC KEY-----\nAAAA\n",
                PrivateKeyFormat::OtherArmoured,
            ),
        ];

        for (name, contents, expected) in cases {
            let path = write_temp(name, contents);
            assert_eq!(detect_private_key_format(&path), expected, "case {name}");
            std::fs::remove_file(path).ok();
        }
    }

    #[test]
    fn detects_header_after_blank_lines() {
        let path = write_temp(
            "leading-blank",
            "\n\n   \n-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n",
        );
        assert_eq!(
            detect_private_key_format(&path),
            PrivateKeyFormat::OpensshContainer
        );
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn missing_and_non_pem_files_are_unknown() {
        let missing = std::env::temp_dir().join("agent2ssh-keyfmt-does-not-exist");
        assert_eq!(
            detect_private_key_format(&missing),
            PrivateKeyFormat::Unknown
        );

        let path = write_temp("plain", "not a key at all\n");
        assert_eq!(detect_private_key_format(&path), PrivateKeyFormat::Unknown);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn hint_requires_container_format_unreadable_build_and_opaque_failure() {
        // The one combination that warrants the diagnosis.
        assert!(hint_decision(PrivateKeyFormat::OpensshContainer, false, true).is_some());

        // A readable build: the format is fine, so say nothing.
        assert!(hint_decision(PrivateKeyFormat::OpensshContainer, true, true).is_none());
        // A described failure (e.g. -18 "Username/PublicKey combination
        // invalid") already explains itself; never displace it.
        assert!(hint_decision(PrivateKeyFormat::OpensshContainer, false, false).is_none());
        // Formats WinCNG can read must never be blamed.
        assert!(hint_decision(PrivateKeyFormat::PemRsa, false, true).is_none());
        assert!(hint_decision(PrivateKeyFormat::PemDsa, false, true).is_none());
        assert!(hint_decision(PrivateKeyFormat::Pkcs8, false, true).is_none());
        assert!(hint_decision(PrivateKeyFormat::Unknown, false, true).is_none());
    }

    #[test]
    fn hint_names_the_conversion_command_and_the_limitation() {
        let hint = hint_decision(PrivateKeyFormat::OpensshContainer, false, true)
            .expect("container format on an unreadable build");
        assert!(hint.contains("ssh-keygen -p -m PEM"), "{hint}");
        assert!(hint.contains("WinCNG"), "{hint}");
        assert!(hint.contains("BEGIN RSA PRIVATE KEY"), "{hint}");
    }

    #[test]
    fn opaque_failures_are_the_undescribed_ones() {
        let opaque = Error::new(ErrorKind::Opaque, "unknown error");
        assert!(is_opaque_failure(&opaque));

        // -18 carries libssh2's own message ("Username/PublicKey combination
        // invalid"), so it is not opaque and must not be re-diagnosed.
        let described = Error::new(
            ErrorKind::PublicKeyRefused,
            "Username/PublicKey combination invalid",
        );
        assert!(!is_opaque_failure(&described));
    }

    #[test]
    fn hint_is_silent_for_a_pem_rsa_key() {
        let path = write_temp("pem-silent", "-----BEGIN RSA PRIVATE KEY-----\nAAAA\n");
        let opaque = Error::new(ErrorKind::Opaque, "unknown error");
        assert!(unusable_key_hint(&path, &opaque).is_none());

        // A container-format key only produces a hint together with an opaque
        // failure; with a described failure it stays silent on every platform.
        let container = write_temp("container-silent", "-----BEGIN OPENSSH PRIVATE KEY-----\n");
        let described = Error::new(
            ErrorKind::PublicKeyRefused,
            "Username/PublicKey combination invalid",
        );
        assert!(unusable_key_hint(&container, &described).is_none());

        std::fs::remove_file(path).ok();
        std::fs::remove_file(container).ok();
    }

    #[test]
    fn container_keys_are_diagnosed_exactly_when_this_build_cannot_read_them() {
        let path = write_temp(
            "container-conditional",
            "-----BEGIN OPENSSH PRIVATE KEY-----\n",
        );
        let opaque = Error::new(ErrorKind::Opaque, "unknown error");
        let hint = unusable_key_hint(&path, &opaque);

        // Tie the expectation to the probe, not to a hard-coded platform, so
        // this test documents the invariant instead of duplicating a guess.
        assert_eq!(hint.is_some(), !reads_openssh_private_keys());

        if let Some(hint) = hint {
            assert!(hint.contains("ssh-keygen -p -m PEM"), "{hint}");
        }
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn format_names_are_distinct() {
        let names = [
            format_name(PrivateKeyFormat::OpensshContainer),
            format_name(PrivateKeyFormat::PemRsa),
            format_name(PrivateKeyFormat::PemDsa),
            format_name(PrivateKeyFormat::Pkcs8),
            format_name(PrivateKeyFormat::OtherArmoured),
            format_name(PrivateKeyFormat::Unknown),
        ];
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(
            unique.len(),
            names.len(),
            "duplicate format name: {names:?}"
        );
    }

    #[test]
    fn generation_usability_is_derived_from_the_readability_probe() {
        assert_eq!(generated_key_type_is_usable(), reads_openssh_private_keys());
    }
}
