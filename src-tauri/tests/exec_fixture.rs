//! Opt-in integration tests against a real `sshd`.
//!
//! These cover runtime behaviour that the unit suites structurally cannot reach:
//! draining stdout and stderr off a single SSH channel, and bounding a remote
//! command by its timeout. Both shipped broken because nothing ever exercised a
//! real server.
//!
//! The suite is skipped unless `AGENT2SSH_TEST_SSH_PORT` points at a reachable
//! sshd. To run it against the local fixture image:
//!
//! ```sh
//! docker build -t a2s-sshd:local scripts/sshd-fixture
//! docker run -d --name a2s-sshd -p 2222:22 a2s-sshd:local
//! AGENT2SSH_TEST_SSH_PORT=2222 \
//!   cargo test --manifest-path src-tauri/Cargo.toml --no-default-features \
//!     --test exec_fixture -- --test-threads=1 --nocapture
//! ```
//!
//! `scripts/e2e-docker.sh` also runs it, against the container that script has
//! already started, with the throwaway key it generated.
//!
//! Optional overrides: `AGENT2SSH_TEST_SSH_HOST` (default `127.0.0.1`),
//! `AGENT2SSH_TEST_SSH_USER` (default `root`), and either
//! `AGENT2SSH_TEST_SSH_KEY` — a private key path, which is what CI and
//! `scripts/e2e-docker.sh` use — or `AGENT2SSH_TEST_SSH_PASSWORD` (default
//! `a2s-fixture`, matching `scripts/sshd-fixture`). Key auth is preferred where
//! both are available: it keeps the encrypted credential-store path out of the
//! suite, for the same reason `scripts/e2e-docker.sh` avoids passwords.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use agent2ssh::core::exec_ssh_core;
use agent2ssh::store::list_audit_raw;
use agent2ssh::types::{AuditFilter, ExecRequest};

/// Bytes the fixture command writes to stderr. Comfortably above any realistic
/// SSH channel window, so a client that stops draining stderr stalls the remote
/// command before it ever writes stdout.
const STDERR_FLOOD_BYTES: usize = 8 * 1024 * 1024;

const MAX_OUTPUT_BYTES: usize = 64 * 1024;

struct Fixture {
    host: String,
    port: u16,
    user: String,
    /// Private key path. Takes precedence over `password` when set.
    key: Option<String>,
    password: String,
}

fn fixture() -> Option<Fixture> {
    let port = std::env::var("AGENT2SSH_TEST_SSH_PORT")
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let key = std::env::var("AGENT2SSH_TEST_SSH_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    Some(Fixture {
        host: std::env::var("AGENT2SSH_TEST_SSH_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
        port,
        user: std::env::var("AGENT2SSH_TEST_SSH_USER").unwrap_or_else(|_| "root".to_string()),
        key,
        password: std::env::var("AGENT2SSH_TEST_SSH_PASSWORD")
            .unwrap_or_else(|_| "a2s-fixture".to_string()),
    })
}

/// Points the app at a throwaway config dir holding exactly one host: the
/// fixture. `~/.agent2ssh` is never touched.
fn isolated_config(fixture: &Fixture, label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "a2s-exec-fixture-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create fixture config dir");
    let mut host = serde_json::json!({
        "name": "fixture",
        "host": fixture.host,
        "port": fixture.port,
        "user": fixture.user,
    });
    match &fixture.key {
        Some(key) => host["key_path"] = serde_json::json!(key),
        None => host["password"] = serde_json::json!(fixture.password),
    }
    let hosts = serde_json::json!({
        "schema_version": 1,
        "hosts": [host],
    });
    std::fs::write(
        dir.join("hosts.json"),
        serde_json::to_vec_pretty(&hosts).unwrap(),
    )
    .expect("write fixture hosts.json");
    std::env::set_var("AGENT2SSH_CONFIG_DIR", &dir);
    dir
}

fn release_config(dir: PathBuf) {
    std::env::remove_var("AGENT2SSH_CONFIG_DIR");
    let _ = std::fs::remove_dir_all(dir);
}

fn request(command: &str, timeout_secs: u64, max_output_bytes: usize) -> ExecRequest {
    ExecRequest {
        host: "fixture".to_string(),
        command: command.to_string(),
        // The fixture commands are benign, but `force` keeps the suite from
        // failing if the risk classifier scores a redirection as high.
        force: true,
        timeout_secs: Some(timeout_secs),
        stdin: None,
        max_output_bytes: Some(max_output_bytes),
        reason: Some("exec fixture test".to_string()),
        change_id: None,
        side_effect: None,
        source: Some("test".to_string()),
    }
}

fn skip() {
    eprintln!(
        "skipping: set AGENT2SSH_TEST_SSH_PORT (see src-tauri/tests/exec_fixture.rs) to run against a real sshd"
    );
}

/// Regression test for the stdout/stderr drain deadlock.
///
/// The command floods stderr, then writes stdout. A client that reads stdout to
/// EOF before touching stderr never gets to that stdout write: the remote blocks
/// once the channel window fills, and the read never returns.
#[tokio::test]
#[serial_test::serial]
async fn drains_stdout_and_stderr_off_one_channel_without_deadlocking() {
    let Some(fixture) = fixture() else {
        return skip();
    };
    let dir = isolated_config(&fixture, "flood");

    let command = format!(
        "sh -c 'head -c {STDERR_FLOOD_BYTES} /dev/zero | tr \"\\0\" x 1>&2; echo STDOUT-DONE'"
    );
    let started = Instant::now();
    let outcome = exec_ssh_core(request(&command, 30, MAX_OUTPUT_BYTES)).await;
    let elapsed = started.elapsed();

    let result = outcome.expect("a stderr-heavy command must not fail the exec");
    assert_eq!(
        result.exit_code,
        Some(0),
        "exit_code was {:?}",
        result.exit_code
    );
    assert!(
        result.stdout.contains("STDOUT-DONE"),
        "stdout lost the trailing marker: {:?}",
        result.stdout
    );
    assert!(
        !result.stderr.is_empty(),
        "stderr was captured as empty even though the command flooded it"
    );
    assert!(
        result.stderr.len() <= MAX_OUTPUT_BYTES,
        "stderr was not bounded: {} bytes",
        result.stderr.len()
    );
    assert!(
        elapsed < Duration::from_secs(25),
        "drain took {elapsed:?}, which means the streams were not interleaved"
    );

    release_config(dir);
}

/// Pins what a timeout can and cannot do.
///
/// Measured against OpenSSH 9.7: a pty-less exec channel's **remote command is
/// not killed** when the client closes the channel or drops the connection —
/// only the pty path (sshd's session cleanup) tears the process group down. So
/// the exec must (a) return promptly instead of waiting for the command,
/// (b) release the connection so the next command works, and (c) leave the
/// remote command running. Callers needing a hard stop must wrap the command
/// server-side, e.g. `timeout 30 <cmd>`.
///
/// If termination is ever implemented (PTY opt-in, server-side wrapper), this
/// test fails on purpose so the documented behaviour changes with it.
///
/// Note: while the client-side leak existed, this binary hung at teardown after
/// the first failing test, because the abandoned blocking task kept the tokio
/// runtime alive. A clean exit is part of the regression signal here.
#[tokio::test]
#[serial_test::serial]
async fn timeout_returns_promptly_without_terminating_a_pty_less_command() {
    let Some(fixture) = fixture() else {
        return skip();
    };
    let dir = isolated_config(&fixture, "timeout");
    let marker = format!("/tmp/a2s-late-marker-{}", std::process::id());

    let _ = exec_ssh_core(request(&format!("rm -f {marker}"), 30, MAX_OUTPUT_BYTES)).await;

    let started = Instant::now();
    let outcome = exec_ssh_core(request(
        &format!("sh -c 'sleep 8; touch {marker}'"),
        2,
        MAX_OUTPUT_BYTES,
    ))
    .await;
    let elapsed = started.elapsed();

    let error = outcome.expect_err("a command that outlives its timeout must fail");
    let message = error.to_string();
    assert!(
        message.contains("timed out"),
        "expected a timeout error, got: {message}"
    );
    assert!(
        elapsed < Duration::from_secs(6),
        "the exec waited {elapsed:?} for the remote command instead of returning at its deadline"
    );

    // The client released the connection: the next command still works.
    let probe = exec_ssh_core(request("echo AFTER-TIMEOUT", 30, MAX_OUTPUT_BYTES))
        .await
        .expect("the host must stay usable after a timeout");
    assert!(probe.stdout.contains("AFTER-TIMEOUT"));

    // Known limitation: the pty-less remote command outlives the timeout.
    tokio::time::sleep(Duration::from_secs(9)).await;
    let marker_probe = exec_ssh_core(request(
        &format!("test -f {marker} && echo PRESENT || echo ABSENT"),
        30,
        MAX_OUTPUT_BYTES,
    ))
    .await
    .expect("probe exec failed");
    assert!(
        marker_probe.stdout.contains("PRESENT"),
        "a pty-less remote command is now terminated — update the timeout contract in \
         types.rs::ExecRequest::timeout_secs and this test: {:?}",
        marker_probe.stdout
    );

    release_config(dir);
}

/// A timed-out command still ran, so it must still appear in the audit log.
/// The timeout used to return straight out of the exec and audit nothing.
#[tokio::test]
#[serial_test::serial]
async fn timeout_is_recorded_in_the_audit_log() {
    let Some(fixture) = fixture() else {
        return skip();
    };
    let dir = isolated_config(&fixture, "audit");
    let needle = format!("a2s-timeout-audit-{}", std::process::id());

    let _ = exec_ssh_core(request(
        &format!("sh -c 'sleep 8; echo {needle}'"),
        2,
        MAX_OUTPUT_BYTES,
    ))
    .await;

    let entries = list_audit_raw(&AuditFilter {
        host: None,
        risk_level: None,
        exit_code: None,
        since: None,
        until: None,
        limit: 200,
        search: None,
        command_pattern: None,
        host_env: None,
        host_role: None,
        host_owner: None,
    })
    .expect("audit log must be readable");

    let recorded = entries.iter().find(|entry| entry.command.contains(&needle));
    let recorded = recorded.unwrap_or_else(|| {
        panic!(
            "the timed-out command is missing from the audit log ({} entries)",
            entries.len()
        )
    });
    assert_eq!(recorded.exit_code, None);
    assert!(
        recorded
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("timed out")),
        "audit entry does not record the timeout: {:?}",
        recorded.reason
    );

    release_config(dir);
}
