use anyhow::{Context, Result};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

use crate::types::{AppConfig, AuditEntry, AuditFilter, ExecResult, RiskLevel};

static STORE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn hosts_lock() -> &'static Mutex<()> {
    STORE_LOCK.get_or_init(|| Mutex::new(()))
}

pub fn config_dir() -> Result<PathBuf> {
    let base =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("unable to locate home directory"))?;
    Ok(base.join(".agent2ssh"))
}

fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("hosts.json"))
}

pub fn audit_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("audit.jsonl"))
}

pub fn ensure_config_dir() -> Result<()> {
    fs::create_dir_all(config_dir()?).context("failed to create ~/.agent2ssh")
}

/// Restrict a sensitive file to owner read/write on Unix.
pub fn restrict_file_to_owner(path: impl AsRef<std::path::Path>) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path.as_ref(), perms).with_context(|| {
            format!("failed to restrict permissions for {}", path.as_ref().display())
        })?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

pub fn load_config() -> Result<AppConfig> {
    ensure_config_dir()?;
    let path = config_path()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let raw = fs::read_to_string(path).context("failed to read hosts config")?;
    Ok(serde_json::from_str(&raw).context("failed to parse hosts config")?)
}

pub fn save_config(config: &AppConfig) -> Result<()> {
    ensure_config_dir()?;
    let raw = serde_json::to_string_pretty(config)?;
    fs::write(config_path()?, raw).context("failed to write hosts config")
}

pub fn append_audit(result: &ExecResult, risk_level: RiskLevel) -> Result<()> {
    use chrono::Utc;
    use uuid::Uuid;

    ensure_config_dir()?;
    rotate_audit_if_needed(10 * 1024 * 1024)?; // 10 MB default
    let entry = AuditEntry {
        id: Uuid::new_v4(),
        ts: Utc::now(),
        host: result.host.clone(),
        command: redact_sensitive_text(&result.command),
        exit_code: result.exit_code,
        duration_ms: result.duration_ms,
        risk_level,
    };
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(audit_path()?)
        .context("failed to open audit log")?;
    writeln!(file, "{}", serde_json::to_string(&entry)?)?;
    Ok(())
}

/// Rotate audit log if it exceeds `max_size_bytes`.
///
/// Keeps at most 3 rotated files: `audit.jsonl.1`, `.2`, `.3`.
/// When rotating, `.2` → `.3`, `.1` → `.2`, current → `.1`.
pub fn rotate_audit_if_needed(max_size_bytes: u64) -> Result<()> {
    let path = audit_path()?;
    if !path.exists() {
        return Ok(());
    }
    let metadata = std::fs::metadata(&path)?;
    if metadata.len() <= max_size_bytes {
        return Ok(());
    }

    // Shift existing rotations: .2 → .3, .1 → .2
    for i in (2..=3).rev() {
        let src = path.with_extension(format!("jsonl.{}", i - 1));
        let dst = path.with_extension(format!("jsonl.{i}"));
        if src.exists() {
            if dst.exists() {
                std::fs::remove_file(&dst)?;
            }
            std::fs::rename(&src, &dst)?;
        }
    }
    // Remove .3 if it would be pushed beyond limit (we only keep 3 rotations)
    let overflow = path.with_extension("jsonl.4");
    if overflow.exists() {
        let _ = std::fs::remove_file(&overflow);
    }

    // Current → .1
    let rotated = path.with_extension("jsonl.1");
    if rotated.exists() {
        std::fs::remove_file(&rotated)?;
    }
    std::fs::rename(&path, &rotated)?;
    Ok(())
}

/// Public wrapper for CLI/daemon invocation of audit rotation with the default 10 MB limit.
pub fn rotate_audit_core() -> Result<()> {
    rotate_audit_if_needed(10 * 1024 * 1024)
}

pub fn redact_sensitive_text(input: &str) -> String {
    let mut out = Vec::new();
    let mut redact_next = false;
    for token in input.split_whitespace() {
        let lower = token.to_ascii_lowercase();
        if redact_next {
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
        out.push(token.to_string());
    }
    out.join(" ")
}

fn is_sensitive_key(key: &str) -> bool {
    matches!(
        key.trim_start_matches('-'),
        "password" | "passwd" | "token" | "secret" | "api-key" | "apikey" | "access-token"
    )
}

pub fn list_audit_raw(filter: &AuditFilter) -> Result<Vec<AuditEntry>> {
    ensure_config_dir()?;
    let path = audit_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path).context("failed to read audit log")?;

    let since = filter.since.as_deref().and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());
    let until = filter.until.as_deref().and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());

    let mut entries: Vec<AuditEntry> = raw
        .lines()
        .filter_map(|line| serde_json::from_str::<AuditEntry>(line).ok())
        .filter(|e| {
            if let Some(h) = &filter.host {
                if !e.host.eq_ignore_ascii_case(h) { return false; }
            }
            if let Some(r) = filter.risk_level {
                if e.risk_level != r { return false; }
            }
            if let Some(code) = filter.exit_code {
                if e.exit_code != Some(code) { return false; }
            }
            if let Some(since) = since {
                if e.ts < since { return false; }
            }
            if let Some(until) = until {
                if e.ts > until { return false; }
            }
            true
        })
        .collect();

    entries.reverse();
    entries.truncate(filter.limit);
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_redact_sensitive_text() {
        let redacted = redact_sensitive_text(
            "deploy --token abc password=hunter2 --api-key key123 --safe value",
        );
        assert_eq!(
            redacted,
            "deploy --token [REDACTED] password=[REDACTED] --api-key [REDACTED] --safe value"
        );
    }
}
