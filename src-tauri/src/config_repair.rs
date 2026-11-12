//! CNB #26 phase D / #28: explicit, audited repair for the two-file config
//! transaction.
//!
//! Two failure states previously had no supported exit:
//!
//! * `recover()` keeps an unreplayable `.commit.json` and refuses every later
//!   write; the only option was to delete the log by hand (#28 `lost-tmp`).
//! * A restored/older `hosts.json` whose content tags no longer match
//!   `secrets.enc` is refused by phase C's guard, but there was no command to
//!   restore the recorded pair.
//!
//! Both are handled here with one CLI entry point. Nothing happens without an
//! explicit `--confirm`; every applied change writes a diagnostic audit entry.

use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::HashSet, fs};

use crate::{config_pair, diagnostics, secrets, store, store_commit};

fn audit(action: &str, applied: bool, fields: Value) {
    let _ = diagnostics::append_diagnostic_log(
        if applied { "warn" } else { "info" },
        "config_repair",
        &format!(
            "config repair {action} {}",
            if applied { "applied" } else { "previewed" }
        ),
        Some(fields),
    );
}

#[derive(Debug, Clone, Serialize)]
pub struct PairSummary {
    pub index: usize,
    pub created_at: u64,
    pub secrets_generation: u64,
    pub hosts_sha256: String,
    pub secrets_sha256: String,
    pub tag_digest: String,
    pub hosts_backup: String,
    pub secrets_backup: String,
}

pub fn list_pairs() -> Result<Vec<PairSummary>> {
    let index = config_pair::read_index();
    Ok(index
        .pairs
        .iter()
        .enumerate()
        .map(|(i, p)| PairSummary {
            index: i,
            created_at: p.created_at,
            secrets_generation: p.secrets_generation,
            hosts_sha256: p.hosts_sha256.clone(),
            secrets_sha256: p.secrets_sha256.clone(),
            tag_digest: p.tag_digest.clone(),
            hosts_backup: p.hosts_backup.clone(),
            secrets_backup: p.secrets_backup.clone(),
        })
        .collect())
}

fn current_hashes() -> (Option<String>, Option<String>) {
    let dir = match store::config_dir() {
        Ok(dir) => dir,
        Err(_) => return (None, None),
    };
    (
        store_commit::sha256_file(&dir.join("hosts.json"))
            .ok()
            .flatten(),
        store_commit::sha256_file(&dir.join(secrets::SECRETS_FILE))
            .ok()
            .flatten(),
    )
}

/// Accounts present in `secrets.enc` but no longer referenced by hosts.json.
/// These are the "orphans" a discarded pending commit may leave behind.
fn orphan_accounts() -> Vec<String> {
    let dir = match store::config_dir() {
        Ok(dir) => dir,
        Err(_) => return Vec::new(),
    };
    let raw = match fs::read_to_string(dir.join("hosts.json")) {
        Ok(raw) => raw,
        Err(_) => return Vec::new(),
    };
    let config: Value = match serde_json::from_str(&raw) {
        Ok(config) => config,
        Err(_) => return Vec::new(),
    };
    let mut referenced = HashSet::new();
    if let Some(hosts) = config.get("hosts").and_then(Value::as_array) {
        for host in hosts {
            let name = host.get("name").and_then(Value::as_str).unwrap_or("");
            if host.get("password").and_then(Value::as_str).is_some() {
                referenced.insert(secrets::host_account(name));
            }
            if host.get("passphrase").and_then(Value::as_str).is_some() {
                referenced.insert(secrets::host_passphrase_account(name));
            }
        }
    }
    if let Some(proxies) = config.get("proxies").and_then(Value::as_array) {
        for proxy in proxies {
            let id = proxy.get("id").and_then(Value::as_str).unwrap_or("");
            if proxy.get("password").and_then(Value::as_str).is_some() {
                referenced.insert(secrets::proxy_account(id));
            }
        }
    }
    let mut orphans: Vec<String> = secrets::entry_tags()
        .into_keys()
        .filter(|account| !referenced.contains(account))
        .collect();
    orphans.sort();
    orphans
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscardPreview {
    pub has_pending: bool,
    pub commit_path: String,
    pub tmp_name: Option<String>,
    pub tmp_exists: bool,
    pub logged_hosts_sha256: Option<String>,
    pub logged_secrets_sha256: Option<String>,
    pub logged_generation: Option<u64>,
    pub current_hosts_sha256: Option<String>,
    pub current_secrets_sha256: Option<String>,
    pub hosts_generation: Option<u64>,
    pub store_generation: Option<u64>,
    pub orphan_accounts: Vec<String>,
    pub index_status: String,
}

/// Read-only preview. Never calls `recover()` — that would itself replay or
/// delete the log, which is exactly the side effect a preview must not have.
pub fn discard_preview() -> Result<DiscardPreview> {
    let path = store_commit::commit_path()?;
    let (current_hosts, current_secrets) = current_hashes();
    let hosts_generation = store::config_dir()
        .ok()
        .and_then(|dir| fs::read_to_string(dir.join("hosts.json")).ok())
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|config| config.get("secrets_generation").and_then(Value::as_u64));

    let mut preview = DiscardPreview {
        has_pending: false,
        commit_path: path.display().to_string(),
        tmp_name: None,
        tmp_exists: false,
        logged_hosts_sha256: None,
        logged_secrets_sha256: None,
        logged_generation: None,
        current_hosts_sha256: current_hosts,
        current_secrets_sha256: current_secrets,
        hosts_generation,
        store_generation: secrets::store_generation(),
        orphan_accounts: orphan_accounts(),
        index_status: format!("{:?}", config_pair::index_status()),
    };

    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(preview),
        Err(error) => return Err(error).context("failed to read pending config commit"),
    };
    preview.has_pending = true;
    if let Ok(log) = serde_json::from_str::<store_commit::CommitLog>(&raw) {
        preview.tmp_name = Some(log.tmp_name.clone());
        preview.tmp_exists = store::config_dir()
            .map(|dir| dir.join(&log.tmp_name).exists())
            .unwrap_or(false);
        preview.logged_hosts_sha256 = Some(log.hosts_sha256);
        preview.logged_secrets_sha256 = Some(log.secrets_sha256);
        preview.logged_generation = Some(log.secrets_generation);
    }
    Ok(preview)
}

pub fn preview_discard() -> Result<DiscardPreview> {
    let preview = discard_preview()?;
    audit(
        "discard-pending",
        false,
        json!({ "has_pending": preview.has_pending, "orphan_accounts": preview.orphan_accounts }),
    );
    Ok(preview)
}

/// Delete the unreplayable commit log. `hosts.json` and `secrets.enc` are left
/// byte-for-byte untouched; orphan ciphertexts are reported, never removed.
pub fn apply_discard(expected: &DiscardPreview) -> Result<String> {
    if !expected.has_pending {
        bail!("no pending config commit to discard");
    }
    // Take the raw file lock rather than `hosts_file_lock()`: the latter runs
    // `recover()`, which is precisely what is refusing writes in this state.
    let _guard = store::lock_config_file(".hosts.lock")?;
    let current = discard_preview()?;
    if !current.has_pending {
        bail!("pending config commit disappeared; nothing was changed");
    }
    if current.tmp_name != expected.tmp_name
        || current.logged_hosts_sha256 != expected.logged_hosts_sha256
        || current.logged_secrets_sha256 != expected.logged_secrets_sha256
    {
        bail!(
            "pending config commit changed since the preview; re-run the preview before applying"
        );
    }
    store_commit::finish()?;
    audit(
        "discard-pending",
        true,
        json!({
            "tmp_name": expected.tmp_name,
            "orphan_accounts": expected.orphan_accounts,
            "hosts_sha256": expected.current_hosts_sha256,
            "secrets_sha256": expected.current_secrets_sha256,
        }),
    );
    Ok(format!(
        "Discarded pending config commit. hosts.json and secrets.enc were NOT modified; {} orphan account(s) remain in the credential store; the prepared temp file was kept for inspection.",
        expected.orphan_accounts.len()
    ))
}

#[derive(Debug, Clone, Serialize)]
pub struct RestorePreview {
    pub index: usize,
    pub pair: PairSummary,
    pub hosts_pair_exists: bool,
    pub secrets_pair_exists: bool,
    /// Both paired copies must still hash to the values recorded at save time.
    pub hashes_match: bool,
    pub current_hosts_sha256: Option<String>,
    pub current_secrets_sha256: Option<String>,
    pub has_pending_commit: bool,
}

pub fn restore_preview(index: usize) -> Result<RestorePreview> {
    let pairs = list_pairs()?;
    let pair = pairs
        .get(index)
        .cloned()
        .ok_or_else(|| anyhow!("no paired snapshot at index {index}; run with --list"))?;
    let dir = store::config_dir()?;
    let hosts_pair = dir.join(&pair.hosts_backup);
    let secrets_pair = dir.join(&pair.secrets_backup);
    let hosts_exists = hosts_pair.is_file();
    let secrets_exists = secrets_pair.is_file();
    let hashes_match = hosts_exists
        && secrets_exists
        && store_commit::sha256_file(&hosts_pair)
            .ok()
            .flatten()
            .as_deref()
            == Some(pair.hosts_sha256.as_str())
        && store_commit::sha256_file(&secrets_pair)
            .ok()
            .flatten()
            .as_deref()
            == Some(pair.secrets_sha256.as_str());
    let (current_hosts, current_secrets) = current_hashes();
    Ok(RestorePreview {
        index,
        pair,
        hosts_pair_exists: hosts_exists,
        secrets_pair_exists: secrets_exists,
        hashes_match,
        current_hosts_sha256: current_hosts,
        current_secrets_sha256: current_secrets,
        has_pending_commit: store_commit::commit_path()?.exists(),
    })
}

pub fn preview_restore(index: usize) -> Result<RestorePreview> {
    let preview = restore_preview(index)?;
    audit(
        "restore-pair",
        false,
        json!({ "index": index, "hashes_match": preview.hashes_match }),
    );
    Ok(preview)
}

/// Roll `hosts.json` and `secrets.enc` back together under both locks.
/// The two replacements are not a single atomic transaction: on a failure in
/// the second write the first is restored from the in-memory backup.
pub fn apply_restore(expected: &RestorePreview) -> Result<String> {
    if expected.has_pending_commit {
        bail!("a pending .commit.json exists; discard it first with --discard-pending --confirm");
    }
    if !expected.hashes_match {
        bail!("paired copies are missing or no longer match the index record; refusing to restore");
    }
    let _hosts_guard = store::lock_config_file(".hosts.lock")?;
    let _secrets_guard = store::lock_config_file(".secrets.lock")?;
    let current = restore_preview(expected.index)?;
    if !current.hashes_match
        || current.has_pending_commit
        || current.pair.hosts_sha256 != expected.pair.hosts_sha256
    {
        bail!("configuration changed since the preview; re-run the preview before applying");
    }

    let dir = store::config_dir()?;
    let hosts_path = dir.join("hosts.json");
    let secrets_path = dir.join(secrets::SECRETS_FILE);
    let hosts_bytes = fs::read(dir.join(&current.pair.hosts_backup))?;
    let secrets_bytes = fs::read(dir.join(&current.pair.secrets_backup))?;
    let hosts_before = fs::read(&hosts_path).unwrap_or_default();

    store::write_private_bytes(&hosts_path, &hosts_bytes)
        .context("failed to restore hosts.json from the paired snapshot")?;
    if let Err(error) = store::write_private_bytes(&secrets_path, &secrets_bytes) {
        // Best-effort rollback of the first replacement.
        let rollback = store::write_private_bytes(&hosts_path, &hosts_before);
        audit(
            "restore-pair",
            false,
            json!({ "index": expected.index, "error": error.to_string(), "rollback_ok": rollback.is_ok() }),
        );
        return Err(anyhow!(
            "failed to restore secrets.enc ({error:#}); hosts.json rollback {}",
            if rollback.is_ok() {
                "succeeded"
            } else {
                "FAILED — inspect the config directory"
            }
        ));
    }
    config_pair::record_current_pair();
    audit(
        "restore-pair",
        true,
        json!({
            "index": expected.index,
            "hosts_sha256": current.pair.hosts_sha256,
            "secrets_sha256": current.pair.secrets_sha256,
            "secrets_generation": current.pair.secrets_generation,
        }),
    );
    Ok(format!(
        "Restored paired snapshot [{}]: hosts.json and secrets.enc now match generation {} ({} / {}).",
        expected.index,
        current.pair.secrets_generation,
        &current.pair.hosts_sha256[..12.min(current.pair.hosts_sha256.len())],
        &current.pair.secrets_sha256[..12.min(current.pair.secrets_sha256.len())]
    ))
}

/// Human-readable rendering shared by preview and apply paths.
pub fn describe_discard(p: &DiscardPreview) -> String {
    let mut out = String::from("Discard pending config commit — preview\n");
    if !p.has_pending {
        out.push_str("  No pending .commit.json found. Nothing to do.\n");
        return out;
    }
    out.push_str(&format!("  commit log: {}\n", p.commit_path));
    out.push_str(&format!(
        "  prepared temp: {} (exists: {})\n",
        p.tmp_name.as_deref().unwrap_or("?"),
        p.tmp_exists
    ));
    out.push_str(&format!(
        "  logged generation: {:?}; hosts.json generation: {:?}; secrets.enc generation: {:?}\n",
        p.logged_generation, p.hosts_generation, p.store_generation
    ));
    out.push_str(&format!(
        "  will keep: hosts.json {} / secrets.enc {}\n",
        p.current_hosts_sha256
            .as_deref()
            .map(|h| &h[..12.min(h.len())])
            .unwrap_or("missing"),
        p.current_secrets_sha256
            .as_deref()
            .map(|h| &h[..12.min(h.len())])
            .unwrap_or("missing")
    ));
    out.push_str(&format!(
        "  orphan credential accounts (NOT removed): {:?}\n",
        p.orphan_accounts
    ));
    out.push_str(&format!("  paired index status: {}\n", p.index_status));
    out.push_str(
        "  Discarding removes only .commit.json; no credential or host data is deleted.\n",
    );
    out
}

pub fn describe_restore(p: &RestorePreview) -> String {
    let mut out = format!("Restore paired snapshot [{}] — preview\n", p.index);
    out.push_str(&format!("  created_at: {}\n", p.pair.created_at));
    out.push_str(&format!(
        "  target generation: {}\n  target hosts hash:   {}\n  target secrets hash: {}\n",
        p.pair.secrets_generation,
        &p.pair.hosts_sha256[..12.min(p.pair.hosts_sha256.len())],
        &p.pair.secrets_sha256[..12.min(p.pair.secrets_sha256.len())]
    ));
    out.push_str(&format!(
        "  paired copies present: hosts={} secrets={}; hashes match record: {}\n",
        p.hosts_pair_exists, p.secrets_pair_exists, p.hashes_match
    ));
    out.push_str(&format!(
        "  current hosts hash:   {}\n  current secrets hash: {}\n",
        p.current_hosts_sha256
            .as_deref()
            .map(|h| &h[..12.min(h.len())])
            .unwrap_or("missing"),
        p.current_secrets_sha256
            .as_deref()
            .map(|h| &h[..12.min(h.len())])
            .unwrap_or("missing")
    ));
    if p.has_pending_commit {
        out.push_str("  BLOCKER: a pending .commit.json exists; discard it first.\n");
    }
    out.push_str("  Both files are replaced together under the hosts and secrets locks.\n");
    out
}

/// Guard for CLI callers: exactly one action must be selected.
pub fn validate_action(list: bool, discard: bool, restore: Option<usize>) -> Result<()> {
    let count = u32::from(list) + u32::from(discard) + u32::from(restore.is_some());
    if count != 1 {
        bail!("choose exactly one of --list, --discard-pending or --restore-pair <N>");
    }
    Ok(())
}
