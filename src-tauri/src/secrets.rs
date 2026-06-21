//! App-managed encrypted credential storage for SSH passwords (K1).
//!
//! Agent2SSH manages its own credential encryption rather than delegating to the
//! OS keychain. Host/proxy passwords are encrypted at rest in
//! `~/.agent2ssh/secrets.enc` with **AES-256-GCM**, under a key derived from a
//! user **master password** via **Argon2id**. There is no plaintext key on disk:
//! the only way to decrypt is to supply the master password, which unlocks the
//! store for the process lifetime.
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

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, RwLock};

use crate::store::{config_dir, ensure_config_dir, restrict_file_to_owner};

/// Encrypted credential file under the config dir.
const SECRETS_FILE: &str = "secrets.enc";

/// On-disk sentinel that replaces a real password in `hosts.json`. When a
/// `password` field equals this exact value, the real secret lives (encrypted)
/// in `secrets.enc` under the account derived from the host/proxy identity.
pub const SECRET_REF: &str = "$agent2ssh-secret$";

/// True if `value` is the encrypted-secret reference marker rather than a real
/// secret (or the empty string).
pub fn is_secret_ref(value: &str) -> bool {
    value == SECRET_REF
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
    static KEY: OnceLock<RwLock<Option<[u8; 32]>>> = OnceLock::new();
    KEY.get_or_init(|| RwLock::new(None))
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
    static STORE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

// ── On-disk encrypted file ────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
struct EncryptedStore {
    version: u32,
    kdf: String,
    /// base64 Argon2 salt.
    salt: String,
    /// base64 AES-GCM nonce (12 bytes).
    nonce: String,
    /// base64 AES-256-GCM ciphertext of the JSON-encoded `HashMap<String,String>`.
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

fn encrypt_map(key: &[u8; 32], map: &HashMap<String, String>) -> Result<(Vec<u8>, Vec<u8>)> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    let plaintext = serde_json::to_vec(map)?;
    let nonce_bytes = random_bytes::<12>()?;
    let cipher = Aes256Gcm::new(key.into());
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
        .map_err(|e| anyhow!("encryption failed: {e}"))?;
    Ok((nonce_bytes.to_vec(), ciphertext))
}

fn decrypt_map(key: &[u8; 32], nonce: &[u8], ciphertext: &[u8]) -> Result<HashMap<String, String>> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    let cipher = Aes256Gcm::new(key.into());
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|_| anyhow!("decryption failed (wrong master password or corrupt store)"))?;
    Ok(serde_json::from_slice(&plaintext)?)
}

/// Read + decrypt the whole secret map using the cached key. Returns an error if
/// the store is locked.
fn load_map() -> Result<HashMap<String, String>> {
    let key = cached_key().ok_or_else(|| anyhow!("credential store is locked"))?;
    let path = secrets_path()?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let raw = std::fs::read_to_string(&path).context("failed to read secrets store")?;
    let store: EncryptedStore =
        serde_json::from_str(&raw).context("failed to parse secrets store")?;
    let nonce = b64()
        .decode(store.nonce.as_bytes())
        .context("bad nonce encoding")?;
    let ciphertext = b64()
        .decode(store.ciphertext.as_bytes())
        .context("bad ciphertext encoding")?;
    decrypt_map(&key, &nonce, &ciphertext)
}

/// Encrypt + write the whole secret map. Reuses the stored Argon2 salt so the
/// cached key stays valid; only the nonce + ciphertext change.
fn save_map(map: &HashMap<String, String>) -> Result<()> {
    let key = cached_key().ok_or_else(|| anyhow!("credential store is locked"))?;
    ensure_config_dir()?;
    let path = secrets_path()?;

    // Preserve the existing salt (the cached key was derived from it). On first
    // write the salt must already have been chosen by `unlock_or_init`.
    let salt = read_salt()?.ok_or_else(|| anyhow!("secrets store salt missing"))?;
    let (nonce, ciphertext) = encrypt_map(&key, map)?;
    let store = EncryptedStore {
        version: 1,
        kdf: "argon2id".into(),
        salt: b64().encode(salt),
        nonce: b64().encode(nonce),
        ciphertext: b64().encode(ciphertext),
    };
    let raw = serde_json::to_string_pretty(&store)?;
    std::fs::write(&path, raw).context("failed to write secrets store")?;
    restrict_file_to_owner(&path)?;
    Ok(())
}

/// Read the stored Argon2 salt, if the file exists.
fn read_salt() -> Result<Option<Vec<u8>>> {
    let path = secrets_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let store: EncryptedStore = serde_json::from_str(&raw)?;
    Ok(Some(b64().decode(store.salt.as_bytes())?))
}

// ── Unlock / init / lock ──────────────────────────────────────────────────────

/// Unlock an existing store, or initialize a new one, with `password`.
///
/// - If `secrets.enc` exists: derive the key from the stored salt and verify it
///   by decrypting; a wrong password returns an error.
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
        // Verify by decrypting the existing ciphertext.
        let raw = std::fs::read_to_string(&path)?;
        let store: EncryptedStore = serde_json::from_str(&raw)?;
        let nonce = b64().decode(store.nonce.as_bytes())?;
        let ciphertext = b64().decode(store.ciphertext.as_bytes())?;
        decrypt_map(&key, &nonce, &ciphertext)?; // errors on wrong password
        set_cached_key(Some(key));
    } else {
        let salt = random_bytes::<16>()?;
        let key = derive_key(password, &salt)?;
        // Write an empty store under the new salt, then cache the key.
        let (nonce, ciphertext) = encrypt_map(&key, &HashMap::new())?;
        let store = EncryptedStore {
            version: 1,
            kdf: "argon2id".into(),
            salt: b64().encode(salt),
            nonce: b64().encode(nonce),
            ciphertext: b64().encode(ciphertext),
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
    let map = load_map()?; // requires unlocked
    let salt = random_bytes::<16>()?;
    let key = derive_key(new_password, &salt)?;
    let (nonce, ciphertext) = encrypt_map(&key, &map)?;
    let store = EncryptedStore {
        version: 1,
        kdf: "argon2id".into(),
        salt: b64().encode(salt),
        nonce: b64().encode(nonce),
        ciphertext: b64().encode(ciphertext),
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
}
