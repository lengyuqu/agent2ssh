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

/// Error type for redaction rule validation and rules-file lifecycle.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RedactRuleError {
    /// The regex pattern is syntactically invalid.
    #[error("invalid regex pattern: {0}")]
    InvalidRegex(String),

    /// The regex pattern can match the empty string (zero-width),
    /// which would cause spurious replacements everywhere.
    #[error("zero-width pattern: {0}")]
    ZeroWidth(String),

    /// Reading or writing `redact_rules.json` failed.
    ///
    /// The A24 lifecycle used to report these as `InvalidRegex`, because the
    /// enum had no other variant — a missing config directory surfaced to the
    /// user as "invalid regex pattern: cannot determine config directory".
    #[error("redaction rules file: {0}")]
    IoError(String),

    /// The rules file is not valid JSON, or a rule in it is malformed.
    #[error("malformed redaction rules: {0}")]
    ParseError(String),

    /// A rule for this pattern is already in the set. The pattern *is* a rule's
    /// identity, so a second one would make edit and delete ambiguous.
    #[error("a rule for this pattern already exists: {0}")]
    Duplicate(String),

    /// No rule in the set has this pattern.
    #[error("no redaction rule matches this pattern: {0}")]
    NotFound(String),
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

/// The built-in rule set, as literal pattern/replacement pairs.
///
/// `default_rules()` compiles these; `is_builtin_pattern` compares against them
/// as plain strings, so asking "is this one of the built-ins?" costs no regex
/// compilation. One array means the seeded set and the built-in set cannot
/// drift apart.
///
/// Order matters: `fe80::` must be tried before `::1`, or `::1` matches inside
/// `fe80::1` and leaves a stray `0:1`.
const BUILTIN_RULES: &[(&str, &str)] = &[
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
];

/// The default redaction rule set: [`BUILTIN_RULES`], compiled.
///
/// These become the live rule set on first run (via `seed_default_rules`) and
/// are the fallback whenever the rules file is missing or unreadable. Handing
/// an empty list to `redact_with_rules` is the only way to run with no rules.
///
/// Each rule uses a regex pattern and a literal replacement (via NoExpand).
/// No capture groups are expanded — the replacement string is used verbatim.
pub fn default_rules() -> Vec<RedactRule> {
    BUILTIN_RULES
        .iter()
        .map(|(p, r)| RedactRule::new_unchecked(p, r))
        .collect()
}

/// Whether `pattern` is one of the built-in rules.
///
/// The desktop UI marks these and says more before deleting one: removing a
/// built-in rule stops a whole class of secret being redacted everywhere the
/// app writes, exported audit logs included.
pub fn is_builtin_pattern(pattern: &str) -> bool {
    BUILTIN_RULES.iter().any(|(p, _)| *p == pattern)
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
// This mirrors `copy_redact.rs` — likewise a seed-once, owner-only file that
// the live redaction path reads — and rssh's `db/highlight.rs:221`.
//
// This lifecycle used to be dead code: it was written, tested and never
// called, because the live redaction path (`store::redact_sensitive_text`,
// and through it audit records, notifications, playbook results and the
// diagnostic export) called `redact_default` unconditionally. Editing
// `redact_rules.json` therefore changed nothing. That path now reads the file,
// so this module is the single source of truth for what the app redacts.

/// The JSON file name for user-editable redaction rules.
const REDACT_RULES_FILE: &str = "redact_rules.json";

/// Path to the user-editable redaction rules file.
fn redact_rules_path() -> Result<std::path::PathBuf, RedactRuleError> {
    crate::store::config_dir()
        .map(|d| d.join(REDACT_RULES_FILE))
        .map_err(|e| RedactRuleError::IoError(e.to_string()))
}

/// Write a rule set to the rules file, owner-only.
///
/// Goes through `store::write_private_json` for the same reason the other two
/// rule files do: this file decides what the app redacts, so it must not be
/// readable by other local accounts. A24 originally used a bare `fs::write`,
/// which lands at whatever the umask allows (`0644` under the default `022`).
fn write_rules_file(path: &std::path::Path, rules: &[RedactRule]) -> Result<(), RedactRuleError> {
    let configs: Vec<RedactRuleConfig> = rules.iter().map(RedactRuleConfig::from).collect();
    crate::store::write_private_json(path, &configs)
        .map_err(|e| RedactRuleError::IoError(e.to_string()))
}

/// A24: Seed the default rules to a JSON file **only if the file does not
/// already exist**. Once the file exists, the user owns it — deleted rules
/// stay deleted.
///
/// This function is idempotent: if the file already exists, it does nothing.
pub fn seed_default_rules() -> Result<(), RedactRuleError> {
    let path = redact_rules_path()?;
    if path.exists() {
        // Already seeded — user may have customized it. Do not overwrite.
        return Ok(());
    }
    write_rules_file(&path, &default_rules())
}

/// A24: Load user-editable rules from the config file. If the file doesn't
/// exist yet, seed it first (one-time) and then load.
///
/// Returns the rules from the file, which may differ from the hardcoded
/// defaults if the user edited them. A missing, unreadable or malformed file
/// falls back to `default_rules()` — redaction must never be *weaker* because
/// the config is broken.
///
/// This is deliberately uncached. It reads and recompiles on every call, which
/// is what `default_rules()` already did on the same path, and it keeps an
/// externally edited file effective immediately. A mtime-keyed cache would
/// have to cope with filesystem mtime granularity, where "write the file then
/// read it back in the same tick" is not guaranteed to invalidate — the exact
/// shape that makes a test flaky.
pub fn load_user_rules() -> Vec<RedactRule> {
    let _ = seed_default_rules();
    let path = match redact_rules_path() {
        Ok(p) => p,
        Err(_) => return default_rules(),
    };
    match std::fs::read_to_string(&path) {
        Ok(json) => load_rules_from_json(&json).unwrap_or_else(|_| default_rules()),
        Err(_) => default_rules(),
    }
}

/// A24: Reset the rules file to the hardcoded defaults, discarding any user
/// customizations. This is the only way to restore a rule the user deleted.
///
/// Note the direction of travel: this restores redaction rules, it never
/// removes them. An earlier revision let that asymmetry decide which operations
/// to expose — reset on every surface, add/update/delete on none — and that was
/// wrong: the same settings panel already edited its sibling
/// `highlight_rules.json` in full, so the restriction never made anything safer,
/// it only pushed editing back to a file nothing pointed at. The asymmetry now
/// decides *framing* instead: removal has to name the class of secret that stops
/// being redacted, and this is the operation that puts it back. All four
/// operations reach every surface.
pub fn reset_default_rules() -> Result<(), RedactRuleError> {
    let path = redact_rules_path()?;
    write_rules_file(&path, &default_rules())
}

/// A24: Redact using the user-editable rules (loaded from file), falling
/// back to hardcoded defaults if the file is missing or corrupt.
///
/// This is what the live path calls — see `store::redact_sensitive_text`.
pub fn redact_with_user_rules(text: &str) -> String {
    let rules = load_user_rules();
    redact_with_rules(text, &rules)
}

// ── A24: Editing the rule set ────────────────────────────────────────────────
//
// A rule's identity is its pattern: there is no separate id, and no two rules
// may share one. Editing a rule that is not in the set, or adding one that
// already is, is an error rather than a silent no-op, so a caller working from
// a stale list learns that instead of quietly overwriting something.

/// List the live rule set, each rule tagged with whether it is built in.
pub fn list_rules() -> Vec<crate::types::RedactRuleInfo> {
    load_user_rules()
        .iter()
        .map(|rule| crate::types::RedactRuleInfo {
            pattern: rule.pattern.as_str().to_string(),
            replacement: rule.replacement.clone(),
            is_builtin: is_builtin_pattern(rule.pattern.as_str()),
        })
        .collect()
}

/// Add a rule, failing if the pattern is already present or does not compile.
pub fn insert_rule(
    pattern: &str,
    replacement: &str,
) -> Result<Vec<crate::types::RedactRuleInfo>, RedactRuleError> {
    // Validate before touching the file, so a bad pattern cannot leave a
    // half-written set behind.
    let rule = RedactRule::new(pattern, replacement)?;
    let mut rules = load_user_rules();
    if rules.iter().any(|r| r.pattern.as_str() == pattern) {
        return Err(RedactRuleError::Duplicate(pattern.to_string()));
    }
    rules.push(rule);
    write_rules_file(&redact_rules_path()?, &rules)?;
    Ok(list_rules())
}

/// Replace the rule identified by `old_pattern`.
///
/// Renaming onto a pattern that already exists is a `Duplicate`; an
/// `old_pattern` with no rule behind it is a `NotFound`.
pub fn update_rule(
    old_pattern: &str,
    pattern: &str,
    replacement: &str,
) -> Result<Vec<crate::types::RedactRuleInfo>, RedactRuleError> {
    let rule = RedactRule::new(pattern, replacement)?;
    let mut rules = load_user_rules();
    let index = rules
        .iter()
        .position(|r| r.pattern.as_str() == old_pattern)
        .ok_or_else(|| RedactRuleError::NotFound(old_pattern.to_string()))?;
    if pattern != old_pattern && rules.iter().any(|r| r.pattern.as_str() == pattern) {
        return Err(RedactRuleError::Duplicate(pattern.to_string()));
    }
    rules[index] = rule;
    write_rules_file(&redact_rules_path()?, &rules)?;
    Ok(list_rules())
}

/// Remove the rule identified by `pattern`.
///
/// Built-in rules can be removed: that is what a user-owned rule set means, and
/// `reset_default_rules` puts them back. Callers are expected to warn first —
/// removal stops that class of secret being redacted everywhere the app writes.
///
/// Note the empty-set case: removing the last rule leaves `[]`, and an empty
/// file is an empty rule set, so *nothing* is redacted afterwards. Callers are
/// expected to say so rather than present an empty list as a neutral state.
pub fn delete_rule(pattern: &str) -> Result<Vec<crate::types::RedactRuleInfo>, RedactRuleError> {
    let mut rules = load_user_rules();
    let index = rules
        .iter()
        .position(|r| r.pattern.as_str() == pattern)
        .ok_or_else(|| RedactRuleError::NotFound(pattern.to_string()))?;
    rules.remove(index);
    write_rules_file(&redact_rules_path()?, &rules)?;
    Ok(list_rules())
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

/// Load custom rules from a JSON config string. Returns an error if the JSON
/// does not parse, or if any rule fails validation (invalid regex or
/// zero-width pattern).
pub fn load_rules_from_json(json: &str) -> Result<Vec<RedactRule>, RedactRuleError> {
    let configs: Vec<RedactRuleConfig> =
        serde_json::from_str(json).map_err(|e| RedactRuleError::ParseError(e.to_string()))?;
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
        //
        // It reads `redact_rules.json` now, so this needs an isolated config
        // dir — otherwise it would seed and read the developer's real
        // `~/.agent2ssh/redact_rules.json` and fail on a customized machine.
        let _dir = crate::store::TestConfigDir::new("a24idem");
        let input = "deploy --token secret123 password=hunter2 --api-key sk-abc123def456ghi789jkl012mno345pqr";
        let once = crate::store::redact_sensitive_text(input);
        let twice = crate::store::redact_sensitive_text(&once);
        assert_eq!(once, twice, "redact_sensitive_text must be idempotent");
    }

    // ── A24: Seed-once editable default rules ──────────────────────────────

    #[test]
    fn a24_seed_creates_file_on_first_run() {
        let dir = crate::store::TestConfigDir::new("a24s");

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

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a24_seed_is_idempotent_does_not_overwrite() {
        let dir = crate::store::TestConfigDir::new("a24i");

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

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a24_reset_restores_defaults() {
        let dir = crate::store::TestConfigDir::new("a24r");

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

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a24_load_user_rules_falls_back_on_corrupt_file() {
        let dir = crate::store::TestConfigDir::new("a24c");

        // Write a corrupt JSON file.
        let rules_path = dir.join(REDACT_RULES_FILE);
        std::fs::write(&rules_path, "{{corrupt json").unwrap();

        let rules = load_user_rules();
        assert!(!rules.is_empty(), "corrupt file must fall back to defaults");
        assert_eq!(rules.len(), default_rules().len());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a24_redact_with_user_rules_works() {
        let dir = crate::store::TestConfigDir::new("a24w");

        seed_default_rules().unwrap();
        let result = redact_with_user_rules(
            "connect to 10.0.0.1 with Bearer abc123def456ghi789jkl012mno345pqr",
        );
        assert!(result.contains("<REDACTED:ip>"), "must redact IP");
        assert!(result.contains("<REDACTED:bearer>"), "must redact bearer");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a24_rules_file_is_owner_only() {
        // The rules file is written through `store::write_private_json`, so it
        // lands at 0600 like every other file under the config dir. The
        // original A24 code used a bare `fs::write` and would have produced
        // 0644 under the default umask.
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::store::TestConfigDir::new("a24perms");
        seed_default_rules().unwrap();
        let mode = std::fs::metadata(dir.join(REDACT_RULES_FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn a24_malformed_json_is_a_parse_error_not_a_regex_error() {
        // The lifecycle used to report every failure — missing config dir,
        // unwritable file, bad JSON — as `InvalidRegex`, which surfaced to the
        // user as "invalid regex pattern: <io error>".
        let err = load_rules_from_json("{{").unwrap_err();
        assert!(
            matches!(err, RedactRuleError::ParseError(_)),
            "expected ParseError, got {err:?}"
        );
    }

    /// The live path must actually read the file.
    ///
    /// Before the wiring, rewriting `redact_rules.json` changed nothing at all,
    /// because `store::redact_sensitive_text` called `redact_default`
    /// unconditionally. This asserts both directions: a rule that only exists
    /// in the file is applied, and a default rule the user deleted is gone —
    /// i.e. the file *is* the rule set, it is not merely additive.
    #[test]
    fn redact_sensitive_text_reads_the_rules_file() {
        let dir = crate::store::TestConfigDir::new("a24live");
        std::fs::write(
            dir.join(REDACT_RULES_FILE),
            r#"[{"pattern": "secret-project", "replacement": "<REDACTED:project>"}]"#,
        )
        .unwrap();

        let result = crate::store::redact_sensitive_text("connect to 10.0.0.5 for secret-project");
        assert!(
            result.contains("<REDACTED:project>"),
            "the file's own rule must be applied, got {result}"
        );
        assert!(
            result.contains("10.0.0.5"),
            "removing the default IP rule must stop IP redaction, got {result}"
        );
    }

    #[test]
    fn redact_sensitive_text_falls_back_to_defaults_on_corrupt_file() {
        let dir = crate::store::TestConfigDir::new("a24livebad");
        std::fs::write(dir.join(REDACT_RULES_FILE), "{{not json").unwrap();

        let result = crate::store::redact_sensitive_text("connect to 10.0.0.5");
        assert!(
            result.contains("<REDACTED:ip>"),
            "a broken rules file must not weaken redaction, got {result}"
        );
    }

    // ── A24: Editing the rule set ─────────────────────────────────────────

    /// `BUILTIN_RULES` exists so the badge and the seeded set cannot drift
    /// apart. If they did, a fresh install would mark a shipped rule as the
    /// user's own and drop the warning that belongs on deleting it.
    #[test]
    fn a24_every_default_rule_is_reported_as_built_in() {
        for rule in default_rules() {
            assert!(
                is_builtin_pattern(rule.pattern.as_str()),
                "{} is seeded but not marked built-in",
                rule.pattern.as_str()
            );
        }
        assert!(!is_builtin_pattern("secret-project"));
    }

    /// An add has to reach the file, because the file is what the live path
    /// reads — a list that only grew in memory would redact nothing.
    #[test]
    fn a24_insert_rule_round_trips_through_the_file() {
        let dir = crate::store::TestConfigDir::new("a24add");
        seed_default_rules().unwrap();

        let rules = insert_rule("secret-project", "<REDACTED:project>").unwrap();
        assert_eq!(rules.len(), default_rules().len() + 1);

        let added = rules
            .iter()
            .find(|r| r.pattern == "secret-project")
            .expect("the added rule must be listed");
        assert_eq!(added.replacement, "<REDACTED:project>");
        assert!(!added.is_builtin, "a user rule is not built in");

        let on_disk = std::fs::read_to_string(dir.join(REDACT_RULES_FILE)).unwrap();
        assert!(on_disk.contains("secret-project"), "file was {on_disk}");
        assert!(redact_with_user_rules("connect to secret-project").contains("<REDACTED:project>"));
    }

    #[test]
    fn a24_insert_rule_rejects_a_duplicate_without_writing() {
        let _dir = crate::store::TestConfigDir::new("a24dup");
        seed_default_rules().unwrap();

        // The pattern is a rule's identity, so a second one would make edit and
        // delete ambiguous — it is an error, not a silent overwrite.
        let existing = list_rules()[0].pattern.clone();
        assert_eq!(
            insert_rule(&existing, "<REDACTED:x>").unwrap_err(),
            RedactRuleError::Duplicate(existing.clone())
        );
        assert_eq!(list_rules().len(), default_rules().len());
    }

    #[test]
    fn a24_insert_rule_validates_before_writing() {
        let _dir = crate::store::TestConfigDir::new("a24bad");
        seed_default_rules().unwrap();

        assert!(matches!(
            insert_rule("(", "<x>").unwrap_err(),
            RedactRuleError::InvalidRegex(_)
        ));
        assert_eq!(
            insert_rule("a*", "<x>").unwrap_err(),
            RedactRuleError::ZeroWidth("a*".to_string())
        );
        assert_eq!(list_rules().len(), default_rules().len());
    }

    #[test]
    fn a24_update_rule_renames_a_rule() {
        let _dir = crate::store::TestConfigDir::new("a24ren");
        seed_default_rules().unwrap();
        insert_rule("alpha", "<A>").unwrap();

        let rules = update_rule("alpha", "beta", "<B>").unwrap();
        assert!(
            rules.iter().all(|r| r.pattern != "alpha"),
            "the old pattern must be gone"
        );
        assert_eq!(
            rules
                .iter()
                .find(|r| r.pattern == "beta")
                .expect("the new pattern must be listed")
                .replacement,
            "<B>"
        );
        assert_eq!(
            rules.len(),
            default_rules().len() + 1,
            "a rename is not an add"
        );
    }

    /// The duplicate check has to skip the rule being edited, or a
    /// replacement-only change would collide with itself.
    #[test]
    fn a24_update_rule_allows_changing_only_the_replacement() {
        let _dir = crate::store::TestConfigDir::new("a24repl");
        seed_default_rules().unwrap();
        insert_rule("alpha", "<A>").unwrap();

        let rules = update_rule("alpha", "alpha", "<A2>").unwrap();
        assert_eq!(
            rules
                .iter()
                .find(|r| r.pattern == "alpha")
                .expect("alpha must survive the edit")
                .replacement,
            "<A2>"
        );
    }

    #[test]
    fn a24_update_rule_rejects_unknown_and_colliding_patterns() {
        let _dir = crate::store::TestConfigDir::new("a24updbad");
        seed_default_rules().unwrap();
        insert_rule("alpha", "<A>").unwrap();
        insert_rule("beta", "<B>").unwrap();

        // An unknown pattern is an error rather than a silent insert, so a
        // caller working from a stale list finds out.
        assert_eq!(
            update_rule("nope", "gamma", "<G>").unwrap_err(),
            RedactRuleError::NotFound("nope".to_string())
        );
        assert_eq!(
            update_rule("alpha", "beta", "<X>").unwrap_err(),
            RedactRuleError::Duplicate("beta".to_string())
        );

        let rules = list_rules();
        assert_eq!(
            rules
                .iter()
                .find(|r| r.pattern == "alpha")
                .expect("alpha must be untouched by both rejections")
                .replacement,
            "<A>"
        );
    }

    /// Deleting is resolved by pattern, and the empty set is a real state:
    /// removing the last rule leaves `[]`, and an empty rules file means the
    /// app redacts nothing at all.
    #[test]
    fn a24_delete_rule_removes_by_pattern_and_can_empty_the_set() {
        let _dir = crate::store::TestConfigDir::new("a24del");
        seed_default_rules().unwrap();

        assert_eq!(
            delete_rule("nope").unwrap_err(),
            RedactRuleError::NotFound("nope".to_string())
        );

        // Built-in rules are deletable — that is what a user-owned set means —
        // and `reset_default_rules` is the way back.
        let shipped = list_rules()[0].pattern.clone();
        let rules = delete_rule(&shipped).unwrap();
        assert_eq!(rules.len(), default_rules().len() - 1);
        assert!(rules.iter().all(|r| r.pattern != shipped));

        let mut remaining: Vec<String> = rules.iter().map(|r| r.pattern.clone()).collect();
        while let Some(pattern) = remaining.pop() {
            delete_rule(&pattern).unwrap();
        }

        assert!(list_rules().is_empty(), "the set must be empty");
        assert!(
            redact_with_user_rules("connect to 10.0.0.5").contains("10.0.0.5"),
            "an empty rule set redacts nothing; the file must not be re-seeded"
        );

        reset_default_rules().unwrap();
        assert_eq!(list_rules().len(), default_rules().len());
    }
}
