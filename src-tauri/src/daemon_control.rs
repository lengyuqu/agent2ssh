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
    let raw = match std::fs::read_to_string(&pid_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("failed to read daemon PID file"),
    };
    let pid = raw.trim().parse::<u32>().ok().filter(|pid| *pid != 0);
    pid.map(Some).ok_or_else(|| {
        anyhow!(
            "invalid daemon PID file {}; file retained, inspect it before starting or stopping the daemon",
            pid_path.display()
        )
    })
}

const DAEMON_PID_LOCK: &str = ".daemon-pid.lock";

/// Serialize PID publication and conditional cleanup between current versions.
/// This lock is for file coordination, not proof of a process's identity.
pub fn write_daemon_pid(pid: u32) -> Result<()> {
    let _guard = crate::store::lock_config_file(DAEMON_PID_LOCK)?;
    if let Some(previous) = read_daemon_pid()? {
        if !pid_definitely_missing(previous) {
            return Err(anyhow!("daemon PID file names process {previous}, which is live or cannot be verified; file retained"));
        }
    }
    std::fs::write(daemon_pid_path()?, pid.to_string()).context("failed to write daemon PID file")
}

fn write_daemon_pid_if_absent(pid: u32) -> Result<()> {
    let _guard = crate::store::lock_config_file(DAEMON_PID_LOCK)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(daemon_pid_path()?)?;
    write!(file, "{pid}")?;
    Ok(())
}

/// Remove only the PID we observed, preserving a replacement or malformed file.
/// Older clients and daemons do not honor this advisory lock.
pub fn remove_daemon_pid_file_if(pid: u32) -> Result<bool> {
    let _guard = crate::store::lock_config_file(DAEMON_PID_LOCK)?;
    let path = daemon_pid_path()?;
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("failed to read daemon PID file for cleanup"),
    };
    if raw.trim().parse::<u32>().ok() != Some(pid) {
        return Ok(false);
    }
    std::fs::remove_file(path).context("failed to remove daemon PID file")?;
    Ok(true)
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
        .and_then(|process| (process.start_time() != 0).then_some(process.start_time()))
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

/// Identity of the process recorded in daemon.pid. An unrecognized live PID
/// is not proof that the daemon is gone and must never be signalled or erased.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonPidState {
    Healthy,
    RunningUnhealthy,
    Unverified,
    Stale,
}

pub fn daemon_health_pid() -> Option<u32> {
    health_identity().map(|identity| identity.pid)
}

/// What the unchanged `/health` route says about the process that answered it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HealthIdentity {
    pid: u32,
    /// `None` when the daemon predates `/health` naming its config directory —
    /// which is not the same as "some other directory" (see
    /// [`DaemonStopError::UnverifiedIdentity`]).
    config_dir: Option<String>,
}

fn health_identity() -> Option<HealthIdentity> {
    let response = crate::daemon_activity::local_daemon_request(
        "GET",
        "/health",
        None,
        Duration::from_millis(500),
    )
    .ok()?;
    if response.status != 200 {
        return None;
    }
    let body: serde_json::Value = serde_json::from_str(&response.body).ok()?;
    if !body.get("ok")?.as_bool()? {
        return None;
    }
    Some(HealthIdentity {
        pid: u32::try_from(body.get("pid")?.as_u64()?).ok()?,
        config_dir: body
            .get("config_dir")
            .and_then(|dir| dir.as_str())
            .map(str::to_string),
    })
}

/// The config directory this build's daemons serve, canonicalized the same way
/// a daemon reports it. `None` when the directory cannot be resolved, which
/// makes any comparison against it unknown rather than false.
fn expected_config_dir() -> Option<String> {
    let dir = config_dir().ok()?;
    Some(
        std::fs::canonicalize(&dir)
            .unwrap_or(dir)
            .display()
            .to_string(),
    )
}

// A health response alone can be forged. This observation is deliberately
// independent of the executable name so a renamed daemon remains visible.
// It is still a seconds-resolution snapshot, not proof of daemon identity.
fn observable_live_start_time(pid: u32) -> Option<u64> {
    let sys_pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sys_pid]), true);
    system.process(sys_pid).and_then(|process| {
        (process.status() != sysinfo::ProcessStatus::Zombie && process.start_time() != 0)
            .then_some(process.start_time())
    })
}

/// Verify that the claimed process is observable before and after the health
/// and token checks. Neither a process snapshot nor the endpoint proves identity.
pub fn daemon_matches_config(pid: u32) -> bool {
    let Some(start_time) = observable_live_start_time(pid) else {
        return false;
    };
    if health_identity().map(|identity| identity.pid) != Some(pid) {
        return false;
    }
    let Some(token) = crate::daemon_activity::read_admin_token() else {
        return false;
    };
    let authenticated = crate::daemon_activity::local_daemon_request(
        "GET",
        "/gate",
        Some(&token),
        Duration::from_millis(500),
    )
    .is_ok_and(|response| response.status == 200);
    authenticated
        && daemon_health_pid() == Some(pid)
        && observable_live_start_time(pid) == Some(start_time)
}

// A missing sysinfo entry can also mean the OS denied inspection. Only treat
// the PID as absent when the platform explicitly reports that it does not exist.
#[cfg(unix)]
fn pid_definitely_missing(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return true;
    };
    if pid <= 0 {
        return false;
    }
    let result = unsafe { libc::kill(pid, 0) };
    result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

#[cfg(windows)]
fn pid_definitely_missing(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_INVALID_PARAMETER};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle != 0 {
            CloseHandle(handle);
            false
        } else {
            GetLastError() == ERROR_INVALID_PARAMETER
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn pid_definitely_missing(_pid: u32) -> bool {
    false
}

pub fn daemon_pid_state(pid: u32) -> DaemonPidState {
    let health_pid = daemon_health_pid();
    if let Some(health_pid) = health_pid {
        return if health_pid == pid && daemon_matches_config(pid) {
            DaemonPidState::Healthy
        } else {
            DaemonPidState::Unverified
        };
    }
    if process_is_alive(pid) {
        return DaemonPidState::RunningUnhealthy;
    }
    let sys_pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sys_pid]), true);
    match system.process(sys_pid) {
        Some(process) if process.status() == sysinfo::ProcessStatus::Zombie => {
            DaemonPidState::Stale
        }
        Some(_) => DaemonPidState::Unverified,
        None if pid_definitely_missing(pid) => DaemonPidState::Stale,
        None => DaemonPidState::Unverified,
    }
}

fn wait_for_daemon_pid(pid: u32, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if daemon_matches_config(pid) {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    daemon_matches_config(pid)
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
        match daemon_pid_state(pid) {
            DaemonPidState::Healthy => {
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
            DaemonPidState::Stale => {
                let _ = crate::diagnostics::append_diagnostic_log(
                    "warn",
                    "daemon_control",
                    "removing stale daemon pid before start",
                    Some(serde_json::json!({ "pid": pid })),
                );
                if !remove_daemon_pid_file_if(pid)? {
                    return Err(anyhow!(
                        "daemon PID file changed during cleanup; retry after inspecting it"
                    ));
                }
            }
            DaemonPidState::RunningUnhealthy | DaemonPidState::Unverified => {
                return Err(anyhow!(
                    "daemon PID {pid} is live or cannot be verified; PID file retained"
                ));
            }
        }
    }
    if let Some(pid) = daemon_health_pid() {
        return Err(anyhow!("daemon health endpoint is already served by pid {pid}; refusing to start another daemon"));
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
    let mut child = command
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
    if !wait_for_daemon_pid(pid, Duration::from_secs(3)) {
        // This is the exact child just spawned by this call, not a PID read
        // from disk. Do not abandon it when startup could not be verified.
        if child
            .try_wait()
            .with_context(|| {
                format!("failed to check daemon child (pid={pid}); check it manually")
            })?
            .is_none()
        {
            if let Err(kill_error) = child.kill() {
                if child
                    .try_wait()
                    .with_context(|| {
                        format!("failed to recheck daemon child (pid={pid}); check it manually")
                    })?
                    .is_none()
                {
                    // Preserve a recoverable PID if the unverified child never
                    // published one. Never overwrite another process's file.
                    let _ = write_daemon_pid_if_absent(pid);
                    return Err(anyhow!("daemon child (pid={pid}) did not become ready and could not be stopped: {kill_error}; PID file retained when possible"));
                }
            }
            child.wait().with_context(|| {
                format!("failed to wait for daemon child (pid={pid}); check it manually")
            })?;
        }
        // A malformed/partial file could belong to another concurrently
        // starting process; only remove a file naming this exact child.
        let _ = remove_daemon_pid_file_if(pid);
        let _ = crate::diagnostics::append_diagnostic_log(
            "error",
            "daemon_control",
            "daemon child did not provide an authenticated health response",
            Some(
                serde_json::json!({ "pid": pid, "log_path": daemon_log_path()?.display().to_string() }),
            ),
        );
        return Err(anyhow!("daemon child (pid={pid}) did not provide an authenticated health response and was stopped; see {}", daemon_log_path()?.display()));
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
    let _guard = crate::store::lock_config_file(DAEMON_PID_LOCK)?;
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

fn clear_daemon_owner_if(pid: u32, start_time: u64) -> Result<bool> {
    let _guard = crate::store::lock_config_file(DAEMON_PID_LOCK)?;
    let path = daemon_owner_path()?;
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("failed to read daemon owner marker"),
    };
    let Ok(marker) = serde_json::from_str::<OwnerMarker>(&raw) else {
        return Ok(false);
    };
    if marker.pid != pid || marker.start_time != start_time {
        return Ok(false);
    }
    std::fs::remove_file(path).context("failed to remove daemon owner marker")?;
    Ok(true)
}

// ── Stopping ────────────────────────────────────────────────────────────────

/// How long a stopped daemon gets to drain before stop returns an error. The
/// daemon gives each WebSocket two seconds to finish and then waits up to four
/// seconds for its drain registry to empty, so this has to exceed six seconds or
/// a graceful stop would be cut short by the very timeout meant to catch a hung
/// one. Those two numbers live in `agent2ssh-daemon.rs` as literals, so a change
/// there has to be reflected here rather than compiled against.
const GRACEFUL_STOP_WAIT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessObservation {
    Exited,
    Running,
    Unknown,
}

fn classify_process_observation(
    snapshot: Option<(u64, bool)>,
    definitely_missing: bool,
    expected_start: u64,
) -> ProcessObservation {
    match snapshot {
        Some((_, true)) => ProcessObservation::Exited,
        Some((0, false)) => ProcessObservation::Unknown,
        Some((start, false)) if start != expected_start => ProcessObservation::Exited,
        Some(_) => ProcessObservation::Running,
        None if definitely_missing => ProcessObservation::Exited,
        None => ProcessObservation::Unknown,
    }
}

// macOS can omit an unreaped zombie from sysinfo while kill(pid, 0) still
// succeeds. Observe our own exited children without consuming their wait status.
#[cfg(unix)]
fn child_has_exited(pid: u32) -> bool {
    let Ok(pid_signed) = i32::try_from(pid) else {
        return false;
    };
    if pid_signed <= 0 {
        return false;
    }
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    result == 0 && unsafe { info.si_pid() } == pid_signed
}

#[cfg(not(unix))]
fn child_has_exited(_pid: u32) -> bool {
    false
}

// A Windows process object remains openable while another process holds its
// handle, even after it exits, so an openable handle is not evidence of life.
// A signaled handle proves termination without consuming the Child's exit
// status; a failed or denied query is not evidence of exit either.
#[cfg(windows)]
fn process_handle_is_signaled(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle == 0 {
            return false;
        }
        let signaled = WaitForSingleObject(handle, 0) == WAIT_OBJECT_0;
        CloseHandle(handle);
        signaled
    }
}

#[cfg(not(windows))]
fn process_handle_is_signaled(_pid: u32) -> bool {
    false
}

fn observe_process(pid: u32, expected_start: u64) -> ProcessObservation {
    let sys_pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sys_pid]), true);
    let snapshot = system.process(sys_pid).map(|process| {
        (
            process.start_time(),
            process.status() == sysinfo::ProcessStatus::Zombie,
        )
    });
    let definitely_missing = snapshot.is_none()
        && (child_has_exited(pid)
            || process_handle_is_signaled(pid)
            || pid_definitely_missing(pid));
    classify_process_observation(snapshot, definitely_missing, expected_start)
}

fn wait_for_exit(pid: u32, expected_start: u64, within: Duration) -> ProcessObservation {
    let start = Instant::now();
    while start.elapsed() < within {
        let observation = observe_process(pid, expected_start);
        if observation == ProcessObservation::Exited {
            return observation;
        }
        thread::sleep(Duration::from_millis(50));
    }
    observe_process(pid, expected_start)
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

/// Why a stop did not happen. These are deliberately distinct: they are three
/// different situations for the person reading the message, and collapsing them
/// into one string is how "the wrong process was killed" becomes unarguable.
#[derive(Debug)]
pub enum DaemonStopError {
    /// The PID could not be identified, or the daemon that answered is not this
    /// config's daemon. Nothing was sent.
    UnverifiedIdentity(String),
    /// A daemon answered, but it is not the one that accepts this config's
    /// admin token.
    NotAuthenticated,
    /// A daemon answered and the request was refused because the daemon that
    /// answered is not the PID the stop was for — the listener moved between
    /// the health check and the shutdown request. Nothing was stopped.
    ExpectedPidRefused {
        expected: u32,
        actual: u32,
    },
    /// The shutdown request could not be sent (connection, protocol, or an
    /// unexpected answer). Nothing was stopped.
    NotAccepted(String),
    /// The shutdown was accepted but the daemon's exit could not be observed.
    UncertainExit(u32),
    /// The daemon would not exit, even after being signalled.
    StillRunning(u32),
    Other(anyhow::Error),
}

impl std::fmt::Display for DaemonStopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnverifiedIdentity(detail) => write!(f, "{detail}"),
            Self::NotAuthenticated => write!(
                f,
                "the daemon that answered did not accept this config's admin token; \
                 nothing was stopped"
            ),
            Self::ExpectedPidRefused { expected, actual } => write!(
                f,
                "the daemon listening now is pid {actual}, not the pid {expected} the stop \
                 was for; it refused and is still running. Retry the stop to address pid \
                 {actual}."
            ),
            Self::NotAccepted(detail) => write!(f, "{detail}"),
            Self::UncertainExit(pid) => write!(
                f,
                "daemon PID {pid} exit cannot be verified; PID file retained"
            ),
            Self::StillRunning(pid) => {
                write!(f, "daemon process {pid} would not exit; PID file retained")
            }
            Self::Other(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for DaemonStopError {}

impl From<anyhow::Error> for DaemonStopError {
    fn from(error: anyhow::Error) -> Self {
        Self::Other(error)
    }
}

/// The outcome of asking the daemon to shut itself down.
enum ShutdownRequest {
    Accepted,
    /// The daemon says it is a different PID than the one asked for.
    Refused {
        actual: u32,
    },
    /// Refused or unreadable for another reason, with something to print.
    NotAccepted(String),
    /// Accepted by a daemon that does not check `expect_pid`, so the answer
    /// says nothing about which process is stopping. Which is usually fine — the
    /// health check a moment earlier named this PID — but it is not the checked
    /// path, and the caller deserves to know which one it got.
    AcceptedUnchecked,
}

/// Post `/shutdown`, naming the process this stop is meant to end.
///
/// `expected_pid` is `None` only for [`request_graceful_shutdown`], which has no
/// PID to name: a real stop always names its target, so a daemon that moved
/// under the caller's feet can refuse instead of exiting.
fn request_shutdown(expected_pid: Option<u32>) -> ShutdownRequest {
    let Some(token) = crate::daemon_activity::read_admin_token() else {
        return ShutdownRequest::NotAccepted(
            "this config directory has no admin token; nothing was stopped".into(),
        );
    };
    // A request that names nothing sends no body *and* no `Content-Type`: an
    // empty body with a JSON content type is a parse error, not "no opinion".
    // `expect_pid: null` is the spelling that means "stop without checking", and
    // only a caller that writes it gets it.
    let body = expected_pid.map(|pid| serde_json::json!({ "expect_pid": pid }));
    let response = match &body {
        Some(body) => crate::daemon_activity::local_daemon_request_with_body(
            "POST",
            "/shutdown",
            Some(&token),
            body,
            Duration::from_secs(2),
        ),
        None => crate::daemon_activity::local_daemon_request(
            "POST",
            "/shutdown",
            Some(&token),
            Duration::from_secs(2),
        ),
    };
    match response {
        Ok(response) if response.status == 200 && expected_pid.is_none() => {
            ShutdownRequest::Accepted
        }
        Ok(response) if response.status == 409 => match expected_pid {
            // An older daemon rejects a body it cannot parse with 422, never
            // 409, so a 409 here is the check answering.
            Some(_) => ShutdownRequest::Refused {
                actual: reported_pid(&response.body).unwrap_or(0),
            },
            None => ShutdownRequest::NotAccepted(
                "the daemon refused an unspecified shutdown".to_string(),
            ),
        },
        // A daemon without the check ignores the body, so this is the old
        // response to a request that asked for more than it can promise.
        Ok(response) if response.status == 200 => ShutdownRequest::AcceptedUnchecked,
        // 401 and 403 are the same answer to this caller: the daemon did not
        // accept the token this config handed it, so it is not our daemon or the
        // token was replaced under us. Nothing was stopped either way.
        Ok(response) if response.status == 401 || response.status == 403 => {
            ShutdownRequest::NotAccepted(
                "the daemon did not accept this config's admin token for a shutdown; \
                 nothing was stopped"
                    .to_string(),
            )
        }
        Ok(response) => ShutdownRequest::NotAccepted(format!(
            "the daemon answered the shutdown request with {}: {}",
            response.status,
            response.body.trim()
        )),
        Err(error) => {
            ShutdownRequest::NotAccepted(format!("the shutdown request could not be sent: {error}"))
        }
    }
}

/// The PID a `409` names, so the error can point at the daemon that refused.
fn reported_pid(body: &str) -> Option<u32> {
    let body: serde_json::Value = serde_json::from_str(body).ok()?;
    let message = body.get("error")?.as_str()?;
    let rest = message.split("is pid ").nth(1)?;
    rest.split_whitespace()
        .next()?
        .trim_end_matches(',')
        .parse()
        .ok()
}

/// Ask the running daemon to shut itself down, over its own admin API.
///
/// Windows has no `SIGTERM`: signalling falls back to
/// `TerminateProcess` there, which skips the drain that closes WebSocket
/// connections cleanly and removes the pid file. Asking the daemon does what
/// the signal does elsewhere.
///
/// Without a PID to name this sends the old, unchecked request, which is all it
/// can do. [`stop_daemon_process`] — the path every real stop takes — names its
/// target instead.
pub fn request_graceful_shutdown() -> bool {
    matches!(request_shutdown(None), ShutdownRequest::Accepted)
}

/// Stop the daemon with this `pid` and wait until it is gone.
///
/// Authenticate the local daemon and request shutdown through its admin route,
/// then signal it by force if it has not exited within [`GRACEFUL_STOP_WAIT`].
/// Returning only once the process has exited is what makes a restart safe — the
/// old daemon still holds the listening port while it drains, so starting the
/// new one straight away would fail to bind.
pub fn stop_daemon_process(pid: u32) -> std::result::Result<(), DaemonStopError> {
    use DaemonStopError as Refused;
    let Some(start_time) = daemon_process_start_time(pid) else {
        if daemon_pid_state(pid) != DaemonPidState::Stale {
            return Err(Refused::UnverifiedIdentity(format!(
                "daemon PID {pid} cannot be safely identified; PID file retained"
            )));
        }
        // With no start time we cannot distinguish an old marker from a
        // replacement process; a stale marker is safer than removing it.
        return Ok(());
    };
    // A matching executable name is not proof that this is our daemon: an
    // unrelated process can reuse the PID or spoof argv[0]. Require the health
    // PID to match, the daemon to serve *this* config directory, and this
    // config's admin token to be accepted before asking for anything.
    let health = health_identity();
    if health.as_ref().map(|identity| identity.pid) == Some(pid) {
        match (
            health.and_then(|identity| identity.config_dir),
            expected_config_dir(),
        ) {
            (Some(answered), Some(expected)) if answered != expected => {
                return Err(Refused::UnverifiedIdentity(format!(
                    "daemon PID {pid} serves config directory {answered}, not {expected}; \
                     PID file retained"
                )))
            }
            // A daemon that does not say which config directory it serves cannot
            // be told apart from one that serves another. Refuse by name rather
            // than guess — and say what to do, since it is not a dead end.
            (None, _) => {
                return Err(Refused::UnverifiedIdentity(format!(
                    "daemon PID {pid} does not report its config directory, so this stop \
                     cannot verify it is the daemon this config started; PID file retained. \
                     Upgrade the daemon (agent2ssh daemon restart) and retry"
                )))
            }
            _ => {}
        }
    } else {
        // No daemon answered, or one did and it is not this PID. The listener
        // may have changed hands, so this says what was seen rather than
        // repeating the PID the caller already knows.
        return Err(Refused::UnverifiedIdentity(format!(
            "the daemon answering for PID {pid} could not be authenticated for shutdown \
             (health PID, config directory, or admin token did not match); PID file retained"
        )));
    }
    if crate::daemon_activity::read_admin_token().is_none() {
        return Err(Refused::NotAuthenticated);
    }
    // The daemon that answered `/health` above is addressed by name here, so a
    // listener that changed hands in between refuses instead of exiting.
    match request_shutdown(Some(pid)) {
        ShutdownRequest::Accepted => {}
        // A daemon too old to know `expect_pid` stops anyway. It is still the
        // daemon that health-checked a moment ago (its config directory matched),
        // so this is the previous behaviour, not a hole it opened — but `false
        // compatibility` would be claiming the check happened when it did not.
        ShutdownRequest::AcceptedUnchecked => {
            let _ = crate::diagnostics::append_diagnostic_log(
                "warn",
                "daemon_control",
                "the daemon accepted the shutdown without checking expect_pid (daemon predates it)",
                Some(serde_json::json!({ "pid": pid })),
            );
        }
        ShutdownRequest::Refused { actual } => {
            return Err(Refused::ExpectedPidRefused {
                expected: pid,
                actual,
            })
        }
        ShutdownRequest::NotAccepted(detail) => return Err(Refused::NotAccepted(detail)),
    }
    match wait_for_exit(pid, start_time, GRACEFUL_STOP_WAIT) {
        ProcessObservation::Exited => {}
        ProcessObservation::Unknown => {
            return Err(Refused::UncertainExit(pid));
        }
        ProcessObservation::Running => {
            let _ = crate::diagnostics::append_diagnostic_log(
                "warn",
                "daemon_control",
                "daemon did not exit after a guarded shutdown; leaving it running",
                Some(serde_json::json!({ "pid": pid })),
            );
            signal_daemon(pid, start_time, true).map_err(DaemonStopError::Other)?;
            match wait_for_exit(pid, start_time, Duration::from_secs(2)) {
                ProcessObservation::Exited => {}
                ProcessObservation::Running => return Err(Refused::StillRunning(pid)),
                ProcessObservation::Unknown => return Err(Refused::UncertainExit(pid)),
            }
        }
    }
    clear_daemon_owner_if(pid, start_time)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_publication_refuses_live_or_invalid_records_and_recovers_absent_processes() {
        let _config = crate::store::TestConfigDir::new("daemon-pid-publication");
        let path = daemon_pid_path().unwrap();
        let me = std::process::id();
        std::fs::write(&path, me.to_string()).unwrap();
        assert!(write_daemon_pid(me)
            .unwrap_err()
            .to_string()
            .contains("file retained"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), me.to_string());
        std::fs::write(&path, "broken").unwrap();
        assert!(write_daemon_pid(me).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken");
        std::fs::write(&path, (u32::MAX - 1).to_string()).unwrap();
        write_daemon_pid(me).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), me.to_string());
    }

    #[test]
    fn pid_cleanup_preserves_replacement_and_invalid_files() {
        let _config = crate::store::TestConfigDir::new("daemon-pid-cleanup");
        let path = daemon_pid_path().unwrap();
        std::fs::write(&path, "123").unwrap();
        assert!(write_daemon_pid_if_absent(456).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "123");
        assert!(!remove_daemon_pid_file_if(456).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "123");
        std::fs::write(&path, "partial").unwrap();
        assert!(!remove_daemon_pid_file_if(123).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "partial");
        std::fs::write(&path, "123").unwrap();
        assert!(remove_daemon_pid_file_if(123).unwrap());
        assert!(!path.exists());
        assert!(!remove_daemon_pid_file_if(123).unwrap());
        write_daemon_pid_if_absent(456).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "456");
    }

    #[test]
    fn invalid_or_partial_pid_files_are_retained_and_block_start() {
        let _config = crate::store::TestConfigDir::new("daemon-invalid-pid");
        let path = daemon_pid_path().unwrap();
        assert_eq!(read_daemon_pid().unwrap(), None);
        for raw in ["", "not-a-pid", "0", "4294967296"] {
            std::fs::write(&path, raw).unwrap();
            let error = read_daemon_pid().unwrap_err().to_string();
            assert!(error.contains("file retained"), "{error}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
            let error = start_daemon_background(&std::env::current_exe().unwrap())
                .unwrap_err()
                .to_string();
            assert!(error.contains("file retained"), "{error}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
        }
        std::fs::write(&path, "123\n").unwrap();
        assert_eq!(read_daemon_pid().unwrap(), Some(123));
    }

    #[test]
    fn only_definitely_absent_pids_are_treated_as_missing() {
        assert!(!pid_definitely_missing(std::process::id()));
        assert!(pid_definitely_missing(u32::MAX - 1));
    }

    /// The refusal names the daemon that answered, so the caller can retry
    /// against it instead of flailing. The message is ours, so this parses it
    /// rather than guessing from a status code alone.
    #[test]
    fn a_refusal_is_read_back_as_the_pid_that_answered() {
        let body = serde_json::json!({
            "error": "this daemon is pid 4242, not the pid 111 the request named; \
                      refusing to shut down"
        })
        .to_string();
        assert_eq!(reported_pid(&body), Some(4242));
        assert_eq!(reported_pid("{\"error\":\"something else\"}"), None);
        assert_eq!(reported_pid("not json"), None);
    }

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

    #[test]
    fn exit_observation_distinguishes_unknown_from_absent_or_reused_pid() {
        assert_eq!(
            classify_process_observation(None, false, 12),
            ProcessObservation::Unknown
        );
        assert_eq!(
            classify_process_observation(None, true, 12),
            ProcessObservation::Exited
        );
        assert_eq!(
            classify_process_observation(Some((0, false)), false, 12),
            ProcessObservation::Unknown
        );
        assert_eq!(
            classify_process_observation(Some((12, false)), false, 12),
            ProcessObservation::Running
        );
        assert_eq!(
            classify_process_observation(Some((13, false)), false, 12),
            ProcessObservation::Exited
        );
        assert_eq!(
            classify_process_observation(Some((12, true)), false, 12),
            ProcessObservation::Exited
        );
    }

    #[cfg(unix)]
    #[test]
    fn exit_observation_handles_unreaped_child_without_consuming_status() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        let pid = child.id();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !child_has_exited(pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let observed = child_has_exited(pid);
        let result = observe_process(pid, 1);
        let reaped = child.wait().unwrap();
        assert!(observed, "child exit should be observable without reaping");
        assert_eq!(result, ProcessObservation::Exited);
        assert!(reaped.success());
    }

    #[cfg(windows)]
    #[test]
    fn exit_observation_handles_exited_child_with_retained_handle() {
        // The child exits with 259 on purpose: that value is indistinguishable
        // from "still active" to an exit-code query, so the handle state has to
        // decide whether it ended.
        let mut child = Command::new("cmd.exe")
            .args(["/C", "exit", "/B", "259"])
            .spawn()
            .unwrap();
        let pid = child.id();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !process_handle_is_signaled(pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let signaled = process_handle_is_signaled(pid);
        let observation = observe_process(pid, 1);
        let status = child.wait().unwrap();
        assert!(signaled, "exited child handle should be signaled");
        assert_eq!(observation, ProcessObservation::Exited);
        assert_eq!(status.code(), Some(259));
    }

    #[cfg(windows)]
    #[test]
    fn a_live_process_handle_is_not_signaled() {
        assert!(!process_handle_is_signaled(std::process::id()));
    }

    #[cfg(unix)]
    #[test]
    fn exit_wait_does_not_mistake_an_unrecognized_live_process_for_exit() {
        let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        let start = process_start_time(pid).expect("inspect child");
        let result = wait_for_exit(pid, start, Duration::ZERO);
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(result, ProcessObservation::Running);
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
        let start = process_start_time(me).unwrap();
        assert!(!clear_daemon_owner_if(me, start.wrapping_add(1)).unwrap());
        assert!(daemon_is_owned(me));

        assert!(clear_daemon_owner_if(me, start).unwrap());
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
