//! Enhanced log redaction: regex-based pattern matching with zero-width
//! validation, NoExpand replacement, and a default rule set.
//!
//! ## Why this module exists
//!
//! The existing `redact_sensitive_text` in `store.rs` uses token-based
//! heuristics: it splits on whitespace and checks for keywords like
//! "password", "token", "Authorization". This misses:
//! - IPv4 addresses (no keyword to anchor on)
//! - Bearer tokens / API keys embedded in free text
//! - JWT tokens (three base64 segments joined by dots)
//! - Hex blobs (SHA-256 hashes, SSH fingerprints)
//! - Patterns that span token boundaries
//!
//! ## Zero-width pattern validation
//!
//! A zero-width regex like `a*` or `\b` can match the empty string at every
//! position. If such a pattern were used with a replacement, it would
//! insert `<REDACTED>` between every character — corrupting the text while
//! giving a false sense of security. We validate patterns with
//! `regex_syntax` to check `minimum_len()` and reject zero-width patterns.
//!
//! ## NoExpand replacement
//!
//! Rust's `Regex::replace` treats `$1`, `$2`, `${name}` in the replacement
//! string as capture group references. If a user writes a rule with pattern
//! `(sk-[A-Za-z0-9]+)` and replacement `$1`, the secret key would be
//! re-inserted verbatim — completely defeating the redaction. We use
//! `regex::NoExpand` to treat the replacement as a literal string, preventing
//! capture group expansion.
//!
//! ## Default rule set
//!
//! The following patterns are built-in and active by default:
//! - Private IPv4 (10.x, 172.16-31.x, 192.168.x)
//! - IPv6 loopback and link-local (::1, fe80::)
//! - Bearer tokens
//! - API keys (sk-, AKIA)
//! - JWT tokens (eyJ...eyJ...)
//! - Hex blobs (32+ hex chars — SHA-256, SSH fingerprints)

use regex::{NoExpand, Regex};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A compiled redaction rule: pattern + literal replacement.
#[derive(Debug, Clone)]
pub struct RedactRule {
    pub pattern: Regex,
    pub replacement: String,
}

/// Error type for redaction rule validation.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RedactRuleError {
    /// The regex pattern is syntactically invalid.
    #[error("invalid regex pattern: {0}")]
    InvalidRegex(String),

    /// The regex pattern can match the empty string (zero-width),
    /// which would cause spurious replacements everywhere.
    #[error("zero-width pattern: {0}")]
    ZeroWidth(String),
}

/// Validate a regex pattern: it must compile and must not be zero-width.
///
/// Zero-width patterns (e.g. `""`, `^`, `$`, `a*`, `\b`) match the empty
/// string at every position. Using them with `replace_all` would insert
/// the replacement between every character, corrupting the text.
pub fn validate_pattern(pattern: &str) -> Result<(), RedactRuleError> {
    let regex = Regex::new(pattern).map_err(|e| RedactRuleError::InvalidRegex(e.to_string()))?;
    if matches_empty(&regex) {
        return Err(RedactRuleError::ZeroWidth(pattern.to_string()));
    }
    Ok(())
}

/// Check if a compiled regex can produce a zero-width (empty) match.
///
/// We check two conditions:
/// 1. Does it match the empty string directly?
/// 2. Does it produce a zero-length match on a non-empty sample string?
///    This catches patterns like `\b` (word boundary) that don't match ""
///    but produce zero-length matches on non-empty input.
fn matches_empty(regex: &Regex) -> bool {
    if regex.is_match("") {
        return true;
    }
    // Check for zero-width matches on a non-empty sample string.
    // This catches anchors and word boundaries that `is_match("")` misses.
    if let Some(m) = regex.find("abcdefghij") {
        if m.end() == m.start() {
            return true;
        }
    }
    false
}

impl RedactRule {
    /// Create a new rule, validating that the pattern is not zero-width.
    pub fn new(pattern: &str, replacement: &str) -> Result<Self, RedactRuleError> {
        validate_pattern(pattern)?;
        Ok(Self {
            pattern: Regex::new(pattern)
                .map_err(|e| RedactRuleError::InvalidRegex(e.to_string()))?,
            replacement: replacement.to_string(),
        })
    }

    /// Create a rule without validation. Only for internal default rules
    /// that are known to be correct.
    fn new_unchecked(pattern: &str, replacement: &str) -> Self {
        Self {
            pattern: Regex::new(pattern).expect("internal redact pattern must compile"),
            replacement: replacement.to_string(),
        }
    }
}

/// The default redaction rule set. These are always active unless explicitly
/// disabled by providing an empty rules list to `redact_with_rules`.
///
/// Each rule uses a regex pattern and a literal replacement (via NoExpand).
/// No capture groups are expanded — the replacement string is used verbatim.
pub fn default_rules() -> Vec<RedactRule> {
    [
        // Private IPv4 ranges (RFC 1918)
        (r"\b10\.\d{1,3}\.\d{1,3}\.\d{1,3}\b", "<REDACTED:ip>"),
        (
            r"\b172\.(1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3}\b",
            "<REDACTED:ip>",
        ),
        (r"\b192\.168\.\d{1,3}\.\d{1,3}\b", "<REDACTED:ip>"),
        // IPv6 loopback and link-local
        // No leading \b because \b doesn't work around ':' (colon is a
        // non-word char, same as space, so no word boundary between them).
        // The trailing \b is sufficient to prevent matching ::1 inside ::123.
        // fe80:: rule must come before ::1 to avoid ::1 matching inside fe80::1.
        (r"fe80::[0-9a-fA-F:]+\b", "<REDACTED:ip>"),
        (r"::1\b", "<REDACTED:ip>"),
        // Bearer tokens
        (r"Bearer\s+[A-Za-z0-9_\-\.]{20,}", "<REDACTED:bearer>"),
        // API keys
        (r"sk-[A-Za-z0-9_\-]{20,}", "<REDACTED:api-key>"),
        (r"AKIA[0-9A-Z]{16}", "<REDACTED:aws-key>"),
        // JWT tokens (three base64url segments joined by dots)
        (
            r"eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]+",
            "<REDACTED:jwt>",
        ),
        // Hex blobs: 32+ hex chars (SHA-256, SSH fingerprints, etc.)
        (r"\b[0-9a-fA-F]{32,}\b", "<REDACTED:hex>"),
    ]
    .into_iter()
    .map(|(p, r)| RedactRule::new_unchecked(p, r))
    .collect()
}

/// Apply redaction rules to a text string.
///
/// Rules are applied in order. Each rule's replacement is treated as a
/// literal string (via `NoExpand`), preventing `$1` capture group expansion
/// from re-inserting sensitive data.
///
/// **Idempotency without bypass.** Existing redaction markers (`<REDACTED:...>`,
/// `[REDACTED...]`) are extracted and placeholder-protected before rules run,
/// then restored afterwards, so a second pass cannot mangle them. The previous
/// whole-text skip (`is_pre_redacted`) was a security hole: any output
/// containing a marker-like substring (`echo '<REDACTED:ip> 10.0.0.1'`) was
/// returned entirely unredacted. Now non-marker text is always redacted.
pub fn redact_with_rules(text: &str, rules: &[RedactRule]) -> String {
    let (protected, markers) = protect_markers(text);
    let mut out = protected;
    for rule in rules {
        out = rule
            .pattern
            .replace_all(&out, NoExpand(&rule.replacement))
            .into_owned();
    }
    restore_markers(&out, &markers)
}

/// Placeholder delimiter for protected markers — a control character that
/// cannot appear in normal text and is not matched by any default rule.
const MARKER_DELIM: char = '\u{1}';

/// Extract existing redaction markers (`<REDACTED:...>` / `[REDACTED...]`)
/// and replace each with a unique placeholder. Returns the protected text and
/// the extracted markers in order, so `restore_markers` can put them back.
fn protect_markers(text: &str) -> (String, Vec<String>) {
    let mut markers = Vec::new();
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let lt = rest.find("<REDACTED:");
        let br = rest.find("[REDACTED");
        let start = match (lt, br) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => {
                // No markers left: flush the remaining text.
                out.push_str(rest);
                break;
            }
        };
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        // `<REDACTED:...>` closes on '>', `[REDACTED...]` on ']'.
        let close_gt = lt.is_some_and(|l| l <= br.unwrap_or(usize::MAX));
        let end = if close_gt {
            after.find('>').map(|e| e + 1)
        } else {
            after.find(']').map(|e| e + 1)
        };
        match end {
            Some(e) => {
                let idx = markers.len();
                out.push_str(&format!("{MARKER_DELIM}M{idx}{MARKER_DELIM}"));
                markers.push(after[..e].to_string());
                rest = &after[e..];
            }
            None => {
                // Unterminated marker: treat as plain text, stop scanning.
                out.push_str(after);
                break;
            }
        }
    }
    (out, markers)
}

/// Restore protected markers after rules have been applied.
fn restore_markers(text: &str, markers: &[String]) -> String {
    let mut out = text.to_string();
    for (i, marker) in markers.iter().enumerate() {
        out = out.replace(&format!("{MARKER_DELIM}M{i}{MARKER_DELIM}"), marker);
    }
    out
}

/// B1: Check whether text has already been redacted.
///
/// Returns `true` if the text contains any known redaction marker, indicating
/// that a previous `redact_with_rules` or `redact_sensitive_text` pass has
/// already processed it. This makes redaction idempotent: calling
/// `redact_default(redact_default(text))` produces the same output as
/// `redact_default(text)`.
///
/// Detected markers:
/// - `<REDACTED:...>` — regex-based redaction markers (from `redaction.rs`)
/// - `[REDACTED]` — token-based redaction markers (from `store.rs`)
/// - `[REDACTED PRIVATE KEY]` — private key redaction
///
/// This check is intentionally cheap (substring search) so it can run on
/// every redaction call without measurable overhead.
pub fn is_pre_redacted(text: &str) -> bool {
    text.contains("<REDACTED:") || text.contains("[REDACTED")
}

/// Apply the default rule set to a text string. This is the zero-config
/// entry point — rules are always available without any setup.
pub fn redact_default(text: &str) -> String {
    let rules = default_rules();
    redact_with_rules(text, &rules)
}

/// Combined redaction: first apply the default rules, then any custom rules.
/// This ensures built-in patterns are always applied even if the caller
/// provides additional rules.
pub fn redact_with_defaults(text: &str, custom_rules: &[RedactRule]) -> String {
    let mut rules = default_rules();
    rules.extend_from_slice(custom_rules);
    redact_with_rules(text, &rules)
}

// ── A24: Seed-once editable default rules ────────────────────────────────────
//
// Default redaction rules are hardcoded in `default_rules()`, but users may
// want to customize, disable, or add rules. The seed-once pattern:
//
// 1. On first run, the default rules are written to `redact_rules.json` in
//    the config dir.
// 2. On subsequent runs, the rules are loaded from the file — if the user
//    deleted a rule, it stays deleted (no re-seeding).
// 3. Only an explicit `reset_default_rules()` restores the hardcoded set.
//
// This mirrors rssh's `db/highlight.rs:221` seed-once pattern, adapted to
// file-based storage.

/// The JSON file name for user-editable redaction rules.
const REDACT_RULES_FILE: &str = "redact_rules.json";

/// Path to the user-editable redaction rules file.
fn redact_rules_path() -> Option<std::path::PathBuf> {
    crate::store::config_dir()
        .ok()
        .map(|d| d.join(REDACT_RULES_FILE))
}

/// A24: Seed the default rules to a JSON file **only if the file does not
/// already exist**. Once the file exists, the user owns it — deleted rules
/// stay deleted.
///
/// Returns the path to the file. This function is idempotent: if the file
/// already exists, it does nothing.
pub fn seed_default_rules() -> Result<(), RedactRuleError> {
    let path = redact_rules_path()
        .ok_or_else(|| RedactRuleError::InvalidRegex("cannot determine config directory".into()))?;
    if path.exists() {
        // Already seeded — user may have customized it. Do not overwrite.
        return Ok(());
    }
    let rules = default_rules();
    let configs: Vec<RedactRuleConfig> = rules.iter().map(Into::into).collect();
    let json = serde_json::to_string_pretty(&configs)
        .map_err(|e| RedactRuleError::InvalidRegex(e.to_string()))?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, json)
        .map_err(|e| RedactRuleError::InvalidRegex(format!("failed to write rules file: {e}")))?;
    Ok(())
}

/// A24: Load user-editable rules from the config file. If the file doesn't
/// exist yet, seed it first (one-time) and then load.
///
/// Returns the rules from the file, which may differ from the hardcoded
/// defaults if the user edited them.
pub fn load_user_rules() -> Vec<RedactRule> {
    let _ = seed_default_rules();
    let path = match redact_rules_path() {
        Some(p) => p,
        None => return default_rules(),
    };
    match std::fs::read_to_string(&path) {
        Ok(json) => load_rules_from_json(&json).unwrap_or_else(|_| default_rules()),
        Err(_) => default_rules(),
    }
}

/// A24: Reset the rules file to the hardcoded defaults, discarding any user
/// customizations. This is the only way to restore a rule the user deleted.
pub fn reset_default_rules() -> Result<(), RedactRuleError> {
    let path = redact_rules_path()
        .ok_or_else(|| RedactRuleError::InvalidRegex("cannot determine config directory".into()))?;
    let rules = default_rules();
    let configs: Vec<RedactRuleConfig> = rules.iter().map(Into::into).collect();
    let json = serde_json::to_string_pretty(&configs)
        .map_err(|e| RedactRuleError::InvalidRegex(e.to_string()))?;
    std::fs::write(&path, json)
        .map_err(|e| RedactRuleError::InvalidRegex(format!("failed to write rules file: {e}")))?;
    Ok(())
}

/// A24: Redact using the user-editable rules (loaded from file), falling
/// back to hardcoded defaults if the file is missing or corrupt.
pub fn redact_with_user_rules(text: &str) -> String {
    let rules = load_user_rules();
    redact_with_rules(text, &rules)
}

/// Serialize the rule set for configuration persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactRuleConfig {
    pub pattern: String,
    pub replacement: String,
}

impl From<&RedactRule> for RedactRuleConfig {
    fn from(rule: &RedactRule) -> Self {
        Self {
            pattern: rule.pattern.as_str().to_string(),
            replacement: rule.replacement.clone(),
        }
    }
}

/// Load custom rules from a JSON config string. Returns an error if any
/// rule fails validation (invalid regex or zero-width pattern).
pub fn load_rules_from_json(json: &str) -> Result<Vec<RedactRule>, RedactRuleError> {
    let configs: Vec<RedactRuleConfig> =
        serde_json::from_str(json).map_err(|e| RedactRuleError::InvalidRegex(e.to_string()))?;
    configs
        .into_iter()
        .map(|c| RedactRule::new(&c.pattern, &c.replacement))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Pattern validation ──────────────────────────────────────────────

    #[test]
    fn rejects_invalid_regex() {
        // The exact error string depends on the regex crate version, so we
        // just check that an invalid pattern produces InvalidRegex.
        let err = validate_pattern("(").unwrap_err();
        assert!(matches!(err, RedactRuleError::InvalidRegex(_)));
    }

    #[test]
    fn marker_substring_cannot_bypass_redaction() {
        // Regression: an attacker-supplied string containing a marker-like
        // substring must not disable redaction for the rest of the text.
        let out = redact_default("connect to 10.0.0.1 then <REDACTED:ip> then 192.168.0.5");
        assert!(out.contains("<REDACTED:ip>"), "IPs must be redacted: {out}");
        // Both real IPs are redacted, and the pre-existing marker is preserved
        // as-is — three markers in total.
        assert_eq!(
            out.matches("<REDACTED:ip>").count(),
            3,
            "both IPs redacted: {out}"
        );
    }

    #[test]
    fn idempotent_second_pass_preserves_markers() {
        // A second pass must not corrupt existing markers, and mixed content
        // still gets redacted.
        let once = redact_default("Bearer abc123def456ghi789jkl012mno345pqr");
        let twice = redact_default(&once);
        assert_eq!(once, twice, "second pass must be a no-op on pure markers");
        let mixed = redact_default(&format!("{once} and 10.1.2.3"));
        assert!(
            mixed.contains("<REDACTED:bearer>"),
            "marker preserved: {mixed}"
        );
        assert!(mixed.contains("<REDACTED:ip>"), "new IP redacted: {mixed}");
    }

    #[test]
    fn unterminated_marker_is_not_protected() {
        // A marker-like substring without its closing bracket is plain text
        // and the rest of the line must still be redacted.
        let out = redact_default("foo <REDACTED:ip then 10.9.8.7");
        assert!(out.contains("<REDACTED:ip>"), "IP redacted: {out}");
    }

    #[test]
    fn rejects_zero_width_patterns() {
        for pattern in ["", "^", "$", "a*", r"\b", r"x*y*"] {
            assert_eq!(
                validate_pattern(pattern).unwrap_err(),
                RedactRuleError::ZeroWidth(pattern.to_string()),
                "pattern {pattern:?} should be rejected as zero-width"
            );
        }
    }

    #[test]
    fn accepts_valid_non_zero_patterns() {
        validate_pattern(r"secret-\d+").unwrap();
        validate_pattern(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b").unwrap();
    }

    // ── Default rules ───────────────────────────────────────────────────

    #[test]
    fn redacts_private_ipv4() {
        assert_eq!(
            redact_default("connect to 10.0.0.5"),
            "connect to <REDACTED:ip>"
        );
        assert_eq!(
            redact_default("ssh 172.16.5.1 port 22"),
            "ssh <REDACTED:ip> port 22"
        );
        assert_eq!(
            redact_default("gateway 192.168.1.1 is up"),
            "gateway <REDACTED:ip> is up"
        );
    }

    #[test]
    fn does_not_redact_public_ipv4() {
        // 8.8.8.8 is not in the 10/172.16-31/192.168 ranges
        assert_eq!(redact_default("dns 8.8.8.8"), "dns 8.8.8.8");
        assert_eq!(redact_default("server 1.2.3.4"), "server 1.2.3.4");
    }

    #[test]
    fn redacts_ipv6_loopback() {
        assert_eq!(redact_default("listen on ::1"), "listen on <REDACTED:ip>");
        assert_eq!(redact_default("link fe80::1"), "link <REDACTED:ip>");
    }

    #[test]
    fn redacts_bearer_token() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIx";
        let result = redact_default(input);
        assert!(result.contains("<REDACTED:bearer>"));
        assert!(!result.contains("eyJhbGciOiJIUzI1NiJ9"));
    }

    #[test]
    fn redacts_api_key() {
        let result = redact_default("key: sk-abc123def456ghi789jkl012mno345pqr");
        assert!(result.contains("<REDACTED:api-key>"));
        assert!(!result.contains("sk-abc123def456ghi789"));
    }

    #[test]
    fn redacts_aws_key() {
        let result = redact_default("creds: AKIAIOSFODNN7EXAMPLE");
        assert!(result.contains("<REDACTED:aws-key>"));
    }

    #[test]
    fn redacts_jwt_token() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abc123def456ghi789";
        let result = redact_default(jwt);
        assert!(result.contains("<REDACTED:jwt>"));
        assert!(!result.contains("eyJhbGciOiJIUzI1NiJ9"));
    }

    #[test]
    fn redacts_hex_blobs() {
        // SHA-256 hash (64 hex chars)
        let hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let result = redact_default(hash);
        assert_eq!(result, "<REDACTED:hex>");
    }

    #[test]
    fn does_not_redact_short_hex() {
        // 16 hex chars is too short (below 32 threshold)
        assert_eq!(
            redact_default("id: deadbeef12345678"),
            "id: deadbeef12345678"
        );
    }

    #[test]
    fn redacts_multiple_patterns_in_one_string() {
        let input = "ssh 10.0.0.1 with token sk-abc123def456ghi789jkl012mno345";
        let result = redact_default(input);
        assert!(result.contains("<REDACTED:ip>"));
        assert!(result.contains("<REDACTED:api-key>"));
        assert!(!result.contains("10.0.0.1"));
        assert!(!result.contains("sk-abc123"));
    }

    // ── NoExpand safety ─────────────────────────────────────────────────

    #[test]
    fn no_expand_prevents_capture_group_reinsertion() {
        // If a user writes a rule with a capture group and $1 in replacement,
        // NoExpand ensures the $1 is treated literally, not as a backreference.
        let rule = RedactRule::new_unchecked(r"(sk-[A-Za-z0-9]+)", "$1");
        let input = "key=sk-abc123def456ghi789jkl012mno345";
        let result = redact_with_rules(input, &[rule]);
        // With NoExpand, $1 is the literal replacement — the secret is NOT
        // re-inserted. Instead, the entire match is replaced with "$1".
        assert_eq!(result, "key=$1");
    }

    #[test]
    fn custom_rules_compose_with_defaults() {
        let custom = RedactRule::new_unchecked(r"my-secret-\d+", "<REDACTED:custom>");
        let input = "connect 10.0.0.1 with my-secret-42";
        let result = redact_with_defaults(input, &[custom]);
        assert!(result.contains("<REDACTED:ip>"));
        assert!(result.contains("<REDACTED:custom>"));
    }

    // ── Config loading ──────────────────────────────────────────────────

    #[test]
    fn load_rules_from_valid_json() {
        let json = r#"[{"pattern":"\\bsecret-\\d+","replacement":"<HIDDEN>"}]"#;
        let rules = load_rules_from_json(json).unwrap();
        assert_eq!(rules.len(), 1);
        let result = redact_with_rules("found secret-42", &rules);
        assert_eq!(result, "found <HIDDEN>");
    }

    #[test]
    fn load_rules_rejects_zero_width() {
        let json = r#"[{"pattern":"a*","replacement":"<HIDDEN>"}]"#;
        assert_eq!(
            load_rules_from_json(json).unwrap_err(),
            RedactRuleError::ZeroWidth("a*".to_string())
        );
    }

    #[test]
    fn load_rules_from_empty_json_returns_empty() {
        let json = "[]";
        let rules = load_rules_from_json(json).unwrap();
        assert!(rules.is_empty());
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn empty_string_is_unchanged() {
        assert_eq!(redact_default(""), "");
    }

    #[test]
    fn text_without_secrets_is_unchanged() {
        let input = "ls -la /tmp && echo hello";
        assert_eq!(redact_default(input), input);
    }

    #[test]
    fn default_rules_all_pass_validation() {
        let rules = default_rules();
        for rule in &rules {
            // The pattern should compile (already guaranteed by new_unchecked)
            // and should not be zero-width.
            assert!(
                !matches_empty(&rule.pattern),
                "default rule pattern {:?} is zero-width",
                rule.pattern.as_str()
            );
        }
    }

    // ── B1: pre_redacted idempotency ──────────────────────────────────────

    #[test]
    fn is_pre_redacted_detects_angle_markers() {
        assert!(is_pre_redacted("connect to <REDACTED:ip> now"));
        assert!(is_pre_redacted("key: <REDACTED:api-key>"));
        assert!(is_pre_redacted("hash <REDACTED:hex> done"));
        assert!(is_pre_redacted("token <REDACTED:bearer> ok"));
    }

    #[test]
    fn is_pre_redacted_detects_square_markers() {
        assert!(is_pre_redacted("password=[REDACTED]"));
        assert!(is_pre_redacted("[REDACTED PRIVATE KEY]"));
        assert!(is_pre_redacted("auth: [REDACTED]"));
    }

    #[test]
    fn is_pre_redacted_rejects_clean_text() {
        assert!(!is_pre_redacted("connect to 10.0.0.1"));
        assert!(!is_pre_redacted("password=secret123"));
        assert!(!is_pre_redacted("ls -la /tmp"));
        assert!(!is_pre_redacted(""));
    }

    #[test]
    fn redact_with_rules_is_idempotent() {
        // Redacting already-redacted text should be a no-op.
        let input = "ssh 10.0.0.1 with sk-abc123def456ghi789jkl012mno345pqr";
        let once = redact_default(input);
        let twice = redact_default(&once);

        assert_eq!(once, twice, "double redaction must be idempotent");
        assert!(once.contains("<REDACTED:ip>"));
        assert!(once.contains("<REDACTED:api-key>"));
    }

    #[test]
    fn redact_with_rules_skips_pre_redacted() {
        // Text that already contains REDACTED markers should be returned as-is.
        let pre_redacted = "connect to <REDACTED:ip> with <REDACTED:api-key>";
        let result = redact_default(pre_redacted);
        assert_eq!(result, pre_redacted, "pre-redacted text must be untouched");
    }

    #[test]
    fn redact_with_rules_preserves_hex_in_marker() {
        // A hex hash inside a <REDACTED:hex> marker must not be re-redacted
        // or corrupted by a second pass.
        let pre_redacted = "hash: <REDACTED:hex>";
        let result = redact_default(pre_redacted);
        assert_eq!(result, pre_redacted);
    }

    #[test]
    fn redact_sensitive_text_idempotent() {
        // The combined redaction function in store.rs should also be idempotent.
        let input = "deploy --token secret123 password=hunter2 --api-key sk-abc123def456ghi789jkl012mno345pqr";
        let once = crate::store::redact_sensitive_text(input);
        let twice = crate::store::redact_sensitive_text(&once);
        assert_eq!(once, twice, "redact_sensitive_text must be idempotent");
    }

    // ── A24: Seed-once editable default rules ──────────────────────────────

    #[test]
    fn a24_seed_creates_file_on_first_run() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-a24s-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);

        // File should not exist yet.
        let rules_path = dir.join(REDACT_RULES_FILE);
        assert!(!rules_path.exists());

        // Seed.
        seed_default_rules().unwrap();
        assert!(rules_path.exists(), "seed must create the rules file");

        // The seeded file must contain valid rules matching defaults.
        let rules = load_user_rules();
        assert!(!rules.is_empty(), "seeded rules must not be empty");
        assert_eq!(
            rules.len(),
            default_rules().len(),
            "seeded rules must match defaults"
        );

        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a24_seed_is_idempotent_does_not_overwrite() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-a24i-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);

        // Seed first.
        seed_default_rules().unwrap();

        // Modify the file — delete one rule.
        let rules_path = dir.join(REDACT_RULES_FILE);
        let mut rules =
            load_rules_from_json(&std::fs::read_to_string(&rules_path).unwrap()).unwrap();
        rules.pop();
        let json = serde_json::to_string_pretty(
            &rules.iter().map(RedactRuleConfig::from).collect::<Vec<_>>(),
        )
        .unwrap();
        std::fs::write(&rules_path, json).unwrap();

        // Seed again — must NOT overwrite the user's modified file.
        seed_default_rules().unwrap();
        let loaded = load_user_rules();
        assert_eq!(
            loaded.len(),
            default_rules().len() - 1,
            "re-seeding must not overwrite user customizations"
        );

        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a24_reset_restores_defaults() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-a24r-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);

        // Seed + modify (remove rules).
        seed_default_rules().unwrap();
        let rules_path = dir.join(REDACT_RULES_FILE);
        std::fs::write(&rules_path, "[]").unwrap(); // Delete all rules.
        let loaded = load_user_rules();
        assert!(loaded.is_empty(), "user deleted all rules");

        // Reset — must restore defaults.
        reset_default_rules().unwrap();
        let loaded = load_user_rules();
        assert_eq!(
            loaded.len(),
            default_rules().len(),
            "reset must restore all default rules"
        );

        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a24_load_user_rules_falls_back_on_corrupt_file() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-a24c-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);

        // Write a corrupt JSON file.
        let rules_path = dir.join(REDACT_RULES_FILE);
        std::fs::write(&rules_path, "{{corrupt json").unwrap();

        let rules = load_user_rules();
        assert!(!rules.is_empty(), "corrupt file must fall back to defaults");
        assert_eq!(rules.len(), default_rules().len());

        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a24_redact_with_user_rules_works() {
        let dir = std::env::temp_dir().join(format!("agent2ssh-a24w-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::store::set_test_config_dir(&dir);

        seed_default_rules().unwrap();
        let result = redact_with_user_rules(
            "connect to 10.0.0.1 with Bearer abc123def456ghi789jkl012mno345pqr",
        );
        assert!(result.contains("<REDACTED:ip>"), "must redact IP");
        assert!(result.contains("<REDACTED:bearer>"), "must redact bearer");

        crate::store::clear_test_config_dir();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
