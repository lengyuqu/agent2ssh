//! SSH algorithm preference configuration (A22).
//!
//! Allows users to configure preferred SSH key exchange, cipher, MAC, hostkey,
//! and compression algorithms. Uses `ssh2::Session::method_pref()` which maps
//! to `libssh2_session_method_pref()` — the preference must be set **before**
//! the handshake.
//!
//! ## Safe defaults
//!
//! The default preferences prioritize modern, secure algorithms and exclude
//! weak/legacy ones (DSA, 3DES-CBC, hmac-sha1, etc.). If the user provides
//! their own preferences, they are validated against libssh2's supported
//! algorithms list — fail-closed if any preference is unknown.
//!
//! Mirrors rssh's `ssh/algorithms.rs` design, adapted to the `ssh2` crate
//! (libssh2 bindings) which uses `MethodType` + comma-delimited string
//! preferences rather than rssh's structured `AlgorithmList`.

use anyhow::{anyhow, Result};
use ssh2::{MethodType, Session};
use std::collections::HashSet;

/// User-configurable SSH algorithm preferences.
///
/// Each field is a comma-delimited list of algorithm names in preference
/// order (most preferred first). An empty string means "use libssh2 defaults".
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SshAlgoPrefs {
    /// Key exchange algorithms (e.g. "curve25519-sha256,diffie-hellman-group16-sha512").
    pub kex: String,
    /// Host key algorithms (e.g. "ssh-ed25519,ecdsa-sha2-nistp256,rsa-sha2-512").
    pub hostkey: String,
    /// Client-to-server ciphers.
    pub cipher_cs: String,
    /// Server-to-client ciphers.
    pub cipher_sc: String,
    /// Client-to-server MACs.
    pub mac_cs: String,
    /// Server-to-client MACs.
    pub mac_sc: String,
    /// Client-to-server compression.
    pub comp_cs: String,
    /// Server-to-client compression.
    pub comp_sc: String,
}

/// Safe default algorithm preferences that exclude weak/legacy algorithms.
///
/// These are applied when the user has not set custom preferences. They
/// prioritize modern algorithms (curve25519, ed25519, aes256-gcm, etc.)
/// and explicitly exclude:
/// - DSA host keys (ssh-dss)
/// - 3DES-CBC cipher
/// - hmac-sha1-96 MAC
/// - zlib compression: the embedded transport (libssh2, as built by the
///   `ssh2` crate) only supports the `none` compression method, so listing
///   `zlib@openssh.com`/`zlib` here fails the fail-closed validation against
///   a real sshd ("unsupported SSH algorithm 'zlib@openssh.com' ...").
pub fn safe_defaults() -> SshAlgoPrefs {
    SshAlgoPrefs {
        kex: "curve25519-sha256,curve25519-sha256@libssh.org,diffie-hellman-group16-sha512,diffie-hellman-group-exchange-sha256,diffie-hellman-group14-sha256".into(),
        hostkey: "ssh-ed25519,ecdsa-sha2-nistp256,rsa-sha2-512,rsa-sha2-256,ssh-rsa".into(),
        cipher_cs: "chacha20-poly1305@openssh.com,aes256-gcm@openssh.com,aes256-ctr,aes192-ctr,aes128-ctr".into(),
        cipher_sc: "chacha20-poly1305@openssh.com,aes256-gcm@openssh.com,aes256-ctr,aes192-ctr,aes128-ctr".into(),
        mac_cs: "hmac-sha2-256,hmac-sha2-512".into(),
        mac_sc: "hmac-sha2-256,hmac-sha2-512".into(),
        comp_cs: "none".into(),
        comp_sc: "none".into(),
    }
}

/// Apply algorithm preferences to a session **before** the handshake.
///
/// If `prefs` is `None`, safe defaults are used. If any preference string
/// contains an algorithm not supported by libssh2, an error is returned
/// (fail-closed).
pub fn apply_algo_prefs(session: &Session, prefs: Option<&SshAlgoPrefs>) -> Result<()> {
    let defaults = safe_defaults();
    let p = prefs.unwrap_or(&defaults);

    apply_method(session, MethodType::Kex, &p.kex)?;
    apply_method(session, MethodType::HostKey, &p.hostkey)?;
    apply_method(session, MethodType::CryptCs, &p.cipher_cs)?;
    apply_method(session, MethodType::CryptSc, &p.cipher_sc)?;
    apply_method(session, MethodType::MacCs, &p.mac_cs)?;
    apply_method(session, MethodType::MacSc, &p.mac_sc)?;
    apply_method(session, MethodType::CompCs, &p.comp_cs)?;
    apply_method(session, MethodType::CompSc, &p.comp_sc)?;

    Ok(())
}

/// Apply a single method preference, validating against supported algorithms.
fn apply_method(session: &Session, method_type: MethodType, prefs: &str) -> Result<()> {
    if prefs.is_empty() {
        return Ok(());
    }

    // Validate: each algorithm in the preference list must be supported by
    // libssh2. This is a fail-closed check — an unknown algorithm name
    // (e.g. a typo) must not silently pass.
    let supported: HashSet<String> = session
        .supported_algs(method_type)
        .map_err(|e| anyhow!("failed to query supported algorithms: {e}"))?
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    for algo in prefs.split(',').map(|s| s.trim()) {
        if !algo.is_empty() && !supported.contains(algo) {
            return Err(anyhow!(
                "unsupported SSH algorithm '{}' in preference list (supported: {})",
                algo,
                supported.iter().cloned().collect::<Vec<_>>().join(",")
            ));
        }
    }

    session
        .method_pref(method_type, prefs)
        .map_err(|e| anyhow!("failed to set algorithm preference: {e}"))?;

    Ok(())
}

/// Load algorithm preferences from the config file. Returns `None` if no
/// preferences are configured (safe defaults will be used).
pub fn load_algo_prefs() -> Option<SshAlgoPrefs> {
    let path = crate::store::config_dir().ok()?.join("ssh_algos.json");
    if !path.exists() {
        return None;
    }
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// The preferences actually used for the next connection: the user's file when
/// it exists and parses, otherwise [`safe_defaults`].
pub fn effective_algo_prefs() -> SshAlgoPrefs {
    load_algo_prefs().unwrap_or_else(safe_defaults)
}

/// True when `ssh_algos.json` exists and could be read, i.e. the connection
/// path is using something other than the built-in defaults.
pub fn has_custom_algo_prefs() -> bool {
    load_algo_prefs().is_some()
}

/// Everything a caller needs to show the algorithm settings in one round trip.
///
/// This lives here rather than beside the desktop command that first needed it,
/// because the CLI and the daemon read and write these same three fields and the
/// desktop's copy sat behind `feature = "tauri"` — compiled out of both of them.
/// A second, same-shaped struct for those two surfaces is the "mirror of a live
/// type" that A24's cleanup already had to delete once.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlgoPrefsState {
    /// Preferences currently in effect for the next connection.
    pub prefs: SshAlgoPrefs,
    /// True when `ssh_algos.json` exists, i.e. the user overrode the defaults.
    pub custom: bool,
    /// The built-in defaults, so a caller can offer a reset target without
    /// duplicating these values.
    pub defaults: SshAlgoPrefs,
}

impl AlgoPrefsState {
    /// What is in effect, whether it is an override, and the defaults it would
    /// fall back to.
    pub fn snapshot() -> Self {
        Self {
            prefs: effective_algo_prefs(),
            custom: has_custom_algo_prefs(),
            defaults: safe_defaults(),
        }
    }
}

/// The compression algorithm libssh2 accepts in this build.
///
/// The `ssh2` crate does not compile in zlib support, so `none` is the only
/// value that survives the fail-closed check in [`apply_algo_prefs`]. Anything
/// else is rejected here rather than at connect time, where it would break
/// every connection until the user edited the file by hand.
const SUPPORTED_COMPRESSION: &str = "none";

/// Validate preferences before they are persisted.
///
/// [`apply_algo_prefs`] is deliberately fail-closed: one unknown algorithm name
/// makes every connection fail. That is the right call for hand-edited files,
/// but it means anything the app itself writes must be checked first, or a typo
/// in the settings dialog would take the whole SSH surface down with it. This
/// validates shape and the one constraint we can know without a live server
/// (compression); server-specific names still cannot be verified offline, which
/// is why [`reset_algo_prefs`] exists as the way back.
pub fn validate_algo_prefs(prefs: &SshAlgoPrefs) -> Result<()> {
    let fields: [(&str, &str); 8] = [
        ("kex", &prefs.kex),
        ("hostkey", &prefs.hostkey),
        ("cipher client->server", &prefs.cipher_cs),
        ("cipher server->client", &prefs.cipher_sc),
        ("mac client->server", &prefs.mac_cs),
        ("mac server->client", &prefs.mac_sc),
        ("compression client->server", &prefs.comp_cs),
        ("compression server->client", &prefs.comp_sc),
    ];

    for (label, value) in fields {
        let entries: Vec<&str> = value
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if entries.is_empty() {
            return Err(anyhow!(
                "{label} must list at least one algorithm (use the safe defaults to reset)"
            ));
        }
        let mut seen = HashSet::new();
        for entry in entries {
            if !is_plausible_algorithm_name(entry) {
                return Err(anyhow!(
                    "unsupported SSH algorithm '{entry}' in {label}: names may only contain \
                     letters, digits and . _ - @"
                ));
            }
            if !seen.insert(entry) {
                return Err(anyhow!("duplicate SSH algorithm '{entry}' in {label}"));
            }
        }
    }

    for (label, value) in [
        ("compression client->server", &prefs.comp_cs),
        ("compression server->client", &prefs.comp_sc),
    ] {
        let entries: Vec<&str> = value
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if entries != [SUPPORTED_COMPRESSION] {
            return Err(anyhow!(
                "{label} must be exactly '{SUPPORTED_COMPRESSION}': this build of libssh2 has no \
                 zlib support, so any other value would make every connection fail"
            ));
        }
    }

    Ok(())
}

/// A conservative syntax check — this is not a guarantee that libssh2 knows the
/// algorithm. It keeps the file to well-formed tokens (so it can round-trip
/// through the settings dialog and the comma-delimited parser) without
/// pretending to be an offline capability probe.
fn is_plausible_algorithm_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@' | '+'))
}

/// Save algorithm preferences to the config file.
///
/// Validates first: see [`validate_algo_prefs`] for why the write path may not
/// simply trust its input.
pub fn save_algo_prefs(prefs: &SshAlgoPrefs) -> Result<()> {
    validate_algo_prefs(prefs)?;
    let path = crate::store::config_dir()?.join("ssh_algos.json");
    crate::store::write_private_json(&path, prefs)
}

/// Drop `ssh_algos.json` so the next connection falls back to [`safe_defaults`].
///
/// This is the documented way out of a preference set that turned out to be
/// unsupported by a given server.
pub fn reset_algo_prefs() -> Result<()> {
    let path = crate::store::config_dir()?.join("ssh_algos.json");
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(anyhow!("failed to remove {}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_defaults_non_empty() {
        let d = safe_defaults();
        assert!(!d.kex.is_empty());
        assert!(!d.hostkey.is_empty());
        assert!(!d.cipher_cs.is_empty());
        assert!(!d.cipher_sc.is_empty());
        assert!(!d.mac_cs.is_empty());
        assert!(!d.mac_sc.is_empty());
        assert!(!d.comp_cs.is_empty());
        assert!(!d.comp_sc.is_empty());
    }

    #[test]
    fn safe_defaults_exclude_weak_algos() {
        let d = safe_defaults();
        // DSA host keys must not appear.
        assert!(!d.hostkey.contains("ssh-dss"), "ssh-dss must be excluded");
        // 3DES-CBC must not appear.
        assert!(
            !d.cipher_cs.contains("3des-cbc"),
            "3des-cbc must be excluded"
        );
        assert!(
            !d.cipher_sc.contains("3des-cbc"),
            "3des-cbc must be excluded"
        );
        // hmac-sha1-96 must not appear.
        assert!(
            !d.mac_cs.contains("hmac-sha1-96"),
            "hmac-sha1-96 must be excluded"
        );
        assert!(
            !d.mac_sc.contains("hmac-sha1-96"),
            "hmac-sha1-96 must be excluded"
        );
    }

    #[test]
    fn safe_defaults_prioritize_modern_algos() {
        let d = safe_defaults();
        // curve25519 should be the first kex.
        assert!(
            d.kex.starts_with("curve25519-sha256"),
            "curve25519 should be the most preferred kex"
        );
        // ed25519 should be the first hostkey.
        assert!(
            d.hostkey.starts_with("ssh-ed25519"),
            "ed25519 should be the most preferred hostkey"
        );
        // chacha20-poly1305 should be the first cipher.
        assert!(
            d.cipher_cs.starts_with("chacha20-poly1305"),
            "chacha20-poly1305 should be the most preferred cipher"
        );
    }

    #[test]
    fn ssh_algo_prefs_serialize_roundtrip() {
        let prefs = SshAlgoPrefs {
            kex: "curve25519-sha256".into(),
            hostkey: "ssh-ed25519".into(),
            cipher_cs: "aes256-gcm@openssh.com".into(),
            cipher_sc: "aes256-gcm@openssh.com".into(),
            mac_cs: "hmac-sha2-256".into(),
            mac_sc: "hmac-sha2-256".into(),
            comp_cs: "none".into(),
            comp_sc: "none".into(),
        };
        let json = serde_json::to_string(&prefs).unwrap();
        let back: SshAlgoPrefs = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kex, "curve25519-sha256");
        assert_eq!(back.hostkey, "ssh-ed25519");
        assert_eq!(back.cipher_cs, "aes256-gcm@openssh.com");
    }

    #[test]
    fn default_prefs_are_empty_strings() {
        let prefs = SshAlgoPrefs::default();
        assert!(prefs.kex.is_empty());
        assert!(prefs.hostkey.is_empty());
        assert!(prefs.cipher_cs.is_empty());
    }

    /// The safe defaults must be writable: they are what the settings dialog
    /// offers as "reset", so a rejection here would make the reset unusable.
    #[test]
    fn safe_defaults_pass_validation() {
        validate_algo_prefs(&safe_defaults()).expect("safe defaults must validate");
    }

    #[test]
    fn validation_rejects_empty_field() {
        let mut prefs = safe_defaults();
        prefs.kex = "   ".into();
        let err = validate_algo_prefs(&prefs).unwrap_err().to_string();
        assert!(err.contains("at least one algorithm"), "got: {err}");
    }

    #[test]
    fn validation_rejects_unparseable_name() {
        let mut prefs = safe_defaults();
        prefs.cipher_cs = "aes256-ctr,not an algorithm".into();
        let err = validate_algo_prefs(&prefs).unwrap_err().to_string();
        assert!(err.contains("unsupported SSH algorithm"), "got: {err}");
    }

    #[test]
    fn validation_rejects_duplicate_entries() {
        let mut prefs = safe_defaults();
        prefs.mac_cs = "hmac-sha2-256,hmac-sha2-256".into();
        let err = validate_algo_prefs(&prefs).unwrap_err().to_string();
        assert!(err.contains("duplicate"), "got: {err}");
    }

    /// libssh2 is built without zlib support, so a compression value other than
    /// `none` fails closed at connect time. It must be caught at save time.
    #[test]
    fn validation_rejects_unsupported_compression() {
        let mut prefs = safe_defaults();
        prefs.comp_cs = "zlib@openssh.com".into();
        let err = validate_algo_prefs(&prefs).unwrap_err().to_string();
        assert!(err.contains("compression"), "got: {err}");

        let mut prefs = safe_defaults();
        prefs.comp_sc = "".into();
        assert!(validate_algo_prefs(&prefs).is_err());
    }
}
