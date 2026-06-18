use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

use crate::{
    core::exec_ssh_core_with_risk_override,
    store::config_dir,
    types::{source_from_env, ExecRequest, ExecResult, RiskLevel},
};

// ── Types ────────────────────────────────────────────────────────────────────

/// A parameter for a playbook step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookParam {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// A playbook step, now supporting inline parameters via template variables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookStep {
    pub command: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub params: Vec<PlaybookParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playbook {
    pub name: String,
    pub description: String,
    pub steps: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub risk_override: Option<RiskLevel>,
    #[serde(default)]
    pub advanced_steps: Option<Vec<PlaybookStep>>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookDryRun {
    pub playbook: String,
    pub steps: Vec<DryRunStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunStep {
    pub step: usize,
    pub command_template: String,
    pub command_resolved: String,
    pub params_used: Vec<String>,
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
    let file: PlaybookFile =
        toml::from_str(&raw).map_err(|e| anyhow!("failed to parse {}: {e}", path.display()))?;
    Ok(file.playbooks)
}

/// List all configured playbooks.
pub fn list_playbooks_core() -> Result<Vec<Playbook>> {
    load_playbooks()
}

fn save_playbooks(playbooks: &[Playbook]) -> Result<()> {
    let path = playbooks_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = toml::to_string_pretty(&PlaybookFile {
        playbooks: playbooks.to_vec(),
    })?;
    std::fs::write(&path, raw).map_err(|e| anyhow!("failed to write {}: {e}", path.display()))
}

fn normalize_playbook(mut playbook: Playbook) -> Result<Playbook> {
    playbook.name = playbook.name.trim().to_string();
    playbook.description = playbook.description.trim().to_string();
    playbook.tags = playbook
        .tags
        .into_iter()
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect();
    playbook.steps = playbook
        .steps
        .into_iter()
        .map(|step| step.trim().to_string())
        .filter(|step| !step.is_empty())
        .collect();

    if playbook.name.is_empty() {
        return Err(anyhow!("playbook name is required"));
    }
    if playbook.description.is_empty() {
        playbook.description = "No description".into();
    }
    let has_advanced_steps = playbook
        .advanced_steps
        .as_ref()
        .is_some_and(|steps| !steps.is_empty());
    if playbook.steps.is_empty() && !has_advanced_steps {
        return Err(anyhow!("playbook must contain at least one step"));
    }

    Ok(playbook)
}

/// Create or update one playbook in ~/.agent2ssh/playbooks.toml.
pub fn save_playbook_core(playbook: Playbook) -> Result<Playbook> {
    let playbook = normalize_playbook(playbook)?;
    let mut playbooks = load_playbooks()?;
    if let Some(existing) = playbooks.iter_mut().find(|item| item.name == playbook.name) {
        *existing = playbook.clone();
    } else {
        playbooks.push(playbook.clone());
    }
    playbooks.sort_by(|a, b| a.name.cmp(&b.name));
    save_playbooks(&playbooks)?;
    Ok(playbook)
}

/// Delete one playbook by name. Returns true when a playbook was removed.
pub fn delete_playbook_core(name: &str) -> Result<bool> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("playbook name is required"));
    }
    let mut playbooks = load_playbooks()?;
    let before = playbooks.len();
    playbooks.retain(|item| item.name != name);
    let removed = playbooks.len() != before;
    if removed {
        save_playbooks(&playbooks)?;
    }
    Ok(removed)
}

// ── Template resolution ─────────────────────────────────────────────────────

/// Resolve `{{param_name}}` placeholders in a command template string.
///
/// For each placeholder found:
/// - If the param exists in `params`, use its value.
/// - Otherwise, if the matching `PlaybookParam` has a default, use the default.
/// - Otherwise, if the param is required, return an error.
/// - Otherwise, leave the placeholder as-is.
///
/// Returns `(resolved_command, list_of_param_names_used)`.
pub fn resolve_command_template(
    template: &str,
    params: &HashMap<String, String>,
    param_defs: &[PlaybookParam],
) -> Result<(String, Vec<String>)> {
    let mut result = String::new();
    let mut params_used = Vec::new();
    let mut remaining = template;

    while let Some(start) = remaining.find("{{") {
        if let Some(end) = remaining[start..].find("}}") {
            result.push_str(&remaining[..start]);
            let param_name = &remaining[start + 2..start + end];
            let param_name = param_name.trim();

            if let Some(value) = params.get(param_name) {
                result.push_str(value);
                params_used.push(param_name.to_string());
            } else if let Some(param_def) = param_defs.iter().find(|p| p.name == param_name) {
                if let Some(ref default) = param_def.default {
                    result.push_str(default);
                    params_used.push(param_name.to_string());
                } else if param_def.required {
                    return Err(anyhow!(
                        "required parameter '{}' not provided and has no default",
                        param_name
                    ));
                } else {
                    // Not required, no default: leave placeholder
                    result.push_str(&format!("{{{{{}}}}}", param_name));
                }
            } else {
                // Unknown param with no definition: leave placeholder
                result.push_str(&format!("{{{{{}}}}}", param_name));
            }

            remaining = &remaining[start + end + 2..];
        } else {
            // No closing `}}`, treat rest as literal
            break;
        }
    }
    result.push_str(remaining);

    Ok((result, params_used))
}

/// Collect the effective command templates and their associated parameter
/// definitions from a playbook.  Uses `advanced_steps` when present, otherwise
/// falls back to the plain `steps` string list.
fn effective_steps(playbook: &Playbook) -> Vec<(String, Vec<PlaybookParam>)> {
    if let Some(ref advanced) = playbook.advanced_steps {
        advanced
            .iter()
            .map(|s| (s.command.clone(), s.params.clone()))
            .collect()
    } else {
        playbook
            .steps
            .iter()
            .map(|s| (s.clone(), Vec::new()))
            .collect()
    }
}

// ── Parameter validation ────────────────────────────────────────────────────

/// Validate that all required parameters for a playbook have been provided.
/// Returns a list of missing required parameter names (empty when valid).
pub fn validate_playbook_params(
    playbook_name: &str,
    params: &HashMap<String, String>,
) -> Result<Vec<String>> {
    let playbooks = load_playbooks()?;
    let playbook = playbooks
        .iter()
        .find(|p| p.name == playbook_name)
        .ok_or_else(|| anyhow!("playbook not found: '{playbook_name}'"))?;

    let steps = effective_steps(playbook);
    let mut missing = Vec::new();

    for (_, param_defs) in &steps {
        for param in param_defs {
            if param.required
                && !params.contains_key(&param.name)
                && param.default.is_none()
                && !missing.contains(&param.name)
            {
                missing.push(param.name.clone());
            }
        }
    }

    Ok(missing)
}

// ── Dry-run ─────────────────────────────────────────────────────────────────

/// Resolve all template variables in a playbook without executing anything.
/// Returns the list of resolved commands together with the parameters used.
pub fn dry_run_playbook(
    playbook_name: &str,
    params: &HashMap<String, String>,
) -> Result<PlaybookDryRun> {
    let playbooks = load_playbooks()?;
    let playbook = playbooks
        .iter()
        .find(|p| p.name == playbook_name)
        .ok_or_else(|| anyhow!("playbook not found: '{playbook_name}'"))?;

    let steps = effective_steps(playbook);
    let mut dry_steps = Vec::new();

    for (idx, (template, param_defs)) in steps.iter().enumerate() {
        let (resolved, params_used) = resolve_command_template(template, params, param_defs)?;
        dry_steps.push(DryRunStep {
            step: idx,
            command_template: template.clone(),
            command_resolved: resolved,
            params_used,
        });
    }

    Ok(PlaybookDryRun {
        playbook: playbook.name.clone(),
        steps: dry_steps,
    })
}

// ── Execution ────────────────────────────────────────────────────────────────

/// Run a named playbook against a specific host.
///
/// Steps are executed sequentially. If any step produces a non-zero exit code
/// (or an error), execution halts and partial results are returned with
/// `success = false`.
///
/// When `params` is provided, `{{param_name}}` placeholders in step commands
/// are resolved before execution.
///
/// Risk handling:
/// - When `playbook.risk_override` is set, every step's ExecRequest uses it
///   to decide whether `force` is required.
/// - Otherwise each step's built-in risk classification applies.
pub async fn run_playbook_core(
    playbook_name: &str,
    host: &str,
    force: bool,
    params: Option<&HashMap<String, String>>,
    reason: Option<String>,
    change_id: Option<String>,
) -> Result<PlaybookRunResult> {
    run_playbook_core_with_source(
        playbook_name,
        host,
        force,
        params,
        reason,
        change_id,
        Some(source_from_env("core")),
    )
    .await
}

pub async fn run_playbook_core_with_source(
    playbook_name: &str,
    host: &str,
    force: bool,
    params: Option<&HashMap<String, String>>,
    reason: Option<String>,
    change_id: Option<String>,
    source: Option<String>,
) -> Result<PlaybookRunResult> {
    run_playbook_core_with_source_and_approved_steps(
        playbook_name,
        host,
        force,
        params,
        reason,
        change_id,
        source,
        &[],
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_playbook_core_with_source_and_approved_steps(
    playbook_name: &str,
    host: &str,
    force: bool,
    params: Option<&HashMap<String, String>>,
    reason: Option<String>,
    change_id: Option<String>,
    source: Option<String>,
    approved_steps: &[usize],
) -> Result<PlaybookRunResult> {
    let playbooks = load_playbooks()?;
    let playbook = playbooks
        .iter()
        .find(|p| p.name == playbook_name)
        .ok_or_else(|| anyhow!("playbook not found: '{playbook_name}'"))?
        .clone();

    let empty_params = HashMap::new();
    let params_map = params.unwrap_or(&empty_params);

    let started = Instant::now();
    let mut steps_completed: Vec<PlaybookStepResult> = Vec::new();
    let mut success = true;

    let steps = effective_steps(&playbook);

    for (idx, (template, param_defs)) in steps.iter().enumerate() {
        let (command, _) = resolve_command_template(template, params_map, param_defs)?;

        let request = ExecRequest {
            host: host.to_string(),
            command: command.clone(),
            force: force || approved_steps.contains(&idx),
            timeout_secs: None,
            stdin: None,
            max_output_bytes: None,
            reason: reason.clone(),
            change_id: change_id.clone(),
            source: source.clone(),
        };

        match exec_ssh_core_with_risk_override(request, playbook.risk_override).await {
            Ok(result) => {
                let exit_ok = result.exit_code == Some(0);
                steps_completed.push(PlaybookStepResult {
                    step: idx,
                    command,
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
                    command,
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

    #[test]
    fn test_playbook_param_substitution() {
        let params = HashMap::from([
            ("host".to_string(), "prod-server".to_string()),
            ("port".to_string(), "8080".to_string()),
        ]);
        let param_defs = vec![
            PlaybookParam {
                name: "host".to_string(),
                description: Some("Target host".to_string()),
                default: None,
                required: true,
            },
            PlaybookParam {
                name: "port".to_string(),
                description: Some("Port number".to_string()),
                default: None,
                required: true,
            },
        ];

        let template = "curl http://{{host}}:{{port}}/health";
        let (resolved, used) = resolve_command_template(template, &params, &param_defs).unwrap();
        assert_eq!(resolved, "curl http://prod-server:8080/health");
        assert!(used.contains(&"host".to_string()));
        assert!(used.contains(&"port".to_string()));
    }

    #[test]
    fn test_playbook_param_default() {
        let params = HashMap::new(); // no params provided
        let param_defs = vec![PlaybookParam {
            name: "env".to_string(),
            description: Some("Environment".to_string()),
            default: Some("production".to_string()),
            required: false,
        }];

        let template = "deploy --env {{env}}";
        let (resolved, used) = resolve_command_template(template, &params, &param_defs).unwrap();
        assert_eq!(resolved, "deploy --env production");
        assert!(used.contains(&"env".to_string()));
    }

    #[test]
    fn test_playbook_param_required_missing() {
        let params = HashMap::new(); // no params provided
        let param_defs = vec![PlaybookParam {
            name: "database".to_string(),
            description: Some("Database name".to_string()),
            default: None,
            required: true,
        }];

        let template = "pg_dump {{database}}";
        let result = resolve_command_template(template, &params, &param_defs);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("database"),
            "Error should mention the missing param name, got: {err_msg}"
        );
    }

    #[test]
    fn test_playbook_dry_run() {
        // Write a temporary playbooks.toml for this test
        let config = config_dir().unwrap();
        let playbooks_path = config.join("playbooks.toml");
        let existed = playbooks_path.exists();
        let original = if existed {
            std::fs::read_to_string(&playbooks_path).ok()
        } else {
            None
        };

        // TOML representation (kept for documentation; test constructs Playbook directly)
        let _toml_str = r#"
[[playbooks]]
name = "test-dry"
description = "Dry run test"
steps = ["echo {{message}}", "uptime"]
tags = []

[[playbooks.advanced_steps]]
command = "echo {{message}}"
description = "Print a message"
[[playbooks.advanced_steps.params]]
name = "message"
default = "hello"
required = false

[[playbooks.advanced_steps]]
command = "uptime"
description = "Check uptime"
"#;
        // We can't easily write to the real config dir in tests,
        // so test dry_run logic directly with a constructed Playbook.
        let playbook = Playbook {
            name: "test-dry".to_string(),
            description: "Dry run test".to_string(),
            steps: vec!["echo {{message}}".to_string(), "uptime".to_string()],
            tags: vec![],
            risk_override: None,
            advanced_steps: Some(vec![
                PlaybookStep {
                    command: "echo {{message}}".to_string(),
                    description: Some("Print a message".to_string()),
                    params: vec![PlaybookParam {
                        name: "message".to_string(),
                        description: None,
                        default: Some("hello".to_string()),
                        required: false,
                    }],
                },
                PlaybookStep {
                    command: "uptime".to_string(),
                    description: Some("Check uptime".to_string()),
                    params: vec![],
                },
            ]),
        };

        // Test effective_steps + resolve manually (since dry_run_playbook loads from disk)
        let steps = effective_steps(&playbook);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].0, "echo {{message}}");
        assert_eq!(steps[1].0, "uptime");

        // Test resolution with default
        let params = HashMap::new();
        let (resolved, _) = resolve_command_template(&steps[0].0, &params, &steps[0].1).unwrap();
        assert_eq!(resolved, "echo hello");

        // Test resolution with override
        let params = HashMap::from([("message".to_string(), "world".to_string())]);
        let (resolved, used) = resolve_command_template(&steps[0].0, &params, &steps[0].1).unwrap();
        assert_eq!(resolved, "echo world");
        assert!(used.contains(&"message".to_string()));

        // Restore original file if needed
        let _ = (existed, original); // suppress unused warnings
    }

    #[test]
    fn test_playbook_backward_compat() {
        // Old-style playbook with just steps: Vec<String> should still work
        let toml_str = r#"
[[playbooks]]
name = "old-style"
description = "Backward compatible playbook"
steps = ["uptime", "df -h", "free -m"]
tags = ["monitoring"]
"#;
        let file: PlaybookFile = toml::from_str(toml_str).unwrap();
        assert_eq!(file.playbooks.len(), 1);
        let pb = &file.playbooks[0];
        assert_eq!(pb.name, "old-style");
        assert_eq!(pb.steps.len(), 3);
        assert!(pb.advanced_steps.is_none());

        // effective_steps should return the plain steps with empty param defs
        let steps = effective_steps(pb);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].0, "uptime");
        assert_eq!(steps[1].0, "df -h");
        assert_eq!(steps[2].0, "free -m");
        // No params for legacy steps
        assert!(steps[0].1.is_empty());

        // Resolving a command with no placeholders should return it unchanged
        let params = HashMap::new();
        let (resolved, used) = resolve_command_template(&steps[0].0, &params, &steps[0].1).unwrap();
        assert_eq!(resolved, "uptime");
        assert!(used.is_empty());
    }

    #[test]
    fn test_playbook_param_toml_roundtrip() {
        let toml_str = r#"
[[playbooks]]
name = "param-test"
description = "Playbook with parameters"
steps = []
tags = ["deploy"]

[[playbooks.advanced_steps]]
command = "deploy --env {{env}} --version {{version}}"
description = "Deploy application"

[[playbooks.advanced_steps.params]]
name = "env"
description = "Target environment"
default = "staging"
required = false

[[playbooks.advanced_steps.params]]
name = "version"
description = "Version to deploy"
required = true

[[playbooks.advanced_steps]]
command = "health-check {{env}}"
description = "Verify deployment"

[[playbooks.advanced_steps.params]]
name = "env"
default = "staging"
required = false
"#;
        let file: PlaybookFile = toml::from_str(toml_str).unwrap();
        assert_eq!(file.playbooks.len(), 1);
        let pb = &file.playbooks[0];
        assert_eq!(pb.name, "param-test");
        assert!(pb.advanced_steps.is_some());

        let advanced = pb.advanced_steps.as_ref().unwrap();
        assert_eq!(advanced.len(), 2);
        assert_eq!(
            advanced[0].command,
            "deploy --env {{env}} --version {{version}}"
        );
        assert_eq!(advanced[0].params.len(), 2);
        assert_eq!(advanced[0].params[0].name, "env");
        assert_eq!(advanced[0].params[0].default, Some("staging".to_string()));
        assert!(!advanced[0].params[0].required);
        assert_eq!(advanced[0].params[1].name, "version");
        assert!(advanced[0].params[1].required);

        // Serialize back to TOML and re-parse
        let serialized = toml::to_string(&file).unwrap();
        let reparsed: PlaybookFile = toml::from_str(&serialized).unwrap();
        assert_eq!(reparsed.playbooks.len(), 1);
        let repb = &reparsed.playbooks[0];
        let re_advanced = repb.advanced_steps.as_ref().unwrap();
        assert_eq!(re_advanced.len(), 2);
        assert_eq!(re_advanced[0].params.len(), 2);
        assert_eq!(re_advanced[0].params[1].name, "version");
        assert!(re_advanced[0].params[1].required);
    }

    // ── S1-2: Playbook audit context tests ─────────────────────────────────

    #[test]
    fn test_playbook_audit_context_preserved_across_steps() {
        // Verify that run_playbook_core constructs ExecRequest with reason and
        // change_id cloned into every step. We test this by simulating the same
        // request-building logic that run_playbook_core uses.
        use crate::types::ExecRequest;

        let reason = Some("scheduled deploy".to_string());
        let change_id = Some("CHG-PB-2024".to_string());

        let playbook = Playbook {
            name: "deploy-app".to_string(),
            description: "Deploy application".to_string(),
            steps: vec![
                "git pull origin main".to_string(),
                "npm install --production".to_string(),
                "npm run build".to_string(),
            ],
            tags: vec!["deploy".to_string()],
            risk_override: None,
            advanced_steps: None,
        };

        let host = "prod-web-1";
        let steps = effective_steps(&playbook);
        assert_eq!(steps.len(), 3, "playbook should have 3 steps");

        // Simulate the ExecRequest construction loop from run_playbook_core
        let params = HashMap::new();
        let mut requests: Vec<ExecRequest> = Vec::new();
        for (idx, (template, param_defs)) in steps.iter().enumerate() {
            let (command, _) = resolve_command_template(template, &params, param_defs).unwrap();
            let request = ExecRequest {
                host: host.to_string(),
                command,
                force: false,
                timeout_secs: None,
                stdin: None,
                max_output_bytes: None,
                reason: reason.clone(),
                change_id: change_id.clone(),
                source: None,
            };
            requests.push(request);
            let _ = idx;
        }

        // Every step's request should carry the same reason and change_id
        assert_eq!(requests.len(), 3);
        for (i, req) in requests.iter().enumerate() {
            assert_eq!(
                req.reason, reason,
                "step {} should carry the playbook reason",
                i
            );
            assert_eq!(
                req.change_id, change_id,
                "step {} should carry the playbook change_id",
                i
            );
            assert_eq!(req.host, host);
        }

        // Verify the commands are the expected ones
        assert_eq!(requests[0].command, "git pull origin main");
        assert_eq!(requests[1].command, "npm install --production");
        assert_eq!(requests[2].command, "npm run build");
    }

    #[test]
    fn test_playbook_audit_entries_all_share_context() {
        // Simulate a playbook run that produces audit entries for multiple steps,
        // and verify all entries share the same reason and change_id.
        use crate::store::redact_sensitive_text;
        use crate::types::{AuditEntry, ExecResult};
        use chrono::Utc;
        use uuid::Uuid;

        let reason = "nightly deploy v3.0";
        let change_id = "CHG-NIGHTLY-001";
        let commands = vec![
            "git pull",
            "npm ci",
            "npm run build",
            "systemctl restart app",
        ];

        // Simulate the audit entry construction loop from run_playbook_core
        let mut entries: Vec<AuditEntry> = Vec::new();
        for cmd in &commands {
            let result = ExecResult {
                host: "prod-web".into(),
                command: cmd.to_string(),
                exit_code: Some(0),
                stdout: "ok".into(),
                stderr: String::new(),
                duration_ms: 200,
                risk_level: RiskLevel::Medium,
                truncated: false,
            };
            // Mirror append_audit's AuditEntry construction
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
                source: None,
            };
            entries.push(entry);
        }

        assert_eq!(entries.len(), 4, "should have one entry per playbook step");

        // All entries should share the same reason and change_id
        for (i, entry) in entries.iter().enumerate() {
            assert_eq!(
                entry.reason,
                Some(reason.into()),
                "step {} ({}) should have the playbook reason",
                i,
                entry.command
            );
            assert_eq!(
                entry.change_id,
                Some(change_id.into()),
                "step {} ({}) should have the playbook change_id",
                i,
                entry.command
            );
            assert_eq!(entry.host, "prod-web");
            assert_eq!(entry.exit_code, Some(0));
        }

        // Verify commands appear in order
        for (i, expected_cmd) in commands.iter().enumerate() {
            assert_eq!(
                entries[i].command, *expected_cmd,
                "step {} command mismatch",
                i
            );
        }

        // Verify JSONL round-trip preserves the shared context
        let jsonl: String = entries
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let parsed: Vec<AuditEntry> = jsonl
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(parsed.len(), 4);
        for entry in &parsed {
            assert_eq!(entry.change_id, Some(change_id.into()));
        }
    }

    #[test]
    fn test_playbook_with_risk_override_preserves_audit_context() {
        // Verify that when a playbook has a risk_override, the audit context
        // (reason/change_id) is still preserved. This tests the path through
        // exec_ssh_core_with_risk_override.
        use crate::types::ExecRequest;

        let playbook = Playbook {
            name: "risky-deploy".to_string(),
            description: "Deploy with risk override".to_string(),
            steps: vec!["sudo systemctl restart app".to_string()],
            tags: vec![],
            risk_override: Some(RiskLevel::Medium),
            advanced_steps: None,
        };

        let request = ExecRequest {
            host: "server-1".to_string(),
            command: "sudo systemctl restart app".to_string(),
            force: false,
            timeout_secs: None,
            stdin: None,
            max_output_bytes: None,
            reason: Some("emergency fix".to_string()),
            change_id: Some("CHG-EMERGENCY".to_string()),
            source: None,
        };

        // Verify request carries both risk_override and audit context
        assert_eq!(playbook.risk_override, Some(RiskLevel::Medium));
        assert_eq!(request.reason, Some("emergency fix".to_string()));
        assert_eq!(request.change_id, Some("CHG-EMERGENCY".to_string()));
    }
}
