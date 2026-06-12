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

fn mcp_bin() -> std::path::PathBuf {
    env!("CARGO_BIN_EXE_agent2ssh-mcp").into()
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
    assert!(
        stdout.contains("--plan") || stdout.contains("plan"),
        "exec --help should mention the --plan flag"
    );
}

#[tokio::test]
async fn mcp_stdio_end_to_end_initialize_tools_and_risk() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::process::Command;

    let mut child = Command::new(mcp_bin())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to start MCP binary");

    let mut stdin = child.stdin.take().expect("missing MCP stdin");
    let stdout = child.stdout.take().expect("missing MCP stdout");
    let mut lines = BufReader::new(stdout).lines();

    let requests = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ssh_risk_check","arguments":{"command":"rm -rf /"}}}"#,
    ];

    for req in requests {
        stdin.write_all(req.as_bytes()).await.unwrap();
        stdin.write_all(b"\n").await.unwrap();
    }
    drop(stdin);

    let init: serde_json::Value = serde_json::from_str(
        &lines.next_line().await.unwrap().expect("missing initialize response"),
    )
    .unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "agent2ssh-mcp");

    let tools: serde_json::Value =
        serde_json::from_str(&lines.next_line().await.unwrap().expect("missing tools response"))
            .unwrap();
    let tool_count = tools["result"]["tools"].as_array().unwrap().len();
    assert_eq!(tool_count, 50);

    let risk: serde_json::Value =
        serde_json::from_str(&lines.next_line().await.unwrap().expect("missing risk response"))
            .unwrap();
    assert_eq!(risk["result"]["structuredContent"]["risk_level"], "blocked");

    let status = child.wait().await.expect("failed waiting for MCP process");
    assert!(status.success());
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
async fn cli_host_list_filters_by_metadata_and_tag() {
    let config_dir = std::env::temp_dir().join(format!(
        "agent2ssh-cli-filter-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&config_dir).expect("create temp config dir");

    let add = tokio::process::Command::new(cli_bin())
        .env("AGENT2SSH_CONFIG_DIR", &config_dir)
        .args([
            "host",
            "add",
            "prod-web-1",
            "--host",
            "10.0.0.1",
            "--env",
            "prod",
            "--role",
            "web",
            "--owner",
            "platform",
            "--tags",
            "blue,web",
            "--json",
        ])
        .output()
        .await
        .expect("failed to run CLI binary");
    assert!(
        add.status.success(),
        "host add should exit 0, stderr: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    let list = tokio::process::Command::new(cli_bin())
        .env("AGENT2SSH_CONFIG_DIR", &config_dir)
        .args([
            "host",
            "list",
            "--env",
            "PROD",
            "--role",
            "web",
            "--owner",
            "platform",
            "--tag",
            "blue",
            "--json",
        ])
        .output()
        .await
        .expect("failed to run CLI binary");
    assert!(
        list.status.success(),
        "host list filter should exit 0, stderr: {}",
        String::from_utf8_lossy(&list.stderr)
    );
    let hosts: serde_json::Value =
        serde_json::from_slice(&list.stdout).expect("host list should return JSON");
    assert_eq!(hosts.as_array().unwrap().len(), 1);
    assert_eq!(hosts[0]["name"], "prod-web-1");
    assert_eq!(hosts[0]["env"], "prod");
    assert_eq!(hosts[0]["role"], "web");
    assert_eq!(hosts[0]["owner"], "platform");

    let empty = tokio::process::Command::new(cli_bin())
        .env("AGENT2SSH_CONFIG_DIR", &config_dir)
        .args(["host", "list", "--env", "staging", "--json"])
        .output()
        .await
        .expect("failed to run CLI binary");
    let hosts: serde_json::Value =
        serde_json::from_slice(&empty.stdout).expect("host list should return JSON");
    assert!(hosts.as_array().unwrap().is_empty());

    let _ = std::fs::remove_dir_all(config_dir);
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

#[tokio::test]
async fn cli_daemon_rotate_token_help_exits_zero() {
    let output = tokio::process::Command::new(cli_bin())
        .args(["daemon", "rotate-token", "--help"])
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh daemon rotate-token --help should exit 0, got: {:?}",
        output.status.code()
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Rotate daemon token"),
        "rotate-token help should mention token rotation"
    );
}

#[tokio::test]
async fn cli_health_help_exits_zero() {
    let output = tokio::process::Command::new(cli_bin())
        .args(["health", "--help"])
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh health --help should exit 0, got: {:?}",
        output.status.code()
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--json") || stdout.contains("json"),
        "health --help should mention the --json flag"
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

// ── Policy subcommand smoke tests ────────────────────────────────────────────

#[tokio::test]
async fn cli_policy_help_exits_zero() {
    let output = tokio::process::Command::new(cli_bin())
        .args(["policy", "--help"])
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh policy --help should exit 0, got: {:?}",
        output.status.code()
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("list") || stdout.contains("add") || stdout.contains("Manage"),
        "policy --help should mention subcommands, got: {}",
        stdout
    );
}

#[tokio::test]
async fn cli_policy_list_json_exits_zero() {
    let output = tokio::process::Command::new(cli_bin())
        .args(["policy", "list", "--json"])
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh policy list --json should exit 0, got: {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(stdout.trim());
    assert!(
        parsed.is_ok(),
        "policy list --json should produce valid JSON, got: {}",
        stdout
    );
    assert!(
        parsed.unwrap().is_array(),
        "policy list --json should produce a JSON array"
    );
}
