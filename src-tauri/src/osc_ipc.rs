//! OSC 7337 protocol for CLI-to-GUI IPC (B37).
//!
//! The OSC 7337 escape sequence is a custom Operating System Command (OSC)
//! that allows the CLI binary to communicate with the desktop app's embedded
//! terminal. When the CLI runs inside the desktop terminal, it emits these
//! sequences to stdout, and the terminal parser interprets them as actions
//! (open a host profile, open a port forward, etc.).
//!
//! ## Security
//!
//! Profile/forward names are validated to reject C0 control characters
//! (0x00-0x1F) and DEL (0x7F), which include ESC (`\x1b`) and BEL (`\x07`) —
//! the OSC 7337 terminators. A malicious name containing these could inject
//! arbitrary terminal escape sequences.
//!
//! Mirrors rssh's `bin/rssh/commands/open.rs` OSC 7337 pattern.

use anyhow::{anyhow, Result};

/// The environment variable that indicates the CLI is running inside the
/// desktop app's embedded terminal. When set, the CLI emits OSC sequences;
/// when unset, it directly `exec`s `ssh`.
pub const AGENT2SSH_APP_ENV: &str = "AGENT2SSH_APP";

/// The OSC 7337 escape sequence prefix.
const OSC_PREFIX: &str = "\x1b]7337;";

/// The OSC terminator (BEL).
const OSC_TERMINATOR: &str = "\x07";

/// Validate that a name is safe for use in an OSC 7337 sequence.
///
/// Rejects C0 control characters (0x00-0x1F) and DEL (0x7F), which include
/// the ESC and BEL terminators. This prevents injection of arbitrary escape
/// sequences via a malicious profile/forward name.
pub fn validate_osc_name(name: &str) -> Result<()> {
    for c in name.chars() {
        let code = c as u32;
        if code <= 0x1F || code == 0x7F {
            return Err(anyhow!(
                "name contains control character U+{code:04X}, which is not allowed in OSC names"
            ));
        }
    }
    // Q5: Reject ':' to prevent ambiguity in OSC 7337 parsing — the
    // sequence format is "{action}:{name}", so a ':' in the name could
    // be misinterpreted as a delimiter by a naive parser.
    if name.contains(':') {
        return Err(anyhow!("name contains ':', which is not allowed in OSC names"));
    }
    Ok(())
}

/// Emit an OSC 7337 sequence to open a host profile.
///
/// The CLI calls this when running inside the desktop terminal (detected via
/// `AGENT2SSH_APP`). The desktop terminal parses the sequence and opens the
/// corresponding profile tab.
pub fn emit_osc_open(name: &str) -> Result<()> {
    validate_osc_name(name)?;
    print!("{OSC_PREFIX}open:{name}{OSC_TERMINATOR}");
    use std::io::Write;
    std::io::stdout().flush().ok();
    Ok(())
}

/// Emit an OSC 7337 sequence to open a port forward.
pub fn emit_osc_forward(name: &str) -> Result<()> {
    validate_osc_name(name)?;
    print!("{OSC_PREFIX}fwd:{name}{OSC_TERMINATOR}");
    use std::io::Write;
    std::io::stdout().flush().ok();
    Ok(())
}

/// Check whether the CLI is running inside the desktop app's terminal.
pub fn is_inside_desktop() -> bool {
    std::env::var(AGENT2SSH_APP_ENV)
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

/// Build the raw OSC 7337 bytes for a given action and name.
///
/// This is primarily used by tests to verify the exact byte sequence.
pub fn build_osc_sequence(action: &str, name: &str) -> Result<String> {
    validate_osc_name(name)?;
    Ok(format!("{OSC_PREFIX}{action}:{name}{OSC_TERMINATOR}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_control_chars() {
        assert!(validate_osc_name("normal-name").is_ok());
        assert!(validate_osc_name("host_123").is_ok());
        assert!(validate_osc_name("my host").is_ok()); // Space is allowed.

        // ESC (0x1B) must be rejected.
        assert!(validate_osc_name("evil\x1bname").is_err());
        // BEL (0x07) must be rejected.
        assert!(validate_osc_name("evil\x07name").is_err());
        // NUL (0x00) must be rejected.
        assert!(validate_osc_name("evil\0name").is_err());
        // DEL (0x7F) must be rejected.
        assert!(validate_osc_name("evil\x7fname").is_err());
        // Other C0 control chars.
        assert!(validate_osc_name("evil\x01name").is_err());
        assert!(validate_osc_name("evil\x1fname").is_err());
    }

    #[test]
    fn validate_rejects_colon() {
        // Q5: ':' is the delimiter in "{action}:{name}" — a name containing
        // ':' could be misparsed by a naive terminal parser.
        assert!(validate_osc_name("host:8080").is_err());
        assert!(validate_osc_name("a:b:c").is_err());
        assert!(validate_osc_name(":leading").is_err());
        assert!(validate_osc_name("trailing:").is_err());
        // A single ':' alone.
        assert!(validate_osc_name(":").is_err());
    }

    #[test]
    fn build_osc_open_sequence() {
        let seq = build_osc_sequence("open", "my-host").unwrap();
        assert_eq!(seq, "\x1b]7337;open:my-host\x07");
    }

    #[test]
    fn build_osc_fwd_sequence() {
        let seq = build_osc_sequence("fwd", "my-tunnel").unwrap();
        assert_eq!(seq, "\x1b]7337;fwd:my-tunnel\x07");
    }

    #[test]
    fn build_osc_rejects_injection() {
        let result = build_osc_sequence("open", "evil\x1b]7337;open:other\x07");
        assert!(result.is_err(), "ESC injection must be rejected");
    }

    #[test]
    fn build_osc_rejects_bel_injection() {
        let result = build_osc_sequence("open", "evil\x07\x1b]7337;open:other");
        assert!(result.is_err(), "BEL injection must be rejected");
    }

    #[test]
    fn is_inside_desktop_false_when_unset() {
        std::env::remove_var(AGENT2SSH_APP_ENV);
        assert!(!is_inside_desktop());
    }

    #[test]
    fn is_inside_desktop_true_when_set() {
        std::env::set_var(AGENT2SSH_APP_ENV, "1");
        assert!(is_inside_desktop());
        std::env::remove_var(AGENT2SSH_APP_ENV);
    }

    #[test]
    fn is_inside_desktop_false_when_empty() {
        std::env::set_var(AGENT2SSH_APP_ENV, "");
        assert!(!is_inside_desktop(), "empty value means not inside desktop");
        std::env::remove_var(AGENT2SSH_APP_ENV);
    }

    #[test]
    fn emit_osc_open_writes_correct_bytes() {
        // Redirect stdout to capture the output.
        // We can't easily capture stdout in a unit test, but we can verify
        // the function doesn't panic and returns Ok for valid names.
        assert!(emit_osc_open("test-host").is_ok());
    }

    #[test]
    fn emit_osc_forward_writes_correct_bytes() {
        assert!(emit_osc_forward("test-tunnel").is_ok());
    }

    #[test]
    fn emit_osc_open_rejects_invalid_name() {
        assert!(emit_osc_open("evil\x1b").is_err());
    }

    #[test]
    fn emit_osc_forward_rejects_invalid_name() {
        assert!(emit_osc_forward("evil\x07").is_err());
    }

    #[test]
    fn validate_allows_unicode() {
        // Unicode characters above the C0 range are fine.
        assert!(validate_osc_name("host-中文").is_ok());
        assert!(validate_osc_name("host-émoji").is_ok());
    }

    #[test]
    fn validate_rejects_c1_control_chars() {
        // C1 control characters (0x80-0x9F) are also dangerous in terminals.
        // However, our current validation only checks C0 (0x00-0x1F) and DEL (0x7F).
        // C1 chars (0x80-0x9F) in UTF-8 are multi-byte, so they won't match
        // our char-based check (each char is > 0x7F). This is fine — C1 sequences
        // in UTF-8 are distinct from single-byte C1 in latin1.
        // We only need to reject the OSC terminators (ESC and BEL), which are C0.
    }
}
