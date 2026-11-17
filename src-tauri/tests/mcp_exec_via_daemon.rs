#![cfg(feature = "daemon")]

//! An MCP `ssh_exec` reaches the daemon, so an approval policy can ask a person.
//!
//! `ssh_exec` used to run inside the MCP process, which authorizes with no
//! approval handler: a command an approval policy covers was a hard failure that
//! no one was ever asked about, and the gate, the rate limits and the retained
//! connections the daemon holds were all out of reach. Now the MCP server hands
//! the exec to the local daemon when there is one, and does it itself only when
//! the daemon cannot be reached at all.
//!
//! These drive the real `agent2ssh-mcp` against the real `agent2ssh-daemon` and a
//! real sshd, with a person played by the test: it finds the pending approval
//! card over the daemon's own API and answers it.
//!
//! Skipped unless `AGENT2SSH_TEST_SSH_PORT` and `AGENT2SSH_TEST_SSH_KEY` name a
//! reachable sshd (same variables as `exec_retained.rs`).

mod common;

use std::time::{Duration, Instant};

use common::daemon::{result_text, McpSession, TestDaemon};
use common::ssh_proxy::Proxy;
use serde_json::{json, Value};

struct Target {
    host: String,
    port: u16,
    user: String,
    key: String,
}

fn target() -> Option<Target> {
    let port = std::env::var("AGENT2SSH_TEST_SSH_PORT")
        .ok()
        .and_then(|raw| raw.trim().parse::<u16>().ok())
        .filter(|port| *port != 0)?;
    let key = std::env::var("AGENT2SSH_TEST_SSH_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    Some(Target {
        host: std::env::var("AGENT2SSH_TEST_SSH_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
        port,
        user: std::env::var("AGENT2SSH_TEST_SSH_USER").unwrap_or_else(|_| "root".to_string()),
        key,
    })
}

/// The command text the approval policy below matches.
const NEEDS_APPROVAL: &str = "echo needs-approval-marker";

/// A daemon, an MCP server bound to it, a counting proxy in front of the sshd,
/// and a host profile `fixture` that goes through the proxy.
struct Rig {
    daemon: TestDaemon,
    mcp: McpSession,
    proxy: Proxy,
}

impl Rig {
    fn new(label: &str) -> Option<Self> {
        let Some(target) = target() else {
            assert!(
                std::env::var("AGENT2SSH_TEST_REQUIRE_SSH").as_deref() != Ok("1"),
                "AGENT2SSH_TEST_REQUIRE_SSH=1 but AGENT2SSH_TEST_SSH_PORT / _KEY are not both set"
            );
            eprintln!(
                "skipping: set AGENT2SSH_TEST_SSH_PORT and AGENT2SSH_TEST_SSH_KEY to run against a real sshd"
            );
            return None;
        };
        let proxy = Proxy::start(&target.host, target.port);
        let daemon = TestDaemon::start(label);
        std::fs::write(
            daemon.dir.join("hosts.json"),
            serde_json::to_vec_pretty(&json!({
                "schema_version": 1,
                "hosts": [{
                    "name": "fixture",
                    "host": "127.0.0.1",
                    "port": proxy.port,
                    "user": target.user,
                    "key_path": target.key,
                }],
            }))
            .unwrap(),
        )
        .expect("write hosts.json");
        std::fs::write(
            daemon.dir.join("approval_policies.toml"),
            r#"
[[policies]]
name = "ask before the marker command"
hosts = ["fixture"]
command_pattern = "*needs-approval-marker*"
requires_approval = true
"#,
        )
        .expect("write approval_policies.toml");
        let mcp = McpSession::start(&daemon);
        Some(Self { daemon, mcp, proxy })
    }

    fn api(&self, method: reqwest::Method, path: &str) -> reqwest::blocking::Response {
        self.api_with(method, path, None)
    }

    fn api_with(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> reqwest::blocking::Response {
        let mut request = self
            .daemon
            .client()
            .request(method, self.daemon.url(path))
            .bearer_auth(&self.daemon.token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        request
            .send()
            .unwrap_or_else(|error| panic!("{path}: {error}"))
    }

    /// The approval card a person would see: wait for it to appear.
    fn pending_approval(&self) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            let cards: Vec<Value> = self
                .api(reqwest::Method::GET, "/approvals")
                .json()
                .expect("approvals list");
            if let Some(card) = cards.into_iter().find(|card| card["status"] == "pending") {
                return card;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("no approval card appeared: the exec never reached the daemon");
    }

    fn call(&mut self, id: u64, tool: &str, arguments: Value) -> Value {
        self.mcp.call(id, tool, arguments);
        self.mcp.next_response()
    }
}

fn exec_args(command: &str) -> Value {
    json!({ "host": "fixture", "command": command, "reason": "mcp via daemon test" })
}

fn error_code(response: &Value) -> &str {
    response["result"]["structuredContent"]["error"]["code"]
        .as_str()
        .unwrap_or_else(|| panic!("not an error result: {response}"))
}

#[test]
fn a_policy_covered_exec_waits_for_a_person_and_runs_when_approved() {
    let Some(mut rig) = Rig::new("approve") else {
        return;
    };

    // The call blocks on the card, so ask without reading the answer yet.
    rig.mcp.call(1, "ssh_exec", exec_args(NEEDS_APPROVAL));
    let card = rig.pending_approval();
    assert_eq!(card["host"], "fixture");
    assert!(
        card["command"]
            .as_str()
            .unwrap()
            .contains("needs-approval-marker"),
        "the card shows another command: {card}"
    );
    let approve = rig.api(
        reqwest::Method::POST,
        &format!("/approvals/{}/approve", card["id"].as_str().unwrap()),
    );
    assert!(
        approve.status().is_success(),
        "approve: {}",
        approve.status()
    );

    let response = rig.mcp.next_response();
    assert!(response["result"].get("isError").is_none(), "{response}");
    let text = result_text(&response);
    assert!(text.starts_with("exit_code: 0 | "), "{text:?}");
    assert!(text.contains("needs-approval-marker"), "{text:?}");
}

#[test]
fn a_rejected_exec_comes_back_as_a_rejection_the_model_can_read() {
    let Some(mut rig) = Rig::new("reject") else {
        return;
    };

    rig.mcp.call(1, "ssh_exec", exec_args(NEEDS_APPROVAL));
    let card = rig.pending_approval();
    let reject = rig.api(
        reqwest::Method::POST,
        &format!("/approvals/{}/reject", card["id"].as_str().unwrap()),
    );
    assert!(reject.status().is_success(), "reject: {}", reject.status());

    let response = rig.mcp.next_response();
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(error_code(&response), "approval_rejected", "{response}");
    let text = result_text(&response);
    assert!(
        text.contains("\nHint: A person rejected this command"),
        "{text:?}"
    );
    // And it did not run anyway.
    assert!(!text.contains("needs-approval-marker\n"), "{text:?}");
}

#[test]
fn an_exec_no_policy_covers_runs_without_a_card() {
    let Some(mut rig) = Rig::new("plain") else {
        return;
    };

    let response = rig.call(1, "ssh_exec", exec_args("echo no-card-needed"));
    let text = result_text(&response);
    assert!(text.starts_with("exit_code: 0 | "), "{text:?}");
    assert!(text.contains("no-card-needed"), "{text:?}");
    let cards: Vec<Value> = rig
        .api(reqwest::Method::GET, "/approvals")
        .json()
        .expect("approvals list");
    assert!(
        cards.is_empty(),
        "an unrelated exec raised a card: {cards:?}"
    );
}

#[test]
fn ssh_exec_multi_asks_too() {
    let Some(mut rig) = Rig::new("multi") else {
        return;
    };

    rig.mcp.call(
        1,
        "ssh_exec_multi",
        json!({ "hosts": ["fixture"], "command": NEEDS_APPROVAL }),
    );
    let card = rig.pending_approval();
    let approve = rig.api(
        reqwest::Method::POST,
        &format!("/approvals/{}/approve", card["id"].as_str().unwrap()),
    );
    assert!(approve.status().is_success());

    let response = rig.mcp.next_response();
    assert!(response["result"].get("isError").is_none(), "{response}");
    let results = &response["result"]["structuredContent"];
    assert!(
        results.to_string().contains("needs-approval-marker"),
        "the batch did not run: {results}"
    );
}

#[test]
fn a_paused_gate_stops_an_mcp_exec_with_a_code_and_a_hint() {
    let Some(mut rig) = Rig::new("gate") else {
        return;
    };

    let pause = rig.api_with(
        reqwest::Method::POST,
        "/gate/pause",
        Some(json!({ "reason": "mcp via daemon test" })),
    );
    assert!(pause.status().is_success(), "pause: {}", pause.status());
    let response = rig.call(1, "ssh_exec", exec_args("echo should-not-run"));
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(error_code(&response), "gate_paused", "{response}");
    let text = result_text(&response);
    assert!(text.contains("agent2ssh resume"), "{text:?}");
    assert!(!text.contains("should-not-run\n"), "{text:?}");

    let resume = rig.api_with(reqwest::Method::POST, "/gate/resume", Some(json!({})));
    assert!(resume.status().is_success());
    let response = rig.call(2, "ssh_exec", exec_args("echo runs-again"));
    assert!(result_text(&response).contains("runs-again"), "{response}");
}

/// A daemon that is not there must not be the end of `ssh_exec`: a stale
/// `daemon.token` with nothing listening is the everyday case. The exec runs in
/// the MCP process as it always did — and, with no way to ask a person, a command
/// an approval policy covers is refused with a code that says why.
#[test]
fn without_a_daemon_ssh_exec_runs_in_process_and_a_policy_hit_is_refused() {
    let Some(mut rig) = Rig::new("fallback") else {
        return;
    };
    // Kill the daemon but leave its token file: "connection refused", not "no token".
    let _ = rig.daemon.child.kill();
    let _ = rig.daemon.child.wait();

    let response = rig.call(1, "ssh_exec", exec_args("echo ran-in-process"));
    assert!(response["result"].get("isError").is_none(), "{response}");
    assert!(result_text(&response).contains("ran-in-process"));

    let response = rig.call(2, "ssh_exec", exec_args(NEEDS_APPROVAL));
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(error_code(&response), "approval_required", "{response}");
}

/// The retained connection is only reused by the process that holds it, so
/// `ssh_connect` has to land where `ssh_exec` does.
#[test]
fn ssh_connect_and_ssh_exec_meet_in_the_daemon() {
    let Some(mut rig) = Rig::new("connect") else {
        return;
    };

    let connected = rig.call(1, "ssh_connect", json!({ "host": "fixture" }));
    assert_eq!(
        connected["result"]["structuredContent"]["backend"], "daemon",
        "{connected}"
    );
    assert_eq!(rig.proxy.accepted(), 1, "the daemon's retained connection");

    for round in 1..=3 {
        let response = rig.call(
            10 + round,
            "ssh_exec",
            exec_args(&format!("echo round-{round}")),
        );
        assert!(
            result_text(&response).contains(&format!("round-{round}")),
            "{response}"
        );
    }
    assert_eq!(
        rig.proxy.accepted(),
        1,
        "three execs must ride the connection the daemon retained"
    );

    // The status the model sees is the daemon's, where the connection is.
    let status = rig.call(20, "ssh_connection_status", json!({}));
    // A list comes back wrapped: `structuredContent` is always an object.
    let hosts = status["result"]["structuredContent"]["result"]
        .as_array()
        .unwrap_or_else(|| panic!("status is not a list: {status}"));
    let fixture = hosts
        .iter()
        .find(|host| host["host"] == "fixture")
        .unwrap_or_else(|| panic!("no fixture in {status}"));
    assert_eq!(fixture["connected"], true, "{status}");

    let disconnected = rig.call(30, "ssh_disconnect", json!({ "host": "fixture" }));
    assert_eq!(
        disconnected["result"]["structuredContent"]["backend"], "daemon",
        "{disconnected}"
    );
    let response = rig.call(31, "ssh_exec", exec_args("echo after-disconnect"));
    assert!(result_text(&response).contains("after-disconnect"));
    assert_eq!(rig.proxy.accepted(), 2, "back to one connection per exec");
}

/// The one way the daemon-first route could make things worse: a daemon that
/// takes the request and then drops the connection may already have run the
/// command, so the MCP server must report that, not quietly run it again in its
/// own process. A refused connection (nothing sent) is the only fallback case.
#[test]
fn a_connection_lost_after_the_request_went_out_is_not_retried_in_process() {
    use std::io::Read;
    use std::net::TcpListener;

    let Some(target) = target() else {
        eprintln!("skipping: set AGENT2SSH_TEST_SSH_PORT and AGENT2SSH_TEST_SSH_KEY");
        return;
    };
    // The sshd sits behind a proxy only to be counted: a fallback to an
    // in-process exec would have to open a connection to it.
    let proxy = Proxy::start(&target.host, target.port);

    // A "daemon" that reads the request and hangs up without answering.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind the fake daemon");
    let addr = listener.local_addr().unwrap().to_string();
    let fake = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            // Dropped here: the connection closes with no response.
        }
    });

    let dir = tempfile::Builder::new()
        .prefix("a2s-fake-daemon-")
        .tempdir()
        .expect("config dir")
        .keep();
    std::fs::write(dir.join("daemon.token"), "fake-daemon-token").unwrap();
    std::fs::write(
        dir.join("hosts.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "hosts": [{
                "name": "fixture",
                "host": "127.0.0.1",
                "port": proxy.port,
                "user": target.user,
                "key_path": target.key,
            }],
        }))
        .unwrap(),
    )
    .unwrap();

    let mut mcp = McpSession::start_in(&dir, &addr);
    mcp.call(1, "ssh_exec", exec_args("echo must-not-run"));
    let response = mcp.next_response();
    let _ = fake.join();

    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(error_code(&response), "daemon_error", "{response}");
    let text = result_text(&response);
    assert!(
        text.contains("ssh_audit"),
        "the hint must say to check the audit: {text:?}"
    );
    assert_eq!(
        proxy.accepted(),
        0,
        "the exec was run in the MCP process after the daemon had the request"
    );
    let _ = std::fs::remove_dir_all(dir);
}
