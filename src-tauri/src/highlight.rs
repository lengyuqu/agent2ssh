//! B24: Terminal highlight rules — rule-based ANSI decoration.
//!
//! Provides CRUD for regex-based highlight rules that the frontend uses to
//! decorate terminal output (e.g. ERROR in red, WARN in yellow). Rules are
//! persisted in `highlight_rules.json` under the config directory using the
//! seed-once pattern (same as `redact_rules.json`).
//!
//! # Architecture
//!
//! The backend manages rule persistence only. The actual ANSI decoration
//! happens in the frontend xterm.js decoration layer — the backend never
//! modifies terminal bytes. This avoids PTY stream corruption (resetting
//! SGR state, tearing OSC sequences) that would occur if we injected ANSI
//! codes into the byte stream.

use crate::store::config_dir;
use crate::types::HighlightRule;
use std::collections::HashSet;

/// The JSON file name for user-editable highlight rules.
const HIGHLIGHT_RULES_FILE: &str = "highlight_rules.json";

/// Errors returned by highlight rule operations.
#[derive(Debug, Clone)]
pub enum HighlightError {
    EmptyKeyword,
    NameRequired,
    NameTooLong,
    InvalidColor,
    KeywordConflict,
    NotFound,
    IoError(String),
    ParseError(String),
}

impl std::fmt::Display for HighlightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyKeyword => write!(f, "highlight_empty_keyword"),
            Self::NameRequired => write!(f, "highlight_name_required"),
            Self::NameTooLong => write!(f, "highlight_name_too_long"),
            Self::InvalidColor => write!(f, "highlight_invalid_color"),
            Self::KeywordConflict => write!(f, "highlight_keyword_conflict"),
            Self::NotFound => write!(f, "highlight_not_found"),
            Self::IoError(msg) => write!(f, "highlight_io_error: {msg}"),
            Self::ParseError(msg) => write!(f, "highlight_parse_error: {msg}"),
        }
    }
}

impl std::error::Error for HighlightError {}

/// Path to the user-editable highlight rules file.
fn highlight_rules_path() -> Option<std::path::PathBuf> {
    config_dir().ok().map(|d| d.join(HIGHLIGHT_RULES_FILE))
}

/// The hardcoded default rules. Seeded into the JSON file on first use;
/// after that the user owns the file and can edit/delete rules freely.
fn default_rules() -> Vec<HighlightRule> {
    vec![
        HighlightRule {
            keyword: "ERROR".into(),
            name: "ERROR".into(),
            color: "#FF6B6B".into(),
            enabled: true,
            is_regex: true,
            is_case_sensitive: false,
        },
        HighlightRule {
            keyword: "WARN".into(),
            name: "WARN".into(),
            color: "#FFD060".into(),
            enabled: true,
            is_regex: true,
            is_case_sensitive: false,
        },
        HighlightRule {
            keyword: "INFO".into(),
            name: "INFO".into(),
            color: "#6EDAA0".into(),
            enabled: true,
            is_regex: true,
            is_case_sensitive: false,
        },
        HighlightRule {
            keyword: "DEBUG".into(),
            name: "DEBUG".into(),
            color: "#40C8E0".into(),
            enabled: true,
            is_regex: true,
            is_case_sensitive: false,
        },
        HighlightRule {
            keyword: r"\b(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(?:\.(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}\b".into(),
            name: "IPv4".into(),
            color: "#D86BFF".into(),
            enabled: true,
            is_regex: true,
            is_case_sensitive: false,
        },
    ]
}

/// Validate a highlight rule before persistence.
fn validate_rule(rule: &HighlightRule) -> Result<(), HighlightError> {
    if rule.keyword.trim().is_empty() {
        return Err(HighlightError::EmptyKeyword);
    }
    if rule.name.trim().is_empty() {
        return Err(HighlightError::NameRequired);
    }
    if rule.name.chars().count() > 100 {
        return Err(HighlightError::NameTooLong);
    }
    if rule.color.len() != 7
        || !rule.color.starts_with('#')
        || !rule.color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(HighlightError::InvalidColor);
    }
    // Regexes are executed by JavaScript in the xterm renderer. Syntax and
    // zero-width validation therefore belongs to that same runtime; Rust's
    // regex crate accepts a different grammar (for example, no look-around).
    Ok(())
}

/// Escape JS RegExp metacharacters in a plain-text keyword so it matches
/// literally when treated as regex.
pub fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c {
            '.' | '*' | '+' | '?' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Normalize a legacy rule: if `is_regex` is false, escape the keyword;
/// if `name` is empty, seed it from the keyword.
fn normalize_rule(mut rule: HighlightRule) -> HighlightRule {
    if !rule.is_regex {
        rule.keyword = regex_escape(&rule.keyword);
        rule.is_regex = true;
    }
    if rule.name.is_empty() {
        rule.name = rule.keyword.clone();
    }
    rule
}

/// Seed the default rules to a JSON file only if the file does not already
/// exist. Once the file exists, the user owns it — deleted rules stay
/// deleted. Idempotent.
pub fn seed_default_rules() -> Result<(), HighlightError> {
    let path = highlight_rules_path()
        .ok_or_else(|| HighlightError::IoError("cannot determine config directory".into()))?;
    if path.exists() {
        return Ok(());
    }
    let rules = default_rules();
    save_rules(&path, &rules)?;
    Ok(())
}

/// Load all highlight rules from the config file. Seeds defaults first if the
/// file does not exist yet.
pub fn list_rules() -> Vec<HighlightRule> {
    let _ = seed_default_rules();
    let path = match highlight_rules_path() {
        Some(p) => p,
        None => return default_rules(),
    };
    match std::fs::read_to_string(&path) {
        Ok(json) => load_rules_from_json(&json).unwrap_or_else(|_| default_rules()),
        Err(_) => default_rules(),
    }
}

/// Parse highlight rules from a JSON string. Legacy rules (is_regex=false)
/// are normalized: keyword is escaped, name is seeded from keyword.
pub fn load_rules_from_json(json: &str) -> Result<Vec<HighlightRule>, HighlightError> {
    let rules: Vec<HighlightRule> =
        serde_json::from_str(json).map_err(|e| HighlightError::ParseError(e.to_string()))?;
    Ok(rules.into_iter().map(normalize_rule).collect())
}

/// Save highlight rules to the JSON file.
fn save_rules(path: &std::path::Path, rules: &[HighlightRule]) -> Result<(), HighlightError> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json =
        serde_json::to_string_pretty(rules).map_err(|e| HighlightError::IoError(e.to_string()))?;
    std::fs::write(path, json)
        .map_err(|e| HighlightError::IoError(format!("failed to write rules file: {e}")))?;
    Ok(())
}

/// Add a new highlight rule. Returns an error if the keyword conflicts with
/// an existing rule (keyword is the identity key).
pub fn insert_rule(rule: HighlightRule) -> Result<Vec<HighlightRule>, HighlightError> {
    validate_rule(&rule)?;
    let rule = normalize_rule(rule);
    let mut rules = list_rules();
    let keywords: HashSet<&str> = rules.iter().map(|r| r.keyword.as_str()).collect();
    if keywords.contains(rule.keyword.as_str()) {
        return Err(HighlightError::KeywordConflict);
    }
    rules.push(rule);
    let path = highlight_rules_path()
        .ok_or_else(|| HighlightError::IoError("cannot determine config directory".into()))?;
    save_rules(&path, &rules)?;
    Ok(rules)
}

/// Delete a highlight rule by keyword (the identity key).
pub fn delete_rule(keyword: &str) -> Result<Vec<HighlightRule>, HighlightError> {
    let mut rules = list_rules();
    let before = rules.len();
    rules.retain(|r| r.keyword != keyword);
    if rules.len() == before {
        return Err(HighlightError::NotFound);
    }
    let path = highlight_rules_path()
        .ok_or_else(|| HighlightError::IoError("cannot determine config directory".into()))?;
    save_rules(&path, &rules)?;
    Ok(rules)
}

/// Update a highlight rule. `old_keyword` identifies the rule to update;
/// the rule struct provides the new values (keyword may change).
pub fn update_rule(
    old_keyword: &str,
    rule: HighlightRule,
) -> Result<Vec<HighlightRule>, HighlightError> {
    validate_rule(&rule)?;
    let rule = normalize_rule(rule);
    let mut rules = list_rules();
    let idx = rules
        .iter()
        .position(|r| r.keyword == old_keyword)
        .ok_or(HighlightError::NotFound)?;
    // If keyword changed, check for conflict with another rule
    if rule.keyword != old_keyword {
        let conflict = rules
            .iter()
            .enumerate()
            .any(|(i, r)| i != idx && r.keyword == rule.keyword);
        if conflict {
            return Err(HighlightError::KeywordConflict);
        }
    }
    rules[idx] = rule;
    let path = highlight_rules_path()
        .ok_or_else(|| HighlightError::IoError("cannot determine config directory".into()))?;
    save_rules(&path, &rules)?;
    Ok(rules)
}

/// Reset all highlight rules to the hardcoded defaults, discarding user
/// customizations.
pub fn reset_defaults() -> Result<Vec<HighlightRule>, HighlightError> {
    let rules = default_rules();
    let path = highlight_rules_path()
        .ok_or_else(|| HighlightError::IoError("cannot determine config directory".into()))?;
    save_rules(&path, &rules)?;
    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_dir() -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("agent2ssh-highlight-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);
        dir
    }

    fn teardown_test_dir() {
        crate::store::clear_test_config_dir();
    }

    #[test]
    fn seed_creates_defaults_on_first_call() {
        let dir = setup_test_dir();
        let rules_path = dir.join(HIGHLIGHT_RULES_FILE);
        assert!(!rules_path.exists());

        seed_default_rules().unwrap();
        assert!(rules_path.exists());

        let rules = list_rules();
        assert_eq!(rules.len(), 5);
        assert!(rules.iter().any(|r| r.keyword == "ERROR"));
        assert!(rules.iter().any(|r| r.keyword == "WARN"));
        assert!(rules.iter().any(|r| r.name == "IPv4"));

        teardown_test_dir();
    }

    #[test]
    fn seed_is_idempotent() {
        setup_test_dir();
        seed_default_rules().unwrap();
        let first = list_rules();

        // Second call should not overwrite
        seed_default_rules().unwrap();
        let second = list_rules();

        assert_eq!(first, second);
        teardown_test_dir();
    }

    #[test]
    fn insert_adds_new_rule() {
        setup_test_dir();
        seed_default_rules().unwrap();

        let rule = HighlightRule {
            keyword: "FATAL".into(),
            name: "FATAL".into(),
            color: "#FF0000".into(),
            enabled: true,
            is_regex: true,
            is_case_sensitive: false,
        };
        let rules = insert_rule(rule).unwrap();
        assert_eq!(rules.len(), 6);
        assert!(rules.iter().any(|r| r.keyword == "FATAL"));

        teardown_test_dir();
    }

    #[test]
    fn insert_rejects_duplicate_keyword() {
        setup_test_dir();
        seed_default_rules().unwrap();

        let rule = HighlightRule {
            keyword: "ERROR".into(),
            name: "My Error".into(),
            color: "#FF0000".into(),
            enabled: true,
            is_regex: true,
            is_case_sensitive: false,
        };
        let err = insert_rule(rule).unwrap_err();
        assert!(matches!(err, HighlightError::KeywordConflict));

        teardown_test_dir();
    }

    #[test]
    fn insert_rejects_empty_keyword() {
        setup_test_dir();
        seed_default_rules().unwrap();

        let rule = HighlightRule {
            keyword: "".into(),
            name: "Empty".into(),
            color: "#000000".into(),
            enabled: true,
            is_regex: true,
            is_case_sensitive: false,
        };
        let err = insert_rule(rule).unwrap_err();
        assert!(matches!(err, HighlightError::EmptyKeyword));

        teardown_test_dir();
    }

    #[test]
    fn insert_rejects_empty_name() {
        setup_test_dir();
        seed_default_rules().unwrap();

        let rule = HighlightRule {
            keyword: "CUSTOM".into(),
            name: "".into(),
            color: "#000000".into(),
            enabled: true,
            is_regex: true,
            is_case_sensitive: false,
        };
        let err = insert_rule(rule).unwrap_err();
        assert!(matches!(err, HighlightError::NameRequired));

        teardown_test_dir();
    }

    #[test]
    fn insert_rejects_name_too_long() {
        setup_test_dir();
        seed_default_rules().unwrap();

        let rule = HighlightRule {
            keyword: "LONG".into(),
            name: "x".repeat(101),
            color: "#000000".into(),
            enabled: true,
            is_regex: true,
            is_case_sensitive: false,
        };
        let err = insert_rule(rule).unwrap_err();
        assert!(matches!(err, HighlightError::NameTooLong));

        teardown_test_dir();
    }

    #[test]
    fn insert_rejects_invalid_color_but_accepts_javascript_regex_syntax() {
        setup_test_dir();
        seed_default_rules().unwrap();

        let invalid_color = HighlightRule {
            keyword: "CUSTOM".into(),
            name: "Custom".into(),
            color: "red".into(),
            enabled: true,
            is_regex: true,
            is_case_sensitive: false,
        };
        assert!(matches!(
            insert_rule(invalid_color).unwrap_err(),
            HighlightError::InvalidColor
        ));

        let javascript_regex = HighlightRule {
            keyword: "(?=ERROR)ERROR".into(),
            name: "JavaScript lookahead".into(),
            color: "#FF0000".into(),
            enabled: true,
            is_regex: true,
            is_case_sensitive: false,
        };
        assert!(insert_rule(javascript_regex).is_ok());

        teardown_test_dir();
    }

    #[test]
    fn delete_removes_rule() {
        setup_test_dir();
        seed_default_rules().unwrap();

        let rules = delete_rule("ERROR").unwrap();
        assert_eq!(rules.len(), 4);
        assert!(!rules.iter().any(|r| r.keyword == "ERROR"));

        teardown_test_dir();
    }

    #[test]
    fn delete_returns_error_for_missing() {
        setup_test_dir();
        seed_default_rules().unwrap();

        let err = delete_rule("NONEXISTENT").unwrap_err();
        assert!(matches!(err, HighlightError::NotFound));

        teardown_test_dir();
    }

    #[test]
    fn update_changes_rule() {
        setup_test_dir();
        seed_default_rules().unwrap();

        let updated = HighlightRule {
            keyword: "ERROR".into(),
            name: "My Error".into(),
            color: "#FF00FF".into(),
            enabled: false,
            is_regex: true,
            is_case_sensitive: true,
        };
        let rules = update_rule("ERROR", updated).unwrap();
        let rule = rules.iter().find(|r| r.keyword == "ERROR").unwrap();
        assert_eq!(rule.name, "My Error");
        assert_eq!(rule.color, "#FF00FF");
        assert!(!rule.enabled);
        assert!(rule.is_case_sensitive);

        teardown_test_dir();
    }

    #[test]
    fn update_supports_keyword_rename() {
        setup_test_dir();
        seed_default_rules().unwrap();

        let updated = HighlightRule {
            keyword: "FATAL_ERROR".into(),
            name: "Fatal".into(),
            color: "#FF0000".into(),
            enabled: true,
            is_regex: true,
            is_case_sensitive: false,
        };
        let rules = update_rule("ERROR", updated).unwrap();
        assert!(rules.iter().any(|r| r.keyword == "FATAL_ERROR"));
        assert!(!rules.iter().any(|r| r.keyword == "ERROR"));

        teardown_test_dir();
    }

    #[test]
    fn update_rejects_conflict_on_rename() {
        setup_test_dir();
        seed_default_rules().unwrap();

        let updated = HighlightRule {
            keyword: "WARN".into(), // Conflict with existing WARN rule
            name: "Renamed".into(),
            color: "#FF0000".into(),
            enabled: true,
            is_regex: true,
            is_case_sensitive: false,
        };
        let err = update_rule("ERROR", updated).unwrap_err();
        assert!(matches!(err, HighlightError::KeywordConflict));

        teardown_test_dir();
    }

    #[test]
    fn reset_restores_defaults() {
        setup_test_dir();
        seed_default_rules().unwrap();

        // Add a custom rule
        let rule = HighlightRule {
            keyword: "CUSTOM".into(),
            name: "Custom".into(),
            color: "#000000".into(),
            enabled: true,
            is_regex: true,
            is_case_sensitive: false,
        };
        insert_rule(rule).unwrap();
        assert_eq!(list_rules().len(), 6);

        // Reset
        let rules = reset_defaults().unwrap();
        assert_eq!(rules.len(), 5);
        assert!(!rules.iter().any(|r| r.keyword == "CUSTOM"));

        teardown_test_dir();
    }

    #[test]
    fn regex_escape_neutralizes_metacharacters() {
        assert_eq!(regex_escape("a.txt"), r"a\.txt");
        assert_eq!(regex_escape("C++"), r"C\+\+");
        assert_eq!(regex_escape("$HOME"), r"\$HOME");
        assert_eq!(regex_escape("[ERROR]"), r"\[ERROR\]");
        assert_eq!(regex_escape("a+b*c?"), r"a\+b\*c\?");
    }

    #[test]
    fn normalize_escapes_legacy_text_and_seeds_name() {
        let rule = HighlightRule {
            keyword: "test.txt".into(),
            name: "".into(),
            color: "#FF0000".into(),
            enabled: true,
            is_regex: false,
            is_case_sensitive: false,
        };
        let normalized = normalize_rule(rule);
        assert_eq!(normalized.keyword, r"test\.txt");
        assert!(normalized.is_regex);
        assert_eq!(normalized.name, r"test\.txt");
    }

    #[test]
    fn normalize_leaves_regex_rule_untouched() {
        let rule = HighlightRule {
            keyword: r"\d+".into(),
            name: "Digits".into(),
            color: "#FF0000".into(),
            enabled: true,
            is_regex: true,
            is_case_sensitive: false,
        };
        let normalized = normalize_rule(rule.clone());
        assert_eq!(normalized, rule);
    }

    #[test]
    fn user_deletions_persist_after_seed() {
        setup_test_dir();
        seed_default_rules().unwrap();

        // Delete a rule
        delete_rule("WARN").unwrap();
        assert_eq!(list_rules().len(), 4);

        // seed_default_rules should NOT restore it (file already exists)
        seed_default_rules().unwrap();
        assert_eq!(list_rules().len(), 4);
        assert!(!list_rules().iter().any(|r| r.keyword == "WARN"));

        teardown_test_dir();
    }
}
