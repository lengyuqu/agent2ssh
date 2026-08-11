//! Structured error codes for machine-consumable error handling.
//!
//! ## Why structured errors?
//!
//! The existing codebase uses `anyhow::Error` for all error paths. While
//! convenient for development, anyhow errors are free-form strings that
//! the frontend cannot programmatically act on — it can only display the
//! raw text. This module introduces:
//!
//! - `ErrorCode`: a stable, serializable enum of known error categories
//! - `CodedError`: a structured error carrying a code + i18n params
//! - Conversion helpers to bridge anyhow errors into structured errors
//!
//! ## Design principles
//!
//! 1. **Stable codes**: Frontend can match on `code` to show conditional UI
//!    (retry button for timeouts, login prompt for auth failures, etc.)
//! 2. **i18n params**: Each error carries structured params for placeholder
//!    replacement in localized messages, not hardcoded English text
//! 3. **Gradual migration**: Existing `anyhow` code paths continue to work;
//!    structured errors are introduced at key boundaries (SSH connection,
//!    authentication, SFTP, exec timeout)
//! 4. **Wire format**: Serialized as `__agent2ssh_err__|{json}` so frontend
//!    can detect and parse it, falling back to raw string for non-coded errors

use serde::{Serialize, Serializer};
use serde_json::json;
use thiserror::Error;

/// Stable error codes that the frontend can match on.
///
/// Each variant corresponds to a category of error that may require
/// different user-facing behavior (retry, re-auth, show config, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// SSH connection failed (network unreachable, connection refused, etc.)
    SshConnectFailed,
    /// SSH authentication rejected (wrong password, key not authorized, etc.)
    SshAuthFailed,
    /// SSH host key verification failed (unknown host, key changed)
    SshHostKeyMismatch,
    /// SSH session timed out
    SshSessionTimeout,
    /// SFTP operation failed (file not found, permission denied, etc.)
    SftpFailed,
    /// Command execution timed out
    ExecTimeout,
    /// Command was blocked by risk classification
    CommandBlocked,
    /// Host profile not found
    HostNotFound,
    /// Configuration file error (corrupt, missing, invalid)
    ConfigError,
    /// IO error (disk, network, permission)
    IoError,
    /// Internal error (unexpected panic, invariant violation)
    Internal,
    /// Forward (port tunnel) error
    ForwardError,
    /// Approval flow was rejected or timed out
    ApprovalRejected,
    /// Policy denied the operation
    PolicyDenied,
    /// Rate limit exceeded
    RateLimited,
}

impl ErrorCode {
    /// Convert to the string representation used in the wire format.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SshConnectFailed => "ssh_connect_failed",
            Self::SshAuthFailed => "ssh_auth_failed",
            Self::SshHostKeyMismatch => "ssh_host_key_mismatch",
            Self::SshSessionTimeout => "ssh_session_timeout",
            Self::SftpFailed => "sftp_failed",
            Self::ExecTimeout => "exec_timeout",
            Self::CommandBlocked => "command_blocked",
            Self::HostNotFound => "host_not_found",
            Self::ConfigError => "config_error",
            Self::IoError => "io_error",
            Self::Internal => "internal_error",
            Self::ForwardError => "forward_error",
            Self::ApprovalRejected => "approval_rejected",
            Self::PolicyDenied => "policy_denied",
            Self::RateLimited => "rate_limited",
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A structured error carrying a stable code and i18n params.
///
/// The `Display` implementation produces a wire format:
/// `__agent2ssh_err__|{"code":"ssh_connect_failed","params":{"host":"..."}}`
///
/// Frontend can detect this prefix, parse the JSON, and look up a localized
/// message using `code` as the i18n key and `params` for placeholder replacement.
#[derive(Debug, Clone, Error)]
#[error("{wire}")]
pub struct CodedError {
    pub code: ErrorCode,
    pub params: serde_json::Value,
    /// Pre-computed wire format string.
    wire: String,
}

impl CodedError {
    /// Create a new structured error with the given code and params.
    pub fn new(code: ErrorCode, params: serde_json::Value) -> Self {
        let wire = format!(
            "{}|{}",
            WIRE_PREFIX,
            json!({ "code": code.as_str(), "params": &params })
        );
        Self { code, params, wire }
    }

    /// Create an error with no params.
    pub fn empty(code: ErrorCode) -> Self {
        Self::new(code, json!({}))
    }

    /// Create an error with a single string param.
    pub fn with_detail(code: ErrorCode, detail: impl Into<String>) -> Self {
        Self::new(code, json!({ "detail": detail.into() }))
    }

    /// Extract the wire-format string.
    pub fn wire(&self) -> &str {
        &self.wire
    }
}

/// Wire format prefix. Frontend detects this to distinguish structured
/// errors from raw anyhow strings.
pub const WIRE_PREFIX: &str = "__agent2ssh_err__";

/// Check if an error string is in the structured wire format.
pub fn is_coded_error(s: &str) -> bool {
    s.starts_with(WIRE_PREFIX)
}

/// Parse a wire-format error string back into its components.
/// Returns `None` if the string is not in the structured format.
pub fn parse_coded_error(s: &str) -> Option<(ErrorCode, serde_json::Value)> {
    let payload = s.strip_prefix(WIRE_PREFIX)?;
    // Skip the `|` separator
    let json_str = payload.strip_prefix('|')?;
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let code_str = value.get("code")?.as_str()?;
    let code = match code_str {
        "ssh_connect_failed" => ErrorCode::SshConnectFailed,
        "ssh_auth_failed" => ErrorCode::SshAuthFailed,
        "ssh_host_key_mismatch" => ErrorCode::SshHostKeyMismatch,
        "ssh_session_timeout" => ErrorCode::SshSessionTimeout,
        "sftp_failed" => ErrorCode::SftpFailed,
        "exec_timeout" => ErrorCode::ExecTimeout,
        "command_blocked" => ErrorCode::CommandBlocked,
        "host_not_found" => ErrorCode::HostNotFound,
        "config_error" => ErrorCode::ConfigError,
        "io_error" => ErrorCode::IoError,
        "internal_error" => ErrorCode::Internal,
        "forward_error" => ErrorCode::ForwardError,
        "approval_rejected" => ErrorCode::ApprovalRejected,
        "policy_denied" => ErrorCode::PolicyDenied,
        "rate_limited" => ErrorCode::RateLimited,
        _ => return None,
    };
    let params = value.get("params").cloned().unwrap_or(json!({}));
    Some((code, params))
}

/// Serialize CodedError as a string for Tauri command returns.
/// Tauri expects errors to be serializable; we serialize as the wire-format
/// string so the frontend can parse it with `parse_coded_error`.
impl Serialize for CodedError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.wire)
    }
}

/// Convenience functions for common error categories.
impl CodedError {
    pub fn ssh_connect_failed(host: &str, err: &str) -> Self {
        Self::new(
            ErrorCode::SshConnectFailed,
            json!({ "host": host, "err": err }),
        )
    }

    pub fn ssh_auth_failed(host: &str, method: &str) -> Self {
        Self::new(
            ErrorCode::SshAuthFailed,
            json!({ "host": host, "method": method }),
        )
    }

    pub fn ssh_host_key_mismatch(host: &str) -> Self {
        Self::new(ErrorCode::SshHostKeyMismatch, json!({ "host": host }))
    }

    pub fn exec_timeout(command: &str, timeout_secs: u64) -> Self {
        Self::new(
            ErrorCode::ExecTimeout,
            json!({ "command": command, "timeout_secs": timeout_secs }),
        )
    }

    pub fn command_blocked(command: &str, risk: &str) -> Self {
        Self::new(
            ErrorCode::CommandBlocked,
            json!({ "command": command, "risk": risk }),
        )
    }

    pub fn host_not_found(name: &str) -> Self {
        Self::new(ErrorCode::HostNotFound, json!({ "name": name }))
    }
}

/// Type alias for results that may carry a CodedError.
/// Can be used alongside anyhow::Result — CodedError implements
/// `std::error::Error` and can be converted into anyhow if needed.
pub type CodedResult<T> = Result<T, CodedError>;

/// Extension trait to convert anyhow errors into CodedError at key boundaries.
/// This is for gradual migration — callers that want structured errors can
/// use `.to_coded(ErrorCode::...)` at the appropriate boundary.
pub trait AnyhowToCodedExt {
    fn to_coded(self, code: ErrorCode) -> CodedError;
}

impl AnyhowToCodedExt for anyhow::Error {
    fn to_coded(self, code: ErrorCode) -> CodedError {
        let detail = format!("{:#}", self);
        CodedError::with_detail(code, detail)
    }
}

/// Walk the full error chain and join each level's `Display` output with
/// `: `. Mirrors rssh's `error_chain()` so the root cause (e.g.
/// "certificate verify failed") is not lost when wrapping errors.
///
/// Example: `error_chain(&anyhow::Error::msg("outer").context("inner"))`
/// → `"inner: outer"`
pub fn error_chain(err: &dyn std::error::Error) -> String {
    let mut parts = Vec::new();
    let mut current: Option<&dyn std::error::Error> = Some(err);
    while let Some(e) = current {
        parts.push(format!("{e}"));
        current = e.source();
    }
    parts.join(": ")
}

/// Same as `error_chain` but takes `anyhow::Error`.
pub fn error_chain_anyhow(err: &anyhow::Error) -> String {
    error_chain(err.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Wire format ────────────────────────────────────────────────────

    #[test]
    fn coded_error_display_format() {
        let e = CodedError::ssh_connect_failed("my-host", "connection refused");
        let s = e.to_string();
        assert!(s.starts_with(WIRE_PREFIX));
        let payload = s
            .strip_prefix(WIRE_PREFIX)
            .unwrap()
            .strip_prefix('|')
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(payload).unwrap();
        assert_eq!(v["code"], "ssh_connect_failed");
        assert_eq!(v["params"]["host"], "my-host");
        assert_eq!(v["params"]["err"], "connection refused");
    }

    #[test]
    fn coded_error_empty_params() {
        let e = CodedError::empty(ErrorCode::SshSessionTimeout);
        let s = e.to_string();
        assert!(s.starts_with(WIRE_PREFIX));
        let payload = s
            .strip_prefix(WIRE_PREFIX)
            .unwrap()
            .strip_prefix('|')
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(payload).unwrap();
        assert_eq!(v["code"], "ssh_session_timeout");
        assert!(v["params"].as_object().unwrap().is_empty());
    }

    // ── Parsing ─────────────────────────────────────────────────────────

    #[test]
    fn parse_valid_coded_error() {
        let e = CodedError::ssh_auth_failed("web-1", "password");
        let s = e.to_string();
        let (code, params) = parse_coded_error(&s).unwrap();
        assert_eq!(code, ErrorCode::SshAuthFailed);
        assert_eq!(params["host"], "web-1");
        assert_eq!(params["method"], "password");
    }

    #[test]
    fn parse_non_coded_string_returns_none() {
        assert!(parse_coded_error("just a regular error message").is_none());
        assert!(parse_coded_error("").is_none());
    }

    // ── Serialization ───────────────────────────────────────────────────

    #[test]
    fn serialize_produces_json_string() {
        let e = CodedError::host_not_found("missing-host");
        let json = serde_json::to_string(&e).unwrap();
        // Should be a JSON string literal containing the wire format
        assert!(json.starts_with("\""));
        assert!(json.contains(WIRE_PREFIX));
        assert!(json.contains("host_not_found"));
    }

    // ── All codes round-trip ──────────────────────────────────────────

    #[test]
    fn all_error_codes_round_trip_through_wire() {
        let codes = [
            ErrorCode::SshConnectFailed,
            ErrorCode::SshAuthFailed,
            ErrorCode::SshHostKeyMismatch,
            ErrorCode::SshSessionTimeout,
            ErrorCode::SftpFailed,
            ErrorCode::ExecTimeout,
            ErrorCode::CommandBlocked,
            ErrorCode::HostNotFound,
            ErrorCode::ConfigError,
            ErrorCode::IoError,
            ErrorCode::Internal,
            ErrorCode::ForwardError,
            ErrorCode::ApprovalRejected,
            ErrorCode::PolicyDenied,
            ErrorCode::RateLimited,
        ];
        for code in &codes {
            let e = CodedError::empty(code.clone());
            let s = e.to_string();
            let (parsed_code, _) = parse_coded_error(&s).expect("round-trip failed");
            assert_eq!(&parsed_code, code);
        }
    }

    // ── Convenience constructors ────────────────────────────────────────

    #[test]
    fn exec_timeout_carries_command_and_duration() {
        let e = CodedError::exec_timeout("rm -rf /tmp", 30);
        assert_eq!(e.code, ErrorCode::ExecTimeout);
        assert_eq!(e.params["command"], "rm -rf /tmp");
        assert_eq!(e.params["timeout_secs"], 30);
    }

    #[test]
    fn command_blocked_carries_risk_level() {
        let e = CodedError::command_blocked("mkfs /dev/sda", "blocked");
        assert_eq!(e.code, ErrorCode::CommandBlocked);
        assert_eq!(e.params["risk"], "blocked");
    }

    // ── Detection ───────────────────────────────────────────────────────

    #[test]
    fn is_coded_error_detects_wire_prefix() {
        let e = CodedError::empty(ErrorCode::Internal);
        assert!(is_coded_error(&e.to_string()));
        assert!(!is_coded_error("plain error"));
    }

    // ── Wire format byte-level pinning ────────────────────────────────
    // These tests pin the exact wire-format bytes so that changes to serde,
    // Display, or field ordering are caught immediately rather than causing
    // silent frontend/backend protocol drift.

    #[test]
    fn wire_format_exact_bytes_ssh_connect_failed() {
        let e = CodedError::ssh_connect_failed("web-1", "timeout");
        let s = e.to_string();
        // Exact expected string — every byte matters.
        let expected = r#"__agent2ssh_err__|{"code":"ssh_connect_failed","params":{"err":"timeout","host":"web-1"}}"#;
        assert_eq!(s, expected);
    }

    #[test]
    fn wire_format_exact_bytes_empty_params() {
        let e = CodedError::empty(ErrorCode::SshSessionTimeout);
        let s = e.to_string();
        let expected = r#"__agent2ssh_err__|{"code":"ssh_session_timeout","params":{}}"#;
        assert_eq!(s, expected);
    }

    #[test]
    fn wire_format_exact_bytes_command_blocked() {
        let e = CodedError::command_blocked("rm -rf /", "blocked");
        let s = e.to_string();
        let expected = r#"__agent2ssh_err__|{"code":"command_blocked","params":{"command":"rm -rf /","risk":"blocked"}}"#;
        assert_eq!(s, expected);
    }

    #[test]
    fn wire_format_exact_bytes_host_not_found() {
        let e = CodedError::host_not_found("missing");
        let s = e.to_string();
        let expected = r#"__agent2ssh_err__|{"code":"host_not_found","params":{"name":"missing"}}"#;
        assert_eq!(s, expected);
    }

    #[test]
    fn wire_prefix_constant_is_stable() {
        assert_eq!(WIRE_PREFIX, "__agent2ssh_err__");
        assert_eq!(WIRE_PREFIX.len(), 17);
    }

    #[test]
    fn wire_format_json_field_order_is_code_then_params() {
        // Ensure "code" always precedes "params" in the JSON payload.
        // Frontend parsers depend on this ordering for fast-path detection.
        let e = CodedError::ssh_auth_failed("host", "password");
        let s = e.to_string();
        let json_part = s.strip_prefix("__agent2ssh_err__|").unwrap();
        let code_pos = json_part.find(r#""code":"#).unwrap();
        let params_pos = json_part.find(r#""params":"#).unwrap();
        assert!(
            code_pos < params_pos,
            "code must precede params in wire format"
        );
    }

    // ── error_chain ─────────────────────────────────────────────────────

    #[test]
    fn error_chain_joins_nested_sources() {
        use thiserror::Error;
        #[derive(Debug, Error)]
        #[error("root cause: connection refused")]
        struct Root;

        #[derive(Debug, Error)]
        #[error("ssh connect failed")]
        struct Middle {
            #[source]
            inner: Root,
        }

        let err = Middle { inner: Root };
        let chain = error_chain(&err);
        assert_eq!(chain, "ssh connect failed: root cause: connection refused");
    }

    #[test]
    fn error_chain_single_error_no_join() {
        let err = anyhow::anyhow!("standalone error");
        let chain = error_chain_anyhow(&err);
        assert_eq!(chain, "standalone error");
    }

    #[test]
    fn error_chain_anyhow_with_context() {
        let err = anyhow::anyhow!("tcp timeout")
            .context("failed to connect to host")
            .context("ssh session setup failed");
        let chain = error_chain_anyhow(&err);
        assert!(chain.contains("ssh session setup failed"));
        assert!(chain.contains("failed to connect to host"));
        assert!(chain.contains("tcp timeout"));
    }
}
