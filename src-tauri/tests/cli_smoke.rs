//! CLI smoke tests — verify that the agent2ssh CLI binary parses arguments
//! correctly and exits cleanly for help / read-only commands.
//!
//! These tests use `tokio::process::Command` to run the actual CLI binary,
//! exercising clap argument parsing end-to-end without requiring any SSH
//! connectivity.

/// Path to the CLI binary, resolved at compile time by cargo.
fn cli_bin() -> std::path::PathBuf {
    env!("CARGO_BIN_EXE_agent2ssh").into()
}

// ── Help flags ────────────────────────────────────────────────────────────

#[tokio::test]
async fn cli_help_exits_zero() {
    let output = tokio::process::Command::new(cli_bin())
        .arg("--help")
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh --help should exit 0, got: {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("SSH capability layer"),
        "Help output should contain program description"
    );
}

#[tokio::test]
async fn cli_host_help_exits_zero() {
    let output = tokio::process::Command::new(cli_bin())
        .args(["host", "--help"])
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh host --help should exit 0, got: {:?}",
        output.status.code()
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("list") || stdout.contains("List"),
        "host --help should mention the 'list' subcommand"
    );
}

#[tokio::test]
async fn cli_exec_help_exits_zero() {
    let output = tokio::process::Command::new(cli_bin())
        .args(["exec", "--help"])
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh exec --help should exit 0, got: {:?}",
        output.status.code()
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--force") || stdout.contains("force"),
        "exec --help should mention the --force flag"
    );
}

// ── Read-only commands ──────────────────────────────────────────────────────

#[tokio::test]
async fn cli_host_list_json_exits_zero() {
    let output = tokio::process::Command::new(cli_bin())
        .args(["host", "list", "--json"])
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh host list --json should exit 0, got: {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Even with no hosts, should produce valid JSON (empty array)
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(stdout.trim());
    assert!(
        parsed.is_ok(),
        "host list --json should produce valid JSON, got: {}",
        stdout
    );
    assert!(
        parsed.unwrap().is_array(),
        "host list --json should produce a JSON array"
    );
}

#[tokio::test]
async fn cli_risk_json_exits_zero() {
    let output = tokio::process::Command::new(cli_bin())
        .args(["risk", "ls", "--json"])
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh risk \"ls\" --json should exit 0, got: {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("risk --json should produce valid JSON");

    assert_eq!(
        parsed["risk_level"], "low",
        "'ls' should be classified as low risk"
    );
}

#[tokio::test]
async fn cli_risk_blocked_command_json() {
    let output = tokio::process::Command::new(cli_bin())
        .args(["risk", "rm -rf /", "--json"])
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh risk \"rm -rf /\" --json should exit 0"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("risk --json should produce valid JSON");

    assert_eq!(
        parsed["risk_level"], "blocked",
        "'rm -rf /' should be classified as blocked"
    );
}

#[tokio::test]
async fn cli_audit_json_exits_zero() {
    let output = tokio::process::Command::new(cli_bin())
        .args(["audit", "--json"])
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh audit --json should exit 0, got: {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(stdout.trim());
    assert!(
        parsed.is_ok(),
        "audit --json should produce valid JSON, got: {}",
        stdout
    );
    assert!(
        parsed.unwrap().is_array(),
        "audit --json should produce a JSON array"
    );
}

// ── Subcommand coverage ────────────────────────────────────────────────────

#[tokio::test]
async fn cli_session_help_exits_zero() {
    let output = tokio::process::Command::new(cli_bin())
        .args(["session", "--help"])
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh session --help should exit 0"
    );
}

#[tokio::test]
async fn cli_forward_help_exits_zero() {
    let output = tokio::process::Command::new(cli_bin())
        .args(["forward", "--help"])
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh forward --help should exit 0"
    );
}

#[tokio::test]
async fn cli_sftp_help_exits_zero() {
    let output = tokio::process::Command::new(cli_bin())
        .args(["sftp", "--help"])
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh sftp --help should exit 0"
    );
}

#[tokio::test]
async fn cli_daemon_help_exits_zero() {
    let output = tokio::process::Command::new(cli_bin())
        .args(["daemon", "--help"])
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh daemon --help should exit 0"
    );
}

// ── Error cases ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn cli_no_args_exits_nonzero() {
    let output = tokio::process::Command::new(cli_bin())
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        !output.status.success(),
        "agent2ssh with no args should exit non-zero (missing subcommand)"
    );
}

#[tokio::test]
async fn cli_unknown_subcommand_exits_nonzero() {
    let output = tokio::process::Command::new(cli_bin())
        .arg("nonexistent-subcommand")
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        !output.status.success(),
        "agent2ssh with unknown subcommand should exit non-zero"
    );
}

// ── Daemon status (no daemon required) ──────────────────────────────────────

#[tokio::test]
async fn cli_daemon_status_exits_cleanly() {
    // The daemon may not be running in CI, but the command should at least
    // parse its arguments and exit without panicking.
    let output = tokio::process::Command::new(cli_bin())
        .args(["daemon", "status"])
        .output()
        .await
        .expect("failed to run CLI binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panic"),
        "daemon status should not panic, stderr: {}",
        stderr
    );
}

// ── Version flag ────────────────────────────────────────────────────────────

#[tokio::test]
async fn cli_version_flag() {
    let output = tokio::process::Command::new(cli_bin())
        .arg("--version")
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh --version should exit 0, got: {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("agent2ssh"),
        "--version output should contain the binary name, got: {}",
        stdout
    );
}
