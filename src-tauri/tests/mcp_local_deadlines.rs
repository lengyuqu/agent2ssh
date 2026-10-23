//! The local-execution MCP tools must wait for the daemon's deadline, including
//! shell warmup and approval. Each case drives the real MCP binary against a
//! slow loopback HTTP fixture and an isolated configuration directory.

mod common;

use std::process::Stdio;
use std::time::Duration;

use axum::{routing::post, Json, Router};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;

async fn delayed_tool_result(tool: &str, path: &str, args: Value, response: Value) {
    let dir = tempfile::tempdir().expect("isolated MCP config");
    std::fs::write(dir.path().join("daemon.token"), "deadline-test-token").unwrap();
    std::fs::write(
        dir.path().join("mcp_bindings.json"),
        json!({"keys": {"deadline-test": "deadline-test-binding"}}).to_string(),
    )
    .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let reply = response.clone();
    let expected_args = args.clone();
    let app = Router::new().route(
        path,
        post(move |Json(body): Json<Value>| async move {
            // Verify that the server received the requested command deadline;
            // a quick schema/route error must never count as a passing test.
            if let Some(timeout) = expected_args.get("timeout_secs") {
                assert_eq!(&body["timeout_secs"], timeout);
            }
            // Deliberately exceeds the old hard-coded HTTP deadline. All three
            // tests run concurrently, so this adds one 31-second wait per suite.
            tokio::time::sleep(Duration::from_secs(31)).await;
            Json(reply)
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent2ssh-mcp"))
        .env(agent2ssh::store::CONFIG_DIR_ENV, dir.path())
        .env("AGENT2SSH_DAEMON_ADDR", addr.to_string())
        .env("AGENT2SSH_SOURCE", "deadline-test")
        .env("AGENT2SSH_BINDING_KEY", "deadline-test-binding")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": tool, "arguments": args},
    });
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    drop(stdin);
    let output = tokio::time::timeout(Duration::from_secs(50), child.wait_with_output()).await;
    server.abort();
    let output = output
        .expect("MCP fixture must finish within 50 seconds")
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reply: Value = serde_json::from_slice(&output.stdout).expect("MCP JSON response");
    assert!(
        reply.get("error").is_none(),
        "{tool} ended before the daemon: {reply}"
    );
    assert_eq!(reply["result"]["structuredContent"], response);
}

#[tokio::test]
async fn local_exec_waits_past_thirty_seconds_for_the_daemon_result() {
    delayed_tool_result(
        "local_exec",
        "/local-exec",
        json!({"host": "local", "command": "echo complete", "timeout_secs": 60}),
        json!({"stdout": "complete", "exit_code": 0}),
    )
    .await;
}

#[tokio::test]
async fn resident_exec_preserves_nonzero_results_after_thirty_seconds() {
    // A daemon may return a normal structured result with a nonzero exit code;
    // the MCP transport must preserve it instead of substituting an HTTP timeout.
    delayed_tool_result(
        "resident_session_exec",
        "/resident-sessions/test-session/exec",
        json!({"session_id": "test-session", "command": "echo complete", "timeout_secs": 60}),
        json!({"stdout": "complete", "exit_code": 7}),
    )
    .await;
}

#[tokio::test]
async fn resident_open_waits_for_shell_warmup_and_approval() {
    delayed_tool_result(
        "resident_session_open",
        "/resident-sessions",
        json!({"host": "local:vs"}),
        json!({"id": "warm-session", "host": "local:vs", "warmup_ms": 31000}),
    )
    .await;
}
