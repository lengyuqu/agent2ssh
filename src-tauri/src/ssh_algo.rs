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

/// Save algorithm preferences to the config file.
pub fn save_algo_prefs(prefs: &SshAlgoPrefs) -> Result<()> {
    crate::store::ensure_config_dir()?;
    let path = crate::store::config_dir()?.join("ssh_algos.json");
    let raw = serde_json::to_string_pretty(prefs)?;
    std::fs::write(&path, raw)?;
    crate::store::restrict_file_to_owner(&path)?;
    Ok(())
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
}
