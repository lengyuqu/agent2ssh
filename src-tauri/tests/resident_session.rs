//! Resident environment sessions, exercised against real shells.
//!
//! Like `tests/local_exec.rs`, the "server" is this machine: each test opens a
//! real shell process, sends commands through the sentinel protocol and reads
//! the captured output back. No daemon, no SSH.
//!
//! Every test mutates the process-global session cap
//! (`AGENT2SSH_MAX_RESIDENT_SESSIONS`), so they are `#[serial]` per the
//! project's rule for anything that touches process-level environment.

use agent2ssh::resident_session::{
    resident_close_core, resident_exec_core, resident_list_core, resident_open_core,
    ResidentSessionInfo,
};
use uuid::Uuid;

// Keeps the audit log and config of these tests off the real `~/.agent2ssh`.
mod common;
fn unlock_session_cap() {
    // 0 = unlimited: keep parallel tests from colliding on the default cap of
    // 8. Read per call, like every other AGENT2SSH_* knob.
    std::env::set_var(agent2ssh::resident_session::MAX_SESSIONS_ENV, "0");
}

/// The environment each test lives in. Windows prefers the explicitly named
/// PowerShell; elsewhere the default shell (a POSIX shell reading stdin)
/// exercises the same sentinel protocol through the `PosixDashC` framing.
fn environment() -> &'static str {
    if cfg!(windows) {
        "local:pwsh"
    } else {
        "local"
    }
}

async fn open_session() -> ResidentSessionInfo {
    resident_open_core(environment())
        .await
        .expect("resident session should open in a shell this machine has")
}

async fn exec(session: &ResidentSessionInfo, command: &str) -> agent2ssh::types::ExecResult {
    resident_exec_core(
        session.id,
        command.to_string(),
        None,
        agent2ssh::types::RiskLevel::Low,
        "test",
        None,
        None,
        None,
    )
    .await
    .expect("resident exec should succeed")
}

#[tokio::test]
#[serial_test::serial]
async fn shell_state_persists_across_commands() {
    unlock_session_cap();
    let session = open_session().await;

    if cfg!(windows) {
        exec(&session, "Set-Location $env:TEMP").await;
        let result = exec(&session, "Get-Location").await;
        assert!(
            result.stdout.to_lowercase().contains("temp"),
            "cwd must persist across commands, got: {}",
            result.stdout
        );
    } else {
        exec(&session, "cd /tmp").await;
        let result = exec(&session, "pwd").await;
        assert!(
            result.stdout.contains("tmp"),
            "cwd must persist across commands, got: {}",
            result.stdout
        );
    }

    resident_close_core(session.id).await.expect("close");
}

// `exit 3` inside the shell is the sentinel protocol's honest failure: the
// shell dies before the sentinel line runs, the session is declared dead, and
// the caller learns the shell's own exit code.
#[tokio::test]
#[serial_test::serial]
async fn exiting_the_shell_kills_the_session_and_reports_its_code() {
    unlock_session_cap();
    let session = open_session().await;

    let error = resident_exec_core(
        session.id,
        "exit 3".to_string(),
        None,
        agent2ssh::types::RiskLevel::Low,
        "test",
        None,
        None,
        None,
    )
    .await
    .expect_err("a command that exits the shell must fail the exec");
    let message = error.to_string();
    assert!(
        message.contains("3"),
        "the shell's exit code must reach the caller, got: {message}"
    );
    assert!(
        message.contains("dead"),
        "the caller must learn the session is dead, got: {message}"
    );

    // And the session is really gone: a second command cannot resurrect it.
    let error = resident_exec_core(
        session.id,
        "echo anything".to_string(),
        None,
        agent2ssh::types::RiskLevel::Low,
        "test",
        None,
        None,
        None,
    )
    .await
    .expect_err("the session is dead");
    assert!(error.to_string().contains("dead"));
}

#[tokio::test]
#[serial_test::serial]
async fn a_timed_out_command_kills_the_session() {
    unlock_session_cap();
    let session = open_session().await;

    let command = if cfg!(windows) {
        "Start-Sleep -Seconds 30"
    } else {
        "sleep 30"
    };
    let error = resident_exec_core(
        session.id,
        command.to_string(),
        Some(1),
        agent2ssh::types::RiskLevel::Low,
        "test",
        None,
        None,
        None,
    )
    .await
    .expect_err("a command past its deadline must fail");
    let message = error.to_string();
    assert!(
        message.contains("timed out"),
        "the timeout must be named, got: {message}"
    );
    assert!(
        message.contains("killed"),
        "the caller must learn the session was killed, got: {message}"
    );

    let error = resident_exec_core(
        session.id,
        "echo anything".to_string(),
        None,
        agent2ssh::types::RiskLevel::Low,
        "test",
        None,
        None,
        None,
    )
    .await
    .expect_err("the session is dead");
    assert!(error.to_string().contains("dead"));
}

#[cfg(windows)]
#[tokio::test]
#[serial_test::serial]
async fn native_command_exit_code_reaches_the_result() {
    unlock_session_cap();
    let session = open_session().await;

    // A native command sets $LASTEXITCODE; the sentinel reports it.
    let result = exec(&session, "cmd /c exit 3").await;
    assert_eq!(result.exit_code, Some(3));

    // A cmdlet-only command has no exit code. The frame resets
    // $LASTEXITCODE first, so the result must be "none" rather than a stale
    // value left behind by the previous command.
    let result = exec(&session, "Write-Output hello").await;
    assert_eq!(result.exit_code, None);
    assert!(result.stdout.contains("hello"));

    resident_close_core(session.id).await.expect("close");
}

#[cfg(windows)]
#[tokio::test]
#[serial_test::serial]
async fn quoted_output_survives_the_round_trip() {
    // cmd's MSVCRT quoting trap is a one-shot-path lesson; here the command
    // reaches a live shell through stdin, where quoting is the shell's own
    // business — but pin it anyway.
    unlock_session_cap();
    let session = open_session().await;
    let result = exec(&session, "Write-Output \"hi there\"").await;
    assert!(result.stdout.contains("hi there"), "got: {}", result.stdout);
    resident_close_core(session.id).await.expect("close");
}

// ── cmd.exe: the other Windows shell family ────────────────────────────────
//
// `local:cmd` is the only environment that exercises the `CmdSlashC` framing:
// the frame is joined with CRLF (cmd treats a lone LF as a line end too, but
// the frame stays with the convention), the sentinel reads `%ERRORLEVEL%`, and
// cmd echoes every line it reads back before running it. These tests pin that
// the echoed input is skipped rather than captured, and that `%VAR%`
// expansion — cmd's own syntax, not PowerShell's — reaches the caller.

#[cfg(windows)]
async fn open_cmd_session() -> ResidentSessionInfo {
    unlock_session_cap();
    resident_open_core("local:cmd")
        .await
        .expect("cmd.exe is on every Windows install")
}

#[cfg(windows)]
#[tokio::test]
#[serial_test::serial]
async fn cmd_shell_state_persists_across_commands() {
    let session = open_cmd_session().await;

    let result = exec(&session, "cd /d %TEMP%").await;
    assert_eq!(result.exit_code, Some(0), "cd: {}", result.stderr);

    let result = exec(&session, "echo %CD%").await;
    assert!(
        result.stdout.to_lowercase().contains("temp"),
        "cmd's cwd must persist across commands, got: {}",
        result.stdout
    );

    // `set` is cmd's export. The value surviving into the *next* command is
    // what proves one shell is really being reused rather than respawned.
    exec(&session, "set AGENT2SSH_RESIDENT_TEST=hot-path").await;
    let result = exec(&session, "echo %AGENT2SSH_RESIDENT_TEST%").await;
    assert!(
        result.stdout.contains("hot-path"),
        "cmd variables must persist across commands, got: {}",
        result.stdout
    );

    resident_close_core(session.id).await.expect("close");
}

#[cfg(windows)]
#[tokio::test]
#[serial_test::serial]
async fn cmd_reports_exit_codes_without_leaking_its_echoed_frame() {
    let session = open_cmd_session().await;

    let result = exec(&session, "echo marker").await;
    assert_eq!(result.exit_code, Some(0), "echo: {}", result.stderr);
    assert!(result.stdout.contains("marker"), "got: {}", result.stdout);
    assert!(
        !result.stdout.contains("__A2S_DONE"),
        "cmd echoes each line it reads, so the sentinel line must be skipped \
         rather than captured; got: {}",
        result.stdout
    );

    // A failing native command reports its code through %ERRORLEVEL% without
    // taking the shell down — the session stays warm for the next command.
    let result = exec(&session, "cmd /c exit 7").await;
    assert_eq!(result.exit_code, Some(7));

    let result = exec(&session, "echo alive").await;
    assert_eq!(result.exit_code, Some(0));
    assert!(result.stdout.contains("alive"), "got: {}", result.stdout);

    resident_close_core(session.id).await.expect("close");
}

#[tokio::test]
#[serial_test::serial]
async fn close_removes_the_session() {
    unlock_session_cap();
    let session = open_session().await;
    let host = resident_close_core(session.id).await.expect("close");
    assert_eq!(host, environment());

    let error = resident_exec_core(
        session.id,
        "echo anything".to_string(),
        None,
        agent2ssh::types::RiskLevel::Low,
        "test",
        None,
        None,
        None,
    )
    .await
    .expect_err("the session no longer exists");
    assert!(error.to_string().contains("unknown"));
}

#[tokio::test]
#[serial_test::serial]
async fn list_reports_open_sessions() {
    unlock_session_cap();
    let session = open_session().await;
    let listed = resident_list_core().await;
    let mine = listed
        .iter()
        .find(|entry| entry.id == session.id)
        .expect("the fresh session is listed");
    assert_eq!(mine.host, environment());
    assert!(!mine.dead);
    resident_close_core(session.id).await.expect("close");
}

#[tokio::test]
#[serial_test::serial]
async fn container_hosts_are_refused() {
    unlock_session_cap();
    let error = resident_open_core("local:docker:abc123")
        .await
        .expect_err("containers are one-shot exec targets");
    assert!(
        error.to_string().contains("container"),
        "the refusal should say why, got: {error}"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn unknown_environments_are_refused_with_the_selectable_list() {
    unlock_session_cap();
    let error = resident_open_core("local:pwsx")
        .await
        .expect_err("an unknown environment is a refusal, never a fallback");
    let message = error.to_string();
    assert!(
        message.contains("pwsh") && message.contains("vs"),
        "the refusal should list what is selectable, got: {message}"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn unknown_session_ids_are_refused() {
    unlock_session_cap();
    let error = resident_exec_core(
        Uuid::new_v4(),
        "echo anything".to_string(),
        None,
        agent2ssh::types::RiskLevel::Low,
        "test",
        None,
        None,
        None,
    )
    .await
    .expect_err("no such session");
    assert!(error.to_string().contains("unknown resident session"));
}
