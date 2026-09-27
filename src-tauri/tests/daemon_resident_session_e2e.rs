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

        let addr = free_loopback_addr();
        let child = Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))
            .env(CONFIG_DIR_ENV, &dir)
            .env("AGENT2SSH_DAEMON_ADDR", &addr)
            .env("AGENT2SSH_LOG", "warn")
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
        self.client()
            .get(self.url(path))
            .bearer_auth(&self.token)
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
        self.client()
            .delete(self.url(path))
            .bearer_auth(&self.token)
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
