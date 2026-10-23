#![cfg(feature = "daemon")]

//! Exercise the real destructive SFTP route through its approval lifecycle.
//! The SSH endpoint only accepts and closes TCP: reaching it proves execution
//! was authorized without requiring an SSH fixture or deleting remote data.
//! The desktop authorization route must never connect, even after approval.

mod common;

use agent2ssh::store::CONFIG_DIR_ENV;
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::io::ErrorKind;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const HOST: &str = "sftp-approval-probe";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const WAIT_BUDGET: Duration = Duration::from_secs(8);

#[derive(Debug)]
struct Reply {
    status: u16,
    body: Value,
}

fn read_reply(response: reqwest::blocking::Response) -> Result<Reply, String> {
    let status = response.status().as_u16();
    let text = response.text().map_err(|error| error.to_string())?;
    let body = serde_json::from_str(&text)
        .map_err(|error| format!("HTTP {status} returned invalid JSON: {error}; body={text}"))?;
    Ok(Reply { status, body })
}

struct TestDaemon {
    child: Option<Child>,
    dir: PathBuf,
    addr: String,
    token: String,
    client: Client,
    workers: Vec<JoinHandle<()>>,
}

impl TestDaemon {
    fn start(ssh_port: u16) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "agent2ssh-sftp-approval-e2e-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create isolated config directory");
        let port = TcpListener::bind("127.0.0.1:0").expect("reserve daemon port");
        let addr = port.local_addr().unwrap().to_string();
        drop(port);
        let token = format!("sftp-approval-test-{}", uuid::Uuid::new_v4());
        let client = Client::builder()
            .no_proxy()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("create HTTP client");
        // Own the directory before seeding or spawning, so failures below are
        // also cleaned up by Drop.
        let mut daemon = Self {
            child: None,
            dir,
            addr,
            token,
            client,
            workers: Vec::new(),
        };
        std::fs::write(daemon.dir.join("daemon.token"), &daemon.token).unwrap();
        std::fs::write(
            daemon.dir.join("daemon_tokens.toml"),
            "[[tokens]]\ntoken = \"sftp-other-host-token\"\n[tokens.scope]\nallowed_hosts = [\"other-host\"]\n",
        )
        .unwrap();
        std::fs::write(
            daemon.dir.join("hosts.json"),
            json!({
                "schema_version": 1,
                "hosts": [{
                    "name": HOST,
                    "host": "127.0.0.1",
                    "port": ssh_port,
                    "user": "test-user"
                }]
            })
            .to_string(),
        )
        .unwrap();
        // No webhook.toml means the daemon's approval notification worker is
        // inert; this test neither sends external messages nor opens windows.
        daemon.child = Some(
            Command::new(env!("CARGO_BIN_EXE_agent2ssh-daemon"))
                .env(CONFIG_DIR_ENV, &daemon.dir)
                .env("AGENT2SSH_DAEMON_ADDR", &daemon.addr)
                .env("AGENT2SSH_SECRETS_BACKEND", "memory")
                .env("AGENT2SSH_LOG", "warn")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn real agent2ssh-daemon"),
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if let Ok(response) = daemon
                .client
                .get(daemon.url("/health"))
                .timeout(Duration::from_millis(500))
                .send()
            {
                if response.status().is_success() {
                    return daemon;
                }
            }
            assert!(
                daemon.child.as_mut().unwrap().try_wait().unwrap().is_none(),
                "daemon exited before serving /health"
            );
            thread::sleep(Duration::from_millis(50));
        }
        panic!("daemon did not become ready within 30 seconds");
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    fn delete_body(path: &str) -> Value {
        json!({ "host": HOST, "path": path, "timeout_secs": 2 })
    }

    fn start_delete(&mut self, path: &str) -> Receiver<Result<Reply, String>> {
        self.start_request("/sftp/rm-rf", path)
    }

    fn start_authorize(&mut self, path: &str) -> Receiver<Result<Reply, String>> {
        self.start_request("/sftp/rm-rf/authorize", path)
    }

    fn start_request(&mut self, endpoint: &str, path: &str) -> Receiver<Result<Reply, String>> {
        let (sender, receiver) = mpsc::channel();
        let request = self
            .client
            .post(self.url(endpoint))
            .bearer_auth(&self.token)
            .json(&Self::delete_body(path));
        self.workers.push(thread::spawn(move || {
            let reply = request
                .send()
                .map_err(|error| error.to_string())
                .and_then(read_reply);
            let _ = sender.send(reply);
        }));
        receiver
    }

    fn approvals(&self) -> Vec<Value> {
        let reply = read_reply(
            self.client
                .get(self.url("/approvals"))
                .bearer_auth(&self.token)
                .timeout(Duration::from_secs(2))
                .send()
                .expect("GET /approvals"),
        )
        .unwrap();
        assert_eq!(reply.status, 200, "{reply:?}");
        reply.body.as_array().expect("approval array").clone()
    }

    fn wait_for_approval(&self, path: &str) -> String {
        let command = format!("sftp rm-rf {path}");
        let deadline = Instant::now() + WAIT_BUDGET;
        while Instant::now() < deadline {
            for approval in self.approvals() {
                if approval["host"] == HOST && approval["command"] == command {
                    assert_eq!(approval["status"], "pending", "{approval}");
                    assert_eq!(approval["risk_level"], "high", "{approval}");
                    return approval["id"].as_str().expect("approval id").to_string();
                }
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!("no pending approval appeared for {command} within {WAIT_BUDGET:?}");
    }

    fn respond(&self, id: &str, decision: &str) {
        let reply = read_reply(
            self.client
                .post(self.url(&format!("/approvals/{id}/{decision}")))
                .bearer_auth(&self.token)
                .timeout(Duration::from_secs(2))
                .send()
                .expect("respond to approval"),
        )
        .unwrap();
        assert_eq!(reply.status, 200, "{reply:?}");
        assert_eq!(reply.body["ok"], true, "{reply:?}");
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        // Killing the daemon releases outstanding HTTP calls. Each call also
        // has its own timeout, so cleanup cannot wait for an approval's TTL.
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn assert_no_ssh_connection(listener: &TcpListener, phase: &str) {
    match listener.accept() {
        Err(error) if error.kind() == ErrorKind::WouldBlock => {}
        Ok(_) => panic!("SSH was contacted {phase}"),
        Err(error) => panic!("failed to inspect SSH listener {phase}: {error}"),
    }
}

fn assert_waiting(request: &Receiver<Result<Reply, String>>) {
    match request.try_recv() {
        Err(TryRecvError::Empty) => {}
        result => panic!("delete completed before approval was answered: {result:?}"),
    }
}

fn finish(request: Receiver<Result<Reply, String>>) -> Reply {
    request
        .recv_timeout(WAIT_BUDGET)
        .expect("delete request must finish within the deadline")
        .expect("delete request must return an HTTP response")
}

#[test]
fn recursive_sftp_delete_waits_for_real_daemon_approval_before_connecting() {
    let ssh = TcpListener::bind("127.0.0.1:0").expect("bind fake SSH endpoint");
    ssh.set_nonblocking(true).unwrap();
    let mut daemon = TestDaemon::start(ssh.local_addr().unwrap().port());

    let blocked = finish(daemon.start_delete("/"));
    assert_eq!(blocked.status, 400, "{blocked:?}");
    assert!(
        blocked.body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("blocked by risk policy"),
        "root deletion must fail for the risk-policy reason: {blocked:?}"
    );
    assert!(
        daemon.approvals().is_empty(),
        "blocked root must not request approval"
    );
    assert_no_ssh_connection(&ssh, "for a blocked root path");

    let denied_request = daemon.start_delete("/tmp/rejected-tree");
    let denied_id = daemon.wait_for_approval("/tmp/rejected-tree");
    assert_waiting(&denied_request);
    assert_no_ssh_connection(&ssh, "while rejection-path approval is pending");
    daemon.respond(&denied_id, "reject");
    let denied = finish(denied_request);
    assert_eq!(denied.status, 403, "{denied:?}");
    assert_eq!(
        denied.body["error"], "command rejected by approver",
        "{denied:?}"
    );
    assert_no_ssh_connection(&ssh, "after the approval was rejected");

    let allowed_request = daemon.start_delete("/tmp/approved-tree");
    let allowed_id = daemon.wait_for_approval("/tmp/approved-tree");
    assert_waiting(&allowed_request);
    assert_no_ssh_connection(&ssh, "before the approval was granted");
    daemon.respond(&allowed_id, "approve");
    let deadline = Instant::now() + WAIT_BUDGET;
    loop {
        match ssh.accept() {
            Ok((stream, _)) => {
                // We deliberately do not speak SSH or delete anything. Close
                // the socket so the authorized request fails its handshake.
                drop(stream);
                break;
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "approved delete never attempted SSH"
                );
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("failed to accept authorized SSH connection: {error}"),
        }
    }
    let allowed = finish(allowed_request);
    assert_eq!(
        allowed.status, 400,
        "fake SSH peer must fail the handshake: {allowed:?}"
    );
    let error = allowed.body["error"]
        .as_str()
        .expect("SSH failure error body");
    assert!(!error.is_empty(), "{allowed:?}");
    assert!(
        !error.contains("approval") && !error.contains("blocked"),
        "{allowed:?}"
    );
}

#[test]
fn desktop_delete_authorization_checks_guards_and_never_connects_to_ssh() {
    let ssh = TcpListener::bind("127.0.0.1:0").expect("bind fake SSH endpoint");
    ssh.set_nonblocking(true).unwrap();
    let mut daemon = TestDaemon::start(ssh.local_addr().unwrap().port());

    let no_auth = read_reply(
        daemon
            .client
            .post(daemon.url("/sftp/rm-rf/authorize"))
            .json(&TestDaemon::delete_body("/tmp/no-auth"))
            .send()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(no_auth.status, 401, "{no_auth:?}");
    let scoped = read_reply(
        daemon
            .client
            .post(daemon.url("/sftp/rm-rf/authorize"))
            .bearer_auth("sftp-other-host-token")
            .json(&TestDaemon::delete_body("/tmp/outside-scope"))
            .send()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(scoped.status, 403, "{scoped:?}");
    assert!(scoped.body["error"]
        .as_str()
        .unwrap_or_default()
        .contains("not in the allowed hosts list"));

    let blocked = finish(daemon.start_authorize("/"));
    assert_eq!(blocked.status, 400, "{blocked:?}");
    assert!(blocked.body["error"]
        .as_str()
        .unwrap_or_default()
        .contains("blocked by risk policy"));
    assert!(daemon.approvals().is_empty());
    assert_no_ssh_connection(&ssh, "after authentication, scope and risk refusals");

    let paused = read_reply(
        daemon
            .client
            .post(daemon.url("/gate/pause"))
            .bearer_auth(&daemon.token)
            .json(&json!({}))
            .send()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(paused.status, 200, "{paused:?}");
    let gated = finish(daemon.start_authorize("/tmp/paused-tree"));
    assert_eq!(gated.status, 423, "{gated:?}");
    assert_eq!(gated.body["error"], "execution gate paused");
    assert!(daemon.approvals().is_empty());
    let resumed = read_reply(
        daemon
            .client
            .post(daemon.url("/gate/resume"))
            .bearer_auth(&daemon.token)
            .json(&json!({}))
            .send()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(resumed.status, 200, "{resumed:?}");
    assert_no_ssh_connection(&ssh, "while the execution gate was paused");

    let denied_path = "/tmp/rejected-desktop-tree";
    let denied_request = daemon.start_authorize(denied_path);
    let denied_id = daemon.wait_for_approval(denied_path);
    assert_waiting(&denied_request);
    assert_no_ssh_connection(&ssh, "while desktop approval is pending");
    daemon.respond(&denied_id, "reject");
    let denied = finish(denied_request);
    assert_eq!(denied.status, 403, "{denied:?}");
    assert_eq!(denied.body["error"], "command rejected by approver");
    assert_no_ssh_connection(&ssh, "after desktop approval is rejected");

    // Keep significant whitespace so the desktop can match the exact path it
    // asked about; authorization must not silently rewrite the target.
    let allowed_path = "/tmp/approved  desktop-tree";
    let allowed_request = daemon.start_authorize(allowed_path);
    let allowed_id = daemon.wait_for_approval(allowed_path);
    assert_waiting(&allowed_request);
    daemon.respond(&allowed_id, "approve");
    let allowed = finish(allowed_request);
    assert_eq!(allowed.status, 200, "{allowed:?}");
    assert_eq!(
        allowed.body,
        json!({
            "authorized": true,
            "host": HOST,
            "path": allowed_path,
            "risk_level": "high",
        }),
    );
    assert_no_ssh_connection(&ssh, "even after desktop authorization succeeds");

    // An authorization result must not become a cached permission to execute
    // the same request on the daemon. Its execution endpoint still prompts.
    let execution = daemon.start_delete(allowed_path);
    let deadline = Instant::now() + WAIT_BUDGET;
    let execution_approval = loop {
        if let Some(pending) = daemon.approvals().into_iter().find(|approval| {
            approval["host"] == HOST
                && approval["command"] == format!("sftp rm-rf {allowed_path}")
                && approval["status"] == "pending"
                && approval["id"] != allowed_id
        }) {
            break pending["id"].as_str().unwrap().to_string();
        }
        assert!(
            Instant::now() < deadline,
            "execution reused an earlier approval"
        );
        thread::sleep(Duration::from_millis(25));
    };
    assert_waiting(&execution);
    assert_no_ssh_connection(&ssh, "while the separate execution approval is pending");
    daemon.respond(&execution_approval, "reject");
    assert_eq!(finish(execution).status, 403);
    assert_no_ssh_connection(&ssh, "after rejecting the separate execution");
}
