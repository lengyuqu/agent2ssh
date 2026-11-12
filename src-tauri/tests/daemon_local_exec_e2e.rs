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
use std::time::{Duration, Instant};

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
        Self::start_seeded(label, None)
    }

    /// Start a daemon whose config dir already holds an `environments.toml`.
    /// Seeded before the process exists because the daemon reads that file once,
    /// at start — which is exactly what the tests below depend on.
    fn start_with_environments(label: &str, environments: &str) -> Self {
        Self::start_seeded(label, Some(environments))
    }

    fn start_seeded(label: &str, environments: Option<&str>) -> Self {
        let _ = label;
        #[cfg(target_os = "macos")]
        let root = PathBuf::from("/tmp");
        #[cfg(not(target_os = "macos"))]
        let root = std::env::temp_dir();
        let dir = tempfile::Builder::new()
            .prefix("a2sl-")
            .tempdir_in(root)
            .expect("create the throwaway config dir")
            .keep();

        // Written before the daemon starts: `load_or_create_daemon_token` keeps
        // an existing non-empty token, so the test knows it up front instead of
        // racing the daemon to the file.
        let token = format!("local-exec-e2e-token-{}", std::process::id());
        std::fs::write(dir.join("daemon.token"), &token).expect("seed daemon.token");
        if let Some(environments) = environments {
            std::fs::write(
                dir.join(agent2ssh::local_environments::ENVIRONMENTS_FILE),
                environments,
            )
            .expect("seed environments.toml");
        }

        let addr = free_loopback_addr();
        let child = Self::spawn(&dir, &addr);
        let mut daemon = Self {
            child,
            dir,
            addr,
            token,
        };
        for attempt in 0..5 {
            match daemon.wait_until_serving() {
                Ok(()) => return daemon,
                Err((true, _)) if attempt < 4 => {
                    daemon.addr = free_loopback_addr();
                    daemon.child = Self::spawn(&daemon.dir, &daemon.addr);
                }
                Err((_, detail)) => panic!("{detail}"),
            }
        }
        unreachable!()
    }

    fn spawn(dir: &std::path::Path, addr: &str) -> Child {
        let stderr =
            std::fs::File::create(dir.join("daemon.stderr")).expect("create daemon stderr");
        Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))
            .env(CONFIG_DIR_ENV, dir)
            .env("AGENT2SSH_DAEMON_ADDR", addr)
            .env("AGENT2SSH_LOG", "warn")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("spawn agent2ssh-daemon")
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
    fn wait_until_serving(&mut self) -> Result<(), (bool, String)> {
        let probe = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(500))
            .build()
            .expect("build the probe client");
        let deadline = Instant::now() + STARTUP_BUDGET;
        while Instant::now() < deadline {
            if let Ok(response) = probe.get(self.url("/health")).send() {
                if response.status().is_success() {
                    if let Ok(body) = response.json::<serde_json::Value>() {
                        if body["pid"].as_u64() == Some(u64::from(self.child.id())) {
                            return Ok(());
                        }
                    }
                }
            }
            if let Some(status) = self.child.try_wait().expect("check daemon process") {
                let stderr =
                    std::fs::read_to_string(self.dir.join("daemon.stderr")).unwrap_or_default();
                let collision = ["os error 48", "os error 98", "os error 10048"]
                    .iter()
                    .any(|code| stderr.contains(code))
                    && !stderr.contains("private daemon socket");
                return Err((
                    collision,
                    format!("daemon exited before health check: {status}; stderr: {stderr}"),
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err((
            false,
            format!("the daemon did not answer /health within {STARTUP_BUDGET:?}"),
        ))
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
