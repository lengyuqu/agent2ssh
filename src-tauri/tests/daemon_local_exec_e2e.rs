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

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agent2ssh::store::CONFIG_DIR_ENV;
use agent2ssh::LOCAL_TARGET_ID;

/// How long the daemon is given to answer `/health` before the test gives up.
/// Generous because a cold start runs the secret migration and reads the config.
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
            "a2s-daemon-local-exec-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock before the epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create the throwaway config dir");

        // Written before the daemon starts: `load_or_create_daemon_token` keeps
        // an existing non-empty token, so the test knows it up front instead of
        // racing the daemon to the file.
        let token = format!("local-exec-e2e-token-{}", std::process::id());
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

    /// Block until `/health` answers, which is the daemon's public liveness
    /// route — no token needed, so this cannot accidentally pass on a 401.
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

    fn post_local_exec(&self, body: serde_json::Value) -> reqwest::blocking::Response {
        self.client()
            .post(self.url("/local-exec"))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .expect("POST /local-exec")
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

    let host = format!("{LOCAL_TARGET_ID}:cmd");
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
    // cmd is not a POSIX shell, so `$0` survives literally — proof that the named
    // environment is the one that ran rather than a default shell standing in.
    assert_eq!(body["stdout"].as_str().unwrap_or_default().trim(), "$0");
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
