//! CLI smoke tests — verify that the agent2ssh CLI binary parses arguments
//! correctly and exits cleanly for help / read-only commands.
//!
//! These tests use `tokio::process::Command` to run the actual CLI binary,
//! exercising clap argument parsing end-to-end without requiring any SSH
//! connectivity.

mod common;

use agent2ssh::store::CONFIG_DIR_ENV;

/// Path to the CLI binary, resolved at compile time by cargo.
fn cli_bin() -> std::path::PathBuf {
    env!("CARGO_BIN_EXE_agent2ssh").into()
}

/// How many tools a default `tools/list` offers: the `core` group.
const DEFAULT_LISTED_TOOLS: usize = 26;
/// How many the registry holds, and `AGENT2SSH_MCP_TOOLS=all` lists.
const ALL_TOOLS: usize = 76;

fn mcp_bin() -> std::path::PathBuf {
    env!("CARGO_BIN_EXE_agent2ssh-mcp").into()
}

fn write_mcp_test_binding(config_dir: &std::path::Path, source: &str, key: &str) {
    std::fs::create_dir_all(config_dir).expect("create MCP test config dir");
    std::fs::write(
        config_dir.join("mcp_bindings.json"),
        serde_json::json!({ "keys": { source: key } }).to_string(),
    )
    .expect("write MCP binding file");
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
async fn cli_completions_generate_all_supported_shell_registrations() {
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let output = tokio::process::Command::new(cli_bin())
            .args(["completions", shell])
            .output()
            .await
            .unwrap_or_else(|error| panic!("failed to generate {shell} completions: {error}"));

        assert!(
            output.status.success(),
            "{shell} completion generation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("COMPLETE") && stdout.contains("agent2ssh"),
            "{shell} output should register dynamic agent2ssh completion, got:\n{stdout}"
        );
    }
}

#[tokio::test]
async fn cli_dynamic_completion_reads_config_candidates() {
    let config_dir = std::env::temp_dir().join(format!(
        "agent2ssh-completion-hosts-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&config_dir).expect("create completion test config dir");
    std::fs::write(
        config_dir.join("hosts.json"),
        r#"{"schema_version":1,"hosts":[{"name":"prod-web","host":"127.0.0.1"}]}"#,
    )
    .expect("write completion hosts config");
    std::fs::write(
        config_dir.join("playbooks.toml"),
        r#"[[playbooks]]
name = "deploy-web"
description = "test"
steps = ["true"]
"#,
    )
    .expect("write completion playbooks config");
    std::fs::write(
        config_dir.join("remotes.toml"),
        r#"[[remotes]]
alias = "edge-daemon"
url = "http://127.0.0.1:9"
"#,
    )
    .expect("write completion remotes config");

    // PowerShell's clap_complete protocol uses the final argument as the
    // completion cursor, which makes it portable to exercise on every OS.
    let output = tokio::process::Command::new(cli_bin())
        .env(CONFIG_DIR_ENV, &config_dir)
        .env("COMPLETE", "powershell")
        .args(["--", "agent2ssh", "exec", ""])
        .output()
        .await
        .expect("run dynamic completion protocol");

    assert!(
        output.status.success(),
        "dynamic completion failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("prod-web"),
        "dynamic host completion should read the existing hosts file"
    );

    let playbooks = tokio::process::Command::new(cli_bin())
        .env(CONFIG_DIR_ENV, &config_dir)
        .env("COMPLETE", "powershell")
        .args(["--", "agent2ssh", "playbook", "run", ""])
        .output()
        .await
        .expect("request playbook candidates");
    assert!(playbooks.status.success());
    assert!(
        String::from_utf8_lossy(&playbooks.stdout).contains("deploy-web"),
        "dynamic playbook completion should read the existing playbooks file"
    );

    let daemons = tokio::process::Command::new(cli_bin())
        .env(CONFIG_DIR_ENV, &config_dir)
        .env("COMPLETE", "powershell")
        .args(["--", "agent2ssh", "--daemon", ""])
        .output()
        .await
        .expect("request daemon candidates");
    assert!(daemons.status.success());
    let daemon_stdout = String::from_utf8_lossy(&daemons.stdout);
    assert!(daemon_stdout.contains("localhost"));
    assert!(daemon_stdout.contains("edge-daemon"));

    let _ = std::fs::remove_dir_all(config_dir);
}

#[tokio::test]
async fn cli_dynamic_completion_reads_daemon_resource_ids_with_get() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let config_dir = std::env::temp_dir().join(format!(
        "agent2ssh-completion-daemon-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&config_dir).expect("create completion daemon config dir");
    std::fs::write(config_dir.join("daemon.token"), "completion-test-token\n")
        .expect("write existing daemon token");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind completion mock daemon");
    let addr = listener.local_addr().expect("completion mock daemon addr");
    let server = tokio::spawn(async move {
        for (expected_path, body) in [
            ("/sessions", r#"[{"id":"session-123","host":"prod-web"}]"#),
            ("/forwards", r#"[{"id":"forward-456","host":"prod-web"}]"#),
        ] {
            let (mut stream, _) = listener.accept().await.expect("accept completion GET");
            let mut request = vec![0_u8; 4096];
            let read = stream
                .read(&mut request)
                .await
                .expect("read completion GET");
            let request = String::from_utf8_lossy(&request[..read]).to_lowercase();
            assert!(
                request.starts_with(&format!("get {expected_path} http/1.1")),
                "completion must use GET for {expected_path}, got:\n{request}"
            );
            assert!(request.contains("authorization: bearer completion-test-token"));

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write completion response");
        }
    });

    let sessions = tokio::process::Command::new(cli_bin())
        .env(CONFIG_DIR_ENV, &config_dir)
        .env("AGENT2SSH_DAEMON_ADDR", addr.to_string())
        .env("COMPLETE", "powershell")
        .args(["--", "agent2ssh", "session", "write", ""])
        .output()
        .await
        .expect("request session ID candidates");
    assert!(sessions.status.success());
    assert!(String::from_utf8_lossy(&sessions.stdout).contains("session-123"));

    let forwards = tokio::process::Command::new(cli_bin())
        .env(CONFIG_DIR_ENV, &config_dir)
        .env("AGENT2SSH_DAEMON_ADDR", addr.to_string())
        .env("COMPLETE", "powershell")
        .args(["--", "agent2ssh", "forward", "rm", ""])
        .output()
        .await
        .expect("request forward ID candidates");
    assert!(forwards.status.success());
    assert!(String::from_utf8_lossy(&forwards.stdout).contains("forward-456"));

    server.await.expect("completion mock daemon task");
    let _ = std::fs::remove_dir_all(config_dir);
}

#[cfg(any(unix, windows))]
#[tokio::test]
async fn cli_completion_does_not_create_config_or_daemon_state() {
    let root = std::env::temp_dir().join(format!(
        "agent2ssh-completion-read-only-{}",
        uuid::Uuid::new_v4()
    ));
    let config_dir = root.join("missing-config");
    assert!(!config_dir.exists());

    let registration = tokio::process::Command::new(cli_bin())
        .env(CONFIG_DIR_ENV, &config_dir)
        .args([
            "completions",
            if cfg!(windows) { "powershell" } else { "bash" },
        ])
        .output()
        .await
        .expect("generate completion registration");
    assert!(registration.status.success());

    let candidates = tokio::process::Command::new(cli_bin())
        .env(CONFIG_DIR_ENV, &config_dir)
        .env("COMPLETE", "powershell")
        .args(["--", "agent2ssh", "session", "write", ""])
        .output()
        .await
        .expect("request session candidates");
    assert!(candidates.status.success());

    assert!(
        !config_dir.exists(),
        "completion must not create the config directory, daemon token, or pid state"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn cli_webdav_help_exits_zero() {
    let output = tokio::process::Command::new(cli_bin())
        .args(["webdav", "--help"])
        .output()
        .await
        .expect("failed to run CLI binary");

    assert!(
        output.status.success(),
        "agent2ssh webdav --help should exit 0, got: {:?}",
        output.status.code()
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("push"), "webdav help should mention push");
    assert!(stdout.contains("pull"), "webdav help should mention pull");
    assert!(
        stdout.contains("status"),
        "webdav help should mention status"
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

    let config_dir =
        std::env::temp_dir().join(format!("agent2ssh-mcp-stdio-{}", uuid::Uuid::new_v4()));
    let source = "cli_smoke";
    let binding_key = "test-binding-key";
    write_mcp_test_binding(&config_dir, source, binding_key);

    let mut child = Command::new(mcp_bin())
        .env(CONFIG_DIR_ENV, &config_dir)
        .env("AGENT2SSH_SOURCE", source)
        .env("AGENT2SSH_BINDING_KEY", binding_key)
        // The listing depends on this; a developer's shell must not change it.
        .env_remove("AGENT2SSH_MCP_TOOLS")
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
        r#"{"jsonrpc":"2.0","id":2,"method":"ping","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"resources/list","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"resources/templates/list","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/list","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"ssh_risk_check","arguments":{"command":"rm -rf /"}}}"#,
    ];

    for req in requests {
        stdin.write_all(req.as_bytes()).await.unwrap();
        stdin.write_all(b"\n").await.unwrap();
    }
    drop(stdin);

    let init: serde_json::Value = serde_json::from_str(
        &lines
            .next_line()
            .await
            .unwrap()
            .expect("missing initialize response"),
    )
    .unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "agent2ssh-mcp");
    assert!(init["result"]["capabilities"]["resources"].is_object());

    let ping: serde_json::Value = serde_json::from_str(
        &lines
            .next_line()
            .await
            .unwrap()
            .expect("missing ping response"),
    )
    .unwrap();
    assert!(ping["result"].as_object().unwrap().is_empty());

    let resources: serde_json::Value = serde_json::from_str(
        &lines
            .next_line()
            .await
            .unwrap()
            .expect("missing resources response"),
    )
    .unwrap();
    assert_eq!(
        resources["result"]["resources"].as_array().unwrap().len(),
        0
    );

    let resource_templates: serde_json::Value = serde_json::from_str(
        &lines
            .next_line()
            .await
            .unwrap()
            .expect("missing resource templates response"),
    )
    .unwrap();
    assert_eq!(
        resource_templates["result"]["resourceTemplates"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let tools: serde_json::Value = serde_json::from_str(
        &lines
            .next_line()
            .await
            .unwrap()
            .expect("missing tools response"),
    )
    .unwrap();
    let tool_count = tools["result"]["tools"].as_array().unwrap().len();
    // A second, independent place the tool count is pinned: `daemon_integration`
    // counts the registry text and `docs/skills.md` (76, everything), this one
    // counts what a real `tools/list` returns over stdio *by default* — the core
    // group (see `agent2ssh_mcp/catalog.rs`). Both have to move together: the
    // daemon suite passing is not evidence that this one will.
    assert_eq!(tool_count, DEFAULT_LISTED_TOOLS);

    let risk: serde_json::Value = serde_json::from_str(
        &lines
            .next_line()
            .await
            .unwrap()
            .expect("missing risk response"),
    )
    .unwrap();
    assert_eq!(risk["result"]["structuredContent"]["risk_level"], "blocked");

    let status = child.wait().await.expect("failed waiting for MCP process");
    assert!(status.success());
    let _ = std::fs::remove_dir_all(config_dir);
}

#[tokio::test]
async fn mcp_remote_daemon_invalid_json_returns_contextual_error() {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use tokio::process::Command;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock daemon");
    let addr = listener.local_addr().expect("mock daemon addr");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept mock request");
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf).await.expect("read mock request");
        let body = "not-json";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write mock response");
    });

    let config_dir = std::env::temp_dir().join(format!(
        "agent2ssh-mcp-bad-response-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&config_dir).expect("create temp config dir");
    let source = "cli_smoke";
    let binding_key = "test-binding-key";
    write_mcp_test_binding(&config_dir, source, binding_key);
    std::fs::write(
        config_dir.join("remotes.toml"),
        format!(
            r#"
[[remotes]]
alias = "bad"
url = "http://{addr}"
token = "tok"
"#
        ),
    )
    .expect("write remotes.toml");

    let mut child = Command::new(mcp_bin())
        .env(CONFIG_DIR_ENV, &config_dir)
        .env("AGENT2SSH_SOURCE", source)
        .env("AGENT2SSH_BINDING_KEY", binding_key)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to start MCP binary");

    let mut stdin = child.stdin.take().expect("missing MCP stdin");
    let stdout = child.stdout.take().expect("missing MCP stdout");
    let mut lines = BufReader::new(stdout).lines();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "ssh_exec",
            "arguments": {
                "daemon_alias": "bad",
                "host": "web1",
                "command": "uptime"
            }
        }
    });
    stdin.write_all(req.to_string().as_bytes()).await.unwrap();
    stdin.write_all(b"\n").await.unwrap();
    drop(stdin);

    let response: serde_json::Value = serde_json::from_str(
        &lines
            .next_line()
            .await
            .unwrap()
            .expect("missing MCP response"),
    )
    .unwrap();
    // A tool that failed answers with a result the model reads, not with a
    // JSON-RPC error.
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], true, "{response}");
    let message = response["result"]["structuredContent"]["error"]["message"]
        .as_str()
        .expect("missing error message");
    assert!(
        message.contains("remote daemon 'bad' /exec returned invalid JSON"),
        "unexpected error message: {message}"
    );
    assert!(
        message.contains("body: not-json"),
        "error should include response body preview: {message}"
    );

    let status = child.wait().await.expect("failed waiting for MCP process");
    assert!(status.success());
    server.await.expect("mock server task");
    let _ = std::fs::remove_dir_all(config_dir);
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
    let config_dir =
        std::env::temp_dir().join(format!("agent2ssh-cli-filter-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&config_dir).expect("create temp config dir");

    let add = tokio::process::Command::new(cli_bin())
        .env(CONFIG_DIR_ENV, &config_dir)
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
        .env(CONFIG_DIR_ENV, &config_dir)
        .args([
            "host", "list", "--env", "PROD", "--role", "web", "--owner", "platform", "--tag",
            "blue", "--json",
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
        .env(CONFIG_DIR_ENV, &config_dir)
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

// ============================================================================
// S3-4: CLI help alignment with documentation
// ============================================================================

#[test]
fn cli_exec_multi_help_shows_reason_and_change_id() {
    // Verify that `exec-multi --help` documents --reason and --change-id flags,
    // matching the README and guides documentation.
    let output = std::process::Command::new(cli_bin())
        .args(["exec-multi", "--help"])
        .output()
        .expect("failed to run exec-multi --help");

    assert!(output.status.success(), "exec-multi --help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("--reason"),
        "exec-multi --help should document --reason flag, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("--change-id"),
        "exec-multi --help should document --change-id flag, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("--command"),
        "exec-multi --help should document --command flag, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("--tags"),
        "exec-multi --help should document --tags flag, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("--concurrency"),
        "exec-multi --help should document --concurrency flag, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("--max-failures"),
        "exec-multi --help should document --max-failures flag, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("--batch-size"),
        "exec-multi --help should document --batch-size flag, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("--compare"),
        "exec-multi --help should document --compare flag, got:\n{}",
        stdout
    );
}

#[test]
fn cli_playbook_run_help_shows_reason_and_change_id() {
    // Verify that `playbook run --help` documents --reason and --change-id flags.
    let output = std::process::Command::new(cli_bin())
        .args(["playbook", "run", "--help"])
        .output()
        .expect("failed to run playbook run --help");

    assert!(output.status.success(), "playbook run --help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("--reason"),
        "playbook run --help should document --reason flag, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("--change-id"),
        "playbook run --help should document --change-id flag, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("--host"),
        "playbook run --help should document --host flag, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("--params"),
        "playbook run --help should document --params flag, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("--force"),
        "playbook run --help should document --force flag, got:\n{}",
        stdout
    );
}

#[test]
fn cli_exec_help_shows_reason_and_change_id() {
    // Verify that `exec --help` documents --reason and --change-id flags.
    let output = std::process::Command::new(cli_bin())
        .args(["exec", "--help"])
        .output()
        .expect("failed to run exec --help");

    assert!(output.status.success(), "exec --help should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("--reason"),
        "exec --help should document --reason flag, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("--change-id"),
        "exec --help should document --change-id flag, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("--force"),
        "exec --help should document --force flag, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("--timeout-secs"),
        "exec --help should document --timeout-secs flag, got:\n{}",
        stdout
    );
}

#[test]
fn cli_policy_validate_accepts_unified_policy_file() {
    let dir = unique_temp_config_dir("policy-validate");
    std::fs::create_dir_all(&dir).expect("create temp config dir");
    let policy_path = dir.join("policy.toml");
    std::fs::write(
        &policy_path,
        r#"
[risk.high]
patterns = ["kubectl delete*"]

[[approval.policies]]
name = "prod high"
tags = ["prod"]
min_risk = "high"
requires_approval = true
"#,
    )
    .expect("write policy file");

    let output = std::process::Command::new(cli_bin())
        .env(CONFIG_DIR_ENV, &dir)
        .args(["policy", "validate", "--json"])
        .output()
        .expect("failed to run policy validate");

    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        output.status.success(),
        "policy validate should exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"valid\": true"),
        "policy validate output should mark the file valid, got:\n{}",
        stdout
    );
}

#[test]
fn cli_approvals_clean_dry_run_then_compact() {
    // K5: a CLI-only user has no daemon to run the hourly compaction, so
    // `approvals clean` is the compactor on demand. The dry run must not touch
    // the file, and its numbers must match what the real run reports.
    let dir = unique_temp_config_dir("approvals-clean");
    std::fs::create_dir_all(&dir).expect("create temp config dir");
    let path = dir.join("approvals.jsonl");
    let expired = serde_json::json!({
        "id": uuid::Uuid::new_v4(),
        "host": "web-1",
        "command": "systemctl restart nginx",
        "risk_level": "high",
        "requested_at": chrono::Utc::now() - chrono::Duration::days(400),
        "ttl_secs": 300,
        "status": "approved",
    })
    .to_string();
    let fresh = serde_json::json!({
        "id": uuid::Uuid::new_v4(),
        "host": "web-1",
        "command": "uptime",
        "risk_level": "low",
        "requested_at": chrono::Utc::now() - chrono::Duration::days(1),
        "ttl_secs": 300,
        "status": "pending",
    })
    .to_string();
    let original = format!("{expired}\n{fresh}\n");
    std::fs::write(&path, &original).expect("write approvals store");

    let dry = std::process::Command::new(cli_bin())
        .env(CONFIG_DIR_ENV, &dir)
        .args(["approvals", "clean", "--dry-run", "--json"])
        .output()
        .expect("failed to run approvals clean --dry-run");
    assert!(
        dry.status.success(),
        "dry-run should exit 0, stderr: {}",
        String::from_utf8_lossy(&dry.stderr)
    );
    let dry_stdout = String::from_utf8_lossy(&dry.stdout);
    assert!(
        dry_stdout.contains("\"dropped\": 1"),
        "dry-run should drop 1, got:\n{dry_stdout}"
    );
    assert!(
        dry_stdout.contains("\"kept\": 1"),
        "dry-run should keep 1, got:\n{dry_stdout}"
    );
    assert!(
        dry_stdout.contains("\"dry_run\": true"),
        "dry-run should report itself, got:\n{dry_stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        original,
        "dry-run must not rewrite the file"
    );

    let run = std::process::Command::new(cli_bin())
        .env(CONFIG_DIR_ENV, &dir)
        .args(["approvals", "clean", "--json"])
        .output()
        .expect("failed to run approvals clean");
    assert!(
        run.status.success(),
        "clean should exit 0, stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let run_stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run_stdout.contains("\"dropped\": 1"),
        "clean should drop 1, got:\n{run_stdout}"
    );

    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        after.lines().count(),
        1,
        "cleaned store should hold 1 entry, got: {after}"
    );
    assert!(
        after.contains("uptime"),
        "the fresh entry should survive, got: {after}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_policy_test_returns_block_for_unified_risk_rule() {
    let dir = unique_temp_config_dir("policy-test");
    std::fs::create_dir_all(&dir).expect("create temp config dir");
    std::fs::write(
        dir.join("policy.toml"),
        r#"
[risk.blocked]
patterns = ["terraform destroy*"]
"#,
    )
    .expect("write policy file");

    let output = std::process::Command::new(cli_bin())
        .env(CONFIG_DIR_ENV, &dir)
        .args([
            "policy",
            "test",
            "terraform destroy -auto-approve",
            "--host",
            "prod",
            "--json",
        ])
        .output()
        .expect("failed to run policy test");

    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        output.status.success(),
        "policy test should exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"decision\": \"block\""),
        "policy test should return block, got:\n{}",
        stdout
    );
}

#[test]
fn cli_policy_test_applies_host_risk_override() {
    let dir = unique_temp_config_dir("policy-host-override");
    std::fs::create_dir_all(&dir).expect("create temp config dir");

    let add = std::process::Command::new(cli_bin())
        .env(CONFIG_DIR_ENV, &dir)
        .args([
            "host",
            "add",
            "sandbox",
            "--host",
            "127.0.0.1",
            "--risk-override",
            "low",
            "--json",
        ])
        .output()
        .expect("failed to add sandbox host");
    assert!(
        add.status.success(),
        "host add should exit 0, stderr: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    let output = std::process::Command::new(cli_bin())
        .env(CONFIG_DIR_ENV, &dir)
        .args([
            "policy",
            "test",
            "sudo whoami",
            "--host",
            "sandbox",
            "--json",
        ])
        .output()
        .expect("failed to run policy test");

    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        output.status.success(),
        "policy test should exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("policy test should return JSON");
    assert_eq!(result["risk_level"], "low");
    assert_eq!(result["decision"], "allow");
}

fn unique_temp_config_dir(name: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("agent2ssh-{name}-{}-{nanos}", std::process::id()))
}

/// Run one MCP server over stdio: write every request, close stdin, and return
/// the responses in the order they were written, with the exit status.
async fn mcp_exchange(
    config_dir: &std::path::Path,
    binding: Option<(&str, &str)>,
    requests: &[&str],
) -> (Vec<serde_json::Value>, std::process::ExitStatus) {
    mcp_exchange_with_env(config_dir, binding, &[], requests).await
}

/// [`mcp_exchange`] with extra environment for the server process.
async fn mcp_exchange_with_env(
    config_dir: &std::path::Path,
    binding: Option<(&str, &str)>,
    env: &[(&str, &str)],
    requests: &[&str],
) -> (Vec<serde_json::Value>, std::process::ExitStatus) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::process::Command;

    std::fs::create_dir_all(config_dir).expect("create MCP test config dir");
    let mut command = Command::new(mcp_bin());
    command
        .env(CONFIG_DIR_ENV, config_dir)
        .env_remove("AGENT2SSH_SOURCE")
        .env_remove("AGENT2SSH_BINDING_KEY")
        .env_remove("AGENT2SSH_MCP_TOOLS")
        .envs(env.iter().copied())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some((source, key)) = binding {
        write_mcp_test_binding(config_dir, source, key);
        command
            .env("AGENT2SSH_SOURCE", source)
            .env("AGENT2SSH_BINDING_KEY", key);
    }
    let mut child = command.spawn().expect("failed to start MCP binary");

    let mut stdin = child.stdin.take().expect("missing MCP stdin");
    for request in requests {
        stdin.write_all(request.as_bytes()).await.unwrap();
        stdin.write_all(b"\n").await.unwrap();
    }
    drop(stdin);

    let mut lines = BufReader::new(child.stdout.take().expect("missing MCP stdout")).lines();
    let mut responses = Vec::new();
    while let Some(line) = lines.next_line().await.unwrap() {
        responses.push(serde_json::from_str(&line).expect("MCP wrote a non-JSON line"));
    }
    let status = child.wait().await.expect("failed waiting for MCP process");
    (responses, status)
}

fn response_with_id(responses: &[serde_json::Value], id: u64) -> &serde_json::Value {
    responses
        .iter()
        .find(|response| response["id"] == id)
        .unwrap_or_else(|| panic!("no response for id {id} in {responses:?}"))
}

/// A failed tool has to reach the model as something it can act on: a result
/// flagged `isError`, with a stable code and — where there is one — the next
/// step. Only a request the server cannot dispatch stays a JSON-RPC error.
#[tokio::test]
async fn mcp_tool_failures_are_results_with_a_code_and_a_hint() {
    let config_dir =
        std::env::temp_dir().join(format!("agent2ssh-mcp-failures-{}", uuid::Uuid::new_v4()));
    let (responses, status) = mcp_exchange(
        &config_dir,
        Some(("cli_smoke", "test-binding-key")),
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ssh_exec","arguments":{"host":"nope","command":"uptime"}}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"ssh_exec","arguments":{"host":"nope"}}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"no_such_tool","arguments":{}}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"ssh_list_hosts","arguments":{}}}"#,
        ],
    )
    .await;
    assert!(status.success());

    let unknown_host = response_with_id(&responses, 1);
    assert!(unknown_host.get("error").is_none(), "{unknown_host}");
    assert_eq!(unknown_host["result"]["isError"], true, "{unknown_host}");
    assert_eq!(
        unknown_host["result"]["structuredContent"]["error"]["code"], "host_not_found",
        "{unknown_host}"
    );
    let text = unknown_host["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(
        text.starts_with("Error [host_not_found]: unknown host profile: nope"),
        "{text:?}"
    );
    // The hint is what turns the failure into a next step.
    assert!(
        text.contains("\nHint: No SSH hosts are configured"),
        "{text:?}"
    );

    let missing_argument = response_with_id(&responses, 2);
    assert_eq!(missing_argument["result"]["isError"], true);
    assert_eq!(
        missing_argument["result"]["structuredContent"]["error"]["code"],
        "invalid_arguments"
    );
    assert_eq!(
        missing_argument["result"]["structuredContent"]["error"]["message"],
        "command required"
    );

    // Nothing to hand the model here: the request named a tool that does not
    // exist, which is the client's error.
    let unknown_tool = response_with_id(&responses, 3);
    assert_eq!(unknown_tool["error"]["code"], -32602, "{unknown_tool}");
    assert!(unknown_tool.get("result").is_none());

    // A success is not flagged, and a non-exec tool answers with compact JSON.
    let hosts = response_with_id(&responses, 4);
    assert!(hosts["result"].get("isError").is_none(), "{hosts}");
    assert_eq!(hosts["result"]["content"][0]["text"], "[]");

    let _ = std::fs::remove_dir_all(config_dir);
}

/// One unparseable line used to end the process — and with it every session and
/// forward the server held. It is answered as a parse error and serving goes on.
#[tokio::test]
async fn mcp_survives_a_line_that_is_not_json() {
    let config_dir =
        std::env::temp_dir().join(format!("agent2ssh-mcp-badline-{}", uuid::Uuid::new_v4()));
    let (responses, status) = mcp_exchange(
        &config_dir,
        Some(("cli_smoke", "test-binding-key")),
        &[
            "this is not json",
            r#"{"jsonrpc":"2.0","id":7,"method":"ping","params":{}}"#,
        ],
    )
    .await;

    assert!(status.success(), "the server must exit cleanly at EOF");
    assert_eq!(responses.len(), 2, "{responses:?}");
    assert_eq!(responses[0]["error"]["code"], -32700, "{responses:?}");
    assert!(responses[0]["id"].is_null());
    assert_eq!(responses[1]["id"], 7);
    assert!(responses[1]["result"].is_object());

    let _ = std::fs::remove_dir_all(config_dir);
}

/// A server whose binding does not verify used to exit before the handshake, so
/// the client saw "failed to start" and the reason went to a stderr nobody
/// reads. It now connects, lists its tools, and refuses every call with the
/// reason — still running nothing.
#[tokio::test]
async fn mcp_without_a_binding_connects_but_runs_no_tool() {
    let config_dir =
        std::env::temp_dir().join(format!("agent2ssh-mcp-unbound-{}", uuid::Uuid::new_v4()));
    let (responses, status) = mcp_exchange(
        &config_dir,
        None,
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ssh_risk_check","arguments":{"command":"ls"}}}"#,
        ],
    )
    .await;

    assert!(status.success());
    assert_eq!(
        response_with_id(&responses, 1)["result"]["serverInfo"]["name"],
        "agent2ssh-mcp"
    );
    assert_eq!(
        response_with_id(&responses, 2)["result"]["tools"]
            .as_array()
            .unwrap()
            .len(),
        DEFAULT_LISTED_TOOLS
    );

    let refused = response_with_id(&responses, 3);
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    assert_eq!(
        refused["result"]["structuredContent"]["error"]["code"], "binding_invalid",
        "{refused}"
    );
    // The tool must not have run: a risk verdict in the result would mean the
    // gate was only cosmetic.
    assert!(
        refused["result"]["structuredContent"]
            .get("risk_level")
            .is_none(),
        "{refused}"
    );
    let text = refused["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("agent2ssh integrate add"), "{text:?}");

    let _ = std::fs::remove_dir_all(config_dir);
}

/// `tools/list` is the one place the registry is paid for in the model's
/// context, so by default it offers the core group — and says so, so the model
/// is not left thinking that is everything. Every tool stays callable by name.
#[tokio::test]
async fn mcp_lists_core_tools_by_default_and_hidden_ones_still_run() {
    let config_dir =
        std::env::temp_dir().join(format!("agent2ssh-mcp-listing-{}", uuid::Uuid::new_v4()));
    let (responses, status) = mcp_exchange(
        &config_dir,
        Some(("cli_smoke", "test-binding-key")),
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
            // `ssh_doctor` is a diagnostics tool: not listed, but it must run.
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ssh_doctor","arguments":{}}}"#,
        ],
    )
    .await;
    assert!(status.success());

    let instructions = response_with_id(&responses, 1)["result"]["instructions"]
        .as_str()
        .expect("initialize carries instructions");
    assert!(
        instructions.contains("more tools are not listed"),
        "{instructions}"
    );
    assert!(
        instructions.contains("AGENT2SSH_MCP_TOOLS=all"),
        "{instructions}"
    );

    let tools = response_with_id(&responses, 2)["result"]["tools"]
        .as_array()
        .unwrap();
    assert_eq!(tools.len(), DEFAULT_LISTED_TOOLS);
    let tool = |name: &str| tools.iter().find(|tool| tool["name"] == name);
    assert!(
        tool("ssh_doctor").is_none(),
        "a diagnostics tool is listed by default"
    );
    // A read-only tool says so; ssh_exec, which can do anything, says nothing.
    assert_eq!(
        tool("ssh_list_hosts").unwrap()["annotations"]["readOnlyHint"],
        true
    );
    assert!(tool("ssh_exec").unwrap().get("annotations").is_none());

    let doctor = response_with_id(&responses, 3);
    assert!(doctor.get("error").is_none(), "{doctor}");
    assert!(doctor["result"].get("isError").is_none(), "{doctor}");

    let _ = std::fs::remove_dir_all(config_dir);
}

#[tokio::test]
async fn mcp_tools_setting_lists_more_groups_or_all() {
    let config_dir =
        std::env::temp_dir().join(format!("agent2ssh-mcp-profile-{}", uuid::Uuid::new_v4()));
    let request = [r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#];
    let listed =
        |responses: &[serde_json::Value]| responses[0]["result"]["tools"].as_array().unwrap().len();

    let (all, _) = mcp_exchange_with_env(
        &config_dir,
        Some(("cli_smoke", "test-binding-key")),
        &[("AGENT2SSH_MCP_TOOLS", "all")],
        &request,
    )
    .await;
    assert_eq!(listed(&all), ALL_TOOLS);

    let (rules, _) = mcp_exchange_with_env(
        &config_dir,
        Some(("cli_smoke", "test-binding-key")),
        &[("AGENT2SSH_MCP_TOOLS", "rules")],
        &request,
    )
    .await;
    // core plus the 13 redaction / highlight / algorithm tools.
    assert_eq!(listed(&rules), DEFAULT_LISTED_TOOLS + 13);

    let _ = std::fs::remove_dir_all(config_dir);
}

/// The protocol defines `structuredContent` as an object, and a strict client
/// validates it as one — an array makes it reject the whole result. These are
/// the tools whose natural answer is a list; each must still come back as an
/// object (the list wrapped under `result`) with the list itself in the text.
#[tokio::test]
async fn mcp_structured_content_is_an_object_even_for_list_tools() {
    let config_dir =
        std::env::temp_dir().join(format!("agent2ssh-mcp-structured-{}", uuid::Uuid::new_v4()));
    let list_tools = [
        ("ssh_list_hosts", "{}"),
        ("ssh_list_daemons", "{}"),
        ("ssh_audit", "{}"),
        ("ssh_session_list", "{}"),
        ("ssh_forward_list", "{}"),
        ("ssh_approval_list", "{}"),
        ("ssh_playbook_list", "{}"),
        ("ssh_snippet_list", "{}"),
        ("ssh_connection_status", "{}"),
        ("ssh_doctor", "{}"),
        ("ssh_redact_rule_list", "{}"),
        ("ssh_highlight_rule_list", "{}"),
        ("ssh_approval_policies_list", "{}"),
        ("ssh_ping", r#"{"hosts":[]}"#),
    ];
    let requests: Vec<String> = list_tools
        .iter()
        .enumerate()
        .map(|(index, (tool, arguments))| {
            format!(
                r#"{{"jsonrpc":"2.0","id":{},"method":"tools/call","params":{{"name":"{tool}","arguments":{arguments}}}}}"#,
                index + 1
            )
        })
        .collect();
    let requests: Vec<&str> = requests.iter().map(String::as_str).collect();
    let (responses, status) = mcp_exchange_with_env(
        &config_dir,
        Some(("cli_smoke", "test-binding-key")),
        &[("AGENT2SSH_MCP_TOOLS", "all")],
        &requests,
    )
    .await;
    assert!(status.success());

    for (index, (tool, _)) in list_tools.iter().enumerate() {
        let response = response_with_id(&responses, index as u64 + 1);
        let result = &response["result"];
        assert!(
            result["structuredContent"].is_object(),
            "{tool}: structuredContent is not an object: {response}"
        );
        // The text the model reads is the list itself, not the wrapper.
        let text: serde_json::Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap())
                .unwrap_or_else(|error| panic!("{tool}: text is not JSON ({error}): {response}"));
        assert!(
            text.is_array(),
            "{tool}: expected a list in the text: {text}"
        );
        assert_eq!(result["structuredContent"]["result"], text, "{tool}");
    }

    let _ = std::fs::remove_dir_all(config_dir);
}

/// `initialize` answers with the revision the client asked for when the server
/// speaks it, and the newest it speaks otherwise — it used to answer every
/// client with one fixed revision.
#[tokio::test]
async fn mcp_initialize_negotiates_the_protocol_revision() {
    let config_dir =
        std::env::temp_dir().join(format!("agent2ssh-mcp-version-{}", uuid::Uuid::new_v4()));
    let (responses, status) = mcp_exchange(
        &config_dir,
        Some(("cli_smoke", "test-binding-key")),
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"initialize","params":{"protocolVersion":"2099-01-01"}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"initialize","params":{}}"#,
        ],
    )
    .await;
    assert!(status.success());

    let version = |id| {
        response_with_id(&responses, id)["result"]["protocolVersion"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(version(1), "2024-11-05");
    assert_eq!(version(2), "2025-03-26");
    assert_eq!(
        version(3),
        "2025-06-18",
        "an unknown revision gets the newest"
    );
    assert_eq!(version(4), "2025-06-18", "no revision gets the newest");

    let _ = std::fs::remove_dir_all(config_dir);
}
