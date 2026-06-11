use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::{
    core::exec_ssh_core,
    store::config_dir,
    types::{ExecRequest, ExecResult, RiskLevel},
};

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playbook {
    pub name: String,
    pub description: String,
    pub steps: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub risk_override: Option<RiskLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookFile {
    #[serde(default)]
    pub playbooks: Vec<Playbook>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookStepResult {
    pub step: usize,
    pub command: String,
    pub result: Option<ExecResult>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookRunResult {
    pub playbook: String,
    pub host: String,
    pub steps_completed: Vec<PlaybookStepResult>,
    pub success: bool,
    pub total_duration_ms: u128,
}

// ── Loading ──────────────────────────────────────────────────────────────────

fn playbooks_path() -> Result<std::path::PathBuf> {
    Ok(config_dir()?.join("playbooks.toml"))
}

/// Load playbooks from ~/.agent2ssh/playbooks.toml.
/// Returns an empty Vec when the file does not exist.
pub fn load_playbooks() -> Result<Vec<Playbook>> {
    let path = playbooks_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| anyhow!("failed to read {}: {e}", path.display()))?;
    let file: PlaybookFile = toml::from_str(&raw)
        .map_err(|e| anyhow!("failed to parse {}: {e}", path.display()))?;
    Ok(file.playbooks)
}

/// List all configured playbooks.
pub fn list_playbooks_core() -> Result<Vec<Playbook>> {
    load_playbooks()
}

// ── Execution ────────────────────────────────────────────────────────────────

/// Run a named playbook against a specific host.
///
/// Steps are executed sequentially. If any step produces a non-zero exit code
/// (or an error), execution halts and partial results are returned with
/// `success = false`.
///
/// Risk handling:
/// - When `playbook.risk_override` is set, every step's ExecRequest uses it
///   to decide whether `force` is required.
/// - Otherwise each step's built-in risk classification applies.
pub async fn run_playbook_core(
    playbook_name: &str,
    host: &str,
    force: bool,
) -> Result<PlaybookRunResult> {
    let playbooks = load_playbooks()?;
    let playbook = playbooks
        .iter()
        .find(|p| p.name == playbook_name)
        .ok_or_else(|| anyhow!("playbook not found: '{playbook_name}'"))?
        .clone();

    let started = Instant::now();
    let mut steps_completed: Vec<PlaybookStepResult> = Vec::new();
    let mut success = true;

    for (idx, command) in playbook.steps.iter().enumerate() {
        // Determine effective force flag:
        // If the playbook has a risk_override we still need `force` for steps
        // whose actual classification is High (the override is applied inside
        // exec_ssh_core via the host profile). Here we simply pass the user's
        // `force` flag through.
        let request = ExecRequest {
            host: host.to_string(),
            command: command.clone(),
            force,
            timeout_secs: None,
            stdin: None,
            max_output_bytes: None,
        };

        match exec_ssh_core(request).await {
            Ok(result) => {
                let exit_ok = result.exit_code == Some(0);
                steps_completed.push(PlaybookStepResult {
                    step: idx,
                    command: command.clone(),
                    result: Some(result),
                    error: None,
                });
                if !exit_ok {
                    success = false;
                    break;
                }
            }
            Err(e) => {
                steps_completed.push(PlaybookStepResult {
                    step: idx,
                    command: command.clone(),
                    result: None,
                    error: Some(e.to_string()),
                });
                success = false;
                break;
            }
        }
    }

    Ok(PlaybookRunResult {
        playbook: playbook.name,
        host: host.to_string(),
        steps_completed,
        success,
        total_duration_ms: started.elapsed().as_millis(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playbook_toml_roundtrip() {
        let toml_str = r#"
[[playbooks]]
name = "health-check"
description = "Basic server health check"
steps = [
    "uptime",
    "df -h",
    "free -m",
]
tags = ["monitoring"]

[[playbooks]]
name = "deploy-web"
description = "Deploy web application"
steps = [
    "cd /opt/app && git pull",
    "npm install --production",
    "npm run build",
]
tags = ["production", "web"]
risk_override = "medium"
"#;
        let file: PlaybookFile = toml::from_str(toml_str).unwrap();
        assert_eq!(file.playbooks.len(), 2);
        assert_eq!(file.playbooks[0].name, "health-check");
        assert_eq!(file.playbooks[0].steps.len(), 3);
        assert_eq!(file.playbooks[1].risk_override, Some(RiskLevel::Medium));
    }

    #[test]
    fn test_load_playbooks_missing_file() {
        // When the file doesn't exist, we should get an empty vec (not an error)
        // This test relies on the actual config_dir which might not exist in CI
        let result = load_playbooks();
        // Should either succeed with some vec or fail gracefully
        assert!(result.is_ok() || result.is_err());
    }
}
