//! URL scheme validation for safe external link opening (B34).
//!
//! When opening a URL in the user's default browser (e.g. via `open` on macOS,
//! `xdg-open` on Linux, or `start` on Windows), the URL scheme must be
//! validated to prevent `file://`, `javascript:`, `data:`, and other dangerous
//! schemes that could execute arbitrary code or access local files.
//!
//! Mirrors rssh's `commands/external.rs:14` `open_external_url()` pattern.

use anyhow::{anyhow, Result};

/// URL schemes that are safe to open in an external browser.
const ALLOWED_SCHEMES: &[&str] = &["http", "https"];

/// Validate that a URL has a safe scheme for external opening.
///
/// Returns `Ok(())` if the scheme is `http` or `https`. Returns an error
/// for `file://`, `javascript:`, `data:`, `vbscript:`, or any other scheme.
///
/// The check is case-insensitive and handles URLs with or without a scheme
/// prefix (schemeless URLs are rejected — they must have an explicit
/// `http://` or `https://` prefix).
pub fn validate_url_scheme(url: &str) -> Result<()> {
    let lower = url.trim().to_ascii_lowercase();

    if lower.is_empty() {
        return Err(anyhow!("URL must not be empty"));
    }

    // Find the scheme: check for "://" first, then fall back to ":"
    // (e.g. "javascript:alert(1)" or "data:text/html,...").
    let scheme = if lower.contains("://") {
        lower.split("://").next().filter(|s| !s.is_empty())
    } else if let Some(idx) = lower.find(':') {
        // Ensure the part before ':' looks like a scheme (alphabetic).
        let candidate = &lower[..idx];
        if !candidate.is_empty() && candidate.chars().all(|c| c.is_ascii_alphabetic()) {
            Some(candidate)
        } else {
            None
        }
    } else {
        None
    };

    match scheme {
        Some(s) if ALLOWED_SCHEMES.contains(&s) => Ok(()),
        Some(s) => Err(anyhow!(
            "URL scheme '{}' is not allowed (permitted: http, https)",
            s
        )),
        None => Err(anyhow!(
            "URL must have an explicit http:// or https:// scheme"
        )),
    }
}

/// Open a URL in the user's default browser after validating the scheme.
///
/// Uses the OS default handler via `std::process::Command`.
pub fn open_external_url(url: &str) -> Result<()> {
    validate_url_scheme(url)?;
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map_err(|e| anyhow!("failed to open URL: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| anyhow!("failed to open URL: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| anyhow!("failed to open URL: {e}"))?;
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        return Err(anyhow!("opening URLs is not supported on this platform"));
    }
    Ok(())
}

/// B40: Strip all ANSI escape sequences from a string. This is used to
/// sanitize user-supplied input before it's included in error messages
/// or log output, preventing OSC 52 clipboard hijacking and other
/// terminal injection attacks.
pub fn strip_ansi_escapes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Escape sequence — skip until we find a terminating byte.
            // CSI: ESC [ ... final byte (0x40-0x7E)
            // OSC: ESC ] ... ST (ESC \ or BEL)
            // Other: ESC followed by a single byte
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    // Skip until a final byte (0x40-0x7E).
                    while let Some(&c2) = chars.peek() {
                        chars.next();
                        if (c2 as u32) >= 0x40 && (c2 as u32) <= 0x7E {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    // OSC: skip until BEL (\x07) or ST (ESC \).
                    while let Some(&c2) = chars.peek() {
                        chars.next();
                        if c2 == '\x07' {
                            break;
                        }
                        if c2 == '\x1b' {
                            // Expect backslash.
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                Some(_) => {
                    // Single-byte escape (e.g. ESC c = reset).
                    chars.next();
                }
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── B34: URL scheme validation ────────────────────────────────────────

    #[test]
    fn accepts_http_url() {
        assert!(validate_url_scheme("http://example.com").is_ok());
        assert!(validate_url_scheme("http://localhost:8080/path").is_ok());
    }

    #[test]
    fn accepts_https_url() {
        assert!(validate_url_scheme("https://example.com").is_ok());
        assert!(validate_url_scheme("HTTPS://EXAMPLE.COM").is_ok());
    }

    #[test]
    fn rejects_file_scheme() {
        let result = validate_url_scheme("file:///etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not allowed"));
    }

    #[test]
    fn rejects_javascript_scheme() {
        let result = validate_url_scheme("javascript:alert(1)");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not allowed"));
    }

    #[test]
    fn rejects_data_scheme() {
        let result = validate_url_scheme("data:text/html,<script>alert(1)</script>");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_vbscript_scheme() {
        let result = validate_url_scheme("vbscript:msgbox(1)");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_schemeless_url() {
        assert!(validate_url_scheme("example.com").is_err());
        assert!(validate_url_scheme("/path/to/file").is_err());
    }

    #[test]
    fn rejects_empty_url() {
        assert!(validate_url_scheme("").is_err());
        assert!(validate_url_scheme("   ").is_err());
    }

    #[test]
    fn open_external_url_validates_first() {
        assert!(open_external_url("https://example.com").is_ok());
        assert!(open_external_url("javascript:alert(1)").is_err());
    }

    #[test]
    fn b40_strips_osc52_clipboard_hijack() {
        let malicious = "host\x1b]52;c;YWJjZGVmZ2g=\x07name";
        let cleaned = strip_ansi_escapes(malicious);
        assert!(
            !cleaned.contains('\x1b'),
            "ANSI escape byte must not appear in cleaned output"
        );
        assert_eq!(cleaned, "hostname");
    }

    #[test]
    fn b40_strips_csi_color_codes() {
        let input = "\x1b[31mred text\x1b[0m";
        let cleaned = strip_ansi_escapes(input);
        assert_eq!(cleaned, "red text");
        assert!(!cleaned.contains('\x1b'));
    }

    #[test]
    fn b40_strips_osc52_without_bell() {
        // OSC 52 terminated by ST (ESC \) instead of BEL.
        let malicious = "data\x1b]52;c;YWJj\x1b\\end";
        let cleaned = strip_ansi_escapes(malicious);
        assert!(!cleaned.contains('\x1b'));
        assert_eq!(cleaned, "dataend");
    }

    #[test]
    fn b40_strips_single_byte_escape() {
        let input = "a\x1bcb";
        let cleaned = strip_ansi_escapes(input);
        assert_eq!(cleaned, "ab");
    }

    #[test]
    fn b40_preserves_normal_text() {
        let input = "normal text without escapes";
        assert_eq!(strip_ansi_escapes(input), input);
    }

    #[test]
    fn b40_strips_multiple_osc52_injections() {
        let malicious = "\x1b]52;c;AAAA\x07\x1b]52;c;BBBB\x07";
        let cleaned = strip_ansi_escapes(malicious);
        assert!(cleaned.is_empty(), "all OSC 52 sequences must be stripped");
        assert!(!cleaned.contains('\x1b'));
    }

    #[test]
    fn b40_cli_input_does_not_leak_ansi_to_stderr() {
        // Simulate what happens when a malicious host name with ANSI escapes
        // is passed to the CLI and appears in an error message.
        let malicious_name = "evil\x1b]52;c;cGl6emE=\x07host";
        let cleaned = strip_ansi_escapes(malicious_name);
        let error_msg = format!("failed to connect to '{cleaned}'");
        assert!(
            !error_msg.contains('\x1b'),
            "stderr must not contain ANSI escape bytes"
        );
    }
}
