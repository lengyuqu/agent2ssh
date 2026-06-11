use anyhow::{Context, Result};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

use crate::types::{AppConfig, AuditEntry, ExecResult};

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

pub fn append_audit(result: &ExecResult) -> Result<()> {
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
    };
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(audit_path()?)
        .context("failed to open audit log")?;
    writeln!(file, "{}", serde_json::to_string(&entry)?)?;
    Ok(())
}

pub fn list_audit_raw(limit: usize) -> Result<Vec<AuditEntry>> {
    ensure_config_dir()?;
    let path = audit_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path).context("failed to read audit log")?;
    let mut entries = raw
        .lines()
        .filter_map(|line| serde_json::from_str::<AuditEntry>(line).ok())
        .collect::<Vec<_>>();
    entries.reverse();
    entries.truncate(limit);
    Ok(entries)
}
