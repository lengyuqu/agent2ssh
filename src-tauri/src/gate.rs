use crate::store::{config_dir, ensure_config_dir, restrict_file_to_owner};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionGateMode {
    Active,
    Paused,
}

impl std::fmt::Display for ExecutionGateMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionGateMode::Active => write!(f, "active"),
            ExecutionGateMode::Paused => write!(f, "paused"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionGateStatus {
    pub mode: ExecutionGateMode,
    pub updated_at: Option<DateTime<Utc>>,
    pub updated_by: Option<String>,
    pub reason: Option<String>,
}

impl Default for ExecutionGateStatus {
    fn default() -> Self {
        Self {
            mode: ExecutionGateMode::Active,
            updated_at: None,
            updated_by: None,
            reason: None,
        }
    }
}

fn gate_path() -> Result<std::path::PathBuf> {
    Ok(config_dir()?.join("execution_gate.json"))
}

pub fn load_execution_gate() -> Result<ExecutionGateStatus> {
    let path = gate_path()?;
    if !path.exists() {
        return Ok(ExecutionGateStatus::default());
    }
    let raw = fs::read_to_string(&path)?;
    let status = serde_json::from_str(&raw)?;
    Ok(status)
}

pub fn save_execution_gate(
    mode: ExecutionGateMode,
    source: Option<String>,
    reason: Option<String>,
) -> Result<ExecutionGateStatus> {
    ensure_config_dir()?;
    let status = ExecutionGateStatus {
        mode,
        updated_at: Some(Utc::now()),
        updated_by: source,
        reason,
    };
    let path = gate_path()?;
    fs::write(&path, serde_json::to_string_pretty(&status)?)?;
    restrict_file_to_owner(&path)?;
    Ok(status)
}

pub fn source_can_bypass_gate(source: &str) -> bool {
    source.trim().eq_ignore_ascii_case("desktop")
}

pub fn gate_blocks_source(status: &ExecutionGateStatus, source: &str) -> bool {
    status.mode == ExecutionGateMode::Paused && !source_can_bypass_gate(source)
}

pub fn execution_gate_blocks_source(source: &str) -> Result<bool> {
    Ok(gate_blocks_source(&load_execution_gate()?, source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_gate_is_active() {
        let status = ExecutionGateStatus::default();
        assert_eq!(status.mode, ExecutionGateMode::Active);
        assert!(!gate_blocks_source(&status, "mcp"));
    }

    #[test]
    fn paused_gate_blocks_non_desktop_sources() {
        let status = ExecutionGateStatus {
            mode: ExecutionGateMode::Paused,
            updated_at: None,
            updated_by: Some("cli".into()),
            reason: Some("maintenance".into()),
        };
        assert!(gate_blocks_source(&status, "cli"));
        assert!(gate_blocks_source(&status, "mcp"));
        assert!(!gate_blocks_source(&status, "desktop"));
    }
}
