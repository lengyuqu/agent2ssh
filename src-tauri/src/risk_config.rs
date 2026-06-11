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
    let path = risk_rules_path();
    let mut c = cache().lock().await;

    let current_modified = std::fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok());

    // Return cached if file hasn't changed
    if c.modified.is_some() && c.modified == current_modified {
        return Ok(c.rules.clone());
    }

    if !path.exists() {
        c.rules = RiskRules::default();
        c.modified = None;
        return Ok(c.rules.clone());
    }

    let raw = std::fs::read_to_string(&path)?;
    let rules: RiskRules = toml::from_str(&raw)?;
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
