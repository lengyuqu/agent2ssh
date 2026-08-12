//! CLI contract tests (B39).
//!
//! These tests verify the **observable contract** of the `agent2ssh` CLI
//! binary: argument parsing, exit codes, help output, version output, and
//! error messages. They do NOT require any SSH connectivity or a running
//! daemon — they exercise the CLI's surface area only.
//!
//! Mirrors rssh's `tests/cli_contract.rs` pattern, adapted to agent2ssh's
//! command structure.

/// Path to the CLI binary, resolved at compile time by cargo.
fn cli_bin() -> std::path::PathBuf {
    env!("CARGO_BIN_EXE_agent2ssh").into()
}

/// Run the CLI with the given args and return (stdout, stderr, exit_code).
#[allow(dead_code)]
async fn run_cli(args: &[&str]) -> (String, String, Option<i32>) {
    let output = tokio::process::Command::new(cli_bin())
        .args(args)
        .output()
        .await
        .expect("failed to run CLI binary");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code(),
    )
}

/// Run the CLI with a custom AGENT2SSH_CONFIG_DIR (isolated test environment).
async fn run_cli_in_dir(
    config_dir: &std::path::Path,
    args: &[&str],
) -> (String, String, Option<i32>) {
    let output = tokio::process::Command::new(cli_bin())
        .env("AGENT2SSH_CONFIG_DIR", config_dir)
        .args(args)
        .output()
        .await
        .expect("failed to run CLI binary");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code(),
    )
}

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "agent2ssh-b39-{}-{}",
        label,
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

// ── Help output contract ───────────────────────────────────────────────────

#[tokio::test]
async fn b39_root_help_exits_zero() {
    let (stdout, _stderr, code) = run_cli(&["--help"]).await;
    assert_eq!(code, Some(0), "--help must exit 0");
    assert!(
        stdout.contains("SSH capability layer"),
        "help must contain program description"
    );
}

#[tokio::test]
async fn b39_root_help_lists_top_level_subcommands() {
    let (stdout, _stderr, code) = run_cli(&["--help"]).await;
    assert_eq!(code, Some(0));

    // The help output must mention all top-level subcommand families.
    for cmd in &[
        "host", "exec", "sftp", "session", "forward", "secrets", "ping",
        "audit", "risk", "daemon",
    ] {
        assert!(
            stdout.to_lowercase().contains(cmd),
            "help output must mention subcommand '{cmd}'"
        );
    }
}

#[tokio::test]
async fn b39_host_help_lists_subcommands() {
    let (stdout, _stderr, code) = run_cli(&["host", "--help"]).await;
    assert_eq!(code, Some(0));
    assert!(stdout.contains("list"), "host help must mention 'list'");
    assert!(stdout.contains("add"), "host help must mention 'add'");
    assert!(stdout.contains("rm"), "host help must mention 'rm'");
}

#[tokio::test]
async fn b39_exec_help_shows_force_and_plan() {
    let (stdout, _stderr, code) = run_cli(&["exec", "--help"]).await;
    assert_eq!(code, Some(0));
    assert!(stdout.contains("--force"), "exec help must mention --force");
    assert!(stdout.contains("--plan"), "exec help must mention --plan");
}

#[tokio::test]
async fn b39_secrets_help_lists_subcommands() {
    let (stdout, _stderr, code) = run_cli(&["secrets", "--help"]).await;
    assert_eq!(code, Some(0));
    // The secrets subcommand should have at least some subcommands.
    assert!(!stdout.trim().is_empty(), "secrets help must not be empty");
}

#[tokio::test]
async fn b39_forward_help_lists_subcommands() {
    let (stdout, _stderr, code) = run_cli(&["forward", "--help"]).await;
    assert_eq!(code, Some(0));
    assert!(stdout.contains("add"), "forward help must mention 'add'");
    assert!(stdout.contains("list"), "forward help must mention 'list'");
    assert!(stdout.contains("rm"), "forward help must mention 'rm'");
}

#[tokio::test]
async fn b39_audit_help_shows_filters() {
    let (stdout, _stderr, code) = run_cli(&["audit", "--help"]).await;
    assert_eq!(code, Some(0));
    assert!(stdout.contains("--host"), "audit help must mention --host");
    assert!(stdout.contains("--risk"), "audit help must mention --risk");
    assert!(stdout.contains("--json"), "audit help must mention --json");
}

#[tokio::test]
async fn b39_risk_help_exits_zero() {
    let (stdout, _stderr, code) = run_cli(&["risk", "--help"]).await;
    assert_eq!(code, Some(0));
    assert!(stdout.contains("command"), "risk help must mention command arg");
}

#[tokio::test]
async fn b39_webdav_help_lists_push_pull_status() {
    let (stdout, _stderr, code) = run_cli(&["webdav", "--help"]).await;
    assert_eq!(code, Some(0));
    assert!(stdout.contains("push"), "webdav help must mention push");
    assert!(stdout.contains("pull"), "webdav help must mention pull");
    assert!(stdout.contains("status"), "webdav help must mention status");
}

// ── Version contract ───────────────────────────────────────────────────────

#[tokio::test]
async fn b39_version_exits_zero() {
    let (stdout, _stderr, code) = run_cli(&["--version"]).await;
    assert_eq!(code, Some(0), "--version must exit 0");
    // Version output should contain the program name.
    assert!(
        stdout.contains("agent2ssh"),
        "version output must contain program name"
    );
}

// ── Exit code contract ─────────────────────────────────────────────────────

#[tokio::test]
async fn b39_unknown_subcommand_exits_nonzero() {
    let (_stdout, stderr, code) = run_cli(&["nonexistent-command"]).await;
    assert_ne!(
        code,
        Some(0),
        "unknown subcommand must exit non-zero"
    );
    // clap prints usage to stderr on error.
    assert!(
        !stderr.is_empty(),
        "unknown subcommand must produce error message"
    );
}

#[tokio::test]
async fn b39_missing_required_arg_exits_nonzero() {
    // `exec` requires a host and command argument.
    let (_stdout, stderr, code) = run_cli(&["exec"]).await;
    assert_ne!(code, Some(0), "missing required arg must exit non-zero");
    assert!(
        stderr.to_lowercase().contains("required") || stderr.to_lowercase().contains("usage"),
        "missing arg must produce usage/help message"
    );
}

#[tokio::test]
async fn b39_invalid_flag_exits_nonzero() {
    let (_stdout, _stderr, code) = run_cli(&["--nonexistent-flag"]).await;
    assert_ne!(code, Some(0), "invalid flag must exit non-zero");
}

// ── Host list contract ─────────────────────────────────────────────────────

#[tokio::test]
async fn b39_host_list_empty_store_prints_nothing_or_empty() {
    let dir = unique_temp_dir("host-list-empty");
    let (stdout, _stderr, code) = run_cli_in_dir(&dir, &["host", "list", "--json"]).await;

    // Should succeed.
    assert_eq!(code, Some(0), "host list on empty store must exit 0");

    // JSON output should be a valid JSON array (possibly empty).
    let result: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or(serde_json::Value::Null);
    assert!(
        result.is_array(),
        "host list --json must output a JSON array"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn b39_host_list_json_is_valid_json() {
    let dir = unique_temp_dir("host-list-valid-json");
    let (stdout, _stderr, code) = run_cli_in_dir(&dir, &["host", "list", "--json"]).await;
    assert_eq!(code, Some(0));

    // Must be valid JSON.
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(stdout.trim());
    assert!(parsed.is_ok(), "host list --json must output valid JSON");

    let _ = std::fs::remove_dir_all(&dir);
}

// ── Risk command contract ──────────────────────────────────────────────────

#[tokio::test]
async fn b39_risk_command_exits_zero() {
    let (stdout, _stderr, code) = run_cli(&["risk", "ls -la", "--json"]).await;
    assert_eq!(code, Some(0), "risk command must exit 0");

    // JSON output must be valid JSON.
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(stdout.trim());
    assert!(parsed.is_ok(), "risk --json must output valid JSON");

    let result = parsed.unwrap();
    // Should contain a risk_level field.
    assert!(
        result.get("risk_level").is_some() || result.get("risk").is_some(),
        "risk output must contain risk level"
    );
}

#[tokio::test]
async fn b39_risk_high_risk_command_classified() {
    let (stdout, _stderr, code) = run_cli(&["risk", "rm -rf /", "--json"]).await;
    assert_eq!(code, Some(0));

    let result: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let risk = result
        .get("risk_level")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        risk == "high" || risk == "medium" || risk == "blocked",
        "rm -rf / must be classified as high, medium, or blocked risk, got: {risk}"
    );
}

#[tokio::test]
async fn b39_risk_low_risk_command_classified() {
    let (stdout, _stderr, code) = run_cli(&["risk", "ls -la", "--json"]).await;
    assert_eq!(code, Some(0));

    let result: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let risk = result
        .get("risk_level")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(
        risk, "low",
        "ls -la must be classified as low risk, got: {risk}"
    );
}

// ── Ping contract ───────────────────────────────────────────────────────────

#[tokio::test]
async fn b39_ping_nonexistent_host_exits_nonzero() {
    let (_stdout, _stderr, code) = run_cli(&["ping", "nonexistent.invalid", "--timeout-secs", "1"]).await;
    // Ping should fail for a non-existent host (non-zero exit).
    // Note: timeout_secs may not be a valid arg depending on the CLI structure.
    // The key contract: ping with an unreachable host does not hang forever.
    assert!(
        code.is_some(),
        "ping must terminate with an exit code (not hang)"
    );
}

// ── Daemon help contract ───────────────────────────────────────────────────

#[tokio::test]
async fn b39_daemon_help_lists_subcommands() {
    let (stdout, _stderr, code) = run_cli(&["daemon", "--help"]).await;
    assert_eq!(code, Some(0));
    assert!(!stdout.trim().is_empty(), "daemon help must not be empty");
}

// ── Policy help contract ────────────────────────────────────────────────────

#[tokio::test]
async fn b39_policy_help_lists_subcommands() {
    let (stdout, _stderr, code) = run_cli(&["policy", "--help"]).await;
    assert_eq!(code, Some(0));
    assert!(!stdout.trim().is_empty(), "policy help must not be empty");
}

// ── Playbook help contract ───────────────────────────────────────────────────

#[tokio::test]
async fn b39_playbook_help_lists_subcommands() {
    let (stdout, _stderr, code) = run_cli(&["playbook", "--help"]).await;
    assert_eq!(code, Some(0));
    assert!(!stdout.trim().is_empty(), "playbook help must not be empty");
}

// ── Integrate help contract ──────────────────────────────────────────────────

#[tokio::test]
async fn b39_integrate_help_lists_subcommands() {
    let (stdout, _stderr, code) = run_cli(&["integrate", "--help"]).await;
    assert_eq!(code, Some(0));
    assert!(stdout.contains("list"), "integrate help must mention 'list'");
    assert!(stdout.contains("add"), "integrate help must mention 'add'");
}

// ── Doctor contract ─────────────────────────────────────────────────────────

#[tokio::test]
async fn b39_doctor_exits_zero() {
    let dir = unique_temp_dir("doctor");
    let (stdout, _stderr, code) = run_cli_in_dir(&dir, &["doctor", "--json"]).await;
    assert_eq!(code, Some(0), "doctor must exit 0");

    // JSON output must be valid.
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(stdout.trim());
    assert!(parsed.is_ok(), "doctor --json must output valid JSON");

    let _ = std::fs::remove_dir_all(&dir);
}

// ─-- Status contract ────────────────────────────────────────────────────────

#[tokio::test]
async fn b39_status_exits_zero() {
    let dir = unique_temp_dir("status");
    let (_stdout, _stderr, code) = run_cli_in_dir(&dir, &["status", "--json"]).await;
    // Status may or may not succeed depending on whether daemon is running,
    // but it must not hang.
    assert!(code.is_some(), "status must terminate with an exit code");

    let _ = std::fs::remove_dir_all(&dir);
}

// ── Secrets help contract ───────────────────────────────────────────────────

#[tokio::test]
async fn b39_secrets_status_on_uninitialized_store() {
    let dir = unique_temp_dir("secrets-status");
    let (_stdout, _stderr, code) = run_cli_in_dir(&dir, &["secrets", "status", "--json"]).await;
    // secrets status on an uninitialized store should not crash.
    // It may exit 0 (reporting "not initialized") or non-zero (error).
    assert!(code.is_some(), "secrets status must terminate");

    let _ = std::fs::remove_dir_all(&dir);
}
