//! App-managed encrypted credential storage for SSH passwords (K1).
//!
//! Agent2SSH manages its own credential encryption rather than delegating to the
//! OS keychain. Host/proxy passwords are encrypted at rest in
//! `~/.agent2ssh/secrets.enc` with **AES-256-GCM**, under a key derived from a
//! user **master password** via **Argon2id**. There is no plaintext key on disk:
//! the only way to decrypt is to supply the master password, which unlocks the
//! store for the process lifetime.
//!
//! **Per-entry encryption with AAD binding (v2).** Each secret is independently
//! encrypted with its own random nonce, and the **account name** (e.g.
//! `host:myhost`) is bound as AEAD associated data (AAD). This prevents
//! cut-and-paste attacks: if an attacker copies the ciphertext from account A
//! to account B, the AEAD tag verification fails because the AAD differs.
//!
//! On disk, `hosts.json` holds only the [`SECRET_REF`] marker in place of a
//! password; the real secret lives (encrypted) in `secrets.enc`. The persistence
//! boundary in [`crate::store`] resolves the marker back into the real password
//! on load **when the store is unlocked**, and re-encrypts on save.
//!
//! Unlocking:
//! - Desktop: a startup dialog prompts for the master password.
//! - CLI / MCP / daemon: read `AGENT2SSH_MASTER_PASSWORD`. If unset, password
//!   hosts are simply unavailable (the marker never resolves) — by design.
//!
//! Locked behavior is safe: a locked load leaves the marker in place (it is not
//! decrypted to `None`), so an unrelated save never clobbers the encrypted
//! secret, and `embedded_ssh` treats the bare marker as "no usable password".
//!
//! **Backward compatibility:** v1 files (single ciphertext for the whole map)
//! are transparently read and migrated to v2 on the next save.

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use std::collections::HashMap;
use std::sync::{Mutex, RwLock};

use crate::app_state::app_state;
use crate::store::{config_dir, ensure_config_dir, restrict_file_to_owner};

/// Encrypted credential file under the config dir.
const SECRETS_FILE: &str = "secrets.enc";

/// On-disk sentinel that replaces a real password in `hosts.json`. When a
/// `password` field equals this exact value, the real secret lives (encrypted)
/// in `secrets.enc` under the account derived from the host/proxy identity.
pub const SECRET_REF: &str = "$agent2ssh-secret$";

/// Legacy marker used by the unfinished OS-keyring-backed storage path. New
/// builds must never treat it as an SSH password.
pub const LEGACY_KEYRING_REF: &str = "$agent2ssh-keyring$";

/// True if `value` is a credential reference marker rather than a real secret
/// (or the empty string).
pub fn is_secret_ref(value: &str) -> bool {
    value == SECRET_REF || value == LEGACY_KEYRING_REF
}

/// True only for the current app-managed encrypted-store marker.
pub fn is_current_secret_ref(value: &str) -> bool {
    value == SECRET_REF
}

/// True for the legacy OS-keyring marker. This is preserved on disk until the
/// user re-enters or explicitly migrates the password into `secrets.enc`.
pub fn is_legacy_keyring_ref(value: &str) -> bool {
    value == LEGACY_KEYRING_REF
}

/// Stable account name for a host profile's encrypted password.
pub fn host_account(host_name: &str) -> String {
    format!("host:{host_name}")
}

/// Stable account name for a proxy profile's encrypted password.
pub fn proxy_account(proxy_id: &str) -> String {
    format!("proxy:{proxy_id}")
}

// ── In-memory unlocked key ────────────────────────────────────────────────────

/// The derived 256-bit key, cached for the process lifetime once unlocked. `None`
/// means locked. Argon2 runs only at unlock time, not per secret operation.
fn key_cell() -> &'static RwLock<Option<[u8; 32]>> {
    &app_state().secrets_key
}

fn cached_key() -> Option<[u8; 32]> {
    *key_cell().read().expect("secrets key lock poisoned")
}

fn set_cached_key(key: Option<[u8; 32]>) {
    *key_cell().write().expect("secrets key lock poisoned") = key;
}

// ── Test backend ──────────────────────────────────────────────────────────────

fn backend_is_memory() -> bool {
    match std::env::var("AGENT2SSH_SECRETS_BACKEND") {
        Ok(v) if v.eq_ignore_ascii_case("memory") => true,
        Ok(v) if v.eq_ignore_ascii_case("encrypted") => false,
        // Unit tests default to an in-memory store so they never need a master
        // password or touch the encrypted file. Production binaries leave this
        // unset and use the encrypted store.
        _ => cfg!(test),
    }
}

fn memory_store() -> &'static Mutex<HashMap<String, String>> {
    &app_state().secrets_memory
}

// ── On-disk encrypted file ────────────────────────────────────────────────────

/// One encrypted secret entry. The account name is NOT stored here in
/// plaintext — it is used as AEAD AAD during encryption/decryption, so the
/// ciphertext is cryptographically bound to its account. The account→entry
/// mapping is stored in the outer `EncryptedStoreV2.entries` map.
#[derive(serde::Serialize, serde::Deserialize)]
struct EncryptedEntry {
    /// base64 AES-GCM nonce (12 bytes) — unique per entry per save.
    nonce: String,
    /// base64 AES-256-GCM ciphertext (includes the 16-byte AEAD tag).
    ciphertext: String,
}

/// v2 on-disk format: per-entry encryption with AAD binding to account name.
///
/// Each entry in `entries` is independently encrypted with its own nonce.
/// The account name is used as AAD, so moving a ciphertext from one account
/// to another causes tag verification failure.
#[derive(serde::Serialize, serde::Deserialize)]
struct EncryptedStoreV2 {
    version: u32,
    kdf: String,
    /// base64 Argon2 salt (16 bytes).
    salt: String,
    /// Per-account encrypted entries. Key = account name (e.g. "host:myhost").
    entries: HashMap<String, EncryptedEntry>,
}

/// v1 on-disk format (legacy): the whole map encrypted as a single blob.
/// Kept for backward-compatible reading; new writes always use v2.
#[derive(serde::Serialize, serde::Deserialize)]
struct EncryptedStoreV1 {
    version: u32,
    kdf: String,
    salt: String,
    nonce: String,
    ciphertext: String,
}

fn secrets_path() -> Result<std::path::PathBuf> {
    Ok(config_dir()?.join(SECRETS_FILE))
}

/// Whether an encrypted store already exists (i.e. a master password has been set).
pub fn is_initialized() -> bool {
    if backend_is_memory() {
        return true;
    }
    secrets_path().map(|p| p.exists()).unwrap_or(false)
}

/// Whether the store is currently unlocked in this process.
pub fn is_unlocked() -> bool {
    if backend_is_memory() {
        return true;
    }
    cached_key().is_some()
}

fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|e| anyhow!("failed to read entropy: {e}"))?;
    Ok(buf)
}

/// Derive the 256-bit key from a master password + salt via Argon2id.
fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32]> {
    use argon2::{Algorithm, Argon2, Params, Version};
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default());
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow!("key derivation failed: {e}"))?;
    Ok(key)
}

/// Encrypt a single secret, binding the account name as AEAD AAD.
///
/// Returns (nonce, ciphertext) where nonce is 12 bytes and ciphertext includes
/// the 16-byte AEAD tag. The account name is fed as associated data, so a
/// ciphertext encrypted for account "host:A" will fail to decrypt under account
/// "host:B" — preventing cut-and-paste attacks.
fn encrypt_entry(key: &[u8; 32], account: &str, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Nonce};
    let nonce_bytes = random_bytes::<12>()?;
    let cipher = Aes256Gcm::new(key.into());
    let payload = Payload {
        msg: plaintext,
        aad: account.as_bytes(),
    };
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), payload)
        .map_err(|e| anyhow!("encryption failed: {e}"))?;
    Ok((nonce_bytes.to_vec(), ciphertext))
}

/// Decrypt a single entry, verifying the AEAD tag against the account name.
///
/// Returns `Err` if the key is wrong, the account name doesn't match the AAD
/// used during encryption, or the ciphertext was tampered with.
fn decrypt_entry(
    key: &[u8; 32],
    account: &str,
    nonce: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Nonce};
    let cipher = Aes256Gcm::new(key.into());
    let payload = Payload {
        msg: ciphertext,
        aad: account.as_bytes(),
    };
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce), payload)
        .map_err(|_| {
            anyhow!("decryption failed (wrong master password, corrupt entry, or account mismatch)")
        })?;
    Ok(plaintext)
}

/// Read + decrypt the whole secret map using the cached key. Returns an error if
/// the store is locked.
///
/// Handles both v1 (single-blob) and v2 (per-entry with AAD) formats. v1 files
/// are transparently read — the next `save_map` will migrate them to v2.
fn load_map() -> Result<HashMap<String, String>> {
    let key = cached_key().ok_or_else(|| anyhow!("credential store is locked"))?;
    let path = secrets_path()?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let raw = std::fs::read_to_string(&path).context("failed to read secrets store")?;
    // Probe the version field to decide the format.
    let version_probe: serde_json::Value =
        serde_json::from_str(&raw).context("failed to parse secrets store")?;
    let version = version_probe
        .get("version")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as u32;

    if version >= 2 {
        // v2: per-entry encryption with AAD.
        let store: EncryptedStoreV2 =
            serde_json::from_str(&raw).context("failed to parse secrets store (v2)")?;
        let mut map = HashMap::with_capacity(store.entries.len());
        for (account, entry) in &store.entries {
            let nonce = b64()
                .decode(entry.nonce.as_bytes())
                .context("bad nonce encoding in entry")?;
            let ciphertext = b64()
                .decode(entry.ciphertext.as_bytes())
                .context("bad ciphertext encoding in entry")?;
            let plaintext = decrypt_entry(&key, account, &nonce, &ciphertext)?;
            let secret = String::from_utf8(plaintext)
                .map_err(|e| anyhow!("secret for '{account}' is not valid UTF-8: {e}"))?;
            map.insert(account.clone(), secret);
        }
        Ok(map)
    } else {
        // v1: single-blob encryption (legacy). Read and return; migration
        // happens automatically on the next save_map.
        let store: EncryptedStoreV1 =
            serde_json::from_str(&raw).context("failed to parse secrets store (v1)")?;
        let nonce = b64()
            .decode(store.nonce.as_bytes())
            .context("bad nonce encoding")?;
        let ciphertext = b64()
            .decode(store.ciphertext.as_bytes())
            .context("bad ciphertext encoding")?;
        decrypt_v1_map(&key, &nonce, &ciphertext)
    }
}

/// Encrypt + write the whole secret map in v2 format (per-entry with AAD).
/// Reuses the stored Argon2 salt so the cached key stays valid; each entry gets
/// its own fresh nonce.
fn save_map(map: &HashMap<String, String>) -> Result<()> {
    let key = cached_key().ok_or_else(|| anyhow!("credential store is locked"))?;
    ensure_config_dir()?;
    let path = secrets_path()?;

    // Preserve the existing salt (the cached key was derived from it). On first
    // write the salt must already have been chosen by `unlock_or_init`.
    let salt = read_salt()?.ok_or_else(|| anyhow!("secrets store salt missing"))?;

    let mut entries = HashMap::with_capacity(map.len());
    for (account, secret) in map {
        let (nonce, ciphertext) = encrypt_entry(&key, account, secret.as_bytes())?;
        entries.insert(
            account.clone(),
            EncryptedEntry {
                nonce: b64().encode(&nonce),
                ciphertext: b64().encode(&ciphertext),
            },
        );
    }
    let store = EncryptedStoreV2 {
        version: 2,
        kdf: "argon2id".into(),
        salt: b64().encode(&salt),
        entries,
    };
    let raw = serde_json::to_string_pretty(&store)?;
    std::fs::write(&path, raw).context("failed to write secrets store")?;
    restrict_file_to_owner(&path)?;
    Ok(())
}

/// Read the stored Argon2 salt, if the file exists. Works with both v1 and v2
/// formats — both store the salt as a base64 string field.
fn read_salt() -> Result<Option<Vec<u8>>> {
    let path = secrets_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    // Parse as a generic JSON value to extract the salt without needing to
    // know the version.
    let probe: serde_json::Value = serde_json::from_str(&raw)?;
    let salt_str = probe
        .get("salt")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("secrets store missing salt field"))?;
    Ok(Some(b64().decode(salt_str.as_bytes())?))
}

/// Decrypt a v1 single-blob encrypted map (legacy format). Used only for
/// backward-compatible reading; the next save will migrate to v2.
fn decrypt_v1_map(
    key: &[u8; 32],
    nonce: &[u8],
    ciphertext: &[u8],
) -> Result<HashMap<String, String>> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    let cipher = Aes256Gcm::new(key.into());
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|_| anyhow!("decryption failed (wrong master password or corrupt store)"))?;
    Ok(serde_json::from_slice(&plaintext)?)
}

// ── Unlock / init / lock ──────────────────────────────────────────────────────

/// Unlock an existing store, or initialize a new one, with `password`.
///
/// - If `secrets.enc` exists: derive the key from the stored salt and verify it
///   by decrypting; a wrong password returns an error. Handles both v1 (single
///   blob) and v2 (per-entry with AAD) formats.
/// - If it does not exist: choose a fresh salt, derive the key, and write an
///   empty encrypted store (this *sets* the master password).
///
/// On success the derived key is cached for the process lifetime.
pub fn unlock_or_init(password: &str) -> Result<()> {
    if backend_is_memory() {
        return Ok(());
    }
    if password.is_empty() {
        return Err(anyhow!("master password must not be empty"));
    }
    ensure_config_dir()?;
    let path = secrets_path()?;

    if path.exists() {
        let salt = read_salt()?.ok_or_else(|| anyhow!("secrets store salt missing"))?;
        let key = derive_key(password, &salt)?;
        // Verify by decrypting the existing ciphertext. Supports both v1 and v2.
        let raw = std::fs::read_to_string(&path)?;
        let probe: serde_json::Value = serde_json::from_str(&raw)?;
        let version = probe.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u32;

        if version >= 2 {
            // v2: verify by decrypting one entry (or succeed if empty).
            let store: EncryptedStoreV2 = serde_json::from_str(&raw)?;
            if let Some((account, entry)) = store.entries.iter().next() {
                let nonce = b64().decode(entry.nonce.as_bytes())?;
                let ciphertext = b64().decode(entry.ciphertext.as_bytes())?;
                decrypt_entry(&key, account, &nonce, &ciphertext)?;
            }
        } else {
            // v1: verify by decrypting the single blob.
            let store: EncryptedStoreV1 = serde_json::from_str(&raw)?;
            let nonce = b64().decode(store.nonce.as_bytes())?;
            let ciphertext = b64().decode(store.ciphertext.as_bytes())?;
            decrypt_v1_map(&key, &nonce, &ciphertext)?;
        }
        set_cached_key(Some(key));
    } else {
        let salt = random_bytes::<16>()?;
        let key = derive_key(password, &salt)?;
        // Write an empty v2 store under the new salt, then cache the key.
        let store = EncryptedStoreV2 {
            version: 2,
            kdf: "argon2id".into(),
            salt: b64().encode(&salt),
            entries: HashMap::new(),
        };
        std::fs::write(&path, serde_json::to_string_pretty(&store)?)?;
        restrict_file_to_owner(&path)?;
        set_cached_key(Some(key));
    }
    Ok(())
}

/// Change the master password: re-encrypt the current secrets under a key derived
/// from `new_password` (with a fresh salt). Requires the store to be unlocked.
pub fn change_master_password(new_password: &str) -> Result<()> {
    if backend_is_memory() {
        return Ok(());
    }
    if new_password.is_empty() {
        return Err(anyhow!("master password must not be empty"));
    }
    let map = load_map()?; // requires unlocked (loads v1 or v2 transparently)
    let salt = random_bytes::<16>()?;
    let key = derive_key(new_password, &salt)?;

    // Save in v2 format with the new key.
    let mut entries = HashMap::with_capacity(map.len());
    for (account, secret) in &map {
        let (nonce, ciphertext) = encrypt_entry(&key, account, secret.as_bytes())?;
        entries.insert(
            account.clone(),
            EncryptedEntry {
                nonce: b64().encode(&nonce),
                ciphertext: b64().encode(&ciphertext),
            },
        );
    }
    let store = EncryptedStoreV2 {
        version: 2,
        kdf: "argon2id".into(),
        salt: b64().encode(&salt),
        entries,
    };
    std::fs::write(secrets_path()?, serde_json::to_string_pretty(&store)?)?;
    restrict_file_to_owner(&secrets_path()?)?;
    set_cached_key(Some(key));
    Ok(())
}

/// Best-effort auto-unlock of an **existing** store from
/// `AGENT2SSH_MASTER_PASSWORD` (used by the headless CLI/MCP/daemon surfaces).
/// Returns true if the store is unlocked afterward. Does **not** create a new
/// store — read paths (`get_secret`, status) must not have the side effect of
/// initializing one; first-time creation happens only on an explicit write
/// (`store_secret`).
pub fn try_unlock_from_env() -> bool {
    if is_unlocked() {
        return true;
    }
    if !is_initialized() {
        return false;
    }
    match std::env::var("AGENT2SSH_MASTER_PASSWORD") {
        Ok(pw) if !pw.is_empty() => match unlock_or_init(&pw) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("warning: master password from AGENT2SSH_MASTER_PASSWORD rejected: {e}");
                false
            }
        },
        _ => false,
    }
}

/// Lock the store (drop the cached key). Mainly for tests.
pub fn lock() {
    set_cached_key(None);
}

// ── Per-account API ───────────────────────────────────────────────────────────

/// Store `secret` for `account`. Requires the store to be unlocked (or auto-
/// unlockable via env); returns an error otherwise so a caller never silently
/// drops or leaks a password.
pub fn store_secret(account: &str, secret: &str) -> Result<()> {
    if backend_is_memory() {
        memory_store()
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_string());
        return Ok(());
    }
    if !is_unlocked() && !try_unlock_from_env() {
        // Not unlocked and no existing store to unlock. First-time headless setup:
        // initialize a new store from AGENT2SSH_MASTER_PASSWORD if present.
        match std::env::var("AGENT2SSH_MASTER_PASSWORD") {
            Ok(pw) if !pw.is_empty() => unlock_or_init(&pw)?,
            _ => {
                return Err(anyhow!(
                    "credential store is locked — set a master password (desktop) or AGENT2SSH_MASTER_PASSWORD to store credentials"
                ))
            }
        }
    }
    let mut map = load_map()?;
    map.insert(account.to_string(), secret.to_string());
    save_map(&map)
}

/// Resolve the secret for `account`. Returns `None` when the store is locked or
/// the account has no stored secret (the caller treats a missing secret as "no
/// password" rather than leaking the marker into an auth attempt).
pub fn get_secret(account: &str) -> Option<String> {
    if backend_is_memory() {
        return memory_store().lock().unwrap().get(account).cloned();
    }
    if !is_unlocked() && !try_unlock_from_env() {
        return None;
    }
    load_map().ok().and_then(|m| m.get(account).cloned())
}

/// Delete any stored secret for `account`. Best-effort: a locked store or missing
/// entry is not treated as an error.
pub fn delete_secret(account: &str) {
    if backend_is_memory() {
        memory_store().lock().unwrap().remove(account);
        return;
    }
    if !is_unlocked() && !try_unlock_from_env() {
        return;
    }
    if let Ok(mut map) = load_map() {
        if map.remove(account).is_some() {
            let _ = save_map(&map);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn memory_backend_roundtrip() {
        std::env::set_var("AGENT2SSH_SECRETS_BACKEND", "memory");
        let account = host_account("secrets-roundtrip");
        store_secret(&account, "hunter2").unwrap();
        assert_eq!(get_secret(&account).as_deref(), Some("hunter2"));
        delete_secret(&account);
        assert_eq!(get_secret(&account), None);
        std::env::remove_var("AGENT2SSH_SECRETS_BACKEND");
    }

    #[test]
    fn ref_marker_detection() {
        assert!(is_secret_ref(SECRET_REF));
        assert!(is_secret_ref(LEGACY_KEYRING_REF));
        assert!(is_current_secret_ref(SECRET_REF));
        assert!(!is_current_secret_ref(LEGACY_KEYRING_REF));
        assert!(is_legacy_keyring_ref(LEGACY_KEYRING_REF));
        assert!(!is_legacy_keyring_ref(SECRET_REF));
        assert!(!is_secret_ref("hunter2"));
        assert!(!is_secret_ref(""));
    }

    #[test]
    #[serial_test::serial]
    fn encrypted_store_init_unlock_roundtrip() {
        // Exercise the real encrypted backend (not the memory test default).
        let dir = std::env::temp_dir().join(format!("agent2ssh-enc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);
        std::env::set_var("AGENT2SSH_SECRETS_BACKEND", "encrypted");
        lock();

        assert!(!is_initialized());
        // Setting the master password initializes the store.
        unlock_or_init("correct horse battery staple").unwrap();
        assert!(is_initialized());
        assert!(is_unlocked());

        let acct = host_account("enc-host");
        store_secret(&acct, "s3cr3t").unwrap();
        assert_eq!(get_secret(&acct).as_deref(), Some("s3cr3t"));

        // Plaintext must not appear on disk.
        let raw = std::fs::read_to_string(dir.join(SECRETS_FILE)).unwrap();
        assert!(
            !raw.contains("s3cr3t"),
            "ciphertext must not leak plaintext"
        );

        // Re-lock, then a wrong password is rejected and the right one works.
        lock();
        assert!(!is_unlocked());
        assert!(unlock_or_init("wrong password").is_err());
        lock();
        unlock_or_init("correct horse battery staple").unwrap();
        assert_eq!(get_secret(&acct).as_deref(), Some("s3cr3t"));

        lock();
        std::env::remove_var("AGENT2SSH_SECRETS_BACKEND");
        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn locked_store_yields_none_and_store_errors() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-enc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);
        std::env::set_var("AGENT2SSH_SECRETS_BACKEND", "encrypted");
        std::env::remove_var("AGENT2SSH_MASTER_PASSWORD");
        lock();

        // Locked + no env: get returns None, store errors (never leaks/loses).
        assert_eq!(get_secret(&host_account("x")), None);
        assert!(store_secret(&host_account("x"), "pw").is_err());

        std::env::remove_var("AGENT2SSH_SECRETS_BACKEND");
        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn aad_binding_prevents_cut_and_paste() {
        // Verify that a ciphertext encrypted for account A cannot be decrypted
        // under account B — the AEAD tag verification must fail.
        let dir = std::env::temp_dir().join(format!("agent2ssh-enc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);
        std::env::set_var("AGENT2SSH_SECRETS_BACKEND", "encrypted");
        lock();

        unlock_or_init("correct horse battery staple").unwrap();

        let acct_a = host_account("host-a");
        let acct_b = host_account("host-b");
        store_secret(&acct_a, "secret-for-a").unwrap();
        assert_eq!(get_secret(&acct_a).as_deref(), Some("secret-for-a"));

        // Read the raw file, extract account A's entry, and try to decrypt it
        // under account B's name — this must fail.
        let key = cached_key().expect("key must be cached after unlock");
        let raw = std::fs::read_to_string(dir.join(SECRETS_FILE)).unwrap();
        let store: EncryptedStoreV2 = serde_json::from_str(&raw).unwrap();
        let entry_a = store
            .entries
            .get(&acct_a)
            .expect("entry for host-a must exist");
        let nonce = b64().decode(entry_a.nonce.as_bytes()).unwrap();
        let ciphertext = b64().decode(entry_a.ciphertext.as_bytes()).unwrap();

        // Decrypting under the correct account works.
        let plaintext = decrypt_entry(&key, &acct_a, &nonce, &ciphertext);
        assert!(plaintext.is_ok());
        assert_eq!(
            String::from_utf8(plaintext.unwrap()).unwrap(),
            "secret-for-a"
        );

        // Decrypting under a different account name must fail (AAD mismatch).
        let result = decrypt_entry(&key, &acct_b, &nonce, &ciphertext);
        assert!(
            result.is_err(),
            "cut-and-paste attack must fail: AAD mismatch should cause tag verification failure"
        );

        lock();
        std::env::remove_var("AGENT2SSH_SECRETS_BACKEND");
        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn v1_to_v2_migration_on_save() {
        // Write a v1 format file manually, then verify it's read correctly and
        // migrated to v2 on the next save.
        let dir = std::env::temp_dir().join(format!("agent2ssh-enc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);
        std::env::set_var("AGENT2SSH_SECRETS_BACKEND", "encrypted");
        lock();

        // Create a v1 store manually.
        let salt = random_bytes::<16>().unwrap();
        let password = "migration-test-pw";
        let key = derive_key(password, &salt).unwrap();
        let mut map = HashMap::new();
        map.insert(host_account("mig-host"), "mig-secret".to_string());
        let (nonce, ciphertext) = {
            use aes_gcm::aead::{Aead, KeyInit};
            use aes_gcm::{Aes256Gcm, Nonce};
            let plaintext = serde_json::to_vec(&map).unwrap();
            let nonce_bytes = random_bytes::<12>().unwrap();
            let cipher = Aes256Gcm::new((&key).into());
            let ct = cipher
                .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
                .unwrap();
            (nonce_bytes.to_vec(), ct)
        };
        let v1_store = serde_json::json!({
            "version": 1u32,
            "kdf": "argon2id",
            "salt": b64().encode(&salt),
            "nonce": b64().encode(&nonce),
            "ciphertext": b64().encode(&ciphertext),
        });
        std::fs::write(
            dir.join(SECRETS_FILE),
            serde_json::to_string_pretty(&v1_store).unwrap(),
        )
        .unwrap();

        // Unlock with the v1 password — should succeed and read v1 format.
        unlock_or_init(password).unwrap();
        assert_eq!(
            get_secret(&host_account("mig-host")).as_deref(),
            Some("mig-secret"),
            "v1 store must be readable"
        );

        // Now store a new secret — this triggers save_map which writes v2.
        store_secret(&host_account("mig-host-2"), "mig-secret-2").unwrap();

        // Verify the file is now v2.
        let raw = std::fs::read_to_string(dir.join(SECRETS_FILE)).unwrap();
        let probe: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            probe["version"].as_u64(),
            Some(2),
            "file must be migrated to v2"
        );

        // Both secrets must be readable after migration.
        assert_eq!(
            get_secret(&host_account("mig-host")).as_deref(),
            Some("mig-secret"),
            "original secret must survive migration"
        );
        assert_eq!(
            get_secret(&host_account("mig-host-2")).as_deref(),
            Some("mig-secret-2"),
            "new secret must be readable after migration"
        );

        lock();
        std::env::remove_var("AGENT2SSH_SECRETS_BACKEND");
        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
