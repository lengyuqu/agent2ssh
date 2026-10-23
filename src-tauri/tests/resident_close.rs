//! Cancellation must remain available while a resident command owns the shell.
//! Uses real shell/child processes and an isolated configuration directory.

mod common;

use agent2ssh::{
    resident_session::{
        resident_close_core, resident_exec_core, resident_get_core, resident_list_core,
        resident_open_core, session_host,
    },
    types::{ExecResult, RiskLevel},
};
use std::{path::Path, time::Duration};
use tokio::time::{sleep, timeout, Instant};
use uuid::Uuid;

// Also close on assertion failures; otherwise a failed cancellation test could
// itself leave the live shell it was meant to check behind.
struct CloseOnDrop(Uuid);

impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        let id = self.0;
        let _ = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("cleanup runtime");
            let _ = runtime.block_on(resident_close_core(id));
        })
        .join();
    }
}

async fn exec(id: Uuid, command: String) -> anyhow::Result<ExecResult> {
    resident_exec_core(
        id,
        command,
        Some(45),
        RiskLevel::Low,
        "test",
        None,
        None,
        None,
    )
    .await
}

fn quote_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    let escaped = if cfg!(windows) {
        path.replace('\'', "''")
    } else {
        path.replace('\'', "'\\''")
    };
    format!("'{escaped}'")
}

fn write_marker(path: &Path) -> String {
    let path = quote_path(path);
    if cfg!(windows) {
        format!("[System.IO.File]::WriteAllText({path}, 'unexpected')")
    } else {
        format!("printf unexpected > {path}")
    }
}

fn child_running(pid: u32) -> bool {
    let pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    system
        .process(pid)
        .is_some_and(|process| process.status() != sysinfo::ProcessStatus::Zombie)
}

// Deliberately uses a single async worker: a blocking close/lookup on that
// worker cannot hide behind another Tokio worker keeping this test responsive.
#[tokio::test]
#[serial_test::serial]
async fn close_cancels_active_and_queued_commands_and_reclaims_the_child_tree() {
    let dir = tempfile::tempdir().expect("isolated process markers");
    let pid_file = dir.path().join("child.pid");
    let completed_file = dir.path().join("completed");
    let queued_file = dir.path().join("queued");
    let host = if cfg!(windows) { "local:pwsh" } else { "local" };
    let session = resident_open_core(host).await.expect("open resident shell");
    let _cleanup = CloseOnDrop(session.id);

    let command = if cfg!(windows) {
        format!(
            "$child = Start-Process -FilePath (Join-Path $env:WINDIR 'System32/ping.exe') \
             -ArgumentList @('-n','30','127.0.0.1') -PassThru -NoNewWindow; \
             [System.IO.File]::WriteAllText({}, [string]$child.Id); \
             Write-Output resident-cancel-running; $child.WaitForExit(); {}",
            quote_path(&pid_file),
            write_marker(&completed_file),
        )
    } else {
        format!(
            "sleep 30 & child=$!; printf '%s' \"$child\" > {}; \
             printf 'resident-cancel-running\\n'; wait \"$child\"; {}",
            quote_path(&pid_file),
            write_marker(&completed_file),
        )
    };
    let active = tokio::spawn(exec(session.id, command));
    let child_pid = timeout(Duration::from_secs(15), async {
        loop {
            if let Ok(pid) = std::fs::read_to_string(&pid_file) {
                if let Ok(pid) = pid.trim().parse::<u32>() {
                    break pid;
                }
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the child must actually start before cancellation");
    assert!(child_running(child_pid), "the test child must be running");

    // The scope check used by daemon close must be able to resolve the host
    // while exec is active; reads/listing must also remain available.
    let observation_started = Instant::now();
    let observation = timeout(Duration::from_secs(2), async {
        let host = session_host(session.id).await;
        let listed = resident_list_core().await;
        let detail = resident_get_core(session.id).await;
        (host, listed, detail)
    })
    .await;
    let observation_elapsed = observation_started.elapsed();

    let queued = tokio::spawn(exec(session.id, write_marker(&queued_file)));
    // Let the queued call acquire its session reference and wait for the
    // command lock, so close must reject it too after waking that waiter.
    sleep(Duration::from_millis(100)).await;
    let close_started = Instant::now();
    let closed = timeout(Duration::from_secs(8), resident_close_core(session.id))
        .await
        .expect("close must not wait for the 30-second command")
        .expect("close succeeds");
    let close_elapsed = close_started.elapsed();
    let active_result = timeout(Duration::from_secs(3), active)
        .await
        .expect("running exec must wake when closed")
        .expect("exec worker joined");
    let queued_result = timeout(Duration::from_secs(3), queued)
        .await
        .expect("queued exec must wake when closed")
        .expect("queued worker joined");

    // Evaluate observations after cleanup, even when testing the old locking
    // behavior, so a failed assertion does not leak its real child process.
    let (observed_host, listed, detail) = observation.expect("observation must not wait for exec");
    assert!(observation_elapsed < Duration::from_secs(2));
    assert_eq!(observed_host.as_deref(), Some(host));
    assert!(listed
        .iter()
        .any(|entry| entry.id == session.id && !entry.dead));
    assert!(!detail.expect("read the active session").dead);
    assert_eq!(closed, host);
    assert!(close_elapsed < Duration::from_secs(8));
    for result in [active_result, queued_result] {
        let error =
            result.expect_err("close must cancel execution, not return a successful result");
        assert!(
            error.to_string().contains("closed") && error.to_string().contains("cancelled"),
            "expected explicit cancellation, got {error}"
        );
    }
    assert!(
        !completed_file.exists(),
        "the running command completed after close"
    );
    assert!(!queued_file.exists(), "the queued command ran after close");
    assert_eq!(session_host(session.id).await, None);
    timeout(Duration::from_secs(3), async {
        while child_running(child_pid) {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("closing the shell must also reap its descendant");
}
