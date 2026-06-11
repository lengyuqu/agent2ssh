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

fn audit_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("audit.jsonl"))
}

pub fn ensure_config_dir() -> Result<()> {
    fs::create_dir_all(config_dir()?).context("failed to create ~/.agent2ssh")
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
    let entry = AuditEntry {
        id: Uuid::new_v4(),
        ts: Utc::now(),
        host: result.host.clone(),
        command: result.command.clone(),
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
