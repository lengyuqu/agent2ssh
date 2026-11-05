use anyhow::{anyhow, Context, Result};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::store::config_dir;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone)]
pub struct DaemonStartResult {
    pub pid: u32,
    pub log_path: PathBuf,
}

pub fn daemon_pid_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("daemon.pid"))
}

pub fn daemon_log_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("daemon.log"))
}

pub fn read_daemon_pid() -> Result<Option<u32>> {
    let pid_path = daemon_pid_path()?;
    if !pid_path.exists() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(&pid_path)?;
    match raw.trim().parse::<u32>() {
        Ok(pid) => Ok(Some(pid)),
        Err(_) => {
            let _ = std::fs::remove_file(pid_path);
            Ok(None)
        }
    }
}

pub fn remove_daemon_pid_file() {
    if let Ok(pid_path) = daemon_pid_path() {
        let _ = std::fs::remove_file(pid_path);
    }
}

pub fn process_is_alive(pid: u32) -> bool {
    daemon_process_start_time(pid).is_some()
}

fn daemon_process_start_time(pid: u32) -> Option<u64> {
    let pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    // Linux truncates process names to 15 bytes, so the daemon's full name
    // must also be read from its command line for the identity check below.
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        true,
        sysinfo::ProcessRefreshKind::nothing().with_cmd(sysinfo::UpdateKind::OnlyIfNotSet),
    );
    system
        .process(pid)
        // A daemon that has exited but whose parent never waited on it stays in
        // the process table as a zombie. The desktop app spawns the daemon and
        // drops the handle, so that is exactly what a stopped daemon looks like
        // to it — and "still listed" would make every stop wait out its timeout.
        .filter(|process| {
            process.status() != sysinfo::ProcessStatus::Zombie && process_looks_like_daemon(process)
        })
        .map(|process| process.start_time())
}

// Only the executable name identifies a daemon. An unrelated shell or editor
// can carry "agent2ssh-daemon" in a later argument and must never match.
fn is_daemon_executable(value: &std::ffi::OsStr) -> bool {
    let name = std::path::Path::new(value)
        .file_name()
        .unwrap_or(value)
        .to_string_lossy();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    matches!(
        name,
        "agent2ssh-daemon"
            | "agent2ssh-daemon-aarch64-apple-darwin"
            | "agent2ssh-daemon-x86_64-unknown-linux-gnu"
            | "agent2ssh-daemon-x86_64-pc-windows-msvc"
    )
}

fn process_looks_like_daemon(process: &sysinfo::Process) -> bool {
    // The name remains useful on Windows when reading the command line is
    // denied; on Linux it may be truncated, so also inspect argv[0].
    is_daemon_executable(process.name())
        || process
            .cmd()
            .first()
            .is_some_and(|arg| is_daemon_executable(arg))
}

pub fn terminate_process(pid: u32) -> Result<()> {
    let sys_pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sys_pid]), true);
    let Some(process) = system.process(sys_pid) else {
        return Ok(());
    };

    let terminated = process
        .kill_with(sysinfo::Signal::Term)
        .unwrap_or_else(|| process.kill());
    if terminated {
        Ok(())
    } else {
        Err(anyhow!("failed to terminate daemon process {pid}"))
    }
}

pub fn daemon_health_ok() -> bool {
    // Resolve the configured local daemon address (honors AGENT2SSH_DAEMON_ADDR,
    // mapping a wildcard bind back to loopback) into a concrete socket address.
    let Some(addr) = crate::local_daemon_connect_addr()
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
    else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));

    if stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }

    let mut response = String::new();
    if stream.read_to_string(&mut response).is_err() {
        return false;
    }
    response.starts_with("HTTP/1.1 200") && response.contains("\"ok\":true")
}

pub fn wait_for_daemon_health(timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if daemon_health_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    daemon_health_ok()
}

/// Rotate daemon.log if it has grown past `max_size_bytes`, keeping two prior
/// generations (`daemon.log.1`, `daemon.log.2`). The daemon writes via a
/// redirected stdout/stderr handle that we cannot rotate while it runs, so this
/// runs at (re)start time — bounding unbounded growth across restarts. Unlike
/// app.log, which the core rotates inline on every write, daemon.log is only
/// pruned here.
fn rotate_daemon_log_if_needed(log_path: &Path, max_size_bytes: u64) {
    let Ok(metadata) = std::fs::metadata(log_path) else {
        return;
    };
    if metadata.len() <= max_size_bytes {
        return;
    }
    for i in (1..=2).rev() {
        let src = log_path.with_extension(format!("log.{i}"));
        let dst = log_path.with_extension(format!("log.{}", i + 1));
        if src.exists() {
            let _ = std::fs::rename(&src, &dst);
        }
    }
    let _ = std::fs::rename(log_path, log_path.with_extension("log.1"));
}

fn append_log_file() -> Result<File> {
    let log_path = daemon_log_path()?;
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    rotate_daemon_log_if_needed(&log_path, 5 * 1024 * 1024);
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .context("failed to open daemon.log")
}

pub fn start_daemon_background(daemon_bin: &Path) -> Result<DaemonStartResult> {
    if !daemon_bin.exists() {
        return Err(anyhow!("daemon binary not found: {}", daemon_bin.display()));
    }

    if let Some(pid) = read_daemon_pid()? {
        if process_is_alive(pid) && daemon_health_ok() {
            let _ = crate::diagnostics::append_diagnostic_log(
                "info",
                "daemon_control",
                "reusing healthy daemon process",
                Some(serde_json::json!({ "pid": pid })),
            );
            return Ok(DaemonStartResult {
                pid,
                log_path: daemon_log_path()?,
            });
        }
        let _ = crate::diagnostics::append_diagnostic_log(
            "warn",
            "daemon_control",
            "removing stale daemon pid before start",
            Some(serde_json::json!({ "pid": pid })),
        );
        remove_daemon_pid_file();
    }

    let log = append_log_file()?;
    let err_log = log
        .try_clone()
        .context("failed to clone daemon.log handle")?;
    let mut command = Command::new(daemon_bin);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err_log));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        unsafe extern "C" {
            fn setsid() -> i32;
        }

        // Start the daemon in a new session so it does not inherit the CLI or
        // desktop helper process group. Without this, closing the launching
        // process can deliver SIGHUP and leave a stale daemon.pid behind.
        unsafe {
            command.pre_exec(|| {
                if setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let child = command
        .spawn()
        .with_context(|| format!("failed to start daemon: {}", daemon_bin.display()))?;

    let pid = child.id();
    let _ = crate::diagnostics::append_diagnostic_log(
        "info",
        "daemon_control",
        "spawned daemon process",
        Some(serde_json::json!({
            "pid": pid,
            "binary": daemon_bin.display().to_string(),
            "log_path": daemon_log_path()?.display().to_string(),
        })),
    );
    if !wait_for_daemon_health(Duration::from_secs(3)) {
        if !process_is_alive(pid) {
            remove_daemon_pid_file();
            let _ = crate::diagnostics::append_diagnostic_log(
                "error",
                "daemon_control",
                "daemon exited before health endpoint became reachable",
                Some(serde_json::json!({
                    "pid": pid,
                    "log_path": daemon_log_path()?.display().to_string(),
                })),
            );
            return Err(anyhow!(
                "daemon process exited before health endpoint became reachable; see {}",
                daemon_log_path()?.display()
            ));
        }
        let _ = crate::diagnostics::append_diagnostic_log(
            "error",
            "daemon_control",
            "daemon health endpoint did not become reachable",
            Some(serde_json::json!({
                "pid": pid,
                "log_path": daemon_log_path()?.display().to_string(),
            })),
        );
        return Err(anyhow!(
            "daemon started (pid={pid}) but health endpoint is not reachable; see {}",
            daemon_log_path()?.display()
        ));
    }

    let _ = crate::diagnostics::append_diagnostic_log(
        "info",
        "daemon_control",
        "daemon health endpoint reachable",
        Some(serde_json::json!({ "pid": pid })),
    );

    Ok(DaemonStartResult {
        pid,
        log_path: daemon_log_path()?,
    })
}

// ── Ownership ───────────────────────────────────────────────────────────────
//
// The desktop app is not the only thing that starts the daemon: the CLI does, an
// MCP client's setup does, and a person may simply have left one running. All of
// them write the same `daemon.pid`, so the pid alone cannot say who started it —
// and "quit the app" must not take down a daemon that agents outside the app are
// in the middle of using. The desktop records the daemons *it* spawned here and
// stops only those when it quits; stopping any daemon on request (the explicit
// Stop button, `agent2ssh daemon stop`) is unaffected.

const DAEMON_OWNER_FILE: &str = "daemon.owner";

/// `pid` alone is not enough to name a process for long: the OS recycles them.
/// The start time makes a marker left behind by an earlier daemon fail to match
/// a later, unrelated process that happens to get the same pid.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct OwnerMarker {
    pid: u32,
    start_time: u64,
}

fn daemon_owner_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(DAEMON_OWNER_FILE))
}

fn process_start_time(pid: u32) -> Option<u64> {
    let pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).map(|process| process.start_time())
}

/// Record that the calling process started the daemon with this `pid`.
pub fn mark_daemon_owned(pid: u32) -> Result<()> {
    let start_time =
        process_start_time(pid).ok_or_else(|| anyhow!("process {pid} is not running"))?;
    let path = daemon_owner_path()?;
    std::fs::write(
        path,
        serde_json::to_string(&OwnerMarker { pid, start_time })?,
    )
    .context("failed to record daemon ownership")
}

/// Whether the daemon with this `pid` is the one the desktop started.
pub fn daemon_is_owned(pid: u32) -> bool {
    let Ok(path) = daemon_owner_path() else {
        return false;
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(marker) = serde_json::from_str::<OwnerMarker>(&raw) else {
        return false;
    };
    marker.pid == pid && process_start_time(pid) == Some(marker.start_time)
}

pub fn clear_daemon_owner() {
    if let Ok(path) = daemon_owner_path() {
        let _ = std::fs::remove_file(path);
    }
}

// ── Stopping ────────────────────────────────────────────────────────────────

/// How long a stopped daemon gets to drain before it is killed outright. The
/// daemon waits up to five seconds for WebSocket connections to finish, so this
/// has to be longer than that or a graceful stop would be cut short by the very
/// timeout meant to catch a hung one.
const GRACEFUL_STOP_WAIT: Duration = Duration::from_secs(8);

fn wait_for_exit(pid: u32, expected_start: u64, within: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < within {
        if daemon_process_start_time(pid) != Some(expected_start) {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    daemon_process_start_time(pid) != Some(expected_start)
}

// Recheck identity immediately before signalling. This narrows the PID reuse
// window but cannot make a PID-based signal atomic with this inspection.
fn signal_daemon(pid: u32, expected_start: u64, force: bool) -> Result<()> {
    let sys_pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[sys_pid]),
        true,
        sysinfo::ProcessRefreshKind::nothing().with_cmd(sysinfo::UpdateKind::OnlyIfNotSet),
    );
    let Some(process) = system.process(sys_pid) else {
        return Ok(());
    };
    if process.start_time() != expected_start
        || process.status() == sysinfo::ProcessStatus::Zombie
        || !process_looks_like_daemon(process)
    {
        return Ok(());
    }
    let sent = if force {
        process.kill()
    } else {
        process
            .kill_with(sysinfo::Signal::Term)
            .unwrap_or_else(|| process.kill())
    };
    if sent {
        Ok(())
    } else {
        Err(anyhow!("failed to signal daemon process {pid}"))
    }
}

/// Ask the running daemon to shut itself down, over its own admin API.
///
/// Windows has no `SIGTERM`: `terminate_process` falls back to
/// `TerminateProcess` there, which skips the drain that closes WebSocket
/// connections cleanly and removes the pid file. Asking the daemon does what
/// the signal does elsewhere. `true` only when the daemon accepted the request.
pub fn request_graceful_shutdown() -> bool {
    let Some(token) = crate::daemon_activity::read_admin_token() else {
        return false;
    };
    crate::daemon_activity::local_daemon_request(
        "POST",
        "/shutdown",
        Some(&token),
        Duration::from_secs(2),
    )
    .is_ok_and(|response| response.status == 200)
}

/// Stop the daemon with this `pid` and wait until it is gone.
///
/// Gracefully first (`SIGTERM`, or the shutdown route on Windows), then by force
/// if it has not exited within [`GRACEFUL_STOP_WAIT`]. Returning only once the
/// process has exited is what makes a restart safe — the old daemon still holds
/// the listening port while it drains, so starting the new one straight away
/// would fail to bind.
pub fn stop_daemon_process(pid: u32) -> Result<()> {
    let Some(start_time) = daemon_process_start_time(pid) else {
        clear_daemon_owner();
        return Ok(());
    };
    let asked_nicely = cfg!(windows) && request_graceful_shutdown();
    if !asked_nicely {
        signal_daemon(pid, start_time, false)?;
    }
    if !wait_for_exit(pid, start_time, GRACEFUL_STOP_WAIT) {
        let _ = crate::diagnostics::append_diagnostic_log(
            "warn",
            "daemon_control",
            "daemon did not exit after a graceful stop; killing it",
            Some(serde_json::json!({ "pid": pid })),
        );
        signal_daemon(pid, start_time, true)?;
        if !wait_for_exit(pid, start_time, Duration::from_secs(2)) {
            return Err(anyhow!("daemon process {pid} would not exit"));
        }
    }
    clear_daemon_owner();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_executable_names_are_not_substring_matches() {
        use std::ffi::OsStr;
        assert!(is_daemon_executable(OsStr::new("/tmp/agent2ssh-daemon")));
        assert!(is_daemon_executable(OsStr::new("agent2ssh-daemon.exe")));
        assert!(is_daemon_executable(OsStr::new(
            "agent2ssh-daemon-x86_64-unknown-linux-gnu"
        )));
        assert!(!is_daemon_executable(OsStr::new("not-agent2ssh-daemon")));
        assert!(!is_daemon_executable(OsStr::new("agent2ssh-daemon-helper")));
    }

    #[cfg(unix)]
    #[test]
    fn a_changed_start_time_is_not_signalled_or_waited_on() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("agent2ssh-daemon");
        symlink("/bin/sleep", &executable).unwrap();
        let mut child = Command::new(executable).arg("30").spawn().unwrap();
        let pid = child.id();
        let start = daemon_process_start_time(pid).expect("identify spawned process");
        let stale_start = start.wrapping_add(1);
        let ignored = signal_daemon(pid, stale_start, true).is_ok();
        let considered_exited = wait_for_exit(pid, stale_start, Duration::from_millis(1));
        let still_running = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let _ = child.wait();
        assert!(ignored && considered_exited && still_running);
    }

    #[test]
    fn a_marker_names_the_process_that_wrote_it() {
        let _config = crate::store::TestConfigDir::new("daemon-owner");
        let me = std::process::id();

        assert!(!daemon_is_owned(me), "no marker yet");
        mark_daemon_owned(me).expect("mark");
        assert!(daemon_is_owned(me));
        // Another pid is not covered by this marker.
        assert!(!daemon_is_owned(me.wrapping_add(1)));

        clear_daemon_owner();
        assert!(!daemon_is_owned(me));
    }

    #[test]
    fn a_marker_does_not_survive_pid_reuse() {
        let _config = crate::store::TestConfigDir::new("daemon-owner-reuse");
        let me = std::process::id();
        // The same pid with a start time that is not this process's: what a
        // marker left by an earlier daemon looks like once the OS recycles its
        // pid for something else.
        let stale = OwnerMarker {
            pid: me,
            start_time: process_start_time(me).unwrap().wrapping_add(10_000),
        };
        std::fs::write(
            daemon_owner_path().unwrap(),
            serde_json::to_string(&stale).unwrap(),
        )
        .unwrap();
        assert!(!daemon_is_owned(me));
    }

    #[test]
    fn a_corrupt_marker_means_not_owned() {
        let _config = crate::store::TestConfigDir::new("daemon-owner-corrupt");
        std::fs::write(daemon_owner_path().unwrap(), "not json").unwrap();
        assert!(!daemon_is_owned(std::process::id()));
    }

    #[test]
    fn marking_a_process_that_is_not_running_fails() {
        let _config = crate::store::TestConfigDir::new("daemon-owner-missing");
        // Far above any pid the OS hands out.
        assert!(mark_daemon_owned(u32::MAX - 1).is_err());
    }

    #[test]
    fn stopping_a_process_that_is_not_running_is_a_no_op() {
        let _config = crate::store::TestConfigDir::new("daemon-stop-missing");
        stop_daemon_process(u32::MAX - 1).expect("nothing to stop");
    }
}
