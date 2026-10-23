#![cfg(feature = "daemon")]

//! End-to-end tests for the daemon's `/resident-sessions` routes, driving the
//! real daemon binary over HTTP.
//!
//! `daemon_integration.rs` runs a *mirrored* router, so the resident-session
//! wiring was only covered by source-level checks there. This file closes that
//! gap the same way `daemon_local_exec_e2e.rs` closes it for `/local-exec`: it
//! spawns `agent2ssh-daemon`, waits for `/health`, and drives the routes the
//! way a client would.
//!
//! The default environment (`local`) is a shell reading stdin on any host, so
//! nothing here needs Windows — this suite runs on macOS and Linux too.
//!
//! Every test gets its own daemon, config directory and port, and passes the
//! environment to the child rather than mutating its own, so these run in
//! parallel safely.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agent2ssh::store::CONFIG_DIR_ENV;

/// How long the daemon is given to answer `/health` before the test gives up.
const STARTUP_BUDGET: Duration = Duration::from_secs(30);

/// A daemon process under test, torn down with its directory on drop.
struct TestDaemon {
    child: Child,
    dir: PathBuf,
    addr: String,
    token: String,
}

impl TestDaemon {
    fn start(label: &str) -> Self {
        Self::start_with_scopes(label, "")
    }

    fn start_with_scopes(label: &str, scoped_tokens: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "a2s-daemon-resident-e2e-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock before the epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create the throwaway config dir");

        // Written before the daemon starts: the loader keeps an existing
        // non-empty token, so the test knows it without racing the daemon.
        let token = format!("resident-e2e-token-{}", std::process::id());
        std::fs::write(dir.join("daemon.token"), &token).expect("seed daemon.token");
        if !scoped_tokens.is_empty() {
            std::fs::write(dir.join("daemon_tokens.toml"), scoped_tokens)
                .expect("seed isolated scoped-token configuration");
        }

        let addr = free_loopback_addr();
        let mut spawn = Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"));
        spawn
            .env(CONFIG_DIR_ENV, &dir)
            .env("AGENT2SSH_DAEMON_ADDR", &addr)
            .env("AGENT2SSH_LOG", "warn");

        // This suite asserts POSIX shell semantics (`export VAR=...`,
        // `echo $VAR`, `$?` exit codes). On Windows the default `local`
        // environment is PowerShell, where `export` is not a command and a
        // cmdlet-only command has no `$LASTEXITCODE` — those assertions can
        // never hold. Pin the daemon's default shell to an installed POSIX
        // shell (Git Bash) so the suite verifies the same contract on every
        // host; without one the suite still runs and reports the gap.
        #[cfg(windows)]
        {
            if let Some(posix) = agent2ssh::shell_list::list_shells()
                .into_iter()
                .find(|shell| shell.family == agent2ssh::shell_list::ShellFamily::Posix)
            {
                spawn.env(agent2ssh::local_exec::SHELL_ENV, &posix.path);
            }
        }

        let child = spawn
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn agent2ssh-daemon");

        let daemon = Self {
            child,
            dir,
            addr,
            token,
        };
        daemon.wait_until_serving();
        daemon
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    fn client(&self) -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("build the http client")
    }

    /// Block until `/health` answers — no token needed, so this cannot
    /// accidentally pass on a 401.
    fn wait_until_serving(&self) {
        let probe = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(500))
            .build()
            .expect("build the probe client");
        let deadline = Instant::now() + STARTUP_BUDGET;
        while Instant::now() < deadline {
            if let Ok(response) = probe.get(self.url("/health")).send() {
                if response.status().is_success() {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("the daemon did not answer /health within {STARTUP_BUDGET:?}");
    }

    fn get(&self, path: &str) -> reqwest::blocking::Response {
        self.get_with_token(path, &self.token)
    }

    fn get_with_token(&self, path: &str, token: &str) -> reqwest::blocking::Response {
        self.client()
            .get(self.url(path))
            .bearer_auth(token)
            .send()
            .expect("GET {path}")
    }

    fn post(&self, path: &str, body: serde_json::Value) -> reqwest::blocking::Response {
        self.client()
            .post(self.url(path))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .expect("POST {path}")
    }

    fn delete(&self, path: &str) -> reqwest::blocking::Response {
        self.delete_with_token(path, &self.token)
    }

    fn delete_with_token(&self, path: &str, token: &str) -> reqwest::blocking::Response {
        self.client()
            .delete(self.url(path))
            .bearer_auth(token)
            .send()
            .expect("DELETE {path}")
    }

    /// Open a resident session in the default `local` shell environment and
    /// assert the essentials every other test relies on.
    fn open_default_session(&self, label: &str) -> serde_json::Value {
        let response = self.post(
            "/resident-sessions",
            serde_json::json!({ "host": "local", "force": true }),
        );
        let status = response.status();
        let body: serde_json::Value = response.json().unwrap_or_else(|error| {
            panic!("{label}: expected a ResidentSessionInfo body, got {error} and status {status}")
        });
        assert_eq!(status, 200, "{label}: {body}");
        assert!(
            body["id"].as_str().is_some(),
            "{label}: the open response must carry the session id: {body}"
        );
        assert_eq!(body["host"], "local", "{label}: {body}");
        body
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A port nothing is listening on.
///
/// Bound and released immediately, which is inherently racy — but the
/// alternative is a fixed port that collides with a developer's own running
/// daemon, and the window here is a few microseconds wide.
fn free_loopback_addr() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let addr = listener.local_addr().expect("read the bound port");
    drop(listener);
    addr.to_string()
}

fn body_text(response: reqwest::blocking::Response) -> String {
    let status = response.status();
    let text = response.text().unwrap_or_default();
    format!("status={status} body={text}")
}

#[test]
fn resident_session_lifecycle_runs_exec_list_and_close() {
    let daemon = TestDaemon::start("lifecycle");
    let session = daemon.open_default_session("open");
    let id = session["id"].as_str().expect("session id").to_string();

    // exec: one command, echoed back as a full ExecResult keyed on the
    // session's own host id.
    let response = daemon.post(
        &format!("/resident-sessions/{id}/exec"),
        serde_json::json!({ "command": "echo resident-e2e-marker", "force": true }),
    );
    let status = response.status();
    let result: serde_json::Value = response.json().unwrap_or_else(|error| {
        panic!("expected an ExecResult body, got {error} and status {status}")
    });
    assert_eq!(status, 200, "exec: {result}");
    assert_eq!(result["host"], "local", "exec: {result}");
    assert_eq!(result["exit_code"], 0, "exec: {result}");
    assert!(
        result["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("resident-e2e-marker"),
        "exec stdout was {:?}",
        result["stdout"]
    );

    // list: the open session must be visible with its own id.
    let listed = daemon.get("/resident-sessions");
    assert_eq!(listed.status(), 200, "{}", body_text(listed));
    // (re-issue as JSON — status already asserted)
    let listed: serde_json::Value = daemon.get("/resident-sessions").json().unwrap();
    let found = listed
        .as_array()
        .expect("the list response is an array")
        .iter()
        .any(|entry| entry["id"] == serde_json::Value::String(id.clone()));
    assert!(found, "the open session was not listed: {listed}");

    // close: the delete route answers, and the session is then gone.
    let closed = daemon.delete(&format!("/resident-sessions/{id}"));
    assert_eq!(closed.status(), 200, "{}", body_text(closed));
    let after = daemon.post(
        &format!("/resident-sessions/{id}/exec"),
        serde_json::json!({ "command": "echo again", "force": true }),
    );
    assert_eq!(after.status(), 400, "{}", body_text(after));
    let body = after.text().unwrap_or_default();
    assert!(
        body.contains("unknown resident session"),
        "expected the unknown-session refusal, got {body}"
    );
}

#[test]
fn resident_session_shell_state_persists_across_commands() {
    let daemon = TestDaemon::start("state");
    let session = daemon.open_default_session("state");
    let id = session["id"].as_str().expect("session id").to_string();

    let export = daemon.post(
        &format!("/resident-sessions/{id}/exec"),
        serde_json::json!({ "command": "export RESIDENT_E2E_VAR=hello-state", "force": true }),
    );
    assert_eq!(export.status(), 200, "{}", body_text(export));

    let read = daemon.post(
        &format!("/resident-sessions/{id}/exec"),
        serde_json::json!({ "command": "echo $RESIDENT_E2E_VAR", "force": true }),
    );
    let status = read.status();
    let result: serde_json::Value = read
        .json()
        .unwrap_or_else(|error| panic!("expected an ExecResult, got {error} and status {status}"));
    assert_eq!(
        result["stdout"].as_str().unwrap_or_default().trim(),
        "hello-state",
        "the exported variable did not survive into the next command: {result}"
    );

    daemon.delete(&format!("/resident-sessions/{id}"));
}

#[test]
fn resident_session_read_reports_detached_output() {
    let daemon = TestDaemon::start("collect");
    let session = daemon.open_default_session("collect");
    let id = session["id"].as_str().expect("session id").to_string();

    // Start-then-collect: the command is backgrounded inside the shell, so the
    // exec's sentinel arrives immediately and the detached job keeps writing
    // into the session buffer after the POST has returned.
    let start = daemon.post(
        &format!("/resident-sessions/{id}/exec"),
        serde_json::json!({
            "command": "(sleep 1; echo collect-me-marker) &",
            "force": true,
        }),
    );
    assert_eq!(start.status(), 200, "{}", body_text(start));

    // The read route must surface the detached job's output from the session's
    // own buffer — no second capture path exists, so a retry loop absorbs the
    // one-second sleep rather than asserting on a race.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut collected = false;
    while Instant::now() < deadline {
        let response = daemon.get(&format!("/resident-sessions/{id}"));
        assert_eq!(response.status(), 200, "{}", body_text(response));
        let detail: serde_json::Value = response.json().unwrap();
        assert_eq!(detail["host"], "local", "read: {detail}");
        if detail["output"]
            .as_str()
            .unwrap_or_default()
            .contains("collect-me-marker")
        {
            collected = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(
        collected,
        "the read endpoint never surfaced the detached command's output"
    );

    daemon.delete(&format!("/resident-sessions/{id}"));
}

#[test]
fn resident_session_open_rejects_a_non_local_host() {
    let daemon = TestDaemon::start("host-guard");

    let response = daemon.post(
        "/resident-sessions",
        serde_json::json!({ "host": "prod-db", "force": true }),
    );
    assert_eq!(response.status(), 400, "{}", body_text(response));
    let body = response.text().unwrap_or_default();
    assert!(
        body.contains("require host"),
        "expected the reserved-host guard to reject this, got {body}"
    );
}

#[test]
fn resident_session_routes_are_behind_the_auth_middleware() {
    let daemon = TestDaemon::start("auth");

    let response = daemon
        .client()
        .post(daemon.url("/resident-sessions"))
        .json(&serde_json::json!({ "host": "local", "force": true }))
        .send()
        .expect("POST /resident-sessions without a token");
    assert_eq!(response.status(), 401, "{}", body_text(response));

    let read = daemon
        .client()
        .get(daemon.url("/resident-sessions/00000000-0000-0000-0000-000000000000"))
        .send()
        .expect("GET /resident-sessions/{id} without a token");
    assert_eq!(read.status(), 401, "{}", body_text(read));
}

#[test]
fn resident_session_read_rejects_an_unknown_session() {
    let daemon = TestDaemon::start("unknown");

    let response = daemon.get("/resident-sessions/00000000-0000-0000-0000-000000000000");
    assert_eq!(response.status(), 400, "{}", body_text(response));
    let body = response.text().unwrap_or_default();
    assert!(
        body.contains("unknown resident session"),
        "expected the unknown-session refusal, got {body}"
    );
}

// Synthetic tokens are accepted only by each test's temporary daemon. The
// local session has no tags, so a tag-only scope must also hide/refuse it.
const RESIDENT_SCOPED_TOKENS: &str = r#"
[[tokens]]
token = "resident-test-other-host"
[tokens.scope]
allowed_hosts = ["other-host"]

[[tokens]]
token = "resident-test-other-tag"
[tokens.scope]
allowed_tags = ["production"]

[[tokens]]
token = "resident-test-local"
[tokens.scope]
allowed_hosts = ["local"]

[[tokens]]
token = "resident-test-reader"
[tokens.scope]
allowed_hosts = ["local"]
allowed_commands = ["resident-session read"]

[[tokens]]
token = "resident-test-closer"
[tokens.scope]
allowed_hosts = ["local"]
allowed_commands = ["resident-session close"]

[[tokens]]
token = "resident-test-denied-actions"
[tokens.scope]
allowed_hosts = ["local"]
allowed_commands = ["resident-session *"]
denied_commands = ["resident-session read", "resident-session close"]
"#;

fn assert_scope_denied(response: reqwest::blocking::Response, reason: &str) {
    let status = response.status();
    let body = response.text().expect("read scope refusal");
    assert_eq!(status, 403, "expected scoped refusal: {body}");
    assert!(body.contains(reason), "expected {reason:?}, got {body}");
}

#[test]
fn resident_session_observation_and_close_enforce_host_and_tag_scopes() {
    let daemon = TestDaemon::start_with_scopes("host-scope", RESIDENT_SCOPED_TOKENS);
    let session = daemon.open_default_session("host-scope");
    let id = session["id"].as_str().expect("session id");
    let path = format!("/resident-sessions/{id}");

    for (token, reason) in [
        ("resident-test-other-host", "not in the allowed hosts list"),
        ("resident-test-other-tag", "no tags matching allowed tags"),
    ] {
        let listed = daemon.get_with_token("/resident-sessions", token);
        assert_eq!(listed.status(), 200, "{}", body_text(listed));
        let listed: serde_json::Value = listed.json().expect("read scoped session list");
        assert_eq!(listed, serde_json::json!([]));
        assert_scope_denied(daemon.get_with_token(&path, token), reason);
        assert_scope_denied(daemon.delete_with_token(&path, token), reason);
    }

    // A rejected close must leave the original session present and usable.
    let read = daemon.get_with_token(&path, "resident-test-local");
    assert_eq!(read.status(), 200, "{}", body_text(read));
    let detail: serde_json::Value = read.json().expect("read allowed session");
    assert_eq!(detail["id"], id);
    assert_eq!(detail["dead"], false);
    let listed: serde_json::Value = daemon
        .get_with_token("/resident-sessions", "resident-test-local")
        .json()
        .expect("read allowed list");
    assert_eq!(listed.as_array().expect("session list").len(), 1);
    assert_eq!(listed[0]["id"], id);
    let exec = daemon.post(
        &format!("{path}/exec"),
        serde_json::json!({ "command": "echo scope-positive-control", "force": true }),
    );
    assert_eq!(exec.status(), 200, "{}", body_text(exec));
    let result: serde_json::Value = exec.json().expect("read positive-control command");
    assert_eq!(result["exit_code"], 0);
    assert!(result["stdout"]
        .as_str()
        .unwrap_or_default()
        .contains("scope-positive-control"));

    let closed = daemon.delete_with_token(&path, "resident-test-local");
    assert_eq!(closed.status(), 200, "{}", body_text(closed));
}

#[test]
fn resident_session_read_and_close_enforce_action_scopes() {
    let daemon = TestDaemon::start_with_scopes("action-scope", RESIDENT_SCOPED_TOKENS);
    let session = daemon.open_default_session("action-scope");
    let id = session["id"].as_str().expect("session id");
    let path = format!("/resident-sessions/{id}");

    let read = daemon.get_with_token(&path, "resident-test-reader");
    assert_eq!(read.status(), 200, "{}", body_text(read));
    assert_scope_denied(
        daemon.delete_with_token(&path, "resident-test-reader"),
        "command 'resident-session close' does not match any allowed command pattern",
    );
    assert_scope_denied(
        daemon.get_with_token(&path, "resident-test-closer"),
        "command 'resident-session read' does not match any allowed command pattern",
    );
    for (response, command) in [
        (
            daemon.get_with_token(&path, "resident-test-denied-actions"),
            "read",
        ),
        (
            daemon.delete_with_token(&path, "resident-test-denied-actions"),
            "close",
        ),
    ] {
        assert_scope_denied(
            response,
            &format!("command 'resident-session {command}' is denied by pattern"),
        );
    }

    // Listing is observation scoped by host/tags; it has no command to run.
    let listed: serde_json::Value = daemon
        .get_with_token("/resident-sessions", "resident-test-reader")
        .json()
        .expect("list visible sessions with a read-only token");
    assert_eq!(listed[0]["id"], id);
    let closed = daemon.delete_with_token(&path, "resident-test-closer");
    assert_eq!(closed.status(), 200, "{}", body_text(closed));
    let after = daemon.get(&path);
    assert_eq!(after.status(), 400, "{}", body_text(after));
    assert!(after
        .text()
        .unwrap_or_default()
        .contains("unknown resident session"));
}

#[test]
fn resident_session_close_respects_the_gate_while_reads_remain_available() {
    let daemon = TestDaemon::start("close-gate");
    let session = daemon.open_default_session("close-gate");
    let id = session["id"].as_str().expect("session id");
    let path = format!("/resident-sessions/{id}");
    let paused = daemon.post("/gate/pause", serde_json::json!({}));
    assert_eq!(paused.status(), 200, "{}", body_text(paused));

    let close = daemon.delete(&path);
    assert_eq!(close.status(), 423, "{}", body_text(close));
    assert!(close
        .text()
        .unwrap_or_default()
        .contains("execution gate paused"));
    let read = daemon.get(&path);
    assert_eq!(read.status(), 200, "{}", body_text(read));
    let detail: serde_json::Value = read.json().expect("read while paused");
    assert_eq!(detail["dead"], false);
    let listed: serde_json::Value = daemon
        .get("/resident-sessions")
        .json()
        .expect("list while paused");
    assert_eq!(listed[0]["id"], id);

    let resumed = daemon.post("/gate/resume", serde_json::json!({}));
    assert_eq!(resumed.status(), 200, "{}", body_text(resumed));
    let close = daemon.delete(&path);
    assert_eq!(close.status(), 200, "{}", body_text(close));
}

#[test]
fn resident_session_close_waits_for_approval_before_killing_the_shell() {
    let daemon = TestDaemon::start("close-approval");
    let session = daemon.open_default_session("close-approval");
    let id = session["id"].as_str().expect("session id");
    let path = format!("/resident-sessions/{id}");
    let policy = daemon
        .client()
        .put(daemon.url("/approval/policies"))
        .bearer_auth(&daemon.token)
        .json(&serde_json::json!([{
            "name": "approve-resident-close",
            "hosts": ["local"],
            "command_pattern": "resident-session close",
            "requires_approval": true,
            "ttl_secs": 20,
        }]))
        .send()
        .expect("configure an isolated close approval policy");
    assert_eq!(policy.status(), 200, "{}", body_text(policy));

    // First reject, then approve. Each request must remain pending while the
    // session is still alive; rejection must leave it usable for another try.
    for approved in [false, true] {
        std::thread::scope(|scope| {
            let close = scope.spawn(|| daemon.delete(&path));
            let deadline = Instant::now() + Duration::from_secs(10);
            let approval_id = loop {
                let response = daemon.get("/approvals");
                assert_eq!(response.status(), 200, "{}", body_text(response));
                let pending: serde_json::Value = response.json().expect("read approvals");
                if let Some(request) =
                    pending
                        .as_array()
                        .expect("approval list")
                        .iter()
                        .find(|request| {
                            request["host"] == "local"
                                && request["command"] == "resident-session close"
                                && request["status"] == "pending"
                        })
                {
                    break request["id"].as_str().expect("approval id").to_string();
                }
                assert!(Instant::now() < deadline, "close never requested approval");
                std::thread::sleep(Duration::from_millis(100));
            };
            let read = daemon.get(&path);
            assert_eq!(read.status(), 200, "{}", body_text(read));
            let detail: serde_json::Value = read.json().expect("read before approval");
            assert_eq!(detail["dead"], false);
            assert!(!close.is_finished(), "close completed before approval");

            let action = if approved { "approve" } else { "reject" };
            let decision = daemon.post(
                &format!("/approvals/{approval_id}/{action}"),
                serde_json::json!({}),
            );
            assert_eq!(decision.status(), 200, "{}", body_text(decision));
            let close = close.join().expect("close worker finished");
            if approved {
                assert_eq!(close.status(), 200, "{}", body_text(close));
            } else {
                assert_scope_denied(close, "command rejected by approver");
                let read = daemon.get(&path);
                assert_eq!(read.status(), 200, "{}", body_text(read));
            }
        });
    }

    let after = daemon.get(&path);
    assert_eq!(after.status(), 400, "{}", body_text(after));
    assert!(after
        .text()
        .unwrap_or_default()
        .contains("unknown resident session"));
}
