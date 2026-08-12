use anyhow::{Context, Result};
use regex::{NoExpand, Regex};
use serde::{Deserialize, Serialize};

/// File name for the copy-redaction rules in the config directory.
const COPY_REDACT_FILE: &str = "copy_redact_rules.json";

/// A single copy-redaction rule (compiled form).
pub struct CopyRedactRule {
    pub pattern: Regex,
    pub replacement: String,
}

/// Serializable form of a copy-redaction rule (for JSON persistence).
#[derive(Serialize, Deserialize)]
pub struct CopyRedactRuleConfig {
    pub pattern: String,
    pub replacement: String,
}

impl From<&CopyRedactRule> for CopyRedactRuleConfig {
    fn from(r: &CopyRedactRule) -> Self {
        Self {
            pattern: r.pattern.as_str().to_string(),
            replacement: r.replacement.clone(),
        }
    }
}

/// Default copy-redaction rules. These are seeded on first run and can be
/// edited by the user afterwards (seed-once semantics, same as A24).
///
/// These rules are intentionally different from the log/AI redaction rules
/// in `redaction.rs`: copy redaction is about clipboard safety, not log
/// sanitisation. For example, IPs are NOT redacted in copy mode (you often
/// need to paste them), but API keys always are.
fn default_copy_rules() -> Vec<CopyRedactRule> {
    [
        // API keys (OpenAI, Anthropic, etc.)
        (r"sk-[A-Za-z0-9_\-]{20,}", "[REDACTED:api-key]"),
        // AWS access keys
        (r"AKIA[0-9A-Z]{16}", "[REDACTED:aws-key]"),
        // Bearer tokens
        (r"Bearer\s+[A-Za-z0-9_\-\.]{20,}", "[REDACTED:bearer]"),
        // JWT tokens
        (
            r"eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]+",
            "[REDACTED:jwt]",
        ),
        // password= assignments
        (r"(?i)(password|passwd)\s*[=:]\s*\S+", "[REDACTED:password]"),
        // secret= assignments
        (r"(?i)secret\s*[=:]\s*\S+", "[REDACTED:secret]"),
        // token= assignments
        (r"(?i)token\s*[=:]\s*\S+", "[REDACTED:token]"),
    ]
    .into_iter()
    .map(|(p, r)| CopyRedactRule {
        pattern: Regex::new(p).expect("internal copy-redact pattern must compile"),
        replacement: r.to_string(),
    })
    .collect()
}

/// Resolve the config directory, honouring the `AGENT2SSH_CONFIG_DIR`
/// environment variable (used by tests).
fn config_dir() -> Result<std::path::PathBuf> {
    crate::store::config_dir()
}

fn ensure_config_dir() -> Result<()> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create config dir {}", dir.display()))?;
    Ok(())
}

/// Load copy-redaction rules from the config file. If the file doesn't exist,
/// seeds it with defaults first (seed-once semantics).
pub fn load_copy_redact_rules() -> Result<Vec<CopyRedactRule>> {
    let path = config_dir()?.join(COPY_REDACT_FILE);
    if !path.exists() {
        // Seed defaults on first run.
        let rules = default_copy_rules();
        save_copy_redact_rules(&rules)?;
        return Ok(rules);
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read copy redact rules file {}", path.display()))?;
    let configs: Vec<CopyRedactRuleConfig> = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse copy redact rules file {}", path.display()))?;
    let mut rules = Vec::with_capacity(configs.len());
    for c in configs {
        let pattern = Regex::new(&c.pattern)
            .with_context(|| format!("invalid regex in copy redact rules: {}", c.pattern))?;
        rules.push(CopyRedactRule {
            pattern,
            replacement: c.replacement,
        });
    }
    Ok(rules)
}

/// Save copy-redaction rules to the config file.
pub fn save_copy_redact_rules(rules: &[CopyRedactRule]) -> Result<()> {
    ensure_config_dir()?;
    let path = config_dir()?.join(COPY_REDACT_FILE);
    let configs: Vec<CopyRedactRuleConfig> = rules.iter().map(Into::into).collect();
    let json = serde_json::to_string_pretty(&configs)?;
    std::fs::write(&path, json)
        .with_context(|| format!("failed to write copy redact rules file {}", path.display()))?;
    restrict_file_to_owner(&path)?;
    Ok(())
}

/// Apply copy-redaction rules to text that will be placed on the clipboard.
///
/// Uses `NoExpand` for replacement, preventing `$1` capture group expansion
/// from re-inserting sensitive data.
pub fn redact_for_clipboard(text: &str) -> String {
    let rules = load_copy_redact_rules().unwrap_or_else(|_| default_copy_rules());

    // Idempotency: skip if already redacted.
    if is_already_redacted(text) {
        return text.to_string();
    }

    let mut out = text.to_string();
    for rule in &rules {
        out = rule
            .pattern
            .replace_all(&out, NoExpand(&rule.replacement))
            .into_owned();
    }
    out
}

/// Check whether text already contains redaction markers.
fn is_already_redacted(text: &str) -> bool {
    text.contains("[REDACTED:")
}

/// Reset the rules file to the hardcoded defaults, discarding any user
/// customizations.
pub fn reset_copy_redact_rules() -> Result<()> {
    let rules = default_copy_rules();
    save_copy_redact_rules(&rules)
}

/// Restrict file permissions to the current user only (Unix only; no-op on
/// Windows where file ACLs are managed differently).
fn restrict_file_to_owner(path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agent2ssh-copyredact-{}-{}",
            label,
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);
        dir
    }

    fn cleanup(dir: &std::path::Path) {
        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn redacts_api_key() {
        let dir = unique_dir("apikey");
        let result = redact_for_clipboard("key: sk-abc123def456ghi789jkl012mno345pqr");
        assert!(result.contains("[REDACTED:api-key]"));
        assert!(!result.contains("sk-abc123"));
        cleanup(&dir);
    }

    #[test]
    fn redacts_password_assignment() {
        let dir = unique_dir("password");
        let result = redact_for_clipboard("password=hunter2");
        assert!(result.contains("[REDACTED:password]"));
        assert!(!result.contains("hunter2"));
        cleanup(&dir);
    }

    #[test]
    fn redacts_bearer_token() {
        let dir = unique_dir("bearer");
        let result = redact_for_clipboard(
            "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abc123def456",
        );
        assert!(result.contains("[REDACTED:bearer]"));
        cleanup(&dir);
    }

    #[test]
    fn does_not_redact_ips_by_default() {
        let dir = unique_dir("ip");
        let result = redact_for_clipboard("connect to 10.0.0.5");
        // IPs are NOT redacted in copy mode (unlike log mode).
        assert!(
            result.contains("10.0.0.5"),
            "IPs should be visible in clipboard content"
        );
        cleanup(&dir);
    }

    #[test]
    fn idempotent_does_not_double_redact() {
        let dir = unique_dir("idempotent");
        let input = "key: sk-abc123def456ghi789jkl012mno345pqr";
        let once = redact_for_clipboard(input);
        let twice = redact_for_clipboard(&once);
        assert_eq!(once, twice, "double redaction must be idempotent");
        cleanup(&dir);
    }

    #[test]
    fn preserves_normal_text() {
        let dir = unique_dir("normal");
        let input = "ls -la /tmp && echo hello";
        let result = redact_for_clipboard(input);
        assert_eq!(result, input);
        cleanup(&dir);
    }

    #[test]
    fn saves_and_loads_custom_rules() {
        let dir = unique_dir("custom");

        // First load seeds defaults.
        let defaults = load_copy_redact_rules().unwrap();
        assert!(!defaults.is_empty());

        // Save custom rules (just one rule).
        let custom = vec![CopyRedactRule {
            pattern: Regex::new(r"my-secret-\d+").unwrap(),
            replacement: "[REDACTED:custom]".into(),
        }];
        save_copy_redact_rules(&custom).unwrap();

        // Load must return the custom rules, not defaults.
        let loaded = load_copy_redact_rules().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].replacement, "[REDACTED:custom]");

        // Redaction must use the custom rule.
        let result = redact_for_clipboard("found my-secret-42 here");
        assert!(result.contains("[REDACTED:custom]"));

        cleanup(&dir);
    }

    #[test]
    fn reset_restores_defaults() {
        let dir = unique_dir("reset");

        // Save custom rules (empty).
        save_copy_redact_rules(&[]).unwrap();
        let loaded = load_copy_redact_rules().unwrap();
        assert!(loaded.is_empty());

        // Reset.
        reset_copy_redact_rules().unwrap();
        let loaded = load_copy_redact_rules().unwrap();
        assert!(!loaded.is_empty(), "reset must restore defaults");

        cleanup(&dir);
    }

    #[test]
    fn independent_from_log_redaction() {
        // The copy redaction rules must be independent from the log/AI
        // redaction rules in redaction.rs. They use different files and
        // different default rule sets.
        let dir = unique_dir("independent");

        // Save custom copy rules.
        let custom = vec![CopyRedactRule {
            pattern: Regex::new(r"custom-copy-\d+").unwrap(),
            replacement: "[REDACTED:copy]".into(),
        }];
        save_copy_redact_rules(&custom).unwrap();

        // The log redaction rules should NOT be affected.
        let log_rules = crate::redaction::default_rules();
        assert!(
            !log_rules.iter().any(|r| r.replacement.contains("copy")),
            "log redaction must not contain copy-specific rules"
        );

        cleanup(&dir);
    }

    #[test]
    fn no_expand_prevents_capture_group_reinsertion() {
        let dir = unique_dir("noexpand");

        // Write a rule with $1 in replacement — NoExpand should prevent
        // the captured secret from being re-inserted.
        let custom = vec![CopyRedactRule {
            pattern: Regex::new(r"(sk-[A-Za-z0-9]+)").unwrap(),
            replacement: "$1".into(), // Without NoExpand, this would re-insert the key!
        }];
        save_copy_redact_rules(&custom).unwrap();

        let result = redact_for_clipboard("key=sk-abc123def456ghi789jkl012mno345");
        assert_eq!(
            result, "key=$1",
            "NoExpand must prevent capture group expansion"
        );

        cleanup(&dir);
    }

    #[test]
    fn redacts_multiple_patterns() {
        let dir = unique_dir("multi");
        let input = "password=hunter2 key=sk-abc123def456ghi789jkl012mno345 Bearer eyJabc.eyJdef.ghi1234567890";
        let result = redact_for_clipboard(input);
        assert!(result.contains("[REDACTED:password]"));
        assert!(result.contains("[REDACTED:api-key]"));
        assert!(result.contains("[REDACTED:bearer]"));
        assert!(!result.contains("hunter2"));
        assert!(!result.contains("sk-abc123"));
        cleanup(&dir);
    }

    #[test]
    fn empty_string_is_unchanged() {
        let dir = unique_dir("empty");
        assert_eq!(redact_for_clipboard(""), "");
        cleanup(&dir);
    }

    #[test]
    fn config_serializes_roundtrip() {
        let config = CopyRedactRuleConfig {
            pattern: r"sk-\w+".into(),
            replacement: "[HIDDEN]".into(),
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: CopyRedactRuleConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.pattern, r"sk-\w+");
        assert_eq!(back.replacement, "[HIDDEN]");
    }
}
