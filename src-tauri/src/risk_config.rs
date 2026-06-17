use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::OnceLock,
    time::SystemTime,
};
use tokio::sync::Mutex;

use crate::types::RiskLevel;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RiskRules {
    #[serde(default)]
    pub blocked: RuleGroup,
    #[serde(default)]
    pub high: RuleGroup,
    #[serde(default)]
    pub medium: RuleGroup,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleGroup {
    #[serde(default)]
    pub patterns: Vec<String>,
}

struct CachedRules {
    rules: RiskRules,
    modified: Option<SystemTime>,
}

static RULES_CACHE: OnceLock<Mutex<CachedRules>> = OnceLock::new();

fn cache() -> &'static Mutex<CachedRules> {
    RULES_CACHE.get_or_init(|| {
        Mutex::new(CachedRules {
            rules: RiskRules::default(),
            modified: None,
        })
    })
}

pub fn risk_rules_path() -> PathBuf {
    crate::store::config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("risk_rules.toml")
}

/// Load risk rules from disk, with file modification time cache.
pub async fn load_risk_rules() -> Result<RiskRules> {
    let policy_path = crate::policy::existing_policy_path()?;
    let path = policy_path.clone().unwrap_or_else(risk_rules_path);
    let mut c = cache().lock().await;

    let current_modified = std::fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok());

    // Return cached if file hasn't changed
    if c.modified.is_some() && c.modified == current_modified {
        return Ok(c.rules.clone());
    }

    if policy_path.is_none() && !path.exists() {
        c.rules = RiskRules::default();
        c.modified = None;
        return Ok(c.rules.clone());
    }

    let rules = if policy_path.is_some() {
        crate::policy::load_policy_from_path(&path)?.risk
    } else {
        let raw = std::fs::read_to_string(&path)?;
        toml::from_str(&raw)?
    };
    c.rules = rules.clone();
    c.modified = current_modified;
    Ok(rules)
}

/// Check if a command matches any user-defined rules.
/// Returns Some(RiskLevel) if a rule matched, None if no user rule matched.
pub async fn classify_with_user_rules(command: &str) -> Option<RiskLevel> {
    let rules = load_risk_rules().await.ok()?;
    let lower = command.trim().to_lowercase();

    if rules.blocked.patterns.iter().any(|p| matches_pattern(&lower, p)) {
        return Some(RiskLevel::Blocked);
    }
    if rules.high.patterns.iter().any(|p| matches_pattern(&lower, p)) {
        return Some(RiskLevel::High);
    }
    if rules.medium.patterns.iter().any(|p| matches_pattern(&lower, p)) {
        return Some(RiskLevel::Medium);
    }
    None
}

/// Merge a user-defined risk rule with the built-in classifier.
///
/// User rules can escalate risk, but they must not lower the built-in
/// classifier's severity. Trusted risk downgrades are handled separately by
/// explicit host/playbook risk overrides.
pub fn merge_user_risk(built_in: RiskLevel, user_risk: RiskLevel) -> RiskLevel {
    built_in.max_severity(user_risk)
}

pub async fn classify_effective_risk(command: &str, built_in: RiskLevel) -> RiskLevel {
    classify_with_user_rules(command)
        .await
        .map(|user_risk| merge_user_risk(built_in, user_risk))
        .unwrap_or(built_in)
}

fn matches_pattern(command: &str, pattern: &str) -> bool {
    let pattern = pattern.trim().to_lowercase();
    if pattern.contains('*') {
        // Simple glob: split on * and check all parts appear in order
        let parts: Vec<&str> = pattern.split('*').collect();
        let mut pos = 0;
        for part in parts {
            if part.is_empty() {
                continue;
            }
            match command[pos..].find(part) {
                Some(idx) => pos += idx + part.len(),
                None => return false,
            }
        }
        true
    } else {
        command.contains(&pattern)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_pattern_exact() {
        assert!(matches_pattern("docker system prune", "docker system prune"));
        assert!(!matches_pattern("docker system prune -a", "docker system prune -af"));
    }

    #[test]
    fn test_matches_pattern_glob() {
        assert!(matches_pattern("git push --force origin main", "git push *force*"));
        assert!(matches_pattern("kubectl delete namespace kube-system", "kubectl delete*"));
        assert!(!matches_pattern("kubectl get pods", "kubectl delete*"));
    }

    #[test]
    fn test_matches_pattern_contains() {
        assert!(matches_pattern("sudo apt install nginx", "apt install"));
        assert!(matches_pattern("terraform destroy -auto-approve", "terraform destroy"));
    }

    #[test]
    fn test_matches_pattern_empty_glob_parts() {
        // "**" should match anything
        assert!(matches_pattern("anything goes here", "*"));
    }

    #[tokio::test]
    async fn test_load_risk_rules_missing_file() {
        // When no file exists, should return default (empty) rules
        let rules = load_risk_rules().await.unwrap();
        assert!(rules.blocked.patterns.is_empty());
        assert!(rules.high.patterns.is_empty());
        assert!(rules.medium.patterns.is_empty());
    }

    #[tokio::test]
    async fn test_classify_with_no_rules() {
        // With no rules file, should return None
        assert_eq!(classify_with_user_rules("ls -la").await, None);
    }

    #[test]
    fn test_merge_user_risk_does_not_downgrade_builtin() {
        assert_eq!(
            merge_user_risk(RiskLevel::High, RiskLevel::Medium),
            RiskLevel::High
        );
        assert_eq!(
            merge_user_risk(RiskLevel::Medium, RiskLevel::High),
            RiskLevel::High
        );
        assert_eq!(
            merge_user_risk(RiskLevel::Blocked, RiskLevel::High),
            RiskLevel::Blocked
        );
    }
}
