//! A real `agent2ssh-daemon` process, and an `agent2ssh-mcp` server bound to
//! it, for the tests that have to cross the daemon boundary.
//!
//! `daemon_integration.rs` runs a *mirrored* router, so nothing in it exercises
//! the real handlers; these spawn the real binaries. The daemon binary exists
//! only under the `daemon` feature (`CARGO_BIN_EXE_agent2ssh-daemon`), hence the
//! gate on this whole module.
//!
//! Every test gets its own daemon, config directory and port, and the
//! environment is passed to the child rather than set on the test process, so
//! tests using it can run in parallel.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use agent2ssh::store::CONFIG_DIR_ENV;

/// How long the daemon is given to answer `/health` before the test gives up.
/// Generous because a cold start runs the secret migration and reads the config.
pub const STARTUP_BUDGET: Duration = Duration::from_secs(30);

/// A daemon process under test, torn down with its directory on drop.
pub struct TestDaemon {
    pub child: Child,
    pub dir: PathBuf,
    pub addr: String,
    pub token: String,
}

impl TestDaemon {
    pub fn start(label: &str) -> Self {
        Self::start_seeded(label, None)
    }

    /// Start a daemon whose config dir already holds an `environments.toml`.
    /// Seeded before the process exists because the daemon reads that file once,
    /// at start — which is exactly what the tests below depend on.
    pub fn start_with_environments(label: &str, environments: &str) -> Self {
        Self::start_seeded(label, Some(environments))
    }

    pub fn start_seeded(label: &str, environments: Option<&str>) -> Self {
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

    pub fn spawn(dir: &std::path::Path, addr: &str) -> Child {
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

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    pub fn client(&self) -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("build the http client")
    }

    /// Block until `/health` answers, which is the daemon's public liveness
    /// route — no token needed, so this cannot accidentally pass on a 401.
    pub fn wait_until_serving(&mut self) -> Result<(), (bool, String)> {
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

    pub fn post_local_exec(&self, body: serde_json::Value) -> reqwest::blocking::Response {
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
pub fn free_loopback_addr() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let addr = listener.local_addr().expect("read the bound port");
    drop(listener);
    addr.to_string()
}

pub fn body_text(response: reqwest::blocking::Response) -> String {
    let status = response.status();
    let text = response.text().unwrap_or_default();
    format!("status={status} body={text}")
}

pub const MCP_SOURCE: &str = "e2e_harness";
pub const MCP_BINDING_KEY: &str = "e2e-binding-key";

/// An MCP server process bound to a test daemon.
pub struct McpSession {
    pub child: Child,
    pub stdin: Option<std::process::ChildStdin>,
    pub stdout: std::io::BufReader<std::process::ChildStdout>,
}

impl McpSession {
    pub fn start(daemon: &TestDaemon) -> Self {
        Self::start_in(&daemon.dir, &daemon.addr)
    }

    /// An MCP server whose config directory is `dir` and whose local daemon is
    /// said to be at `daemon_addr` — which need not be a real daemon, for the
    /// tests that stand in for a misbehaving one.
    pub fn start_in(dir: &std::path::Path, daemon_addr: &str) -> Self {
        std::fs::write(
            dir.join("mcp_bindings.json"),
            serde_json::json!({ "keys": { MCP_SOURCE: MCP_BINDING_KEY } }).to_string(),
        )
        .expect("seed the MCP binding");
        let mut child = Command::new(env!("CARGO_BIN_EXE_agent2ssh-mcp"))
            .env(CONFIG_DIR_ENV, dir)
            .env("AGENT2SSH_DAEMON_ADDR", daemon_addr)
            .env("AGENT2SSH_SOURCE", MCP_SOURCE)
            .env("AGENT2SSH_BINDING_KEY", MCP_BINDING_KEY)
            .env_remove("AGENT2SSH_MCP_MAX_OUTPUT_BYTES")
            .env_remove("AGENT2SSH_MCP_OUTPUT_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn agent2ssh-mcp");
        let stdin = child.stdin.take();
        let stdout = std::io::BufReader::new(child.stdout.take().expect("MCP stdout"));
        Self {
            child,
            stdin,
            stdout,
        }
    }

    pub fn call(&mut self, id: u64, tool: &str, arguments: serde_json::Value) {
        use std::io::Write;
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": tool, "arguments": arguments }
        });
        let stdin = self.stdin.as_mut().expect("MCP stdin is open");
        writeln!(stdin, "{request}").expect("write the MCP request");
        stdin.flush().expect("flush the MCP request");
    }

    pub fn next_response(&mut self) -> serde_json::Value {
        use std::io::BufRead;
        let mut line = String::new();
        let read = self.stdout.read_line(&mut line).expect("read MCP stdout");
        assert!(read > 0, "the MCP server closed stdout before answering");
        serde_json::from_str(&line)
            .unwrap_or_else(|error| panic!("MCP wrote a non-JSON line ({error}): {line}"))
    }
}

impl Drop for McpSession {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn result_text(response: &serde_json::Value) -> &str {
    response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("the response carries no text content: {response}"))
}
