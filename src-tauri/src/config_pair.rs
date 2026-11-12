//! CNB #26 phase C: paired-snapshot index and restore-time reference checks.
//!
//! Phase A/B made each credential reference self-describing with a content tag.
//! Phase C uses that tag to decide whether a `hosts.json` from a snapshot,
//! template, WebDAV pull or desktop cloud backup may be written over the
//! current `secrets.enc`. Restoring an older `hosts.json` used to silently
//! resolve its bare markers to same-named but newer ciphertexts.
//!
//! `config_pair_index.json` records paired copies of the *current* state
//! (`hosts.json.pair` + `secrets.enc.pair`) together with both hashes, the
//! content generation and a digest of the tag set. It is diagnostic evidence
//! and the basis for a later explicit paired-restore command (phase D); it is
//! deliberately **not** a new hard dependency for ordinary writes.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    secrets::{self, parse_secret_ref, SecretRef},
    store::{config_dir, write_private_bytes},
    store_commit::sha256_bytes,
};

pub const PAIR_INDEX_FILE: &str = "config_pair_index.json";
pub const HOSTS_PAIR_BACKUP: &str = "hosts.json.pair";
pub const SECRETS_PAIR_BACKUP: &str = "secrets.enc.pair";
const SCHEMA_VERSION: u32 = 1;
const MAX_PAIRS: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairRecord {
    pub hosts_sha256: String,
    pub secrets_sha256: String,
    pub secrets_generation: u64,
    /// SHA-256 of the sorted `(account, tag)` list at the time of the save.
    pub tag_digest: String,
    pub hosts_backup: String,
    pub secrets_backup: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PairIndex {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub pairs: Vec<PairRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexStatus {
    Consistent,
    Missing,
    Unreadable,
    Stale,
}

pub fn index_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(PAIR_INDEX_FILE))
}

/// Digest of the current `(account, tag)` set. Stable across re-encryption.
pub fn tag_digest() -> String {
    let mut pairs: Vec<String> = secrets::entry_tags()
        .into_iter()
        .map(|(account, tag)| format!("{account}={tag}"))
        .collect();
    pairs.sort();
    sha256_bytes(pairs.join("\n").as_bytes())
}

pub fn read_index() -> PairIndex {
    let Ok(path) = index_path() else {
        return PairIndex::default();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return PairIndex::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Read-only consistency check. Never blocks a write; used for diagnostics and
/// to tell a restore path whether a paired snapshot is available as evidence.
pub fn index_status() -> IndexStatus {
    let Ok(path) = index_path() else {
        return IndexStatus::Unreadable;
    };
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return IndexStatus::Missing;
        }
        Err(_) => return IndexStatus::Unreadable,
    };
    let index: PairIndex = match serde_json::from_str(&raw) {
        Ok(index) => index,
        Err(_) => return IndexStatus::Unreadable,
    };
    if index.schema_version != SCHEMA_VERSION {
        return IndexStatus::Unreadable;
    }
    let Some(latest) = index.pairs.last() else {
        return IndexStatus::Missing;
    };
    let Ok(dir) = config_dir() else {
        return IndexStatus::Unreadable;
    };
    let hosts = fs::read(dir.join("hosts.json"))
        .map(|bytes| sha256_bytes(&bytes))
        .unwrap_or_default();
    let secrets = fs::read(dir.join(secrets::SECRETS_FILE))
        .map(|bytes| sha256_bytes(&bytes))
        .unwrap_or_default();
    // C6: an index whose tag digest no longer matches the live store is stale
    // even when both file hashes still line up (the credential set changed
    // without a paired snapshot being recorded).
    if hosts == latest.hosts_sha256
        && secrets == latest.secrets_sha256
        && tag_digest() == latest.tag_digest
    {
        IndexStatus::Consistent
    } else {
        IndexStatus::Stale
    }
}

/// Record a paired snapshot of the state that was just committed. Called after
/// the `hosts.json` rename and before the commit log is cleared. All failures
/// are non-fatal: an auxiliary index must never turn into a write blocker.
pub fn record_current_pair() {
    let result = (|| -> Result<()> {
        let dir = config_dir()?;
        let hosts = fs::read(dir.join("hosts.json")).context("read hosts.json")?;
        let secrets = fs::read(dir.join(secrets::SECRETS_FILE)).context("read secrets.enc")?;
        fs::copy(dir.join("hosts.json"), dir.join(HOSTS_PAIR_BACKUP)).context("copy hosts pair")?;
        fs::copy(
            dir.join(secrets::SECRETS_FILE),
            dir.join(SECRETS_PAIR_BACKUP),
        )
        .context("copy secrets pair")?;
        let mut index = read_index();
        index.schema_version = SCHEMA_VERSION;
        index.pairs.push(PairRecord {
            hosts_sha256: sha256_bytes(&hosts),
            secrets_sha256: sha256_bytes(&secrets),
            secrets_generation: secrets::store_generation().unwrap_or(0),
            tag_digest: tag_digest(),
            hosts_backup: HOSTS_PAIR_BACKUP.to_string(),
            secrets_backup: SECRETS_PAIR_BACKUP.to_string(),
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        });
        if index.pairs.len() > MAX_PAIRS {
            let drop = index.pairs.len() - MAX_PAIRS;
            index.pairs.drain(0..drop);
        }
        let raw = serde_json::to_string_pretty(&index)?;
        write_private_bytes(&index_path()?, raw.as_bytes())
    })();
    if let Err(error) = result {
        eprintln!("warning: could not record a paired config snapshot: {error:#}");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreVerdict {
    /// Every tagged reference matches the current credential store.
    Allowed,
    /// No tagged reference could be verified (bare markers, keyring markers,
    /// malformed markers, or content that is not a config JSON).
    LegacyOnly(Vec<String>),
    /// At least one tagged reference points at a different content version.
    Refused(Vec<String>),
}

fn check_field(
    field: Option<&str>,
    account: &str,
    label: &str,
    tags: &std::collections::HashMap<String, String>,
    legacy: &mut Vec<String>,
    refused: &mut Vec<String>,
) {
    let Some(value) = field else {
        return;
    };
    match parse_secret_ref(value) {
        Some(SecretRef::Tagged(tag)) => {
            let current = tags.get(account).map(String::as_str);
            if current != Some(tag) {
                refused.push(format!(
                    "{label} (account {account}): expected tag {tag}, store has {}",
                    current.unwrap_or("no matching entry")
                ));
            }
        }
        Some(SecretRef::Bare) => legacy.push(format!("{label}: legacy bare marker")),
        Some(SecretRef::Keyring) => legacy.push(format!("{label}: legacy keyring marker")),
        Some(SecretRef::Malformed) => legacy.push(format!("{label}: malformed reference")),
        None => {}
    }
}

/// Decide whether `hosts_bytes` may be written over the current `secrets.enc`.
pub fn verify_hosts_restore(hosts_bytes: &[u8]) -> RestoreVerdict {
    let tags = secrets::entry_tags();
    let mut legacy = Vec::new();
    let mut refused = Vec::new();

    let parsed: Result<Value, _> = serde_json::from_slice(hosts_bytes);
    let Ok(config) = parsed else {
        legacy.push("hosts.json is not valid JSON; no tagged reference could be verified".into());
        return RestoreVerdict::LegacyOnly(legacy);
    };

    if let Some(hosts) = config.get("hosts").and_then(Value::as_array) {
        for host in hosts {
            let name = host.get("name").and_then(Value::as_str).unwrap_or("");
            check_field(
                host.get("password").and_then(Value::as_str),
                &secrets::host_account(name),
                &format!("host '{name}' password"),
                &tags,
                &mut legacy,
                &mut refused,
            );
            check_field(
                host.get("passphrase").and_then(Value::as_str),
                &secrets::host_passphrase_account(name),
                &format!("host '{name}' passphrase"),
                &tags,
                &mut legacy,
                &mut refused,
            );
        }
    }
    if let Some(proxies) = config.get("proxies").and_then(Value::as_array) {
        for proxy in proxies {
            let id = proxy.get("id").and_then(Value::as_str).unwrap_or("");
            check_field(
                proxy.get("password").and_then(Value::as_str),
                &secrets::proxy_account(id),
                &format!("proxy '{id}' password"),
                &tags,
                &mut legacy,
                &mut refused,
            );
        }
    }

    if !refused.is_empty() {
        RestoreVerdict::Refused(refused)
    } else if !legacy.is_empty() {
        RestoreVerdict::LegacyOnly(legacy)
    } else {
        RestoreVerdict::Allowed
    }
}

/// Guard used by every path that overwrites `hosts.json` without going through
/// `save_config`. A mismatched tag is a hard error with an actionable message.
pub fn guard_restore(hosts_bytes: &[u8], context: &str) -> Result<()> {
    match verify_hosts_restore(hosts_bytes) {
        RestoreVerdict::Allowed => Ok(()),
        RestoreVerdict::LegacyOnly(reasons) => {
            eprintln!(
                "warning: {context}: credential references cannot be verified ({}); the current secrets.enc is unchanged",
                reasons.join("; ")
            );
            Ok(())
        }
        RestoreVerdict::Refused(reasons) => Err(anyhow!(
            "refusing to {context}: {} tagged credential reference(s) were written against a different secrets.enc content version: {}. Re-enter the affected credentials, or restore hosts.json and secrets.enc as a pair (an explicit paired restore is phase D, not implemented).",
            reasons.len(),
            reasons.join("; ")
        )),
    }
}
