//! Integration tests for the local exec target.
//!
//! No fixture and no network are needed: the "server" is this machine. What they
//! do need is process-global environment — the config dir the audit log lands in,
//! the shell under test, and the exec-depth marker — so every test here is
//! serialized and runs under its own [`IsolatedEnv`].
//!
//! The shell is pinned to `cmd.exe` on Windows and `/bin/sh` elsewhere rather
//! than left to `default_shell()`, because the assertions below are written
//! against those shells' quoting rules. `resolve_shell` reads the override per
//! call precisely so this is possible.

use std::path::PathBuf;
use std::time::{Duration, Instant};

mod common;

use agent2ssh::container_runtime::{available as available_runtimes, ContainerRuntime};
use agent2ssh::store::{list_audit_raw, CONFIG_DIR_ENV};
use agent2ssh::types::{AuditEntry, AuditFilter, ExecRequest};
use agent2ssh::{
    exec_local_core, ExecTarget, LOCAL_EXEC_DEPTH_ENV, LOCAL_EXEC_SHELL_ENV, LOCAL_TARGET_ID,
};

#[cfg(unix)]
const TEST_SHELL: &str = "/bin/sh";
#[cfg(windows)]
const TEST_SHELL: &str = "cmd.exe";

/// A command that echoes whatever is piped into it.
#[cfg(unix)]
const STDIN_ECHO: &str = "cat";
#[cfg(windows)]
const STDIN_ECHO: &str = "findstr .";

/// A command that stays busy well past any timeout used here.
#[cfg(unix)]
const LONG_COMMAND: &str = "sleep 30";
#[cfg(windows)]
const LONG_COMMAND: &str = "ping -n 30 127.0.0.1";

/// Point the app at a throwaway config dir and pin the shell. Restores and
/// removes both on drop, so a panicking assertion cannot leak state into the
/// next test.
struct IsolatedEnv {
    dir: PathBuf,
}

impl IsolatedEnv {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "a2s-local-exec-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create isolated config dir");
        std::env::set_var(CONFIG_DIR_ENV, &dir);
        std::env::set_var(LOCAL_EXEC_SHELL_ENV, TEST_SHELL);
        std::env::remove_var(LOCAL_EXEC_DEPTH_ENV);
        Self { dir }
    }

    fn marker(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for IsolatedEnv {
    fn drop(&mut self) {
        // Back to the isolated default, not unset: unset would send later tests
        // to the real config directory.
        common::restore_default_config_env();
        std::env::remove_var(LOCAL_EXEC_SHELL_ENV);
        std::env::remove_var(LOCAL_EXEC_DEPTH_ENV);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn local_request(command: &str) -> ExecRequest {
    ExecRequest {
        host: LOCAL_TARGET_ID.to_string(),
        command: command.to_string(),
        // The commands here are benign, but `force` keeps the suite from failing
        // if the risk classifier scores a redirection as high.
        force: true,
        timeout_secs: Some(30),
        stdin: None,
        max_output_bytes: None,
        reason: Some("local exec integration test".to_string()),
        change_id: None,
        side_effect: None,
        source: Some("test".to_string()),
    }
}

fn audit_entries() -> Vec<AuditEntry> {
    list_audit_raw(&AuditFilter {
        limit: 50,
        ..AuditFilter::default()
    })
    .expect("read audit log")
}

#[tokio::test]
#[serial_test::serial]
async fn local_exec_captures_stdout_exit_code_and_audit_entry() {
    let _env = IsolatedEnv::new("stdout");

    let result = exec_local_core(local_request("echo hello-local"))
        .await
        .expect("local exec");

    assert_eq!(result.host, LOCAL_TARGET_ID);
    assert_eq!(result.exit_code, Some(0));
    assert!(
        result.stdout.contains("hello-local"),
        "stdout was {:?}",
        result.stdout
    );
    assert!(result.stderr.is_empty(), "stderr was {:?}", result.stderr);

    // The local target must not be a quieter code path than SSH: a successful
    // run has to land in the same audit log, under the reserved host id.
    let entries = audit_entries();
    assert_eq!(entries.len(), 1, "expected exactly one audit entry");
    assert_eq!(entries[0].host, LOCAL_TARGET_ID);
    assert_eq!(entries[0].exit_code, Some(0));
    // The audit log has to be readable as local-vs-remote without the reader
    // knowing that `local` is a reserved spelling.
    assert_eq!(entries[0].target(), ExecTarget::Local);
}

#[tokio::test]
#[serial_test::serial]
async fn local_exec_keeps_stderr_out_of_stdout() {
    let _env = IsolatedEnv::new("stderr");

    let result = exec_local_core(local_request("echo problem 1>&2"))
        .await
        .expect("local exec");

    assert!(
        result.stderr.contains("problem"),
        "stderr was {:?}",
        result.stderr
    );
    assert!(
        result.stdout.trim().is_empty(),
        "stdout was {:?}",
        result.stdout
    );
}

#[tokio::test]
#[serial_test::serial]
async fn local_exec_surfaces_a_nonzero_exit_code() {
    let _env = IsolatedEnv::new("exit-code");

    let result = exec_local_core(local_request("exit 3"))
        .await
        .expect("local exec");

    assert_eq!(result.exit_code, Some(3));
}

#[tokio::test]
#[serial_test::serial]
async fn local_exec_pipes_stdin_into_the_child() {
    let _env = IsolatedEnv::new("stdin");

    let mut request = local_request(STDIN_ECHO);
    request.stdin = Some("piped-through-local-exec\n".to_string());

    let result = exec_local_core(request).await.expect("local exec");

    assert!(
        result.stdout.contains("piped-through-local-exec"),
        "stdout was {:?}",
        result.stdout
    );
}

#[tokio::test]
#[serial_test::serial]
async fn local_exec_truncates_with_the_same_head_and_tail_budget_as_ssh() {
    let _env = IsolatedEnv::new("truncate");

    let mut request = local_request(&format!("echo {}", "a".repeat(5000)));
    request.max_output_bytes = Some(1024);

    let result = exec_local_core(request).await.expect("local exec");

    assert!(result.truncated, "expected the output to be truncated");
    assert_eq!(result.stdout.len(), 1024, "budget is per stream and exact");
    assert!(
        result.dropped_bytes > 0,
        "the dropped middle must be counted"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn local_exec_refuses_a_host_alias_that_is_not_the_local_target() {
    let _env = IsolatedEnv::new("host-guard");

    let mut request = local_request("echo should-not-run");
    // A real alias would have been authorized against that profile — risk
    // override included — and then run here, which is target confusion the audit
    // trail could never reveal.
    request.host = "prod-db".to_string();

    let error = exec_local_core(request)
        .await
        .expect_err("a non-local host must be refused");

    assert!(
        error.to_string().contains(LOCAL_TARGET_ID),
        "error was {error}"
    );
    assert!(
        audit_entries().is_empty(),
        "a refused target must not be audited as an exec"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn local_exec_refuses_a_blocked_command_even_with_force() {
    let _env = IsolatedEnv::new("blocked");

    let mut request = local_request("echo x >/dev/sda");
    request.force = true;

    let error = exec_local_core(request)
        .await
        .expect_err("a blocked command must be refused");

    assert!(error.to_string().contains("blocked"), "error was {error}");
    let entries = audit_entries();
    assert_eq!(entries.len(), 1, "a blocked exec is audited as rejected");
    assert_eq!(entries[0].host, LOCAL_TARGET_ID);
}

#[tokio::test]
#[serial_test::serial]
async fn local_exec_refuses_to_nest_beyond_the_depth_limit() {
    let _env = IsolatedEnv::new("depth");
    std::env::set_var(LOCAL_EXEC_DEPTH_ENV, "1");

    let error = exec_local_core(local_request("echo nope"))
        .await
        .expect_err("nesting must be refused");

    assert!(error.to_string().contains("refused"), "error was {error}");
    let entries = audit_entries();
    assert_eq!(
        entries.len(),
        1,
        "the refusal is audited like any rejection"
    );
    assert_eq!(entries[0].host, LOCAL_TARGET_ID);
}

#[tokio::test]
#[serial_test::serial]
async fn local_exec_stops_a_command_that_outlives_its_timeout() {
    let _env = IsolatedEnv::new("timeout");

    let mut request = local_request(LONG_COMMAND);
    request.timeout_secs = Some(1);

    let started = Instant::now();
    let error = exec_local_core(request)
        .await
        .expect_err("the timeout must fail the exec");

    assert!(error.to_string().contains("timed out"), "error was {error}");
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "the timeout did not bound the call: {:?}",
        started.elapsed()
    );
}

/// The headline property of the local target: a timed-out command leaves nothing
/// running behind it. The marker is written by a *descendant* of the shell, so
/// killing only the shell would leave it alive to complete the write.
///
/// This also covers the collector's release path: the shell exits immediately
/// after backgrounding the subshell, so the lingering inherited pipe — not the
/// timeout — is what a naive implementation would wait on.
#[cfg(unix)]
#[tokio::test]
#[serial_test::serial]
async fn local_exec_reaps_the_process_group_and_not_just_the_shell() {
    let env = IsolatedEnv::new("group");
    let marker = env.marker("late.txt");

    let command = format!("(sleep 3; echo late > \"{}\") &", marker.display());
    let mut request = local_request(&command);
    request.timeout_secs = Some(1);

    let started = Instant::now();
    // Deliberately not asserted to fail: the shell itself exits cleanly, so the
    // interesting behaviour is that the call returns promptly at all.
    let _ = exec_local_core(request).await;
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "a lingering descendant held the call open: {:?}",
        started.elapsed()
    );

    std::thread::sleep(Duration::from_secs(4));
    assert!(
        !marker.exists(),
        "the descendant survived the exec that started it"
    );
}

/// Windows counterpart of the test above: the inner `cmd` is a child of the
/// shell, and the marker is written by that child. Killing only the outer shell
/// would leave the child to finish the job.
///
/// Skipped when the temp directory contains a space, because the inner command
/// has to reach `cmd /c` as one quoted string and cmd's quoting cannot be made
/// reliable for that case from here.
#[cfg(windows)]
#[tokio::test]
#[serial_test::serial]
async fn local_exec_reaps_the_process_tree_and_not_just_the_shell() {
    let env = IsolatedEnv::new("tree");
    let marker = env.marker("late.txt");

    if marker.to_string_lossy().contains(' ') {
        eprintln!("skipping: temp path contains a space and cannot be quoted through cmd");
        return;
    }

    let command = format!(
        "cmd /c \"ping -n 3 127.0.0.1 >nul & echo late > {}\"",
        marker.display()
    );
    let mut request = local_request(&command);
    request.timeout_secs = Some(1);

    let error = exec_local_core(request)
        .await
        .expect_err("the timeout must fail the exec");
    assert!(error.to_string().contains("timed out"), "error was {error}");

    std::thread::sleep(Duration::from_secs(3));
    assert!(!marker.exists(), "the child process survived the timeout");
}

/// A named environment has to actually reach that environment — that is the
/// entire point of naming one.
///
/// `$0` is the discriminator rather than some variable: it is a shell parameter,
/// so cmd echoes it literally while a POSIX shell substitutes itself. An
/// inherited variable could not tell the two apart, because the child inherits
/// this test process's environment either way.
#[tokio::test]
#[serial_test::serial]
async fn a_named_environment_runs_in_that_environment() {
    let _env = IsolatedEnv::new("named");

    let mut request = local_request("echo $0");
    request.host = format!("{LOCAL_TARGET_ID}:gitbash");
    let bash = exec_local_core(request).await.expect("git bash exec");
    assert_eq!(bash.host, format!("{LOCAL_TARGET_ID}:gitbash"));
    assert!(
        bash.stdout.to_lowercase().contains("bash"),
        "a POSIX shell should have substituted $0: {:?}",
        bash.stdout
    );

    let mut request = local_request("echo $0");
    request.host = format!("{LOCAL_TARGET_ID}:cmd");
    // The cmd half is Windows evidence — cmd does not exist elsewhere, so the
    // assertion only runs where the environment does. The gitbash half above
    // already proves on every host that a named environment is the one that
    // ran; the cmd-specific discriminator ($0 echoed literally) is recorded
    // on Windows, per the outstanding-work split in AGENTS.md.
    if cfg!(windows) {
        let cmd = exec_local_core(request).await.expect("cmd exec");
        assert_eq!(cmd.exit_code, Some(0), "stderr: {}", cmd.stderr);
        assert_eq!(
            cmd.stdout.trim(),
            "$0",
            "cmd must not substitute $0, so this shell is not a POSIX one"
        );
    }
}

/// The environment has to survive into the audit record, or an operator cannot
/// tell an MSVC build from one that ran in a plain shell.
///
/// The named environment is host-dependent — `cmd` only exists on Windows — so
/// each platform asserts against one it actually has; the audit contract under
/// test (the host id records the *named* environment, not the bare `local`) is
/// the same either way.
#[tokio::test]
#[serial_test::serial]
async fn the_audit_records_which_environment_ran_the_command() {
    let _env = IsolatedEnv::new("named-audit");

    let environment = if cfg!(windows) { "cmd" } else { "gitbash" };
    let mut request = local_request("echo named");
    request.host = format!("{LOCAL_TARGET_ID}:{environment}");
    exec_local_core(request).await.expect("named exec");

    let entries = audit_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].host, format!("{LOCAL_TARGET_ID}:{environment}"));
    assert_eq!(entries[0].target(), ExecTarget::Local);
}

#[tokio::test]
#[serial_test::serial]
async fn an_unknown_environment_is_refused_and_audited() {
    let _env = IsolatedEnv::new("named-unknown");

    let mut request = local_request("echo nope");
    request.host = format!("{LOCAL_TARGET_ID}:nope");

    let error = exec_local_core(request)
        .await
        .expect_err("an unknown environment must be refused");

    let message = error.to_string();
    assert!(message.contains("nope"), "message was {message}");
    assert!(
        message.contains("gitbash"),
        "the refusal should say what is available: {message}"
    );
    assert_eq!(
        audit_entries().len(),
        1,
        "an unbuildable environment is a rejected exec and must be recorded"
    );
}

/// The headline case for named environments: MSVC is not on a normal `PATH`, so
/// a compile only works if the developer shell is actually established.
///
/// The control run under `cmd` is what makes this a test rather than a
/// coincidence — if `cl.exe` were already on the `PATH`, both runs would pass and
/// prove nothing.
#[cfg(windows)]
#[tokio::test]
#[serial_test::serial]
async fn the_visual_studio_environment_puts_msvc_on_path() {
    let _env = IsolatedEnv::new("vs");

    let mut control_request = local_request("where.exe cl");
    control_request.host = format!("{LOCAL_TARGET_ID}:cmd");
    let control = exec_local_core(control_request).await.expect("cmd exec");
    if control.exit_code == Some(0) {
        eprintln!("skipping: cl.exe is already on the PATH, so the difference proves nothing");
        return;
    }

    let mut request = local_request("where.exe cl");
    request.host = format!("{LOCAL_TARGET_ID}:vs");
    request.timeout_secs = Some(120);
    let result = match exec_local_core(request).await {
        Ok(result) => result,
        Err(error) => {
            eprintln!("skipping: no Visual Studio developer shell available ({error})");
            return;
        }
    };

    assert_eq!(result.exit_code, Some(0), "stderr: {}", result.stderr);
    assert!(
        result.stdout.to_lowercase().contains("cl.exe"),
        "stdout was {:?}",
        result.stdout
    );
}

#[cfg(windows)]
#[tokio::test]
#[serial_test::serial]
async fn the_powershell_environment_runs_powershell() {
    let _env = IsolatedEnv::new("pwsh");

    let mut request = local_request("$PSVersionTable.PSVersion.Major");
    request.host = format!("{LOCAL_TARGET_ID}:pwsh");
    let result = match exec_local_core(request).await {
        Ok(result) => result,
        Err(error) => {
            eprintln!("skipping: no PowerShell on PATH ({error})");
            return;
        }
    };

    assert_eq!(result.exit_code, Some(0), "stderr: {}", result.stderr);
    let version = result.stdout.trim();
    assert!(
        !version.is_empty() && version.chars().all(|ch| ch.is_ascii_digit()),
        "expected a major version number, got {version:?}"
    );
}

// ── Container capture exec ──────────────────────────────────────────────────
//
// Addressed as `local:<runtime>:<container-id>`; the capture exec reuses the
// whole local envelope, so what these tests pin down is the refusal paths (in
// the plain suite, no runtime needed) and, behind an explicit gate, one real
// capture run inside a real container.

/// The first installed runtime, for tests that need *a* runtime but not a
/// specific one. `None` on a machine with none, which is what the gated tests
/// key their skip on.
fn first_runtime() -> Option<ContainerRuntime> {
    available_runtimes()
        .first()
        .map(|resolved| resolved.runtime)
}

/// A container exec is a local exec: a refused one must land in the audit log
/// as a rejection under the exact host id that was asked for, container form
/// included.
#[tokio::test]
#[serial_test::serial]
async fn a_container_host_with_an_unknown_runtime_is_refused_and_audited() {
    let _env = IsolatedEnv::new("container-runtime");

    let mut request = local_request("echo nope");
    request.host = format!("{LOCAL_TARGET_ID}:nosuchruntime:abc123");

    let error = exec_local_core(request)
        .await
        .expect_err("an unknown runtime must be refused");

    let message = error.to_string();
    assert!(message.contains("nosuchruntime"), "message was {message}");
    assert!(
        message.contains("container-id"),
        "the refusal should mention the container form: {message}"
    );
    let entries = audit_entries();
    assert_eq!(entries.len(), 1, "a refusal is audited like any rejection");
    assert_eq!(
        entries[0].host,
        format!("{LOCAL_TARGET_ID}:nosuchruntime:abc123")
    );
    assert_eq!(entries[0].target(), ExecTarget::Local);
}

/// The id check runs before any runtime lookup, so this refusal is
/// machine-independent and needs no container runtime at all.
#[tokio::test]
#[serial_test::serial]
async fn a_container_exec_with_a_malformed_container_id_is_refused_and_audited() {
    let _env = IsolatedEnv::new("container-id");

    let mut request = local_request("echo nope");
    request.host = format!("{LOCAL_TARGET_ID}:docker:a;b");

    let error = exec_local_core(request)
        .await
        .expect_err("a malformed container id must be refused");

    assert!(
        error.to_string().contains("invalid container id"),
        "error was {error}"
    );
    assert_eq!(audit_entries().len(), 1);
}

/// A container capture exec runs without `-i`, so a request that carries a
/// stdin payload is refused rather than silently dropped. Needs an installed
/// runtime only — the refusal happens before any spawn, so no container is
/// required.
#[tokio::test]
#[serial_test::serial]
async fn a_container_exec_refuses_a_stdin_payload() {
    let _env = IsolatedEnv::new("container-stdin");
    let Some(runtime) = first_runtime() else {
        eprintln!("skipping: no container runtime installed");
        return;
    };

    let mut request = local_request("cat");
    request.host = format!("{LOCAL_TARGET_ID}:{}:abc123", runtime.id());
    request.stdin = Some("never forwarded\n".to_string());
    let host = request.host.clone();

    let error = exec_local_core(request)
        .await
        .expect_err("stdin must not be dropped silently");

    let message = error.to_string();
    assert!(message.contains("stdin"), "message was {message}");
    assert!(
        message.contains(host.as_str()),
        "the refusal should name the target: {message}"
    );
}

/// One real capture exec inside one real container. Gated on
/// `AGENT2SSH_TEST_CONTAINER` naming a *running* container (the same
/// opt-in pattern as `AGENT2SSH_TEST_SSH_PORT` for the sshd fixture), and
/// optionally `AGENT2SSH_CONTAINER_RUNTIME` to pin which runtime owns it.
///
/// The discriminator is filesystem truth, not an echoed string: the command
/// writes a file *inside* the container and reads it back in the same exec, so
/// a pass proves the command ran in some container's filesystem, not merely
/// that some process printed the words.
#[tokio::test]
#[serial_test::serial]
async fn a_container_exec_runs_inside_that_container() {
    let _env = IsolatedEnv::new("container-real");
    let Some(container) = std::env::var("AGENT2SSH_TEST_CONTAINER")
        .ok()
        .filter(|name| !name.trim().is_empty())
    else {
        eprintln!("skipping: AGENT2SSH_TEST_CONTAINER does not name a running container");
        return;
    };
    let Some(runtime) = first_runtime() else {
        eprintln!("skipping: no container runtime installed");
        return;
    };
    if let Ok(pinned) = std::env::var(agent2ssh::container_runtime::RUNTIME_ENV) {
        assert_eq!(
            ContainerRuntime::from_id(&pinned),
            Some(runtime),
            "the installed runtime must be the pinned one"
        );
    }

    let command = "echo a2s-container-marker > /tmp/a2s-marker.txt && cat /tmp/a2s-marker.txt";
    let mut request = local_request(command);
    request.host = format!("{LOCAL_TARGET_ID}:{}:{container}", runtime.id());
    request.timeout_secs = Some(30);
    let host = request.host.clone();

    let result = exec_local_core(request).await.expect("container exec");

    assert_eq!(result.host, host);
    assert_eq!(result.exit_code, Some(0), "stderr: {}", result.stderr);
    assert!(
        result.stdout.contains("a2s-container-marker"),
        "the command did not run inside the container: {:?}",
        result.stdout
    );

    // The capture contract is the shared one: same audit record, same target
    // classification, under the exact host id that was requested.
    let entries = audit_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].host, host);
    assert_eq!(entries[0].target(), ExecTarget::Local);
    assert_eq!(entries[0].exit_code, Some(0));
}
