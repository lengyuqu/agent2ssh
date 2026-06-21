//! Opt-in, local-only telemetry (K10).
//!
//! Agent2SSH already has rich *local* diagnostics and metrics; what it lacked was
//! a developer-facing aggregation of crashes/usage. This module adds an
//! **opt-in** sink that is **off by default** and **never leaves the machine** —
//! when enabled, events are appended to `~/.agent2ssh/telemetry.jsonl` (a
//! size-capped local file) so a user can collect and share them deliberately.
//! There is intentionally no network exporter: enabling telemetry does not
//! exfiltrate anything; it only starts local aggregation the user can inspect,
//! attach to a bug report, or disable again.
//!
//! The toggle is surfaced in Settings; `record_event` is a no-op until the user
//! flips it on.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;

use crate::store::{config_dir, lock_config_file, restrict_file_to_owner};

const TELEMETRY_FILE: &str = "telemetry.jsonl";
const CONFIG_FILE: &str = "telemetry.toml";
/// Cap the local telemetry log so it can't grow without bound.
const MAX_TELEMETRY_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelemetryConfig {
    /// Off by default. When false, [`record_event`] does nothing.
    #[serde(default)]
    pub enabled: bool,
}

fn config_file_path() -> Result<std::path::PathBuf> {
    Ok(config_dir()?.join(CONFIG_FILE))
}

/// Load the telemetry config, defaulting to disabled when the file is absent.
pub fn load_telemetry_config() -> Result<TelemetryConfig> {
    let path = config_file_path()?;
    if !path.exists() {
        return Ok(TelemetryConfig::default());
    }
    let raw = std::fs::read_to_string(&path).context("failed to read telemetry config")?;
    Ok(toml::from_str(&raw).unwrap_or_default())
}

/// Persist the telemetry opt-in setting.
pub fn save_telemetry_config(enabled: bool) -> Result<()> {
    crate::store::ensure_config_dir()?;
    let _guard = lock_config_file(".telemetry.lock")?;
    let path = config_file_path()?;
    let raw = toml::to_string_pretty(&TelemetryConfig { enabled })
        .context("failed to serialize telemetry config")?;
    std::fs::write(&path, raw).context("failed to write telemetry config")?;
    restrict_file_to_owner(&path)?;
    Ok(())
}

/// Whether telemetry is currently enabled.
pub fn telemetry_enabled() -> bool {
    load_telemetry_config().map(|c| c.enabled).unwrap_or(false)
}

/// Append a telemetry event when enabled; otherwise a no-op. Best-effort: a
/// telemetry write failure never propagates to the caller's operation.
pub fn record_event(kind: &str, data: serde_json::Value) {
    if !telemetry_enabled() {
        return;
    }
    if let Err(e) = record_event_inner(kind, data) {
        eprintln!("warning: telemetry record failed: {e}");
    }
}

fn record_event_inner(kind: &str, data: serde_json::Value) -> Result<()> {
    crate::store::ensure_config_dir()?;
    let _guard = lock_config_file(".telemetry.lock")?;
    let path = config_dir()?.join(TELEMETRY_FILE);
    // Drop the log if it has grown past the cap (keep it bounded; this is local
    // aggregation, not a durable audit trail).
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > MAX_TELEMETRY_BYTES {
            let _ = std::fs::remove_file(&path);
        }
    }
    let entry = serde_json::json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "kind": kind,
        "data": data,
    });
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .context("failed to open telemetry log")?;
    writeln!(file, "{}", serde_json::to_string(&entry)?)?;
    restrict_file_to_owner(&path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn disabled_by_default_and_toggleable() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-tele-{}", uuid::Uuid::new_v4()));
        std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);

        // Default: off, and recording is a no-op (no file created).
        assert!(!telemetry_enabled());
        record_event("test", serde_json::json!({"a": 1}));
        assert!(!dir.join(TELEMETRY_FILE).exists());

        // Opt in.
        save_telemetry_config(true).unwrap();
        assert!(telemetry_enabled());
        record_event("test", serde_json::json!({"a": 2}));
        let log = std::fs::read_to_string(dir.join(TELEMETRY_FILE)).unwrap();
        assert!(log.contains("\"kind\":\"test\""), "event written: {log}");

        // Opt back out: recording stops.
        save_telemetry_config(false).unwrap();
        assert!(!telemetry_enabled());

        std::env::remove_var("AGENT2SSH_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
