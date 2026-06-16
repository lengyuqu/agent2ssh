use crate::approval::{ApprovalPolicy, ApprovalPolicyFile};
use crate::risk_config::RiskRules;
use crate::store::config_dir;
use crate::types::RiskLevel;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentPolicyFile {
    #[serde(default)]
    pub risk: RiskRules,
    #[serde(default)]
    pub approval: ApprovalPolicyFile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyDecision {
    Allow,
    Approve,
    Block,
}

impl std::fmt::Display for PolicyDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyDecision::Allow => write!(f, "allow"),
            PolicyDecision::Approve => write!(f, "approve"),
            PolicyDecision::Block => write!(f, "block"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyTestResult {
    pub command: String,
    pub host: String,
    pub risk_level: RiskLevel,
    pub decision: PolicyDecision,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_approval_policy: Option<String>,
    pub matched_user_rule: bool,
}

pub fn policy_toml_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("policy.toml"))
}

pub fn policy_json_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("policy.json"))
}

pub fn existing_policy_path() -> Result<Option<PathBuf>> {
    let toml_path = policy_toml_path()?;
    if toml_path.exists() {
        return Ok(Some(toml_path));
    }
    let json_path = policy_json_path()?;
    if json_path.exists() {
        return Ok(Some(json_path));
    }
    Ok(None)
}

pub fn load_policy_file() -> Result<Option<AgentPolicyFile>> {
    existing_policy_path()?
        .map(|path| load_policy_from_path(&path))
        .transpose()
}

pub fn load_policy_from_path(path: &Path) -> Result<AgentPolicyFile> {
    let raw = std::fs::read_to_string(path)?;
    parse_policy(path, &raw)
}

pub fn parse_policy(path: &Path, raw: &str) -> Result<AgentPolicyFile> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => Ok(serde_json::from_str(raw)?),
        Some("toml") | None => Ok(toml::from_str(raw)?),
        Some(ext) => Err(anyhow!("unsupported policy file extension: {ext}")),
    }
}

pub fn validate_policy_path(path: Option<&Path>) -> Result<AgentPolicyFile> {
    match path {
        Some(path) => load_policy_from_path(path),
        None => load_policy_file()?.ok_or_else(|| {
            anyhow!(
                "no policy.toml or policy.json found in {}",
                config_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .display()
            )
        }),
    }
}

pub fn policy_risk_rules() -> Result<Option<RiskRules>> {
    Ok(load_policy_file()?.map(|policy| policy.risk))
}

pub fn policy_approval_policies() -> Result<Option<Vec<ApprovalPolicy>>> {
    Ok(load_policy_file()?.map(|policy| policy.approval.policies))
}

pub fn save_policy_approval_policies(policies: &[ApprovalPolicy]) -> Result<bool> {
    let Some(path) = existing_policy_path()? else {
        return Ok(false);
    };
    let mut policy = load_policy_from_path(&path)?;
    policy.approval.policies = policies.to_vec();
    save_policy_to_path(&path, &policy)?;
    Ok(true)
}

fn save_policy_to_path(path: &Path, policy: &AgentPolicyFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => serde_json::to_string_pretty(policy)?,
        Some("toml") | None => toml::to_string_pretty(policy)?,
        Some(ext) => return Err(anyhow!("unsupported policy file extension: {ext}")),
    };
    std::fs::write(path, raw)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_toml_policy_with_risk_and_approval() {
        let raw = r#"
[risk.blocked]
patterns = ["terraform destroy*"]

[[approval.policies]]
name = "prod sudo"
tags = ["prod"]
min_risk = "high"
requires_approval = true
"#;
        let policy = parse_policy(Path::new("policy.toml"), raw).unwrap();
        assert_eq!(
            policy.risk.blocked.patterns,
            vec!["terraform destroy*".to_string()]
        );
        assert_eq!(policy.approval.policies[0].name, "prod sudo");
    }

    #[test]
    fn parse_json_policy() {
        let raw = r#"{"risk":{"medium":{"patterns":["apt install*"]}},"approval":{"policies":[]}}"#;
        let policy = parse_policy(Path::new("policy.json"), raw).unwrap();
        assert_eq!(
            policy.risk.medium.patterns,
            vec!["apt install*".to_string()]
        );
    }
}
