//! Crash-recovery log for the two-file `hosts.json` + `secrets.enc` commit.
//!
//! `store::save_config_unlocked_strict_host` seals new credentials into
//! `secrets.enc` *before* it replaces `hosts.json`. A crash in that window
//! leaves ciphertexts that no config references (harmless), but a later save
//! could also observe a half-finished pair. The log below makes the window
//! explicit and recoverable:
//!
//! 1. seal secrets (each `store_secret` is one atomic replace, all under
//!    `.secrets.lock`),
//! 2. write and fsync `hosts.json.tmp.<pid>`,
//! 3. atomically write `.commit.json` naming the target hosts hash, the
//!    secrets hash observed after step 1, the content generation, and the
//!    temp file,
//! 4. rename the temp file over `hosts.json`,
//! 5. remove `.commit.json`.
//!
//! [`recover`] runs under `.hosts.lock` before any later writer proceeds. It is
//! idempotent: a log whose target hosts hash is already on disk is just
//! removed; if the rename never happened but the prepared temp file is intact,
//! the rename is replayed; if the credentials never finished committing, the
//! old `hosts.json` is still a valid state and the log is discarded. A log that
//! cannot be replayed is **kept** and writes are refused — silently guessing
//! which half committed would be worse than blocking until a human looks.
//!
//! The log contains only hashes and a file name, never a credential. The
//! temp-file name is validated as a bare file name so a tampered log cannot
//! redirect the replay outside the config directory.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::store::{config_dir, replace_file_atomic, write_private_text};

pub const COMMIT_FILE: &str = ".commit.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitLog {
    pub version: u32,
    /// SHA-256 of the exact `hosts.json` bytes this commit is about to install.
    pub hosts_sha256: String,
    /// SHA-256 of `secrets.enc` after the credentials were sealed.
    pub secrets_sha256: String,
    pub secrets_generation: u64,
    /// Bare file name of the prepared hosts temp file.
    pub tmp_name: String,
}

pub struct Recovery {
    /// True when `hosts.json` was brought forward by replaying the prepared
    /// temp file; the caller must invalidate the in-process config cache.
    pub replayed: bool,
}

pub fn commit_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(COMMIT_FILE))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// SHA-256 of a file, or `None` when it does not exist.
pub fn sha256_file(path: &Path) -> Result<Option<String>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(sha256_bytes(&bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to hash {}", path.display())),
    }
}

pub fn secrets_file_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(crate::secrets::SECRETS_FILE))
}

/// Record an in-flight commit. Called only when this save actually changed the
/// credential store; a save that touches `hosts.json` alone is already covered
/// by the single atomic rename and needs no log.
pub fn begin(entry: &CommitLog) -> Result<()> {
    let path = commit_path()?;
    let raw = serde_json::to_string_pretty(entry)?;
    write_private_text(&path, &raw)
        .with_context(|| format!("failed to write pending config commit {}", path.display()))
}

pub fn finish() -> Result<()> {
    let path = commit_path()?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("failed to clear pending config commit {}", path.display())),
    }
}

fn is_bare_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && name != "."
        && name != ".."
        && Path::new(name)
            .file_name()
            .map(|part| part.to_string_lossy().into_owned())
            .as_deref()
            == Some(name)
}

/// Resolve a commit left behind by a crashed writer. **Must** be called while
/// holding `.hosts.lock`, so no other writer can interleave with the replay.
pub fn recover() -> Result<Recovery> {
    let path = commit_path()?;
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Recovery { replayed: false });
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!("failed to read pending config commit {}", path.display())
            });
        }
    };
    let log: CommitLog = serde_json::from_str(&raw).map_err(|error| {
        anyhow!(
            "unreadable pending config commit {} ({error}); refusing to write until it is inspected",
            path.display()
        )
    })?;
    if log.version != 1 || !is_bare_file_name(&log.tmp_name) {
        return Err(anyhow!(
            "pending config commit {} has an unsupported version or an unsafe temp file name; refusing to write until it is inspected",
            path.display()
        ));
    }

    let dir = config_dir()?;
    let hosts = dir.join("hosts.json");
    let tmp = dir.join(&log.tmp_name);
    let secrets = secrets_file_path()?;

    // Case 1: the hosts rename already completed (crash between step 4 and 5).
    if sha256_file(&hosts)?.as_deref() == Some(log.hosts_sha256.as_str()) {
        let _ = fs::remove_file(&tmp);
        finish()?;
        return Ok(Recovery { replayed: false });
    }

    // Case 2: the credentials never finished committing. The old hosts file is
    // still the authority; extra ciphertexts are orphans that a retry rewrites.
    if sha256_file(&secrets)?.as_deref() != Some(log.secrets_sha256.as_str()) {
        let _ = fs::remove_file(&tmp);
        finish()?;
        return Ok(Recovery { replayed: false });
    }

    // Case 3: credentials are in place, hosts are not. Replay the prepared
    // rename — but only if the prepared bytes are still exactly what we logged.
    match sha256_file(&tmp)? {
        Some(hash) if hash == log.hosts_sha256 => {
            replace_file_atomic(&tmp, &hosts).with_context(|| {
                format!(
                    "failed to replay pending config commit {} over {}",
                    tmp.display(),
                    hosts.display()
                )
            })?;
            finish()?;
            Ok(Recovery { replayed: true })
        }
        _ => Err(anyhow!(
            "pending config commit cannot be replayed: prepared hosts file {} is missing or altered. hosts.json still holds the previous configuration; inspect {} before writing again",
            tmp.display(),
            path.display()
        )),
    }
}
