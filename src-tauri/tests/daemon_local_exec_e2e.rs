#![cfg(feature = "daemon")]

//! End-to-end tests for the daemon's `/local-exec` route, driving the real
//! daemon binary over HTTP.
//!
//! The rest of the daemon suite (`daemon_integration.rs`) runs a *mirrored*
//! router — its own handlers, in-process — so none of it exercises the real
//! handler code. The `/local-exec` wiring was therefore only covered by a
//! source-level check. This file closes that gap: it spawns `agent2ssh-daemon`,
//! waits for it to serve, and drives the route the way a client would.
//!
//! The binary is located through `CARGO_BIN_EXE_agent2ssh-daemon`, which only
//! exists when the `daemon` feature is enabled — hence the crate-level `cfg`,
//! which also keeps the `env!` below from being expanded without the binary.
//!
//! Every test gets its own daemon, config directory and port, and passes the
//! environment to the child rather than mutating its own, so these run in
//! parallel safely.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use agent2ssh::LOCAL_TARGET_ID;

mod common;
use common::daemon::{body_text, result_text, McpSession, TestDaemon};

#[test]
fn local_exec_route_runs_a_command_on_the_daemon_host() {
    let daemon = TestDaemon::start("run");

    let response = daemon.post_local_exec(serde_json::json!({
        "host": LOCAL_TARGET_ID,
        "command": "echo local-exec-e2e-marker",
        "force": true,
    }));
    let status = response.status();
    let body: serde_json::Value = response.json().unwrap_or_else(|error| {
        panic!("expected an ExecResult body, got {error} and status {status}")
    });

    assert_eq!(status, 200, "unexpected response: {body}");
    assert_eq!(body["host"], LOCAL_TARGET_ID);
    assert_eq!(body["exit_code"], 0, "body: {body}");
    assert!(
        body["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("local-exec-e2e-marker"),
        "stdout was {:?}",
        body["stdout"]
    );

    // The run has to be visible through the daemon's own audit surface, under
    // the reserved host id — this is what makes a local run distinguishable from
    // an SSH one after the fact.
    let audit = daemon
        .client()
        .get(daemon.url("/audit"))
        .bearer_auth(&daemon.token)
        .send()
        .expect("GET /audit");
    let status = audit.status();
    let entries: serde_json::Value = audit
        .json()
        .unwrap_or_else(|error| panic!("expected an audit array, got {error} and status {status}"));

    let recorded = entries
        .as_array()
        .unwrap_or_else(|| panic!("the audit response was not an array: {entries}"))
        .iter()
        .any(|entry| {
            entry["host"] == LOCAL_TARGET_ID
                && entry["command"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("local-exec-e2e-marker")
        });
    assert!(recorded, "the local exec was not audited: {entries}");
}

#[test]
fn local_exec_route_rejects_a_host_that_is_not_the_local_target() {
    let daemon = TestDaemon::start("host-guard");

    // A real alias must not become a way to run something here: the scope and
    // approval checks upstream would have been evaluated against that alias.
    let response = daemon.post_local_exec(serde_json::json!({
        "host": "prod-db",
        "command": "echo should-not-run",
        "force": true,
    }));

    // Asserting the reason, not just the status: a bare 400 would also come from
    // a failure to resolve any host at all, and that would let this test pass
    // while the guard it is meant to protect was gone.
    assert_eq!(response.status(), 400, "{}", body_text(response));
    let body = response.text().unwrap_or_default();
    assert!(
        body.contains("requires host"),
        "expected the reserved-host guard to reject this, got {body}"
    );
}

#[test]
fn local_exec_route_is_behind_the_auth_middleware() {
    let daemon = TestDaemon::start("auth");

    // The route must be authenticated like every other control-plane endpoint;
    // reaching it without a bearer token would mean the local target sits
    // outside the token model entirely.
    let response = daemon
        .client()
        .post(daemon.url("/local-exec"))
        .json(&serde_json::json!({
            "host": LOCAL_TARGET_ID,
            "command": "echo unauthenticated",
            "force": true,
        }))
        .send()
        .expect("POST /local-exec without a token");

    assert_eq!(response.status(), 401, "{}", body_text(response));
}

#[test]
fn local_exec_route_applies_the_risk_classifier() {
    let daemon = TestDaemon::start("risk");

    // Blocked commands are refused regardless of `force`, which is the one
    // property that makes the risk pipeline worth having on this route.
    let response = daemon.post_local_exec(serde_json::json!({
        "host": LOCAL_TARGET_ID,
        "command": "echo x >/dev/sda",
        "force": true,
    }));

    assert_eq!(response.status(), 400, "{}", body_text(response));
    let body = response.text().unwrap_or_default();
    assert!(
        body.contains("blocked"),
        "expected a blocked-by-policy refusal, got {body}"
    );
}

#[test]
fn local_exec_route_selects_a_named_environment() {
    let daemon = TestDaemon::start("named");

    // The discriminator between "the named environment ran" and "a default
    // shell stood in": cmd echoes `$0` literally (Windows only), a POSIX shell
    // substitutes itself. Each platform asserts against an environment it has.
    let (host, expect_literal_dollar_zero) = if cfg!(windows) {
        (format!("{LOCAL_TARGET_ID}:cmd"), true)
    } else {
        (format!("{LOCAL_TARGET_ID}:gitbash"), false)
    };
    let response = daemon.post_local_exec(serde_json::json!({
        "host": host,
        "command": "echo $0",
        "force": true,
    }));
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .unwrap_or_else(|error| panic!("expected an ExecResult, got {error} and status {status}"));

    assert_eq!(status, 200, "unexpected response: {body}");
    assert_eq!(body["host"], host, "the environment must be echoed back");
    let stdout = body["stdout"].as_str().unwrap_or_default().trim();
    if expect_literal_dollar_zero {
        assert_eq!(stdout, "$0", "cmd must not substitute $0: {body}");
    } else {
        assert!(
            stdout.to_lowercase().contains("bash"),
            "a POSIX shell should have substituted $0: {body}"
        );
    }
}

/// The end-to-end version of the user-defined environment: a real daemon reads
/// `environments.toml` at start and a real `/local-exec` request runs in it.
#[test]
fn local_exec_route_runs_a_user_defined_environment() {
    let (base, show) = if cfg!(windows) {
        ("cmd", "echo %A2S_CUSTOM%")
    } else {
        ("default", "echo \"$A2S_CUSTOM\"")
    };
    let daemon = TestDaemon::start_with_environments(
        "custom-env",
        &format!(
            "[[environment]]\nid = \"e2e-env\"\nbase = \"{base}\"\n\
             [environment.env]\nA2S_CUSTOM = \"from-the-file\"\n"
        ),
    );

    let host = format!("{LOCAL_TARGET_ID}:e2e-env");
    let response = daemon.post_local_exec(serde_json::json!({
        "host": host,
        "command": show,
        "force": true,
    }));
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .unwrap_or_else(|error| panic!("expected an ExecResult, got {error} and status {status}"));

    assert_eq!(status, 200, "unexpected response: {body}");
    assert_eq!(body["host"], host);
    assert!(
        body["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("from-the-file"),
        "the variable from environments.toml should reach the command: {body}"
    );

    // The daemon read the file at start. Changing it now must change nothing —
    // the environment this daemon serves is the one it started with.
    std::fs::write(
        daemon
            .dir
            .join(agent2ssh::local_environments::ENVIRONMENTS_FILE),
        "[[environment]]\nid = \"e2e-env\"\n[environment.env]\nA2S_CUSTOM = \"tampered\"\n\
         [[environment]]\nid = \"late\"\n",
    )
    .expect("rewrite environments.toml");
    let again = daemon.post_local_exec(serde_json::json!({
        "host": host,
        "command": show,
        "force": true,
    }));
    let body: serde_json::Value = again.json().expect("an ExecResult");
    let stdout = body["stdout"].as_str().unwrap_or_default();
    assert!(
        stdout.contains("from-the-file") && !stdout.contains("tampered"),
        "an edit after start must not take effect: {body}"
    );
    let late = daemon.post_local_exec(serde_json::json!({
        "host": format!("{LOCAL_TARGET_ID}:late"),
        "command": "echo hi",
        "force": true,
    }));
    assert_eq!(late.status(), 400, "{}", body_text(late));
}

/// A definition the daemon rejected is refused over the wire with its reason.
#[test]
fn local_exec_route_refuses_a_rejected_user_defined_environment() {
    let daemon = TestDaemon::start_with_environments(
        "custom-env-rejected",
        "[[environment]]\nid = \"risky\"\nbase = \"gitbash\"\nprelude = \"sudo true\"\n",
    );

    let response = daemon.post_local_exec(serde_json::json!({
        "host": format!("{LOCAL_TARGET_ID}:risky"),
        "command": "echo should-not-run",
        "force": true,
    }));

    assert_eq!(response.status(), 400, "{}", body_text(response));
    let body = response.text().unwrap_or_default();
    assert!(
        body.contains("rejected") && body.contains("risk"),
        "expected the refusal to carry the reason: {body}"
    );
    assert!(
        !body.contains("should-not-run"),
        "the command must not have run: {body}"
    );
}

#[test]
fn local_exec_route_rejects_an_unknown_environment() {
    let daemon = TestDaemon::start("unknown-env");

    let response = daemon.post_local_exec(serde_json::json!({
        "host": format!("{LOCAL_TARGET_ID}:nope"),
        "command": "echo should-not-run",
        "force": true,
    }));

    assert_eq!(response.status(), 400, "{}", body_text(response));
    let body = response.text().unwrap_or_default();
    assert!(
        body.contains("nope"),
        "expected the refusal to name the unknown environment: {body}"
    );
    assert!(
        body.contains("gitbash"),
        "expected the refusal to list what is available: {body}"
    );
}

/// The end-to-end version of the headline case: a real daemon establishes a real
/// MSVC developer shell and finds a real compiler in it.
///
/// The control run under `cmd` is what makes this a test rather than a
/// coincidence — if `cl.exe` were already on the `PATH`, both runs would pass.
#[cfg(windows)]
#[test]
fn local_exec_route_can_establish_a_visual_studio_shell() {
    let daemon = TestDaemon::start("vs");

    let control = daemon.post_local_exec(serde_json::json!({
        "host": format!("{LOCAL_TARGET_ID}:cmd"),
        "command": "where.exe cl",
        "force": true,
    }));
    let control_status = control.status();
    let control: serde_json::Value = control.json().unwrap_or_else(|error| {
        panic!(
            "expected an ExecResult for the control run, got {error} and status {control_status}"
        )
    });
    if control["exit_code"] == 0 {
        eprintln!("skipping: cl.exe is already on the PATH, so the difference proves nothing");
        return;
    }

    let response = daemon.post_local_exec(serde_json::json!({
        "host": format!("{LOCAL_TARGET_ID}:vs"),
        "command": "where.exe cl",
        "force": true,
        "timeout_secs": 120,
    }));
    let status = response.status();
    let body: serde_json::Value = response.json().unwrap_or_else(|error| {
        panic!("expected an ExecResult for the developer shell, got {error} and status {status}")
    });
    if status != 200 {
        eprintln!("skipping: no Visual Studio developer shell available ({body})");
        return;
    }

    assert_eq!(body["exit_code"], 0, "body: {body}");
    assert!(
        body["stdout"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase()
            .contains("cl.exe"),
        "stdout was {:?}",
        body["stdout"]
    );
}

// ── What a harness sees ─────────────────────────────────────────────────────
//
// The tests below drive the MCP server against the real daemon, because the
// contract they pin lives between the two: the daemon's error body has to carry
// a timeout's partial output for the MCP result to show it, and only a command
// that really runs can prove calls are concurrent or that a large output is
// bounded and saved.

/// A command that prints its marker, then stays busy past any timeout here.
#[cfg(unix)]
const PRINT_THEN_HANG: &str = "echo before-the-deadline; sleep 30";
#[cfg(windows)]
const PRINT_THEN_HANG: &str = "echo before-the-deadline& ping -n 30 127.0.0.1 >nul";

/// A command that takes a few seconds, then prints.
#[cfg(unix)]
const SLOW_COMMAND: &str = "sleep 3; echo slow-done";
#[cfg(windows)]
const SLOW_COMMAND: &str = "ping -n 4 127.0.0.1 >nul & echo slow-done";

/// A command that prints the numbers 1..=20000, one per line (~109 KiB).
#[cfg(unix)]
const LARGE_OUTPUT: &str = "seq 1 20000";
#[cfg(windows)]
const LARGE_OUTPUT: &str = "for /L %i in (1,1,20000) do @echo %i";

/// The host the three platform-specific commands above run on.
///
/// `local` means "whatever this app would open as a terminal", and on the
/// Windows runners that is PowerShell 7 — a different language from the `cmd`
/// verbs those constants are written in. Naming `local:cmd` keeps the
/// assertions about exec (partial output, bounding, concurrency) from depending
/// on which shell a runner happens to default to; a POSIX default already
/// matches the Unix spellings.
#[cfg(unix)]
const LOCAL_HOST: &str = LOCAL_TARGET_ID;
#[cfg(windows)]
const LOCAL_HOST: &str = "local:cmd";

#[test]
fn local_exec_route_keeps_partial_output_on_timeout() {
    let daemon = TestDaemon::start("timeout-partial");

    let response = daemon.post_local_exec(serde_json::json!({
        "host": LOCAL_HOST,
        "command": PRINT_THEN_HANG,
        "force": true,
        "timeout_secs": 2,
    }));
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .unwrap_or_else(|error| panic!("expected an error body, got {error} ({status})"));

    assert_eq!(status, 400, "body: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("timed out after 2s"),
        "body: {body}"
    );
    assert_eq!(body["timed_out"], true, "body: {body}");
    assert_eq!(body["process_stopped"], true, "body: {body}");
    assert!(
        body["partial_stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("before-the-deadline"),
        "body: {body}"
    );
}

#[test]
fn mcp_exec_answers_with_text_and_a_timeout_keeps_its_output() {
    let daemon = TestDaemon::start("mcp-text");
    let mut mcp = McpSession::start(&daemon);

    mcp.call(
        1,
        "local_exec",
        serde_json::json!({ "host": LOCAL_TARGET_ID, "command": "echo mcp-text-marker", "force": true }),
    );
    let response = mcp.next_response();
    let text = result_text(&response);
    assert!(text.starts_with("exit_code: 0 | "), "text was {text:?}");
    assert!(
        text.contains("\n[stdout]\nmcp-text-marker"),
        "the output must be readable text, not a JSON string: {text:?}"
    );
    assert!(response["result"].get("isError").is_none(), "{response}");
    assert_eq!(response["result"]["structuredContent"]["exit_code"], 0);

    mcp.call(
        2,
        "local_exec",
        serde_json::json!({
            "host": LOCAL_HOST, "command": PRINT_THEN_HANG, "force": true, "timeout_secs": 2
        }),
    );
    let response = mcp.next_response();
    // A failed tool is a result the model reads, not a JSON-RPC error.
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], true, "{response}");
    let structured = &response["result"]["structuredContent"];
    assert_eq!(structured["error"]["code"], "exec_timeout", "{response}");
    assert_eq!(structured["timed_out"], true, "{response}");
    assert_eq!(structured["process_stopped"], true, "{response}");
    let text = result_text(&response);
    assert!(text.starts_with("Error [exec_timeout]: "), "{text:?}");
    assert!(text.contains("\nHint: "), "{text:?}");
    assert!(
        text.contains("[stdout before the timeout]\nbefore-the-deadline"),
        "the timeout must keep what the command printed: {text:?}"
    );
}

#[test]
fn mcp_bounds_a_large_output_and_saves_the_rest() {
    let daemon = TestDaemon::start("mcp-large");
    let mut mcp = McpSession::start(&daemon);

    mcp.call(
        1,
        "local_exec",
        serde_json::json!({
            "host": LOCAL_HOST, "command": LARGE_OUTPUT, "force": true, "timeout_secs": 120
        }),
    );
    let response = mcp.next_response();
    let structured = &response["result"]["structuredContent"];
    assert_eq!(structured["exit_code"], 0, "{response}");

    let inline = structured["stdout"].as_str().expect("inline stdout");
    let total = structured["stdout_total_bytes"]
        .as_u64()
        .expect("a clipped stream reports its full size") as usize;
    assert!(total > 100_000, "the command printed only {total} bytes");
    assert!(
        inline.len() < 34 * 1024,
        "the inline stdout was {} bytes, over the 32 KiB default",
        inline.len()
    );
    // Head and tail both survive, around a marker naming what was left out.
    assert!(inline.trim_start().starts_with('1'), "{:?}", &inline[..40]);
    assert!(inline.trim_end().ends_with("20000"));
    assert!(
        inline.contains(" bytes omitted; full stdout ("),
        "{inline:?}"
    );

    // The full capture is on disk where the agent's file tools can reach it.
    let file = PathBuf::from(structured["stdout_file"].as_str().expect("a spill file"));
    assert!(
        file.starts_with(daemon.dir.join("outputs")),
        "spilled to {file:?}"
    );
    let saved = std::fs::read_to_string(&file).expect("read the spilled output");
    assert_eq!(saved.len(), total);
    assert!(saved.contains("\n10000"), "the middle must be in the file");

    // The text the model reads is the same bounded output.
    let text = result_text(&response);
    assert!(text.contains(&file.display().to_string()), "{text:?}");
    assert!(text.len() < 36 * 1024, "the text was {} bytes", text.len());
}

#[test]
fn mcp_runs_tool_calls_concurrently() {
    let daemon = TestDaemon::start("mcp-concurrent");
    let mut mcp = McpSession::start(&daemon);

    let started = Instant::now();
    mcp.call(
        1,
        "local_exec",
        serde_json::json!({ "host": LOCAL_HOST, "command": SLOW_COMMAND, "force": true }),
    );
    mcp.call(
        2,
        "local_exec",
        serde_json::json!({ "host": LOCAL_TARGET_ID, "command": "echo fast-done", "force": true }),
    );

    // Served one at a time, the quick call would wait out the slow one and
    // answer second.
    let first = mcp.next_response();
    assert_eq!(
        first["id"], 2,
        "the quick call was held up by the slow one: {first}"
    );
    assert!(result_text(&first).contains("fast-done"));
    assert!(
        started.elapsed() < Duration::from_millis(2500),
        "the quick call took {:?}",
        started.elapsed()
    );

    let second = mcp.next_response();
    assert_eq!(second["id"], 1);
    assert!(result_text(&second).contains("slow-done"));
}
