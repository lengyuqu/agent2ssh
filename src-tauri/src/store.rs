use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use crate::app_state::app_state;
use crate::types::{
    default_host_group, default_host_groups, AppConfig, AuditEntry, AuditFilter, ExecResult,
    RiskLevel,
};

// Process-local config file write lock, delegated to AppState (P2 #5).
pub fn hosts_lock() -> &'static Mutex<()> {
    &app_state().store_lock
}

type HostLabelMap =
    std::collections::HashMap<String, (Option<String>, Option<String>, Option<String>)>;

pub struct StoreWriteGuard {
    _process_guard: MutexGuard<'static, ()>,
    _file_guard: FileLockGuard,
}

pub struct FileLockGuard {
    _file: File,
}

pub fn store_write_lock() -> Result<StoreWriteGuard> {
    let process_guard = hosts_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("store lock poisoned"))?;
    let file_guard = lock_config_file(".hosts.lock")?;
    Ok(StoreWriteGuard {
        _process_guard: process_guard,
        _file_guard: file_guard,
    })
}

fn audit_write_lock() -> Result<FileLockGuard> {
    lock_config_file(".audit.lock")
}

/// Acquire an exclusive cross-process advisory lock backed by a dedicated lock
/// file under the config dir (e.g. `.hosts.lock`, `.audit.lock`, `.app_log.lock`).
/// Held by the returned guard until it drops. Use this — not only a process-local
/// `Mutex` — whenever a file under `~/.agent2ssh/` is written by more than one of
/// the CLI/MCP/daemon/desktop processes, so concurrent writers cannot interleave
/// or race a rotation.
pub fn lock_config_file(name: &str) -> Result<FileLockGuard> {
    ensure_config_dir()?;
    let path = config_dir()?.join(name);
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("failed to open lock file {}", path.display()))?;
    restrict_file_to_owner(&path)?;
    file.lock_exclusive()
        .with_context(|| format!("failed to lock {}", path.display()))?;
    Ok(FileLockGuard { _file: file })
}

pub fn config_dir() -> Result<PathBuf> {
    // Thread-local override takes priority (used by tests to avoid env-var
    // race conditions when running in parallel).
    #[cfg(test)]
    if let Some(path) = THREAD_CONFIG_DIR.with(|d| d.borrow().clone()) {
        return Ok(path);
    }

    if let Some(path) = config_dir_override(std::env::var("AGENT2SSH_CONFIG_DIR").ok()) {
        return Ok(path);
    }

    let base =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("unable to locate home directory"))?;
    Ok(base.join(".agent2ssh"))
}

// Thread-local override for the config directory. Tests should use
// `set_test_config_dir` / `clear_test_config_dir` instead of mutating the
// process environment to avoid races between parallel tests.
#[cfg(test)]
thread_local! {
    pub(crate) static THREAD_CONFIG_DIR: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Set a thread-local config directory override (test-only).
#[cfg(test)]
pub fn set_test_config_dir(path: impl Into<PathBuf>) {
    THREAD_CONFIG_DIR.with(|d| *d.borrow_mut() = Some(path.into()));
}

/// Clear the thread-local config directory override (test-only).
#[cfg(test)]
pub fn clear_test_config_dir() {
    THREAD_CONFIG_DIR.with(|d| *d.borrow_mut() = None);
}

fn config_dir_override(path: Option<String>) -> Option<PathBuf> {
    path.filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("hosts.json"))
}

/// Current `hosts.json` schema version. Bump this whenever the on-disk shape
/// changes in a way that needs an explicit migration step in [`migrate_config`].
/// Version 0 means "legacy / unversioned" (files written before K8). Version 1
/// is the first explicitly-versioned schema.
pub const CONFIG_SCHEMA_VERSION: u32 = 1;

/// Upgrade a freshly-parsed config from its persisted `schema_version` up to the
/// current one. Migrations run in order and must be idempotent; a config already
/// at (or ahead of) the current version is left untouched so a newer app's file
/// is never silently downgraded. The version is *not* stamped here — that happens
/// in [`normalize_config`] on save — so loading alone never rewrites the file.
fn migrate_config(mut config: AppConfig) -> AppConfig {
    // Forward-compat: a file written by a newer build (version > current) is left
    // as-is. We still serve it; on next save we keep its higher version number so
    // we don't advertise a downgrade.
    if config.schema_version >= CONFIG_SCHEMA_VERSION {
        return config;
    }

    // 0 -> 1: baseline. Pre-K8 files had no `schema_version`. There is no field
    // rename to perform (legacy defaults were already handled by serde + the
    // structural fix-ups in `normalize_config`), so this step only advances the
    // version marker. Future steps slot in here as additional
    // `if config.schema_version < N { ... }` blocks.
    if config.schema_version < 1 {
        config.schema_version = 1;
    }

    config
}

/// Cache for the parsed `hosts.json` (I5). `load_config` is on a hot path — every
/// host lookup for exec/list/SFTP/sessions resolves through it — yet the file
/// changes only on explicit host edits. The `(mtime, len)` signature picks up
/// cross-process edits (CLI/desktop) automatically, and `save_config_unlocked`
/// invalidates after every in-process write so a save is observed immediately.
static HOSTS_CACHE: crate::config_cache::ConfigCache<AppConfig> =
    crate::config_cache::ConfigCache::new();

pub fn audit_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("audit.jsonl"))
}

pub fn ensure_config_dir() -> Result<()> {
    fs::create_dir_all(config_dir()?).context("failed to create ~/.agent2ssh")
}

/// Restrict a sensitive file (tokens, keys, host config) to owner-only access.
///
/// On Unix this sets mode `0600`. On Windows it strips inherited ACEs and grants
/// the current user sole Full control via `icacls`, so other local accounts —
/// which would otherwise inherit read access from the parent directory — cannot
/// read `daemon.token`, `keys/`, or `hosts.json`. Without this, Windows had no
/// protection at all (the old code was `#[cfg(unix)]`-only).
pub fn restrict_file_to_owner(path: impl AsRef<std::path::Path>) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path.as_ref(), perms).with_context(|| {
            format!(
                "failed to restrict permissions for {}",
                path.as_ref().display()
            )
        })?;
    }
    #[cfg(windows)]
    {
        restrict_file_to_owner_windows(path.as_ref())?;
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
    }
    Ok(())
}

/// Windows owner-only ACL via `icacls`. We `/reset` then disable inheritance with
/// `/inheritance:r` (drops all inherited ACEs) and `/grant:r` Full control to the
/// current user only. The user principal is resolved from the `USERNAME`
/// environment variable (qualified with `USERDOMAIN` when present). If the env
/// var is missing or contains characters outside the safe set (defense against
/// a polluted environment redirecting the ACL grant to an unintended principal),
/// we fall back to `whoami`, then to a literal `%USERNAME%` only as a last
/// resort. Mirrors Unix `0600` (owner read/write, no group/other access).
#[cfg(windows)]
fn restrict_file_to_owner_windows(path: &std::path::Path) -> Result<()> {
    use std::process::Command;

    let user = resolve_windows_user_principal()?;

    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("path is not valid UTF-8: {}", path.display()))?;

    let output = Command::new("icacls")
        .arg(path_str)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{user}:(F)"))
        .output()
        .with_context(|| format!("failed to invoke icacls for {}", path.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "icacls failed to restrict {} ({}): {}",
            path.display(),
            output.status,
            stderr.trim()
        );
    }
    Ok(())
}

/// Resolve the current Windows user principal for ACL grant, hardening against
/// a polluted `USERNAME` environment variable. The previous implementation
/// trusted `USERNAME` directly — on Windows this is user-settable, so a
/// tampered environment could redirect the `0600`-equivalent grant to an
/// unintended account. We validate the env-var form first (alphanumerics,
/// `_`, `-`, `.` only, max 104 chars) and fall back to `whoami` if it fails.
#[cfg(windows)]
fn resolve_windows_user_principal() -> Result<String> {
    fn is_safe_name(s: &str) -> bool {
        !s.is_empty()
            && s.len() <= 104
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    }

    if let (Ok(domain), Ok(name)) = (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
        if is_safe_name(&domain) && is_safe_name(&name) {
            return Ok(format!("{domain}\\{name}"));
        }
    } else if let Ok(name) = std::env::var("USERNAME") {
        if is_safe_name(&name) {
            return Ok(name);
        }
    }

    // Env var missing or contains unsafe chars — ask the OS via `whoami`.
    // `whoami` returns `DOMAIN\username` (or just `username` in workgroup).
    let output = std::process::Command::new("whoami")
        .output()
        .context("failed to invoke whoami for ACL principal lookup")?;
    if output.status.success() {
        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // whoami output form is `DOMAIN\user` or `user` — both safe-by-construction
        // since they came from the OS, not the environment. Still reject empty.
        if !raw.is_empty() {
            return Ok(raw);
        }
    }

    // Both env var and whoami failed — bail closed. Callers of
    // restrict_file_to_owner treat failure as fatal, so the file won't be
    // written without an owner-only ACL. This is safer than guessing a
    // principal that might grant access to the wrong account.
    Err(anyhow::anyhow!(
        "could not resolve current Windows user principal (USERNAME env var invalid and whoami failed)"
    ))
}

pub fn load_config() -> Result<AppConfig> {
    ensure_config_dir()?;
    let path = config_path()?;
    HOSTS_CACHE.load_with(&path, || {
        if !path.exists() {
            return Ok(normalize_config(AppConfig::default()));
        }
        // save_config uses temp-file + atomic rename, so agent2ssh's own writes
        // never expose a half-written file to readers. The retry below covers
        // the residual case of an *external* non-atomic write (e.g. a text
        // editor truncating then writing hosts.json) racing with our read.
        // On parse failure, wait briefly and try once more — if it still fails
        // we surface the error rather than silently returning a default.
        match load_config_from_disk(&path) {
            Ok(config) => Ok(config),
            Err(first_err) => {
                std::thread::sleep(std::time::Duration::from_millis(50));
                load_config_from_disk(&path).map_err(|_| first_err)
            }
        }
    })
}

fn load_config_from_disk(path: &Path) -> Result<AppConfig> {
    let raw = fs::read_to_string(path).context("failed to read hosts config")?;
    let config: AppConfig = serde_json::from_str(&raw).context("failed to parse hosts config")?;
    let mut config = normalize_config(migrate_config(config));
    internalize_secrets(&mut config);
    Ok(config)
}

/// Resolve encrypted-secret references back into real passwords after loading
/// (K1). On disk a stored password is the [`crate::secrets::SECRET_REF`] marker;
/// when the credential store is **unlocked** we decrypt the real secret so every
/// downstream consumer keeps reading `HostProfile.password` as the actual
/// password. When **locked**, the marker is left in place (not blanked to
/// `None`), so a later save preserves the encrypted secret rather than orphaning
/// it; `embedded_ssh` treats a bare marker as "no usable password".
///
/// Legacy `$agent2ssh-keyring$` references are also left intact. They point at
/// the removed OS-keyring storage path, so they are not resolvable here; keeping
/// the marker prevents an unrelated save from destroying the only migration
/// signal.
fn internalize_secrets(config: &mut AppConfig) {
    let unlocked = crate::secrets::is_unlocked() || crate::secrets::try_unlock_from_env();
    for host in &mut config.hosts {
        if let Some(pw) = &host.password {
            if crate::secrets::is_current_secret_ref(pw) {
                if let Some(real) =
                    crate::secrets::get_secret(&crate::secrets::host_account(&host.name))
                {
                    host.password = Some(real);
                } else if unlocked {
                    // Genuinely missing (entry deleted) — don't leak the marker.
                    host.password = None;
                }
                // else: locked — keep the marker so save preserves the ref.
            }
        }
        if let Some(passphrase) = &host.passphrase {
            if crate::secrets::is_current_secret_ref(passphrase) {
                if let Some(real) =
                    crate::secrets::get_secret(&crate::secrets::host_passphrase_account(&host.name))
                {
                    host.passphrase = Some(real);
                } else if unlocked {
                    host.passphrase = None;
                }
            }
        }
    }
    for proxy in &mut config.proxies {
        if let Some(pw) = &proxy.password {
            if crate::secrets::is_current_secret_ref(pw) {
                if let Some(real) =
                    crate::secrets::get_secret(&crate::secrets::proxy_account(&proxy.id))
                {
                    proxy.password = Some(real);
                } else if unlocked {
                    proxy.password = None;
                }
            }
        }
    }
}

/// Encrypt real credentials into the app-managed store before persisting (K1),
/// replacing each with the [`crate::secrets::SECRET_REF`] marker on disk. An
/// existing marker is left untouched. Legacy password/proxy behavior remains
/// compatible when the vault is locked, but private-key passphrases fail closed
/// so they are never newly written to `hosts.json` in plaintext.
fn externalize_secrets(config: &mut AppConfig) -> Result<()> {
    use crate::secrets::{
        host_account, host_passphrase_account, is_secret_ref, proxy_account, store_secret,
        SECRET_REF,
    };

    for host in &mut config.hosts {
        if let Some(pw) = &host.password {
            if pw.is_empty() || is_secret_ref(pw) {
                continue;
            }
            match store_secret(&host_account(&host.name), pw) {
                Ok(()) => host.password = Some(SECRET_REF.to_string()),
                Err(e) => eprintln!(
                    "warning: could not encrypt password for host '{}' ({e}); left as plaintext until the credential store is unlocked",
                    host.name
                ),
            }
        }
        if let Some(passphrase) = &host.passphrase {
            if passphrase.is_empty() || is_secret_ref(passphrase) {
                continue;
            }
            match store_secret(&host_passphrase_account(&host.name), passphrase) {
                Ok(()) => host.passphrase = Some(SECRET_REF.to_string()),
                Err(e) => {
                    return Err(e).with_context(|| {
                        format!(
                            "refusing to persist an unencrypted key passphrase for host '{}'",
                            host.name
                        )
                    })
                }
            }
        }
    }
    for proxy in &mut config.proxies {
        if let Some(pw) = &proxy.password {
            if pw.is_empty() || is_secret_ref(pw) {
                continue;
            }
            match store_secret(&proxy_account(&proxy.id), pw) {
                Ok(()) => proxy.password = Some(SECRET_REF.to_string()),
                Err(e) => eprintln!(
                    "warning: could not encrypt password for proxy '{}' ({e}); left as plaintext until the credential store is unlocked",
                    proxy.id
                ),
            }
        }
    }
    Ok(())
}

/// Encrypt any plaintext passwords still living in `hosts.json` into the
/// app-managed store (K1 migration). Safe to call repeatedly and at startup. Only
/// runs when the store is unlocked (directly or via `AGENT2SSH_MASTER_PASSWORD`);
/// when locked it is a no-op so plaintext is left intact until a master password
/// is available. Returns the number of hosts/proxies migrated this call.
pub fn migrate_plaintext_secrets() -> Result<usize> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(0);
    }
    // Count plaintext secrets present on disk *before* migration.
    let raw = fs::read_to_string(&path).context("failed to read hosts config")?;
    let on_disk: AppConfig = serde_json::from_str(&raw).context("failed to parse hosts config")?;
    let plaintext_count = on_disk
        .hosts
        .iter()
        .filter_map(|h| h.password.as_deref())
        .filter(|pw| !pw.is_empty() && !crate::secrets::is_secret_ref(pw))
        .count()
        + on_disk
            .hosts
            .iter()
            .filter_map(|h| h.passphrase.as_deref())
            .filter(|value| !value.is_empty() && !crate::secrets::is_secret_ref(value))
            .count()
        + on_disk
            .proxies
            .iter()
            .filter_map(|p| p.password.as_deref())
            .filter(|pw| !pw.is_empty() && !crate::secrets::is_secret_ref(pw))
            .count();

    if plaintext_count == 0 {
        return Ok(0);
    }
    // Need the store unlocked to encrypt; otherwise leave plaintext for now.
    if !crate::secrets::is_unlocked() && !crate::secrets::try_unlock_from_env() {
        return Ok(0);
    }

    let config = load_config()?;
    save_config(&config)?;
    Ok(plaintext_count)
}

pub fn save_config(config: &AppConfig) -> Result<()> {
    let _guard = store_write_lock()?;
    save_config_unlocked(config)
}

pub(crate) fn save_config_unlocked(config: &AppConfig) -> Result<()> {
    ensure_config_dir()?;
    let mut normalized = normalize_config(config.clone());
    // K1: encrypt real passwords into the app-managed store, leaving only a
    // reference marker on disk. Operates on the clone, so the caller's `config`
    // (and any value it shares with the in-memory cache) keeps the real secrets.
    externalize_secrets(&mut normalized)?;
    let raw = serde_json::to_string_pretty(&normalized)?;
    let path = config_path()?;
    let tmp_path = path.with_extension(format!("json.tmp.{}", std::process::id()));
    let write_result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp_path)
            .with_context(|| format!("failed to open temp config {}", tmp_path.display()))?;
        file.write_all(raw.as_bytes())
            .with_context(|| format!("failed to write temp config {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync temp config {}", tmp_path.display()))?;
        restrict_file_to_owner(&tmp_path)?;
        // Write-before backup: snapshot the last-good file so a bad write (or a
        // hand-edit gone wrong) can be rolled back from `hosts.json.bak`. The
        // rename below is already atomic, so this protects against bad *content*,
        // not torn writes. Backup failures are non-fatal — we don't block a save
        // because the previous file couldn't be copied.
        if path.exists() {
            let backup = path.with_extension("json.bak");
            if let Err(e) = fs::copy(&path, &backup) {
                eprintln!(
                    "warning: failed to back up {} before save: {e}",
                    path.display()
                );
            } else {
                let _ = restrict_file_to_owner(&backup);
            }
        }
        fs::rename(&tmp_path, &path)
            .with_context(|| format!("failed to replace hosts config {}", path.display()))?;
        restrict_file_to_owner(&path)?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    } else {
        // Drop the cached value so this process reads the new hosts immediately,
        // independent of filesystem mtime granularity. (I5)
        HOSTS_CACHE.invalidate();
    }
    write_result
}

fn normalize_config(mut config: AppConfig) -> AppConfig {
    // Stamp the schema version. Never downgrade: if a newer build wrote a higher
    // version, preserve it (forward-compat) rather than claiming this build's
    // older schema.
    config.schema_version = config.schema_version.max(CONFIG_SCHEMA_VERSION);
    if config.groups.is_empty() {
        config.groups = default_host_groups();
    }
    if !config
        .groups
        .iter()
        .any(|group| group.id == default_host_group())
    {
        config.groups.insert(0, default_host_groups().remove(0));
    }
    for group in &mut config.groups {
        group.id = group.id.trim().to_string();
        group.name = group.name.trim().to_string();
        if group.id.is_empty() {
            group.id = default_host_group();
        }
        if group.name.is_empty() {
            group.name = group.id.clone();
        }
    }
    config.groups.sort_by(|a, b| {
        if a.id == default_host_group() {
            std::cmp::Ordering::Less
        } else if b.id == default_host_group() {
            std::cmp::Ordering::Greater
        } else {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        }
    });

    let default_group = default_host_group();
    let valid_groups: std::collections::HashSet<String> =
        config.groups.iter().map(|group| group.id.clone()).collect();
    for proxy in &mut config.proxies {
        proxy.id = proxy.id.trim().to_string();
        proxy.name = proxy.name.trim().to_string();
        proxy.host = proxy.host.trim().to_string();
        proxy.username = proxy
            .username
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        proxy.password = proxy
            .password
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
    }
    config.proxies.retain(|proxy| {
        !proxy.id.is_empty() && !proxy.name.is_empty() && !proxy.host.is_empty() && proxy.port > 0
    });
    config
        .proxies
        .sort_by_key(|proxy| proxy.name.to_lowercase());
    let valid_proxies: std::collections::HashSet<String> = config
        .proxies
        .iter()
        .map(|proxy| proxy.id.clone())
        .collect();
    for host in &mut config.hosts {
        host.group = host.group.trim().to_string();
        if host.group.is_empty() || !valid_groups.contains(&host.group) {
            host.group = default_group.clone();
        }
        host.proxy_id = host
            .proxy_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty() && valid_proxies.contains(*value))
            .map(ToOwned::to_owned);
    }

    config
}

pub fn append_audit(
    result: &ExecResult,
    risk_level: RiskLevel,
    reason: Option<&str>,
    change_id: Option<&str>,
    source: Option<&str>,
) -> Result<()> {
    use chrono::Utc;
    use uuid::Uuid;

    ensure_config_dir()?;
    let _guard = audit_write_lock()?;
    rotate_audit_if_needed_unlocked(10 * 1024 * 1024)?; // 10 MB default
    // Finding 16: Derive action and outcome from the ExecResult.
    let action = derive_audit_action(&result.command);
    let outcome = derive_audit_outcome(result.exit_code, risk_level);
    let entry = AuditEntry {
        id: Uuid::new_v4(),
        ts: Utc::now(),
        host: result.host.clone(),
        command: redact_sensitive_text(&result.command),
        exit_code: result.exit_code,
        duration_ms: result.duration_ms,
        risk_level,
        reason: reason.map(str::to_string),
        change_id: change_id.map(str::to_string),
        side_effect: result.side_effect.clone(),
        source: source.map(str::to_string),
        action,
        outcome,
    };
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(audit_path()?)
        .context("failed to open audit log")?;
    writeln!(file, "{}", serde_json::to_string(&entry)?)?;
    detect_and_publish_audit_anomalies(&entry);
    Ok(())
}

/// Finding 16: Derive the action category from the command string.
fn derive_audit_action(command: &str) -> Option<String> {
    let lower = command.to_lowercase();
    if lower.starts_with("sftp upload") {
        Some("sftp_upload".into())
    } else if lower.starts_with("sftp download") {
        Some("sftp_download".into())
    } else if lower.starts_with("sftp mkdir") {
        Some("sftp_mkdir".into())
    } else if lower.starts_with("sftp rename") {
        Some("sftp_rename".into())
    } else if lower.starts_with("sftp rm") || lower.starts_with("sftp rmdir") || lower.starts_with("sftp rm-rf") {
        Some("sftp_remove".into())
    } else if lower.starts_with("sftp ls") {
        Some("sftp_list".into())
    } else if lower.starts_with("sftp stat") {
        Some("sftp_stat".into())
    } else if lower.starts_with("sftp read") {
        Some("sftp_read".into())
    } else if lower.starts_with("sftp walk") {
        Some("sftp_walk".into())
    } else if lower.starts_with("forward add") || lower.starts_with("forward add-multi") {
        Some("forward_add".into())
    } else if lower.starts_with("forward remove") || lower.starts_with("forward del") {
        Some("forward_remove".into())
    } else if lower.starts_with("forward stop") {
        Some("forward_stop".into())
    } else if lower.starts_with("forward start") {
        Some("forward_start".into())
    } else if lower.starts_with("forward list") {
        Some("forward_list".into())
    } else if lower.starts_with("config update") || lower.starts_with("host add") || lower.starts_with("host update") || lower.starts_with("host remove") || lower.starts_with("host delete") {
        Some("config_update".into())
    } else {
        Some("exec".into())
    }
}

/// Finding 16: Derive the outcome from exit_code and risk_level.
fn derive_audit_outcome(exit_code: Option<i32>, risk_level: RiskLevel) -> Option<String> {
    if risk_level == RiskLevel::Blocked {
        Some("blocked".into())
    } else {
        match exit_code {
            Some(0) => Some("success".into()),
            Some(_) => Some("error".into()),
            None => Some("error".into()),
        }
    }
}

fn detect_and_publish_audit_anomalies(entry: &AuditEntry) {
    let Ok(config) = crate::anomaly::load_anomaly_config() else {
        return;
    };
    if !config.enabled {
        return;
    }
    let filter = AuditFilter {
        since: Some((entry.ts - chrono::Duration::seconds(config.window_secs.max(1))).to_rfc3339()),
        until: Some(entry.ts.to_rfc3339()),
        limit: 1000,
        ..Default::default()
    };
    let Ok(entries) = list_audit_raw(&filter) else {
        return;
    };
    let findings = crate::anomaly::detect_anomalies(&entries, entry, &config);
    crate::anomaly::publish_anomalies(&findings);
}

/// Rotate audit log if it exceeds `max_size_bytes`.
///
/// Keeps a bounded number of rotated files (`audit.jsonl.1...`), so high-volume
/// operations retain more history before dropping the oldest entry.
pub fn rotate_audit_if_needed(max_size_bytes: u64) -> Result<()> {
    let _guard = audit_write_lock()?;
    rotate_audit_if_needed_unlocked(max_size_bytes)
}

fn rotate_audit_if_needed_unlocked(max_size_bytes: u64) -> Result<()> {
    const AUDIT_ROTATION_COUNT: usize = 10;
    let path = audit_path()?;
    if !path.exists() {
        return Ok(());
    }
    let metadata = std::fs::metadata(&path)?;
    if metadata.len() <= max_size_bytes {
        return Ok(());
    }

    // Shift existing rotations: `.n-1` -> `.n`, dropping the oldest if needed.
    for i in (2..=AUDIT_ROTATION_COUNT).rev() {
        let src = path.with_extension(format!("jsonl.{}", i - 1));
        let dst = path.with_extension(format!("jsonl.{i}"));
        if src.exists() {
            if dst.exists() {
                std::fs::remove_file(&dst)?;
            }
            std::fs::rename(&src, &dst)?;
        }
    }
    // Remove the overflow file when it would exceed the configured rotation count.
    let overflow = path.with_extension(format!("jsonl.{}", AUDIT_ROTATION_COUNT + 1));
    if overflow.exists() {
        let _ = std::fs::remove_file(&overflow);
    }

    // Current → .1
    let rotated = path.with_extension("jsonl.1");
    if rotated.exists() {
        std::fs::remove_file(&rotated)?;
    }
    std::fs::rename(&path, &rotated)?;

    // Publish audit rotated event
    crate::events::publish_event(
        crate::events::EventType::AuditRotated,
        serde_json::json!({"file": rotated.display().to_string()}),
    );

    Ok(())
}

/// Public wrapper for CLI/daemon invocation of audit rotation with the default 10 MB limit.
pub fn rotate_audit_core() -> Result<()> {
    rotate_audit_if_needed(10 * 1024 * 1024)
}

// ── Metrics Trends (F6-3) ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsTrend {
    pub period: TrendPeriod,
    pub total_executions: usize,
    pub success_count: usize,
    pub failure_count: usize,
    pub blocked_count: usize,
    pub failure_rate: f64,
    pub risk_distribution: RiskDistribution,
    pub avg_duration_ms: f64,
    pub top_hosts: Vec<HostExecutionCount>,
    pub hourly_breakdown: Vec<HourlyBucket>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TrendPeriod {
    Last24h,
    Last7d,
    Last30d,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskDistribution {
    pub low: usize,
    pub medium: usize,
    pub high: usize,
    pub blocked: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostExecutionCount {
    pub host: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyBucket {
    pub hour: String, // ISO-8601 truncated to hour
    pub count: usize,
    pub failures: usize,
}

/// Compute metrics trends from audit data.
pub fn compute_metrics_trend(period: TrendPeriod) -> Result<MetricsTrend> {
    let now = chrono::Utc::now();
    let since = match period {
        TrendPeriod::Last24h => Some((now - chrono::Duration::hours(24)).to_rfc3339()),
        TrendPeriod::Last7d => Some((now - chrono::Duration::days(7)).to_rfc3339()),
        TrendPeriod::Last30d => Some((now - chrono::Duration::days(30)).to_rfc3339()),
        TrendPeriod::All => None,
    };

    let filter = crate::types::AuditFilter {
        host: None,
        risk_level: None,
        exit_code: None,
        since,
        until: None,
        limit: usize::MAX,
        search: None,
        command_pattern: None,
        host_env: None,
        host_role: None,
        host_owner: None,
    };

    let entries = list_audit_raw(&filter)?;

    let total_executions = entries.len();
    let failure_count = entries
        .iter()
        .filter(|e| e.exit_code.map(|c| c != 0).unwrap_or(true))
        .count();
    let success_count = total_executions.saturating_sub(failure_count);
    let blocked_count = entries
        .iter()
        .filter(|e| e.risk_level == RiskLevel::Blocked)
        .count();

    let failure_rate = if total_executions > 0 {
        failure_count as f64 / total_executions as f64
    } else {
        0.0
    };

    let risk_distribution = RiskDistribution {
        low: entries
            .iter()
            .filter(|e| e.risk_level == RiskLevel::Low)
            .count(),
        medium: entries
            .iter()
            .filter(|e| e.risk_level == RiskLevel::Medium)
            .count(),
        high: entries
            .iter()
            .filter(|e| e.risk_level == RiskLevel::High)
            .count(),
        blocked: entries
            .iter()
            .filter(|e| e.risk_level == RiskLevel::Blocked)
            .count(),
    };

    let avg_duration_ms = if total_executions > 0 {
        entries.iter().map(|e| e.duration_ms as f64).sum::<f64>() / total_executions as f64
    } else {
        0.0
    };

    // Top 10 hosts by execution count
    let mut host_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for e in &entries {
        *host_counts.entry(e.host.clone()).or_insert(0) += 1;
    }
    let mut top_hosts: Vec<HostExecutionCount> = host_counts
        .into_iter()
        .map(|(host, count)| HostExecutionCount { host, count })
        .collect();
    top_hosts.sort_by_key(|host| std::cmp::Reverse(host.count));
    top_hosts.truncate(10);

    // Hourly breakdown
    let mut hourly: std::collections::BTreeMap<String, (usize, usize)> =
        std::collections::BTreeMap::new();
    for e in &entries {
        let hour_key = e.ts.format("%Y-%m-%dT%H:00:00Z").to_string();
        let entry = hourly.entry(hour_key).or_insert((0, 0));
        entry.0 += 1;
        if e.exit_code.map(|c| c != 0).unwrap_or(true) {
            entry.1 += 1;
        }
    }
    let hourly_breakdown: Vec<HourlyBucket> = hourly
        .into_iter()
        .map(|(hour, (count, failures))| HourlyBucket {
            hour,
            count,
            failures,
        })
        .collect();

    Ok(MetricsTrend {
        period,
        total_executions,
        success_count,
        failure_count,
        blocked_count,
        failure_rate,
        risk_distribution,
        avg_duration_ms,
        top_hosts,
        hourly_breakdown,
    })
}

pub fn redact_sensitive_text(input: &str) -> String {
    // B1: Idempotency — if the text has already been redacted (contains
    // redaction markers like `<REDACTED:...>` or `[REDACTED]`), return it
    // as-is. This prevents double-redaction from corrupting structured
    // payloads where a redaction marker's content might match a rule
    // (e.g., a hex hash inside `<REDACTED:hex>` would match the hex rule
    // again on a second pass).
    if crate::redaction::is_pre_redacted(input) {
        return input.to_string();
    }

    // First pass: regex-based default rules (IP, API keys, JWT, hex blobs).
    let regex_redacted = crate::redaction::redact_default(input);

    // Second pass: existing token-based heuristics (keyword=value, bearer,
    // private keys, high-entropy strings).
    let upper = regex_redacted.to_ascii_uppercase();
    if upper.contains("BEGIN ") && upper.contains("PRIVATE KEY") {
        return "[REDACTED PRIVATE KEY]".to_string();
    }

    let mut out = Vec::new();
    let mut redact_next = false;
    for token in regex_redacted.split_whitespace() {
        let lower = token.to_ascii_lowercase();
        if redact_next {
            if matches!(lower.as_str(), "bearer" | "basic") {
                redact_next = true;
                continue;
            }
            out.push("[REDACTED]".to_string());
            redact_next = false;
            continue;
        }
        if let Some((key, _)) = token.split_once('=') {
            let key_lower = key.to_ascii_lowercase();
            if is_sensitive_key(&key_lower) {
                out.push(format!("{key}=[REDACTED]"));
                continue;
            }
        }
        if is_sensitive_key(&lower) {
            out.push(token.to_string());
            redact_next = true;
            continue;
        }
        out.push(redact_token_fallback(token));
    }
    out.join(" ")
}

fn redact_token_fallback(token: &str) -> String {
    let url_redacted = redact_url_userinfo(token);
    if looks_like_high_entropy_secret(&url_redacted) {
        "[REDACTED]".to_string()
    } else {
        url_redacted
    }
}

fn redact_url_userinfo(token: &str) -> String {
    let Some(scheme_end) = token.find("://") else {
        return token.to_string();
    };
    let authority_start = scheme_end + 3;
    let authority_end = token[authority_start..]
        .find(['/', '?', '#'])
        .map(|idx| authority_start + idx)
        .unwrap_or(token.len());
    let authority = &token[authority_start..authority_end];
    let Some(at_idx) = authority.rfind('@') else {
        return token.to_string();
    };
    let host_start = authority_start + at_idx + 1;
    format!(
        "{}[REDACTED]@{}",
        &token[..authority_start],
        &token[host_start..]
    )
}

fn looks_like_high_entropy_secret(token: &str) -> bool {
    let trimmed = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | '`' | ',' | ';' | ')' | '(' | '[' | ']' | '{' | '}' | '<' | '>'
        )
    });
    let value = trimmed
        .strip_prefix("sha256:")
        .or_else(|| trimmed.strip_prefix("SHA256:"))
        .unwrap_or(trimmed);
    if value.len() >= 32 && value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return true;
    }
    if value.len() < 40 {
        return false;
    }
    let mut has_alpha = false;
    let mut has_digit = false;
    let mut has_secret_alphabet = false;
    let mut valid = true;
    for ch in value.chars() {
        if ch.is_ascii_alphabetic() {
            has_alpha = true;
        } else if ch.is_ascii_digit() {
            has_digit = true;
        } else if matches!(ch, '+' | '/' | '_' | '-' | '=') {
            has_secret_alphabet = true;
        } else {
            valid = false;
            break;
        }
    }
    valid && has_alpha && has_digit && has_secret_alphabet
}

fn is_sensitive_key(key: &str) -> bool {
    matches!(
        key.trim_start_matches('-').trim_end_matches(':'),
        "password"
            | "passwd"
            | "token"
            | "secret"
            | "api-key"
            | "apikey"
            | "access-token"
            | "authorization"
            | "bearer"
            | "cookie"
            | "set-cookie"
    )
}

pub fn list_audit_raw(filter: &AuditFilter) -> Result<Vec<AuditEntry>> {
    ensure_config_dir()?;
    let path = audit_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    if filter.limit == 0 {
        return Ok(Vec::new());
    }

    let since = filter
        .since
        .as_deref()
        .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());
    let until = filter
        .until
        .as_deref()
        .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());

    // Build a lookup map from host name to labels for host group filtering.
    let host_label_map: Option<HostLabelMap> =
        if filter.host_env.is_some() || filter.host_role.is_some() || filter.host_owner.is_some() {
            let config = load_config().unwrap_or_default();
            let mut map = std::collections::HashMap::new();
            for h in &config.hosts {
                map.insert(
                    h.name.clone(),
                    (h.env.clone(), h.role.clone(), h.owner.clone()),
                );
            }
            Some(map)
        } else {
            None
        };

    // Compute the set of host names matching the host group filters.
    let matching_hosts: Option<std::collections::HashSet<String>> = host_label_map.map(|map| {
        map.into_iter()
            .filter(|(_, (env, role, owner))| {
                let env_ok = match &filter.host_env {
                    Some(v) => env
                        .as_deref()
                        .map(|e| e.eq_ignore_ascii_case(v))
                        .unwrap_or(false),
                    None => true,
                };
                let role_ok = match &filter.host_role {
                    Some(v) => role
                        .as_deref()
                        .map(|r| r.eq_ignore_ascii_case(v))
                        .unwrap_or(false),
                    None => true,
                };
                let owner_ok = match &filter.host_owner {
                    Some(v) => owner
                        .as_deref()
                        .map(|o| o.eq_ignore_ascii_case(v))
                        .unwrap_or(false),
                    None => true,
                };
                env_ok && role_ok && owner_ok
            })
            .map(|(name, _)| name)
            .collect()
    });

    let search_lower = filter.search.as_deref().map(|s| s.to_lowercase());

    let matches = |e: &AuditEntry| -> bool {
        if let Some(h) = &filter.host {
            if !e.host.eq_ignore_ascii_case(h) {
                return false;
            }
        }
        if let Some(r) = filter.risk_level {
            if e.risk_level != r {
                return false;
            }
        }
        if let Some(code) = filter.exit_code {
            if e.exit_code != Some(code) {
                return false;
            }
        }
        if let Some(since) = since {
            if e.ts < since {
                return false;
            }
        }
        if let Some(until) = until {
            if e.ts > until {
                return false;
            }
        }
        // F6-1: full-text search (case-insensitive substring on command and host)
        if let Some(ref needle) = search_lower {
            if !e.command.to_lowercase().contains(needle)
                && !e.host.to_lowercase().contains(needle)
                && !e
                    .source
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(needle)
            {
                return false;
            }
        }
        // F6-1: command pattern (glob-style match)
        if let Some(ref pattern) = filter.command_pattern {
            if !glob_match(pattern, &e.command) {
                return false;
            }
        }
        // F6-1: host group filtering
        if let Some(ref hosts_set) = matching_hosts {
            if !hosts_set.contains(&e.host) {
                return false;
            }
        }
        true
    };

    // J2: scan newest-first and stop early. The result is the newest `limit`
    // matching entries (newest-first), identical to the previous
    // collect-all → reverse → truncate, but without parsing the whole file in
    // the common "recent N" case.
    let mut entries: Vec<AuditEntry> = Vec::new();
    visit_lines_reverse(&path, |line| {
        if entries.len() >= filter.limit {
            return Ok(false);
        }
        let Ok(entry) = serde_json::from_str::<AuditEntry>(line) else {
            return Ok(true);
        };
        // Entries are appended with `ts = Utc::now()`, so file order is
        // non-decreasing in time. Reading backward, once we pass `since` every
        // earlier line is older too — bounding metrics/trend (since-window)
        // scans without parsing ancient history. `matches` still rechecks the
        // bound, so this only ever stops work earlier, never changes the result.
        if let Some(since) = since {
            if entry.ts < since {
                return Ok(false);
            }
        }
        if matches(&entry) {
            entries.push(entry);
        }
        Ok(true)
    })?;
    Ok(entries)
}

fn visit_lines_reverse(path: &Path, mut visit: impl FnMut(&str) -> Result<bool>) -> Result<()> {
    const CHUNK_SIZE: u64 = 64 * 1024;

    let mut file =
        File::open(path).with_context(|| format!("failed to open audit log {}", path.display()))?;
    let mut pos = file
        .seek(SeekFrom::End(0))
        .with_context(|| format!("failed to seek audit log {}", path.display()))?;
    let mut carry: Vec<u8> = Vec::new();

    while pos > 0 {
        let read_len = CHUNK_SIZE.min(pos) as usize;
        pos -= read_len as u64;
        file.seek(SeekFrom::Start(pos))
            .with_context(|| format!("failed to seek audit log {}", path.display()))?;
        let mut buf = vec![0; read_len];
        file.read_exact(&mut buf)
            .with_context(|| format!("failed to read audit log {}", path.display()))?;
        buf.extend_from_slice(&carry);

        let mut end = buf.len();
        while let Some(idx) = buf[..end].iter().rposition(|byte| *byte == b'\n') {
            let line = &buf[idx + 1..end];
            if !line.is_empty() {
                let line = std::str::from_utf8(line)
                    .with_context(|| format!("audit log is not valid UTF-8: {}", path.display()))?;
                if !visit(line)? {
                    return Ok(());
                }
            }
            end = idx;
        }
        carry = buf[..end].to_vec();
    }

    if !carry.is_empty() {
        let line = std::str::from_utf8(&carry)
            .with_context(|| format!("audit log is not valid UTF-8: {}", path.display()))?;
        let _ = visit(line)?;
    }

    Ok(())
}

/// Simple glob-style pattern matching supporting `*` (any sequence) and `?` (any single char).
/// Case-insensitive.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.to_lowercase().chars().collect();
    let txt: Vec<char> = text.to_lowercase().chars().collect();
    let mut prev = vec![false; txt.len() + 1];
    prev[0] = true;

    for pattern_char in pat {
        let mut next = vec![false; txt.len() + 1];
        if pattern_char == '*' {
            next[0] = prev[0];
            for i in 1..=txt.len() {
                next[i] = prev[i] || next[i - 1];
            }
        } else {
            for i in 1..=txt.len() {
                next[i] = prev[i - 1] && (pattern_char == '?' || pattern_char == txt[i - 1]);
            }
        }
        prev = next;
    }
    prev[txt.len()]
}

// ── Audit Export (F6-2) ─────────────────────────────────────────────────────

/// Export audit entries as JSONL (one JSON object per line).
/// Redaction is already applied at write time, so entries are emitted as-is.
pub fn export_audit_jsonl(filter: &AuditFilter) -> Result<String> {
    let entries = list_audit_raw(filter)?;
    let mut output = String::new();
    for entry in &entries {
        output.push_str(&serde_json::to_string(entry)?);
        output.push('\n');
    }
    Ok(output)
}

/// Export audit entries as CSV with headers.
/// Fields: id, timestamp, host, command, exit_code, duration_ms, risk_level, reason, change_id, source
/// Fields containing commas, quotes, or newlines are properly quoted/escaped per RFC 4180.
pub fn export_audit_csv(filter: &AuditFilter) -> Result<String> {
    let entries = list_audit_raw(filter)?;
    let mut output = String::new();
    // Header row
    output.push_str(
        "id,timestamp,host,command,exit_code,duration_ms,risk_level,reason,change_id,source\n",
    );
    for entry in &entries {
        output.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{}\n",
            entry.id,
            entry.ts.to_rfc3339(),
            csv_escape(&entry.host),
            csv_escape(&entry.command),
            entry.exit_code.map(|c| c.to_string()).unwrap_or_default(),
            entry.duration_ms,
            entry.risk_level,
            csv_escape(entry.reason.as_deref().unwrap_or("")),
            csv_escape(entry.change_id.as_deref().unwrap_or("")),
            csv_escape(entry.source.as_deref().unwrap_or("")),
        ));
    }
    Ok(output)
}

/// Escape a field value for CSV output per RFC 4180.
/// If the value contains a comma, double-quote, or newline, wrap it in
/// double-quotes and double any internal quotes.
fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_dir_uses_env_override() {
        let expected =
            std::env::temp_dir().join(format!("agent2ssh-config-{}", uuid::Uuid::new_v4()));

        assert_eq!(
            config_dir_override(Some(expected.display().to_string())).unwrap(),
            expected
        );
        assert!(config_dir_override(Some("   ".to_string())).is_none());
        assert!(config_dir_override(None).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_restrict_file_to_owner_sets_0600() {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::temp_dir().join(format!("agent2ssh-perms-{}", uuid::Uuid::new_v4()));
        fs::write(&path, "secret").unwrap();

        restrict_file_to_owner(&path).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let _ = fs::remove_file(&path);
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn migrate_config_stamps_legacy_version() {
        // A legacy file has no `schema_version`, so serde defaults it to 0.
        let legacy: AppConfig =
            serde_json::from_str(r#"{"hosts":[],"proxies":[],"groups":[]}"#).unwrap();
        assert_eq!(legacy.schema_version, 0);

        let migrated = migrate_config(legacy);
        assert_eq!(migrated.schema_version, CONFIG_SCHEMA_VERSION);
    }

    #[test]
    fn migrate_config_is_idempotent_and_does_not_downgrade() {
        // Already-current config is unchanged.
        let mut current = AppConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            ..Default::default()
        };
        current = migrate_config(current);
        assert_eq!(current.schema_version, CONFIG_SCHEMA_VERSION);

        // A file written by a hypothetical newer build is never downgraded.
        let future = AppConfig {
            schema_version: CONFIG_SCHEMA_VERSION + 5,
            ..Default::default()
        };
        let preserved = migrate_config(future);
        assert_eq!(preserved.schema_version, CONFIG_SCHEMA_VERSION + 5);
    }

    #[test]
    fn normalize_stamps_current_version_and_preserves_future() {
        // normalize bumps a legacy/zero version up to current...
        let stamped = normalize_config(AppConfig::default());
        assert_eq!(stamped.schema_version, CONFIG_SCHEMA_VERSION);

        // ...but never below an existing higher version.
        let future = AppConfig {
            schema_version: CONFIG_SCHEMA_VERSION + 2,
            ..Default::default()
        };
        let normalized = normalize_config(future);
        assert_eq!(normalized.schema_version, CONFIG_SCHEMA_VERSION + 2);
    }

    #[test]
    #[serial_test::serial]
    fn save_config_writes_backup_of_previous_file() {
        // Isolate the config dir for this test process.
        let dir = std::env::temp_dir().join(format!("agent2ssh-bak-{}", uuid::Uuid::new_v4()));
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);

        let first = AppConfig {
            hosts: vec![crate::types::HostProfile {
                name: "alpha".into(),
                host: "alpha.example".into(),
                user: None,
                port: None,
                key_path: None,
                password: None,
                jump_host: None,
                proxy_id: None,
                risk_override: None,
                tags: vec![],
                group: default_host_group(),
                env: None,
                role: None,
                owner: None,
                init_command: None,
                passphrase: None,
            }],
            ..Default::default()
        };
        save_config(&first).unwrap();
        assert!(
            !dir.join("hosts.json.bak").exists(),
            "no backup on first write"
        );

        // Second save snapshots the first file into hosts.json.bak.
        let mut second = first.clone();
        second.hosts[0].host = "beta.example".into();
        save_config(&second).unwrap();

        let backup = dir.join("hosts.json.bak");
        assert!(backup.exists(), "backup created on overwrite");
        let backup_raw = fs::read_to_string(&backup).unwrap();
        assert!(
            backup_raw.contains("alpha.example"),
            "backup holds the previous version, got: {backup_raw}"
        );

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn passwords_persist_as_marker_not_plaintext() {
        // cfg(test) routes secrets to the in-memory backend.
        let dir = std::env::temp_dir().join(format!("agent2ssh-kc-{}", uuid::Uuid::new_v4()));
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);

        let host = crate::types::HostProfile {
            name: "kc-host".into(),
            host: "10.0.0.9".into(),
            user: Some("root".into()),
            port: Some(22),
            key_path: None,
            password: Some("super-secret".into()),
            jump_host: None,
            proxy_id: None,
            risk_override: None,
            tags: vec![],
            group: default_host_group(),
            env: None,
            role: None,
            owner: None,
            init_command: None,
            passphrase: Some("key-passphrase".into()),
        };
        save_config(&AppConfig {
            hosts: vec![host],
            ..Default::default()
        })
        .unwrap();

        // On disk: the marker, never the plaintext.
        let raw = fs::read_to_string(dir.join("hosts.json")).unwrap();
        assert!(
            !raw.contains("super-secret"),
            "plaintext password must not be on disk: {raw}"
        );
        assert!(
            !raw.contains("key-passphrase"),
            "plaintext key passphrase must not be on disk: {raw}"
        );
        assert!(
            raw.contains(crate::secrets::SECRET_REF),
            "on-disk password must be the reference marker: {raw}"
        );

        // On load: the real password is resolved back from the store.
        let loaded = load_config().unwrap();
        assert_eq!(loaded.hosts[0].password.as_deref(), Some("super-secret"));
        assert_eq!(
            loaded.hosts[0].passphrase.as_deref(),
            Some("key-passphrase")
        );

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn migrate_secrets_moves_legacy_plaintext() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-kc-mig-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);

        // Hand-write a legacy config with a plaintext password (as pre-K1 builds did).
        let legacy = r#"{
            "schema_version": 0,
            "groups": [],
            "proxies": [],
            "hosts": [{"name":"legacy","host":"1.2.3.4","password":"plain-pw","group":"default","tags":[]}]
        }"#;
        fs::write(dir.join("hosts.json"), legacy).unwrap();

        let migrated = migrate_plaintext_secrets().unwrap();
        assert_eq!(migrated, 1);

        let raw = fs::read_to_string(dir.join("hosts.json")).unwrap();
        assert!(
            !raw.contains("plain-pw"),
            "plaintext gone after migration: {raw}"
        );
        assert!(raw.contains(crate::secrets::SECRET_REF));

        // Idempotent: a second run finds nothing to migrate.
        assert_eq!(migrate_plaintext_secrets().unwrap(), 0);

        // The secret is still resolvable.
        let loaded = load_config().unwrap();
        assert_eq!(loaded.hosts[0].password.as_deref(), Some("plain-pw"));

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn legacy_keyring_marker_is_preserved_and_not_migrated_as_plaintext() {
        let dir =
            std::env::temp_dir().join(format!("agent2ssh-legacy-kc-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);

        let legacy = format!(
            r#"{{
            "schema_version": 0,
            "groups": [],
            "proxies": [],
            "hosts": [{{"name":"legacy","host":"1.2.3.4","password":"{}","group":"default","tags":[]}}]
        }}"#,
            crate::secrets::LEGACY_KEYRING_REF
        );
        fs::write(dir.join("hosts.json"), legacy).unwrap();

        assert_eq!(migrate_plaintext_secrets().unwrap(), 0);
        let loaded = load_config().unwrap();
        assert_eq!(
            loaded.hosts[0].password.as_deref(),
            Some(crate::secrets::LEGACY_KEYRING_REF)
        );

        save_config(&loaded).unwrap();
        let raw = fs::read_to_string(dir.join("hosts.json")).unwrap();
        assert!(raw.contains(crate::secrets::LEGACY_KEYRING_REF));
        assert!(!raw.contains(crate::secrets::SECRET_REF));

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_redact_sensitive_text() {
        let redacted = redact_sensitive_text(
            "deploy --token abc password=hunter2 --api-key key123 --safe value",
        );
        assert_eq!(
            redacted,
            "deploy --token [REDACTED] password=[REDACTED] --api-key [REDACTED] --safe value"
        );

        let auth = redact_sensitive_text("Authorization: Bearer abc123\ncookie=session-id");
        assert_eq!(auth, "Authorization: [REDACTED] cookie=[REDACTED]");

        let private_key = redact_sensitive_text(
            "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----",
        );
        assert_eq!(private_key, "[REDACTED PRIVATE KEY]");

        let generic = redact_sensitive_text(
            "curl https://user:pass@example.com/hook key 0123456789abcdef0123456789abcdef eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9_1234567890",
        );
        assert_eq!(
            generic,
            "curl https://[REDACTED]@example.com/hook key <REDACTED:hex> [REDACTED]"
        );

        let safe = redact_sensitive_text(
            "session 550e8400-e29b-41d4-a716-446655440000 /tmp/agent2ssh-run output.txt",
        );
        assert_eq!(
            safe,
            "session 550e8400-e29b-41d4-a716-446655440000 /tmp/agent2ssh-run output.txt"
        );
    }

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("hello", "hello"));
        assert!(!glob_match("hello", "world"));
    }

    #[test]
    fn test_glob_match_star() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("sudo *", "sudo rm -rf /"));
        assert!(glob_match("*.sh", "deploy.sh"));
        assert!(!glob_match("*.sh", "deploy.py"));
        assert!(glob_match(
            "kubectl delete *",
            "kubectl delete namespace default"
        ));
    }

    #[test]
    fn test_glob_match_question() {
        assert!(glob_match("h?llo", "hello"));
        assert!(glob_match("h?llo", "hallo"));
        assert!(!glob_match("h?llo", "heello"));
    }

    #[test]
    fn test_glob_match_case_insensitive() {
        assert!(glob_match("SUDO *", "sudo whoami"));
        assert!(glob_match("sudo *", "SUDO REBOOT"));
    }

    #[test]
    fn test_glob_match_many_stars_is_linear_safe() {
        let pattern = format!("{}z", "*a".repeat(128));
        assert!(!glob_match(&pattern, &"a".repeat(128)));
    }

    #[test]
    fn test_audit_filter_search() {
        // Test the search/glob logic directly without relying on env vars
        // (env vars have race conditions in parallel tests)
        use crate::types::{AuditEntry, RiskLevel};
        use chrono::Utc;
        use uuid::Uuid;

        let entry1 = AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            host: "prod-server".into(),
            command: "sudo apt update".into(),
            exit_code: Some(0),
            duration_ms: 100,
            risk_level: RiskLevel::High,
            reason: None,
            change_id: None,
            side_effect: None,
            source: None,
        action: None,
        outcome: None,
    };
        let entry2 = AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            host: "dev-box".into(),
            command: "ls -la".into(),
            exit_code: Some(0),
            duration_ms: 50,
            risk_level: RiskLevel::Low,
            reason: None,
            change_id: None,
            side_effect: None,
            source: None,
        action: None,
        outcome: None,
    };

        // Test search: "apt" should match entry1's command
        let needle = "apt".to_lowercase();
        let entries = [&entry1, &entry2];
        let matches: Vec<_> = entries
            .iter()
            .filter(|e| {
                e.command.to_lowercase().contains(&needle)
                    || e.host.to_lowercase().contains(&needle)
            })
            .collect();
        assert_eq!(matches.len(), 1);
        assert!(matches[0].command.contains("apt"));

        // Test search: "prod" should match entry1's host
        let needle = "prod".to_lowercase();
        let entries = [&entry1, &entry2];
        let matches: Vec<_> = entries
            .iter()
            .filter(|e| {
                e.command.to_lowercase().contains(&needle)
                    || e.host.to_lowercase().contains(&needle)
            })
            .collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].host, "prod-server");
    }

    #[test]
    fn test_audit_filter_command_pattern() {
        use crate::types::{AuditEntry, RiskLevel};
        use chrono::Utc;
        use uuid::Uuid;

        let entry1 = AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            host: "server".into(),
            command: "kubectl delete namespace default".into(),
            exit_code: Some(0),
            duration_ms: 200,
            risk_level: RiskLevel::High,
            reason: None,
            change_id: None,
            side_effect: None,
            source: None,
        action: None,
        outcome: None,
    };
        let entry2 = AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            host: "server".into(),
            command: "ls -la".into(),
            exit_code: Some(0),
            duration_ms: 50,
            risk_level: RiskLevel::Low,
            reason: None,
            change_id: None,
            side_effect: None,
            source: None,
        action: None,
        outcome: None,
    };

        // Test command_pattern: "kubectl delete *" should match entry1
        let pattern = "kubectl delete *";
        let entries = [&entry1, &entry2];
        let matches: Vec<_> = entries
            .iter()
            .filter(|e| glob_match(pattern, &e.command))
            .collect();
        assert_eq!(matches.len(), 1);
        assert!(matches[0].command.starts_with("kubectl delete"));

        // Test command_pattern: "ls *" should match entry2
        let pattern = "ls *";
        let entries = [&entry1, &entry2];
        let matches: Vec<_> = entries
            .iter()
            .filter(|e| glob_match(pattern, &e.command))
            .collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].command, "ls -la");
    }

    // ── F6-3: Metrics trend tests ───────────────────────────────────────────

    #[test]
    fn test_metrics_trend_empty() {
        // compute_metrics_trend should work even if there is no audit data.
        // We don't set AGENT2SSH_CONFIG_DIR to avoid parallel test race conditions.
        // Just verify it returns a valid MetricsTrend with correct structure.
        let trend = super::compute_metrics_trend(super::TrendPeriod::All);
        // It should succeed regardless of whether audit data exists
        assert!(trend.is_ok(), "compute_metrics_trend should not fail");
        let trend = trend.unwrap();
        assert_eq!(trend.period, super::TrendPeriod::All);
        // Verify structural integrity
        assert!(trend.failure_rate >= 0.0 && trend.failure_rate <= 1.0);
        assert!(trend.avg_duration_ms >= 0.0);
        assert_eq!(
            trend.risk_distribution.low
                + trend.risk_distribution.medium
                + trend.risk_distribution.high
                + trend.risk_distribution.blocked,
            trend.total_executions
        );
    }

    #[test]
    fn test_risk_distribution_serialization() {
        let dist = super::RiskDistribution {
            low: 10,
            medium: 5,
            high: 2,
            blocked: 1,
        };
        let json = serde_json::to_string(&dist).unwrap();
        let de: super::RiskDistribution = serde_json::from_str(&json).unwrap();
        assert_eq!(de.low, 10);
        assert_eq!(de.medium, 5);
        assert_eq!(de.high, 2);
        assert_eq!(de.blocked, 1);
    }

    #[test]
    fn test_trend_period_values() {
        let periods = vec![
            super::TrendPeriod::Last24h,
            super::TrendPeriod::Last7d,
            super::TrendPeriod::Last30d,
            super::TrendPeriod::All,
        ];
        for p in &periods {
            let json = serde_json::to_string(p).unwrap();
            let de: super::TrendPeriod = serde_json::from_str(&json).unwrap();
            assert_eq!(*p, de);
        }
        // Verify serialized names
        assert_eq!(
            serde_json::to_string(&super::TrendPeriod::Last24h).unwrap(),
            "\"last24h\""
        );
        assert_eq!(
            serde_json::to_string(&super::TrendPeriod::Last7d).unwrap(),
            "\"last7d\""
        );
        assert_eq!(
            serde_json::to_string(&super::TrendPeriod::Last30d).unwrap(),
            "\"last30d\""
        );
        assert_eq!(
            serde_json::to_string(&super::TrendPeriod::All).unwrap(),
            "\"all\""
        );
    }

    // ── F6-2: Audit export tests ────────────────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn test_export_audit_jsonl_empty() {
        // With a temp config dir (no audit data), JSONL export should be empty
        let config_dir =
            std::env::temp_dir().join(format!("agent2ssh-export-jsonl-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&config_dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &config_dir);

        let filter = AuditFilter::default();
        let output = super::export_audit_jsonl(&filter).unwrap();
        assert!(output.is_empty(), "empty audit should return empty string");

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&config_dir);
    }

    #[test]
    #[serial_test::serial]
    fn test_export_audit_csv_headers() {
        // CSV output should always contain the correct header row
        let config_dir =
            std::env::temp_dir().join(format!("agent2ssh-export-csv-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&config_dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &config_dir);

        let filter = AuditFilter::default();
        let output = super::export_audit_csv(&filter).unwrap();
        assert!(output.starts_with(
            "id,timestamp,host,command,exit_code,duration_ms,risk_level,reason,change_id,source\n"
        ));
        // Should only contain the header row (no data)
        assert_eq!(output.lines().count(), 1);

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&config_dir);
    }

    #[test]
    #[serial_test::serial]
    fn list_audit_raw_reverse_early_stop_is_correct() {
        // J2: the reverse early-stop scan must return the same newest-first,
        // limit-bounded results as the previous full parse, including under
        // host and since filters.
        let dir =
            std::env::temp_dir().join(format!("agent2ssh-auditscan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);

        let n: usize = 5000;
        let base = chrono::DateTime::<chrono::Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let mut body = String::new();
        for i in 0..n {
            let entry = super::AuditEntry {
                id: uuid::Uuid::new_v4(),
                ts: base + chrono::Duration::seconds(i as i64),
                host: if i % 2 == 0 {
                    "alpha".into()
                } else {
                    "beta".into()
                },
                command: format!("cmd {i}"),
                exit_code: Some(0),
                duration_ms: 1,
                risk_level: super::RiskLevel::Low,
                reason: None,
                change_id: None,
                side_effect: None,
                source: None,
                action: None,
                outcome: None,
            };
            body.push_str(&serde_json::to_string(&entry).unwrap());
            body.push('\n');
        }
        std::fs::write(super::audit_path().unwrap(), body).unwrap();

        let commands = |filter: &AuditFilter| -> Vec<String> {
            super::list_audit_raw(filter)
                .unwrap()
                .into_iter()
                .map(|e| e.command)
                .collect()
        };

        // Newest 3, no filter -> highest indices, newest first.
        let recent = commands(&AuditFilter {
            limit: 3,
            ..Default::default()
        });
        assert_eq!(recent, vec!["cmd 4999", "cmd 4998", "cmd 4997"]);

        // Host filter: only even indices are "alpha".
        let alpha = commands(&AuditFilter {
            host: Some("alpha".into()),
            limit: 3,
            ..Default::default()
        });
        assert_eq!(alpha, vec!["cmd 4998", "cmd 4996", "cmd 4994"]);

        // Since window: entries with ts >= base+4997s -> indices 4997..4999.
        let since = (base + chrono::Duration::seconds(4997)).to_rfc3339();
        let windowed = commands(&AuditFilter {
            since: Some(since),
            limit: usize::MAX,
            ..Default::default()
        });
        assert_eq!(windowed, vec!["cmd 4999", "cmd 4998", "cmd 4997"]);

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn load_config_reflects_saved_hosts_via_cache() {
        // I5: load_config is cached, but a save must invalidate it so the write
        // is never served stale within the same process.
        let config_dir =
            std::env::temp_dir().join(format!("agent2ssh-hostscache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&config_dir).unwrap();
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &config_dir);

        // Drop any cache state leaked from an earlier test in this process.
        super::HOSTS_CACHE.invalidate();

        // Fresh dir: no hosts. This also populates the cache with the default.
        assert!(super::load_config().unwrap().hosts.is_empty());

        // Saving a host must be visible to the very next load (cache invalidated).
        let cfg: AppConfig = serde_json::from_value(serde_json::json!({
            "hosts": [{ "name": "alpha", "host": "10.0.0.1" }]
        }))
        .unwrap();
        super::save_config(&cfg).unwrap();

        let after_add = super::load_config().unwrap();
        assert_eq!(after_add.hosts.len(), 1, "saved host must be observed");
        assert_eq!(after_add.hosts[0].name, "alpha");

        // Removing it again must also be reflected immediately.
        super::save_config(&AppConfig::default()).unwrap();
        assert!(
            super::load_config().unwrap().hosts.is_empty(),
            "removal must invalidate the cache too"
        );

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&config_dir);
    }

    #[test]
    fn test_export_audit_csv_escaping() {
        // Test that csv_escape handles commas, quotes, and newlines
        assert_eq!(super::csv_escape("simple"), "simple");
        assert_eq!(super::csv_escape("has,comma"), "\"has,comma\"");
        assert_eq!(super::csv_escape("has\"quote"), "\"has\"\"quote\"");
        assert_eq!(super::csv_escape("has\nnewline"), "\"has\nnewline\"");
        assert_eq!(
            super::csv_escape("both,\"comma and quote\""),
            "\"both,\"\"comma and quote\"\"\""
        );
    }

    #[test]
    fn test_export_audit_jsonl_with_data() {
        // Test the JSONL and CSV formatting logic directly without relying on
        // env vars (which have race conditions in parallel tests).
        use chrono::Utc;
        use uuid::Uuid;

        let entry1 = AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            host: "test-host".into(),
            command: "ls -la".into(),
            exit_code: Some(0),
            duration_ms: 100,
            risk_level: RiskLevel::Low,
            reason: None,
            change_id: None,
            side_effect: None,
            source: None,
        action: None,
        outcome: None,
    };
        let entry2 = AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            host: "prod-host".into(),
            command: "sudo apt update".into(),
            exit_code: Some(0),
            duration_ms: 5000,
            risk_level: RiskLevel::High,
            reason: Some("weekly update".into()),
            change_id: Some("CHG-001".into()),
            side_effect: None,
            source: Some("cli".into()),
        action: None,
        outcome: None,
    };

        let entries = vec![entry1, entry2];

        // Test JSONL formatting (same logic as export_audit_jsonl)
        let mut jsonl_output = String::new();
        for entry in &entries {
            jsonl_output.push_str(&serde_json::to_string(entry).unwrap());
            jsonl_output.push('\n');
        }
        let lines: Vec<&str> = jsonl_output.lines().collect();
        assert_eq!(lines.len(), 2, "should have 2 JSONL lines");
        for line in &lines {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(parsed.get("host").is_some());
        }

        // Test CSV formatting (same logic as export_audit_csv)
        let mut csv_output = String::new();
        csv_output.push_str(
            "id,timestamp,host,command,exit_code,duration_ms,risk_level,reason,change_id\n",
        );
        for entry in &entries {
            csv_output.push_str(&format!(
                "{},{},{},{},{},{},{},{},{}\n",
                entry.id,
                entry.ts.to_rfc3339(),
                super::csv_escape(&entry.host),
                super::csv_escape(&entry.command),
                entry.exit_code.map(|c| c.to_string()).unwrap_or_default(),
                entry.duration_ms,
                entry.risk_level,
                super::csv_escape(entry.reason.as_deref().unwrap_or("")),
                super::csv_escape(entry.change_id.as_deref().unwrap_or("")),
            ));
        }
        let csv_lines: Vec<&str> = csv_output.lines().collect();
        assert_eq!(csv_lines.len(), 3, "header + 2 data rows");
        assert!(csv_lines[0].starts_with("id,"));
        // Verify data row contains expected values
        assert!(csv_lines[1].contains("test-host"));
        assert!(csv_lines[2].contains("prod-host"));
        assert!(csv_lines[2].contains("CHG-001"));
    }

    // ── S1-1: exec-multi audit context tests ───────────────────────────────

    #[test]
    fn test_exec_multi_audit_entries_reason_and_change_id() {
        // Verify that audit entries constructed for an exec-multi scenario
        // correctly carry reason and change_id through the full JSONL
        // serialisation round-trip — one entry per target host.
        use chrono::Utc;
        use uuid::Uuid;

        let reason = "deploy v2.3.1";
        let change_id = "CHG-20240614-001";
        let hosts = vec!["web-1", "web-2", "web-3"];

        // Simulate what append_audit does for each host in an exec-multi
        let mut jsonl_lines = Vec::new();
        for host in &hosts {
            let result = ExecResult {
                host: host.to_string(),
                command: "systemctl restart app".into(),
                exit_code: Some(0),
                stdout: "ok".into(),
                stderr: String::new(),
                duration_ms: 150,
                risk_level: RiskLevel::Medium,
                truncated: false,
                dropped_bytes: 0,
                side_effect: None,
            };
            // Mirror the AuditEntry construction in append_audit
            let entry = AuditEntry {
                id: Uuid::new_v4(),
                ts: Utc::now(),
                host: result.host.clone(),
                command: redact_sensitive_text(&result.command),
                exit_code: result.exit_code,
                duration_ms: result.duration_ms,
                risk_level: RiskLevel::Medium,
                reason: Some(reason.to_string()),
                change_id: Some(change_id.to_string()),
                side_effect: None,
                source: Some("mcp".into()),
        action: None,
        outcome: None,
    };
            jsonl_lines.push(serde_json::to_string(&entry).unwrap());
        }

        assert_eq!(jsonl_lines.len(), 3, "one audit entry per host");

        let mut seen_hosts = Vec::new();
        for line in &jsonl_lines {
            let entry: AuditEntry = serde_json::from_str(line).unwrap();
            assert_eq!(
                entry.reason,
                Some(reason.into()),
                "audit entry for {} should have reason",
                entry.host
            );
            assert_eq!(
                entry.change_id,
                Some(change_id.into()),
                "audit entry for {} should have change_id",
                entry.host
            );
            assert_eq!(entry.command, "systemctl restart app");
            assert_eq!(entry.exit_code, Some(0));
            assert_eq!(entry.risk_level, RiskLevel::Medium);
            seen_hosts.push(entry.host);
        }

        for host in &hosts {
            assert!(
                seen_hosts.contains(&host.to_string()),
                "host {} should appear in audit entries",
                host
            );
        }
    }

    #[test]
    fn test_exec_multi_audit_entries_without_reason() {
        // Verify exec-multi without reason/change_id produces entries with None
        use chrono::Utc;
        use uuid::Uuid;

        let entry = AuditEntry {
            id: Uuid::new_v4(),
            ts: Utc::now(),
            host: "db-1".into(),
            command: "pg_dump mydb".into(),
            exit_code: Some(0),
            duration_ms: 3000,
            risk_level: RiskLevel::Low,
            reason: None,
            change_id: None,
            side_effect: None,
            source: None,
        action: None,
        outcome: None,
    };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: AuditEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.reason, None);
        assert_eq!(parsed.change_id, None);
        assert_eq!(parsed.host, "db-1");
    }

    #[test]
    fn test_audit_entry_jsonl_roundtrip_multi_host() {
        // Simulate a full exec-multi audit trail: write JSONL entries for
        // multiple hosts, read them back, and verify reason/change_id survive
        // the round-trip — including search-style filtering.
        use chrono::Utc;
        use uuid::Uuid;

        let entries = [
            AuditEntry {
                id: Uuid::new_v4(),
                ts: Utc::now(),
                host: "alpha".into(),
                command: "uptime".into(),
                exit_code: Some(0),
                duration_ms: 50,
                risk_level: RiskLevel::Low,
                reason: Some("health check".into()),
                change_id: Some("CHG-100".into()),
                side_effect: None,
                source: Some("cli".into()),
        action: None,
        outcome: None,
    },
            AuditEntry {
                id: Uuid::new_v4(),
                ts: Utc::now(),
                host: "beta".into(),
                command: "df -h".into(),
                exit_code: Some(0),
                duration_ms: 80,
                risk_level: RiskLevel::Low,
                reason: None,
                change_id: None,
                side_effect: None,
                source: None,
        action: None,
        outcome: None,
    },
            AuditEntry {
                id: Uuid::new_v4(),
                ts: Utc::now(),
                host: "gamma".into(),
                command: "free -m".into(),
                exit_code: Some(1),
                duration_ms: 120,
                risk_level: RiskLevel::Medium,
                reason: Some("health check".into()),
                change_id: Some("CHG-100".into()),
                side_effect: None,
                source: Some("mcp".into()),
        action: None,
        outcome: None,
    },
        ];

        // Write JSONL and read back
        let jsonl: String = entries
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect::<Vec<_>>()
            .join("\n");

        let parsed: Vec<AuditEntry> = jsonl
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();

        assert_eq!(parsed.len(), 3);

        // alpha and gamma share the same change_id
        let with_change: Vec<_> = parsed
            .iter()
            .filter(|e| e.change_id == Some("CHG-100".into()))
            .collect();
        assert_eq!(with_change.len(), 2);
        assert!(with_change.iter().any(|e| e.host == "alpha"));
        assert!(with_change.iter().any(|e| e.host == "gamma"));

        // beta has no reason/change_id
        let beta = parsed.iter().find(|e| e.host == "beta").unwrap();
        assert_eq!(beta.reason, None);
        assert_eq!(beta.change_id, None);

        // Search for "health" in reason context identifies the right entries
        let health_entries: Vec<_> = parsed
            .iter()
            .filter(|e| e.reason.as_deref() == Some("health check"))
            .collect();
        assert_eq!(health_entries.len(), 2);
    }
}
