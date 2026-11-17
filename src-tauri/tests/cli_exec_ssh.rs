//! The `agent2ssh exec` command line as a script or a harness uses it, against
//! a real sshd.
//!
//! Pins the contract the `exec --help` text and `docs/skills.md` state, which is
//! easy to get subtly wrong and hard to see from the source:
//!
//! | situation                          | exit status        | stdout                 |
//! |------------------------------------|--------------------|------------------------|
//! | command ran, plain                 | the command's own  | its stdout             |
//! | command ran, `--json`              | 0                  | the result, with `exit_code` |
//! | Agent2SSH itself failed            | 1                  | empty, or under `--json` `{"ok": false, "error": {"code", "message"}}` |
//! | bad usage                          | 2                  | empty                  |
//!
//! Skipped unless `AGENT2SSH_TEST_SSH_PORT` and `AGENT2SSH_TEST_SSH_KEY` name a
//! reachable sshd (same variables as `exec_fixture.rs`); fails instead of
//! skipping under `AGENT2SSH_TEST_REQUIRE_SSH=1`.

mod common;

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use agent2ssh::store::CONFIG_DIR_ENV;
use serde_json::{json, Value};

struct Cli {
    dir: PathBuf,
}

fn cli() -> Option<Cli> {
    let port = std::env::var("AGENT2SSH_TEST_SSH_PORT")
        .ok()
        .and_then(|raw| raw.trim().parse::<u16>().ok())
        .filter(|port| *port != 0);
    let key = std::env::var("AGENT2SSH_TEST_SSH_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let (Some(port), Some(key)) = (port, key) else {
        assert!(
            std::env::var("AGENT2SSH_TEST_REQUIRE_SSH").as_deref() != Ok("1"),
            "AGENT2SSH_TEST_REQUIRE_SSH=1 but AGENT2SSH_TEST_SSH_PORT / _KEY are not both set"
        );
        eprintln!("skipping: set AGENT2SSH_TEST_SSH_PORT and AGENT2SSH_TEST_SSH_KEY");
        return None;
    };
    let dir = std::env::temp_dir().join(format!("a2s-cli-exec-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("config dir");
    std::fs::write(
        dir.join("hosts.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "hosts": [{
                "name": "fixture",
                "host": std::env::var("AGENT2SSH_TEST_SSH_HOST")
                    .unwrap_or_else(|_| "127.0.0.1".to_string()),
                "port": port,
                "user": std::env::var("AGENT2SSH_TEST_SSH_USER")
                    .unwrap_or_else(|_| "root".to_string()),
                "key_path": key,
            }],
        }))
        .unwrap(),
    )
    .expect("hosts.json");
    Some(Cli { dir })
}

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Cli {
    /// `agent2ssh exec fixture <args>`, with `input` on its stdin.
    fn exec(&self, args: &[&str], input: Option<&str>) -> Run {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agent2ssh"))
            .env(CONFIG_DIR_ENV, &self.dir)
            .env("AGENT2SSH_SECRETS_BACKEND", "memory")
            .arg("exec")
            .arg("fixture")
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn agent2ssh");
        if let Some(input) = input {
            let mut stdin = child.stdin.take().unwrap();
            stdin.write_all(input.as_bytes()).unwrap();
        }
        let output = child.wait_with_output().expect("agent2ssh output");
        Run {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

impl Drop for Cli {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn json_of(run: &Run) -> Value {
    serde_json::from_str(&run.stdout)
        .unwrap_or_else(|error| panic!("stdout is not JSON ({error}): {:?}", run.stdout))
}

#[test]
fn without_json_the_exit_status_is_the_remote_commands_own() {
    let Some(cli) = cli() else { return };
    for status in [0, 1, 2, 7] {
        let run = cli.exec(&[&format!("exit {status}")], None);
        assert_eq!(run.code, Some(status), "`exit {status}`: {}", run.stderr);
        assert!(run.stdout.is_empty());
    }
    // Output goes through untouched.
    let run = cli.exec(&["echo out; echo err >&2; exit 3"], None);
    assert_eq!(run.code, Some(3));
    assert_eq!(run.stdout, "out\n");
    assert_eq!(run.stderr.trim(), "err");
}

#[test]
fn with_json_a_command_that_ran_exits_zero_and_the_status_is_in_the_result() {
    let Some(cli) = cli() else { return };
    let run = cli.exec(&["exit 7", "--json"], None);
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    let result = json_of(&run);
    assert_eq!(result["exit_code"], 7);
    // A result is not a failure object: `ok` is only ever present, and false,
    // on a failure.
    assert!(result.get("ok").is_none(), "{result}");
}

#[test]
fn an_agent2ssh_failure_exits_one_and_under_json_says_why_on_stdout() {
    let Some(cli) = cli() else { return };

    // Blocked by risk policy: refused before anything connects.
    let run = cli.exec(&["mkfs /dev/sda", "--json"], None);
    assert_eq!(run.code, Some(1));
    let failure = json_of(&run);
    assert_eq!(failure["ok"], false);
    assert_eq!(failure["error"]["code"], "command_blocked", "{failure}");
    assert!(run.stderr.starts_with("Error: "), "{:?}", run.stderr);

    // High risk without --force: the CLI has no approval handler, so it is
    // `approval_required`, and the message itself names the way out.
    let run = cli.exec(&["rm -rf /tmp/a2s-never-created", "--json"], None);
    assert_eq!(run.code, Some(1));
    let failure = json_of(&run);
    assert_eq!(failure["error"]["code"], "approval_required", "{failure}");
    assert!(
        failure["error"]["message"]
            .as_str()
            .unwrap()
            .contains("--force"),
        "{failure}"
    );

    // A timeout is Agent2SSH's failure, not the command's exit status.
    let run = cli.exec(&["sleep 30", "--timeout-secs", "2", "--json"], None);
    assert_eq!(run.code, Some(1), "stderr: {}", run.stderr);
    assert_eq!(json_of(&run)["error"]["code"], "timeout");

    // The same failures without --json: stdout stays empty.
    let run = cli.exec(&["mkfs /dev/sda"], None);
    assert_eq!(run.code, Some(1));
    assert!(run.stdout.is_empty(), "{:?}", run.stdout);
}

#[test]
fn stdin_file_feeds_a_file_or_the_process_stdin_and_keeps_trailing_newlines() {
    let Some(cli) = cli() else { return };
    let payload = "first line\nsecond line\n\n\n";

    // `-` is this process's own stdin — what a harness pipes into.
    let run = cli.exec(&["cat", "--stdin-file", "-"], Some(payload));
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert_eq!(run.stdout, payload, "trailing newlines must survive");

    // A file works the same way.
    let file = cli.dir.join("payload.txt");
    std::fs::write(&file, payload).unwrap();
    let run = cli.exec(&["cat", "--stdin-file", file.to_str().unwrap()], None);
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    assert_eq!(run.stdout, payload);

    // Larger than a command line can carry (macOS ARG_MAX is ~1 MiB).
    let big = "x".repeat(2 * 1024 * 1024) + "\n";
    let run = cli.exec(&["wc -c", "--stdin-file", "-", "--json"], Some(&big));
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    let result = json_of(&run);
    assert_eq!(
        result["stdout"].as_str().unwrap().trim(),
        big.len().to_string(),
        "{result}"
    );
}

#[test]
fn max_output_bytes_keeps_the_head_and_tail_and_says_so() {
    let Some(cli) = cli() else { return };
    let run = cli.exec(
        &["seq 1 20000", "--max-output-bytes", "200", "--json"],
        None,
    );
    assert_eq!(run.code, Some(0), "stderr: {}", run.stderr);
    let result = json_of(&run);
    assert_eq!(result["truncated"], true, "{result}");
    assert!(
        result["dropped_bytes"].as_u64().unwrap() > 100_000,
        "{result}"
    );
    let stdout = result["stdout"].as_str().unwrap();
    assert!(stdout.len() <= 200, "kept {} bytes", stdout.len());
    assert!(stdout.starts_with("1\n"), "{stdout:?}");
    assert!(stdout.trim_end().ends_with("20000"), "{stdout:?}");
}
