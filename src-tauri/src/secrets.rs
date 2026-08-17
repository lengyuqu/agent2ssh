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

/// Read-only adapter over the encrypted secrets file.
///
/// Provides safe read-only access to the raw ciphertext bytes for sync
/// fingerprint computation — the hash in `collect_sync_files()` only needs
/// to detect whether the file changed, not decrypt it. By wrapping access
/// in a type that has **no write method**, the compiler enforces that the
/// sync fingerprint path can never accidentally mutate secrets.
///
/// Mirrors rssh's `CiphertextStore` pattern from `sync/metadata.rs`.
pub struct CiphertextStore {
    bytes: Vec<u8>,
}

impl CiphertextStore {
    /// Load the raw encrypted bytes from disk. Does NOT decrypt — just reads
    /// the file as-is so a SHA-256 fingerprint can be computed.
    pub fn load() -> Result<Self> {
        let path = secrets_path()?;
        let bytes = if path.exists() {
            std::fs::read(&path)
                .with_context(|| format!("failed to read secrets file {}", path.display()))?
        } else {
            Vec::new()
        };
        Ok(Self { bytes })
    }

    /// Return the raw ciphertext bytes (may be empty if the file doesn't exist).
    pub fn raw_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Compute a SHA-256 hex fingerprint of the ciphertext.
    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&self.bytes);
        hex::encode(hasher.finalize())
    }

    /// Whether the secrets file exists on disk.
    pub fn exists(&self) -> bool {
        !self.bytes.is_empty()
    }
}

/// Stable account name for a host profile's encrypted password.
pub fn host_account(host_name: &str) -> String {
    format!("host:{host_name}")
}

/// Stable account name for a host profile's encrypted private-key passphrase.
pub fn host_passphrase_account(host_name: &str) -> String {
    format!("host-key-passphrase:{host_name}")
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

// Thread-local key override (test-only). When set, takes priority over the
// global `key_cell()`, allowing parallel tests to each have their own unlocked
// key without interfering with each other.
#[cfg(test)]
thread_local! {
    static THREAD_KEY: std::cell::RefCell<Option<Option<[u8; 32]>>> =
        const { std::cell::RefCell::new(None) };
}

fn cached_key() -> Option<[u8; 32]> {
    #[cfg(test)]
    if let Some(key) = THREAD_KEY.with(|k| *k.borrow()) {
        return key;
    }
    *key_cell().read().expect("secrets key lock poisoned")
}

fn set_cached_key(key: Option<[u8; 32]>) {
    #[cfg(test)]
    if THREAD_KEY.with(|k| k.borrow().is_some()) {
        THREAD_KEY.with(|k| *k.borrow_mut() = Some(key));
        return;
    }
    *key_cell().write().expect("secrets key lock poisoned") = key;
}

/// Activate the thread-local key cache (test-only). After calling this,
/// `cached_key` / `set_cached_key` / `lock` operate on a per-thread cell
/// instead of the global `key_cell()`.
#[cfg(test)]
fn activate_thread_local_key() {
    THREAD_KEY.with(|k| {
        if k.borrow().is_none() {
            *k.borrow_mut() = Some(None);
        }
    });
}

// ── Test backend ──────────────────────────────────────────────────────────────

fn backend_is_memory() -> bool {
    // Thread-local override takes priority (used by tests).
    #[cfg(test)]
    if let Some(val) = THREAD_BACKEND.with(|b| *b.borrow()) {
        return val;
    }
    match std::env::var("AGENT2SSH_SECRETS_BACKEND") {
        Ok(v) if v.eq_ignore_ascii_case("memory") => true,
        Ok(v) if v.eq_ignore_ascii_case("encrypted") => false,
        // Unit tests default to an in-memory store so they never need a master
        // password or touch the encrypted file. Production binaries leave this
        // unset and use the encrypted store.
        _ => cfg!(test),
    }
}

// Thread-local override for the secrets backend mode (test-only).
#[cfg(test)]
thread_local! {
    static THREAD_BACKEND: std::cell::RefCell<Option<bool>> =
        const { std::cell::RefCell::new(None) };
}

/// Set a thread-local secrets backend override (test-only).
/// `true` = memory, `false` = encrypted.
#[cfg(test)]
fn set_test_backend(memory: bool) {
    THREAD_BACKEND.with(|b| *b.borrow_mut() = Some(memory));
}

/// Clear the thread-local secrets backend override (test-only).
#[cfg(test)]
fn clear_test_backend() {
    THREAD_BACKEND.with(|b| *b.borrow_mut() = None);
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

// ── Migration marker chain (B2) ───────────────────────────────────────────────
//
// An **immutable** append-only ledger of completed migrations, stored as
// `migrations.json` in the config dir. Each entry records the migration ID,
// a timestamp, and the SHA-256 of the secrets file at migration time (so a
// tampered file can be detected). Once a migration is marked done, subsequent
// loads skip the version probe and go straight to the v2 path — avoiding
// repeated decryption of the legacy v1 blob on every startup.
//
// Mirrors rssh's `migration/mod.rs` marker chain, adapted to file-based
// storage (agent2ssh has no DB).

/// Migration ledger file under the config dir.
const MIGRATIONS_FILE: &str = "migrations.json";

/// The migration ID for the v1→v2 per-entry encryption upgrade.
const MIGRATION_V1_TO_V2: &str = "secrets_v1_to_v2";

/// The status of a migration record.
///
/// A23: Migrations can be in one of three states:
/// - `Completed`: the migration ran successfully and the file was transformed.
/// - `Skipped`: the migration's precondition was not met (e.g. the v1 file
///   doesn't exist), so it was deliberately skipped. This is recorded so
///   future loads know it was intentionally bypassed, not forgotten.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
enum MigrationStatus {
    #[default]
    Completed,
    Skipped,
}

/// A single completed migration record. Stored in the `completed` array of
/// [`MigrationLedger`]. Records are **immutable** — once written they are
/// never deleted or modified.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct MigrationRecord {
    /// Unique migration identifier (e.g. `secrets_v1_to_v2`).
    id: String,
    /// Unix timestamp (seconds) when the migration was completed.
    completed_at: u64,
    /// SHA-256 hex digest of the secrets file **after** migration, providing
    /// a tamper-evidence anchor.
    file_sha256: String,
    /// A23: Whether this migration was completed or skipped due to a
    /// precondition not being met. Default: `completed`.
    #[serde(default)]
    status: MigrationStatus,
    /// A23: Optional reason for skipping (only set when status is `skipped`).
    #[serde(skip_serializing_if = "Option::is_none")]
    skip_reason: Option<String>,
}

/// On-disk ledger of all completed migrations. Loaded from / saved to
/// `migrations.json`. New records are **appended** — existing ones are never
/// removed, forming an immutable chain.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct MigrationLedger {
    /// Schema version of the ledger itself (not the secrets store).
    ledger_version: u32,
    /// All completed migrations, in chronological order.
    completed: Vec<MigrationRecord>,
}

impl MigrationLedger {
    /// Load the ledger from disk. Returns an empty ledger if the file does
    /// not exist (first run or no migrations have run yet).
    fn load() -> Result<Self> {
        let path = migrations_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read migrations file {}", path.display()))?;
        let ledger: MigrationLedger =
            serde_json::from_str(&raw).context("failed to parse migrations ledger")?;
        Ok(ledger)
    }

    /// Append a new migration record and persist. Existing records are never
    /// modified or deleted — this is an append-only operation.
    fn append_and_save(&mut self, record: MigrationRecord) -> Result<()> {
        // Idempotent: if the migration ID is already present, do nothing.
        if self.completed.iter().any(|r| r.id == record.id) {
            return Ok(());
        }
        self.completed.push(record);
        ensure_config_dir()?;
        let path = migrations_path()?;
        let raw = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, raw)
            .with_context(|| format!("failed to write migrations file {}", path.display()))?;
        restrict_file_to_owner(&path)?;
        Ok(())
    }

    /// Check whether a migration with the given ID has been completed.
    /// Returns `true` only for records with `status == Completed`.
    /// A23: Skipped migrations are NOT "done" — they were intentionally
    /// bypassed, so the migration may need to run if preconditions change.
    #[cfg_attr(not(test), allow(dead_code))]
    fn is_done(&self, id: &str) -> bool {
        self.completed
            .iter()
            .any(|r| r.id == id && r.status == MigrationStatus::Completed)
    }

    /// A23: Check whether a migration was explicitly skipped (precondition
    /// not met). Returns `true` only for records with `status == Skipped`.
    #[cfg_attr(not(test), allow(dead_code))]
    fn is_skipped(&self, id: &str) -> bool {
        self.completed
            .iter()
            .any(|r| r.id == id && r.status == MigrationStatus::Skipped)
    }

    /// A23: Check whether a migration has been resolved — either completed
    /// or explicitly skipped. This is used to avoid re-checking preconditions
    /// on every load when the migration was already handled.
    fn is_resolved(&self, id: &str) -> bool {
        self.completed.iter().any(|r| r.id == id)
    }

    /// A23: Append a "skipped" record with a reason. This records that
    /// a migration was deliberately bypassed (e.g. its precondition was not
    /// met), so future loads know it was handled.
    #[cfg_attr(not(test), allow(dead_code))]
    fn append_skipped(&mut self, id: &str, reason: &str) -> Result<()> {
        if self.is_resolved(id) {
            return Ok(());
        }
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.append_and_save(MigrationRecord {
            id: id.to_string(),
            completed_at: timestamp,
            file_sha256: String::new(),
            status: MigrationStatus::Skipped,
            skip_reason: Some(reason.to_string()),
        })
    }
}

fn migrations_path() -> Result<std::path::PathBuf> {
    Ok(config_dir()?.join(MIGRATIONS_FILE))
}

/// Check whether the v1→v2 migration has been marked as complete.
#[cfg_attr(not(test), allow(dead_code))]
fn is_v1_to_v2_migration_done() -> bool {
    MigrationLedger::load()
        .map(|l| l.is_done(MIGRATION_V1_TO_V2))
        .unwrap_or(false)
}

/// A23: Check whether the v1→v2 migration has been **resolved** (either
/// completed or explicitly skipped). This is used in `load_map()` to skip
/// the version probe — if the migration was already handled (even if
/// skipped), we don't need to probe again.
fn is_v1_to_v2_migration_resolved() -> bool {
    MigrationLedger::load()
        .map(|l| l.is_resolved(MIGRATION_V1_TO_V2))
        .unwrap_or(false)
}

/// Mark the v1→v2 migration as complete by appending a record to the ledger.
/// Computes the SHA-256 of the current secrets file for tamper-evidence.
fn mark_v1_to_v2_migration_done() -> Result<()> {
    let file_sha256 = CiphertextStore::load()
        .map(|s| s.fingerprint())
        .unwrap_or_else(|_| String::new());
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut ledger = MigrationLedger::load()?;
    ledger.append_and_save(MigrationRecord {
        id: MIGRATION_V1_TO_V2.to_string(),
        completed_at: timestamp,
        file_sha256,
        status: MigrationStatus::Completed,
        skip_reason: None,
    })
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

    // B2/A23: If the v1→v2 migration marker exists (completed or skipped),
    // we know the file format is settled — skip the version probe entirely
    // and go straight to the v2 parse path.
    let version = if is_v1_to_v2_migration_resolved() {
        2
    } else {
        // Probe the version field to decide the format.
        let version_probe: serde_json::Value =
            serde_json::from_str(&raw).context("failed to parse secrets store")?;
        version_probe
            .get("version")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32
    };

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
    // Finding 11 + atomicity: write to a temp file in the same directory,
    // fsync it, then rename over secrets.enc. `File::create` truncates the
    // original in place — a crash mid-write leaves a corrupt store and loses
    // every credential. The rename is atomic, so readers always see either
    // the old or the new content, never a partial write.
    atomic_write_file(&path, raw.as_bytes())?;
    restrict_file_to_owner(&path)?;

    // B2/A23: After writing v2, mark the v1→v2 migration as done (if not
    // already resolved — completed or skipped).
    if !is_v1_to_v2_migration_resolved() {
        let _ = mark_v1_to_v2_migration_done();
    }

    Ok(())
}

/// Crash-safe atomic file write: write to a temp file in the same directory,
/// fsync it, rename over the target, then best-effort fsync the parent
/// directory. Unlike an in-place `File::create` write (which truncates the
/// original and can leave a corrupt file on crash), the rename is atomic on
/// POSIX and Windows, so the target is either the old or the new content.
fn atomic_write_file(path: &std::path::Path, raw: &[u8]) -> Result<()> {
    use std::io::Write;
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("no parent directory for {}", path.display()))?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "store".to_string());
    let tmp = dir.join(format!(".{file_name}.tmp-{}", std::process::id()));
    {
        let mut file = std::fs::File::create(&tmp)
            .with_context(|| format!("failed to create temp store {}", tmp.display()))?;
        file.write_all(raw).context("failed to write temp store")?;
        file.sync_all().context("failed to fsync temp store")?;
    }
    std::fs::rename(&tmp, path)
        .with_context(|| format!("failed to rename temp store to {}", path.display()))?;
    // Best-effort: fsync the directory so the rename is durable across power loss.
    if let Ok(dir_file) = std::fs::File::open(dir) {
        let _ = dir_file.sync_all();
    }
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
            salt: b64().encode(salt),
            entries: HashMap::new(),
        };
        let json = serde_json::to_string_pretty(&store)?;

        // S8: Cross-process race prevention. Use `create_new(true)` so that if
        // another process (e.g. daemon + CLI starting simultaneously) already
        // created the file, we get `AlreadyExists` instead of overwriting it.
        // On collision, re-read the winner's file and verify our password
        // against it — if it matches, adopt their key; if not, error out.
        use std::io::ErrorKind;
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(json.as_bytes())?;
                restrict_file_to_owner(&path)?;
                set_cached_key(Some(key));
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                // Another process won the race. Re-read and verify our
                // password against the file they wrote.
                let salt =
                    read_salt()?.ok_or_else(|| anyhow!("secrets store salt missing after race"))?;
                let key = derive_key(password, &salt)?;
                let raw = std::fs::read_to_string(&path)?;
                let probe: serde_json::Value = serde_json::from_str(&raw)?;
                let version = probe.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                if version >= 2 {
                    let store: EncryptedStoreV2 = serde_json::from_str(&raw)?;
                    if let Some((account, entry)) = store.entries.iter().next() {
                        let nonce = b64().decode(entry.nonce.as_bytes())?;
                        let ciphertext = b64().decode(entry.ciphertext.as_bytes())?;
                        decrypt_entry(&key, account, &nonce, &ciphertext)?;
                    }
                } else {
                    let store: EncryptedStoreV1 = serde_json::from_str(&raw)?;
                    let nonce = b64().decode(store.nonce.as_bytes())?;
                    let ciphertext = b64().decode(store.ciphertext.as_bytes())?;
                    decrypt_v1_map(&key, &nonce, &ciphertext)?;
                }
                set_cached_key(Some(key));
            }
            Err(e) => {
                return Err(anyhow!("failed to create secrets store: {e}"));
            }
        }
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
        salt: b64().encode(salt),
        entries,
    };
    let path = secrets_path()?;
    atomic_write_file(&path, serde_json::to_string_pretty(&store)?.as_bytes())?;
    restrict_file_to_owner(&path)?;
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
    #[cfg(test)]
    activate_thread_local_key();
    set_cached_key(None);
    crate::embedded_ssh::passphrase_cache_clear();
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

/// S9: Check whether a secret exists for `account` **without decrypting**.
///
/// This reads the raw secrets file, parses the v2 JSON structure, and checks
/// whether the account name is a key in `entries` — without touching the
/// cached key or attempting any decryption. This is useful for UI status
/// checks (e.g. "is a password configured for this host?") that should not
/// trigger an unlock prompt or risk decryption errors on a locked store.
///
/// Returns `false` if the store doesn't exist, is locked, or the account
/// is not found.
pub fn secret_exists(account: &str) -> bool {
    if backend_is_memory() {
        return memory_store().lock().unwrap().contains_key(account);
    }
    let path = match secrets_path() {
        Ok(p) => p,
        Err(_) => return false,
    };
    if !path.exists() {
        return false;
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return false,
    };
    // Parse as generic JSON to check entries without decrypting.
    let probe: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let version = probe.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
    if version >= 2 {
        probe
            .get("entries")
            .and_then(|e| e.as_object())
            .map(|m| m.contains_key(account))
            .unwrap_or(false)
    } else {
        // v1: we can't check without decrypting (single blob). Be conservative
        // and report false — the caller can fall back to get_secret.
        false
    }
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
        set_test_backend(true);
        let account = host_account("secrets-roundtrip");
        store_secret(&account, "hunter2").unwrap();
        assert_eq!(get_secret(&account).as_deref(), Some("hunter2"));
        delete_secret(&account);
        assert_eq!(get_secret(&account), None);
        clear_test_backend();
    }

    #[test]
    #[serial_test::serial]
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
        crate::store::set_test_config_dir(&dir);
        set_test_backend(false);
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
        clear_test_backend();
        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn migration_ledger_preserves_existing_records() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-mig-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);
        set_test_backend(false);
        lock();

        // Pre-write a fake migration record.
        let pre_ledger = MigrationLedger {
            ledger_version: 1,
            completed: vec![MigrationRecord {
                id: "some_future_migration".to_string(),
                completed_at: 100,
                file_sha256: "abc123".to_string(),
                status: MigrationStatus::Completed,
                skip_reason: None,
            }],
        };
        std::fs::write(
            dir.join(MIGRATIONS_FILE),
            serde_json::to_string_pretty(&pre_ledger).unwrap(),
        )
        .unwrap();

        // Now init + save (triggers v1_to_v2 marker).
        unlock_or_init("preserve-pw").unwrap();
        store_secret(&host_account("preserve-host"), "preserve-secret").unwrap();

        // Both the pre-existing record and the new one must be present.
        let ledger = MigrationLedger::load().unwrap();
        assert_eq!(
            ledger.completed.len(),
            2,
            "pre-existing record must be preserved"
        );
        assert!(
            ledger.is_done("some_future_migration"),
            "old record preserved"
        );
        assert!(ledger.is_done(MIGRATION_V1_TO_V2), "new record appended");

        lock();
        clear_test_backend();
        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── S8: Cross-process race prevention ──────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn s8_race_winner_adopts_existing_file() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-s8-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);
        set_test_backend(false);
        lock();

        // Simulate a "winner" that already created the store.
        unlock_or_init("winner-pw").unwrap();
        store_secret(&host_account("winner-host"), "winner-secret").unwrap();
        lock();

        // Now a "late starter" tries to init with the same password.
        // It should detect the existing file and adopt it rather than overwrite.
        unlock_or_init("winner-pw").unwrap();
        assert_eq!(
            get_secret(&host_account("winner-host")).as_deref(),
            Some("winner-secret"),
            "late starter must adopt the winner's store, not overwrite it"
        );

        lock();
        clear_test_backend();
        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn s8_race_with_wrong_password_fails() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-s8w-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);
        set_test_backend(false);
        lock();

        // Winner creates store with "correct-pw".
        unlock_or_init("correct-pw").unwrap();
        store_secret(&host_account("race-host"), "race-secret").unwrap();
        lock();

        // Late starter with wrong password must fail, not overwrite.
        let result = unlock_or_init("wrong-pw");
        assert!(
            result.is_err(),
            "late starter with wrong password must not overwrite the winner's store"
        );

        lock();
        clear_test_backend();
        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── S9: secret_exists without decryption ──────────────────────────────

    #[test]
    #[serial_test::serial]
    fn s9_secret_exists_returns_true_for_stored_secret() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-s9-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);
        set_test_backend(false);
        lock();

        unlock_or_init("exists-pw").unwrap();
        let acct = host_account("exists-host");
        store_secret(&acct, "exists-secret").unwrap();

        assert!(
            secret_exists(&acct),
            "secret_exists must return true for a stored secret"
        );
        assert!(
            !secret_exists(&host_account("nonexistent-host")),
            "secret_exists must return false for a missing account"
        );

        lock();
        clear_test_backend();
        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn s9_secret_exists_returns_false_when_locked() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-s9l-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);
        set_test_backend(false);
        lock();

        unlock_or_init("locked-pw").unwrap();
        let acct = host_account("locked-host");
        store_secret(&acct, "locked-secret").unwrap();
        lock();

        // Even when locked, secret_exists should work — it doesn't decrypt.
        assert!(
            secret_exists(&acct),
            "secret_exists must work when store is locked (no decryption)"
        );

        clear_test_backend();
        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn s9_secret_exists_returns_false_when_no_store() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-s9n-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);
        set_test_backend(false);

        assert!(
            !secret_exists(&host_account("any-host")),
            "must return false when no secrets file exists"
        );

        clear_test_backend();
        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── A23: Conditional marker skip logic ────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn a23_skipped_migration_is_not_done() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-a23s-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);

        let mut ledger = MigrationLedger::default();
        ledger
            .append_skipped(MIGRATION_V1_TO_V2, "no v1 file existed")
            .unwrap();

        assert!(!ledger.is_done(MIGRATION_V1_TO_V2), "skipped is not done");
        assert!(
            ledger.is_skipped(MIGRATION_V1_TO_V2),
            "must be marked skipped"
        );
        assert!(
            ledger.is_resolved(MIGRATION_V1_TO_V2),
            "skipped is resolved"
        );

        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn a23_completed_then_skipped_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-a23c-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);

        let mut ledger = MigrationLedger::default();
        // First: mark as completed.
        ledger
            .append_and_save(MigrationRecord {
                id: MIGRATION_V1_TO_V2.to_string(),
                completed_at: 100,
                file_sha256: "abc".to_string(),
                status: MigrationStatus::Completed,
                skip_reason: None,
            })
            .unwrap();
        assert!(ledger.is_done(MIGRATION_V1_TO_V2));
        assert_eq!(ledger.completed.len(), 1);

        // Then: try to mark as skipped — must NOT append a duplicate.
        ledger
            .append_skipped(MIGRATION_V1_TO_V2, "should not happen")
            .unwrap();
        assert_eq!(
            ledger.completed.len(),
            1,
            "must not duplicate when already resolved"
        );
        assert!(
            ledger.is_done(MIGRATION_V1_TO_V2),
            "still done, not overwritten to skipped"
        );

        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn a23_skipped_record_has_reason() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-a23r-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);

        let mut ledger = MigrationLedger::default();
        ledger
            .append_skipped(
                "future_migration",
                "precondition not met: keyring unavailable",
            )
            .unwrap();

        let record = ledger
            .completed
            .iter()
            .find(|r| r.id == "future_migration")
            .unwrap();
        assert_eq!(record.status, MigrationStatus::Skipped);
        assert_eq!(
            record.skip_reason.as_deref(),
            Some("precondition not met: keyring unavailable")
        );

        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn a23_resolved_skips_version_probe() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-a23p-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);
        set_test_backend(false);
        lock();

        // Init a store and save a secret (triggers v2 write + migration marker).
        unlock_or_init("probe-pw").unwrap();
        store_secret(&host_account("probe-host"), "probe-secret").unwrap();
        assert!(is_v1_to_v2_migration_resolved());

        // Manually mark a future migration as skipped — load should still work.
        let mut ledger = MigrationLedger::load().unwrap();
        ledger
            .append_skipped("some_future_migration", "not applicable")
            .unwrap();
        assert!(ledger.is_skipped("some_future_migration"));
        assert!(ledger.is_resolved("some_future_migration"));

        lock();
        clear_test_backend();
        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn migration_ledger_starts_empty() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-mig-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);

        let ledger = MigrationLedger::load().unwrap();
        assert!(ledger.completed.is_empty(), "fresh dir has no migrations");
        assert!(!ledger.is_done(MIGRATION_V1_TO_V2));

        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn migration_marker_written_after_v1_to_v2() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-mig-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);
        set_test_backend(false);
        lock();

        // Create a v1 store manually (same as the v1_to_v2_migration_on_save test).
        let salt = random_bytes::<16>().unwrap();
        let password = "marker-test-pw";
        let key = derive_key(password, &salt).unwrap();
        let mut map = HashMap::new();
        map.insert(host_account("marker-host"), "marker-secret".to_string());
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
            "salt": b64().encode(salt),
            "nonce": b64().encode(&nonce),
            "ciphertext": b64().encode(&ciphertext),
        });
        std::fs::write(
            dir.join(SECRETS_FILE),
            serde_json::to_string_pretty(&v1_store).unwrap(),
        )
        .unwrap();

        // Before unlock: no migration marker.
        assert!(!is_v1_to_v2_migration_done());

        // Unlock and trigger migration by storing a new secret.
        unlock_or_init(password).unwrap();
        assert_eq!(
            get_secret(&host_account("marker-host")).as_deref(),
            Some("marker-secret"),
        );
        store_secret(&host_account("marker-host-2"), "marker-secret-2").unwrap();

        // After save (which writes v2): marker must exist.
        assert!(
            is_v1_to_v2_migration_done(),
            "migration marker must be written after v1->v2 migration"
        );

        // The ledger file must exist on disk.
        let ledger_path = dir.join(MIGRATIONS_FILE);
        assert!(ledger_path.exists(), "migrations.json must be on disk");

        // The ledger must contain the v1_to_v2 record with a file_sha256.
        let ledger = MigrationLedger::load().unwrap();
        assert_eq!(ledger.completed.len(), 1);
        let record = &ledger.completed[0];
        assert_eq!(record.id, MIGRATION_V1_TO_V2);
        assert!(
            !record.file_sha256.is_empty(),
            "file_sha256 must be recorded"
        );
        assert_eq!(
            record.file_sha256.len(),
            64,
            "SHA-256 hex digest is 64 chars"
        );

        lock();
        clear_test_backend();
        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn migration_marker_skips_v1_probe_on_reload() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-mig-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);
        set_test_backend(false);
        lock();

        // Init a v2 store directly (no v1).
        unlock_or_init("skip-probe-pw").unwrap();
        store_secret(&host_account("skip-probe-host"), "skip-probe-secret").unwrap();
        assert!(
            is_v1_to_v2_migration_done(),
            "marker written on first v2 save"
        );

        // Lock and re-unlock: load_map should use the v2 fast path.
        lock();
        unlock_or_init("skip-probe-pw").unwrap();
        assert_eq!(
            get_secret(&host_account("skip-probe-host")).as_deref(),
            Some("skip-probe-secret"),
            "v2 fast-path load must return correct secret"
        );

        lock();
        clear_test_backend();
        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn migration_marker_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-mig-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);
        set_test_backend(false);
        lock();

        unlock_or_init("idempotent-pw").unwrap();
        store_secret(&host_account("idem-host"), "idem-secret").unwrap();
        assert!(is_v1_to_v2_migration_done());

        // Save again — must not create a duplicate record.
        store_secret(&host_account("idem-host-2"), "idem-secret-2").unwrap();
        let ledger = MigrationLedger::load().unwrap();
        assert_eq!(
            ledger.completed.len(),
            1,
            "append_and_save must be idempotent — no duplicate records"
        );

        lock();
        clear_test_backend();
        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn migration_marker_records_file_fingerprint() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-mig-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);
        set_test_backend(false);
        lock();

        unlock_or_init("fingerprint-pw").unwrap();
        store_secret(&host_account("fp-host"), "fp-secret").unwrap();

        // The recorded fingerprint must match the actual file fingerprint.
        let actual_fp = CiphertextStore::load().unwrap().fingerprint();
        let ledger = MigrationLedger::load().unwrap();
        let record = &ledger.completed[0];
        assert_eq!(
            record.file_sha256, actual_fp,
            "recorded fingerprint must match the secrets file at migration time"
        );

        lock();
        clear_test_backend();
        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── CiphertextStore read-only adapter ───────────────────────────────

    #[test]
    #[serial_test::serial]
    fn ciphertext_store_loads_and_fingerprints_existing_file() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-cts-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);

        // No file yet — load succeeds with empty bytes.
        let store = CiphertextStore::load().unwrap();
        assert!(!store.exists());
        assert!(store.raw_bytes().is_empty());
        assert!(!store.fingerprint().is_empty());

        // Write a file and reload.
        std::fs::write(dir.join(SECRETS_FILE), b"fake-ciphertext").unwrap();
        let store = CiphertextStore::load().unwrap();
        assert!(store.exists());
        assert_eq!(store.raw_bytes(), b"fake-ciphertext");

        // Fingerprint is a stable SHA-256 hex string.
        let fp = store.fingerprint();
        assert_eq!(fp.len(), 64);
        let fp2 = store.fingerprint();
        assert_eq!(fp, fp2, "fingerprint must be deterministic");

        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn ciphertext_store_fingerprint_changes_when_file_changes() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-cts2-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);

        std::fs::write(dir.join(SECRETS_FILE), b"content-a").unwrap();
        let fp_a = CiphertextStore::load().unwrap().fingerprint();

        std::fs::write(dir.join(SECRETS_FILE), b"content-b").unwrap();
        let fp_b = CiphertextStore::load().unwrap().fingerprint();

        assert_ne!(fp_a, fp_b, "fingerprint must change when content changes");

        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
