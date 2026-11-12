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

/// The config directory as an absolute path.
///
/// `AGENT2SSH_CONFIG_DIR` is the one setting every surface of this project reads
/// and none of them shares a working directory: the desktop sidecar is started
/// by the app, the daemon by the CLI or by that app, an MCP server by its client.
/// A relative value only means something against the directory of the process
/// that read it, so resolving one here keeps each of them on its own reading and
/// makes the paths this module reports absolute — a relative `daemon.pid` path
/// says nothing about which file was actually read.
///
/// It does *not* make `AGENT2SSH_CONFIG_DIR=./cfg` mean one file to two processes
/// started from different places: each resolves `./cfg` against its own working
/// directory, and those are two directories. Nothing can reconcile that without
/// changing what a relative path means, so the guide says to use an absolute one
/// (`docs/guides/configuration-guide.md`).
fn config_dir_absolute() -> Result<PathBuf> {
    let dir = config_dir()?;
    if dir.is_absolute() {
        return Ok(dir);
    }
    Ok(std::env::current_dir()
        .context("failed to resolve the working directory for a relative AGENT2SSH_CONFIG_DIR")?
        .join(dir))
}

pub fn daemon_pid_path() -> Result<PathBuf> {
    Ok(config_dir_absolute()?.join("daemon.pid"))
}

pub fn daemon_log_path() -> Result<PathBuf> {
    Ok(config_dir_absolute()?.join("daemon.log"))
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
        #[cfg(target_os = "macos")]
        anyhow::bail!("daemon PID file names {previous}; file retained because automatic replacement is disabled on macOS. Inspect it, then use 'agent2ssh daemon clear-stale --expect-pid {previous}' only if the process has exited");
        #[cfg(not(target_os = "macos"))]
        if !pid_definitely_missing(previous) {
            return Err(anyhow!("daemon PID file names process {previous}, which is live or cannot be verified; file retained"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        // The existence check above cannot constrain a legacy writer that
        // ignores the lock. Never truncate a file it created in the meantime,
        // and do not follow a dangling symlink to create a different file.
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(daemon_pid_path()?)
            .context("failed to create daemon PID file; existing path retained")?;
        write!(file, "{pid}").context("failed to write daemon PID file")
    }
    #[cfg(not(target_os = "macos"))]
    std::fs::write(daemon_pid_path()?, pid.to_string()).context("failed to write daemon PID file")
}

/// Explicitly remove an unchanged PID record only when the OS reports the PID
/// absent while the cooperating writers' lock is held. This is not proof of the
/// old process identity, and cannot protect against older non-cooperating
/// writers. Available on every platform: the safety comes from the re-read
/// under the lock, not from the OS.
pub fn clear_stale_daemon_pid(expected_pid: u32) -> Result<()> {
    let path = daemon_pid_path()?;
    if !std::fs::symlink_metadata(&path)
        .context("cannot inspect daemon PID file; retained")?
        .file_type()
        .is_file()
    {
        anyhow::bail!("daemon PID path is not a regular file; retained for manual inspection");
    }
    let snapshot =
        std::fs::read_to_string(&path).context("cannot read daemon PID file; retained")?;
    clear_stale_daemon_pid_with_snapshot(expected_pid, &snapshot)
}

fn clear_stale_daemon_pid_with_snapshot(expected_pid: u32, snapshot: &str) -> Result<()> {
    let _guard = crate::store::lock_config_file(DAEMON_PID_LOCK)?;
    let ipc_dir = config_dir_absolute()?.join(".daemon-ipc");
    match std::fs::symlink_metadata(&ipc_dir) {
        Ok(metadata) if !metadata.file_type().is_dir() => {
            anyhow::bail!("invalid identity socket directory; retained for manual inspection")
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).context("cannot inspect identity socket directory"),
    }
    for name in ["daemon.instance.json", ".daemon-ipc/s"] {
        let path = config_dir_absolute()?.join(name);
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).context("cannot inspect identity recovery record"),
            Ok(_) => anyhow::bail!("identity records exist; use clear-stale with --expect-instance after inspecting them"),
        }
    }
    let path = daemon_pid_path()?;
    if !std::fs::symlink_metadata(&path)
        .context("cannot inspect daemon PID file; retained")?
        .file_type()
        .is_file()
    {
        anyhow::bail!("daemon PID path is not a regular file; retained for manual inspection");
    }
    let raw = std::fs::read_to_string(&path).context("cannot read daemon PID file; retained")?;
    if raw != snapshot {
        anyhow::bail!("daemon PID file changed during cleanup; retained");
    }
    let actual = raw
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|pid| *pid != 0)
        .ok_or_else(|| {
            anyhow!(
                "invalid daemon PID file {}; retained for manual inspection",
                path.display()
            )
        })?;
    if actual != expected_pid {
        anyhow::bail!(
            "daemon PID file changed (expected {expected_pid}, found {actual}); retained"
        );
    }
    if !pid_definitely_missing(actual) {
        anyhow::bail!("daemon PID {actual} exists or cannot be observed; PID file retained");
    }
    std::fs::remove_file(path).context("failed to remove stale daemon PID file")?;
    Ok(())
}

fn write_daemon_pid_if_absent(pid: u32) -> Result<()> {
    let _guard = crate::store::lock_config_file(DAEMON_PID_LOCK)?;
    #[cfg(target_os = "macos")]
    refuse_ipc_artifacts()?;
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
    #[cfg(target_os = "macos")]
    refuse_ipc_artifacts()?;
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

#[cfg(target_os = "macos")]
fn refuse_ipc_artifacts() -> Result<()> {
    let dir = config_dir_absolute()?;
    let ipc_dir = dir.join(".daemon-ipc");
    match std::fs::symlink_metadata(&ipc_dir) {
        Ok(m) if !m.file_type().is_dir() => {
            anyhow::bail!("invalid identity socket directory; retained")
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    for name in ["daemon.instance.json", ".daemon-ipc/s"] {
        match std::fs::symlink_metadata(dir.join(name)) {
            Ok(_) => anyhow::bail!("identity records exist; retained for explicit recovery"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
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
    // `u32::MAX - 1` — the pid a test uses for "certainly nothing" — does not
    // fit an `i32`, and `as i32` would make it -2, i.e. a *group* signal
    // target rather than a missing process. Anything that cannot be named as a
    // single positive pid is not a pid this process could be.
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
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };
    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        );
        if handle == 0 {
            // An invalid parameter is "no such process". Access denied and
            // every other error are not: the process may be running under
            // another account, and this function must not call that absent.
            return GetLastError() == ERROR_INVALID_PARAMETER;
        }
        // A process that has exited but whose handle someone still holds (the
        // launcher, the desktop app, a test's `Child`) still opens, and
        // `OpenProcess` succeeding would otherwise read as "still alive". Ask
        // the object itself: a signaled handle means the process ended.
        let ended = WaitForSingleObject(handle, 0) == WAIT_OBJECT_0;
        CloseHandle(handle);
        ended
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

/// What the OS said about a recorded PID — with "could not observe" kept
/// separate from "does not exist".
///
/// A failed lookup is not proof of absence: `OpenProcess` denies access for
/// processes this account may not query, and `kill(pid, 0)` returns `EPERM` for
/// a live process it may not signal. Only an explicit "no such process" may be
/// reported as [`PidObservation::Absent`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum PidObservation {
    /// The OS explicitly reports that no process has this PID.
    Absent,
    /// Observable and live. `start_time` is a seconds-resolution snapshot, not
    /// proof of identity.
    Live { start_time: u64 },
    /// Observable and already terminated.
    Zombie,
    /// The OS refused to describe the process. Access denied is not absence.
    Denied { reason: String },
    /// Not observable and not explicitly absent.
    Unknown { reason: String },
}

/// State of the `daemon.pid` file itself, independent of any process.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum DaemonPidRecord {
    Missing,
    Pid { pid: u32 },
    Malformed { raw: String, reason: String },
    Unreadable { reason: String },
    NotARegularFile,
}

/// What the local `/health` endpoint says, if anything answers it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum HealthObservation {
    Inactive,
    Reporting {
        pid: u32,
        same_pid: bool,
        /// `None` when the answering daemon predates reporting its config
        /// directory — unknown, not "different".
        config_dir_matches: Option<bool>,
    },
}

/// Whether the admin token was accepted. The token itself is never reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TokenObservation {
    Accepted,
    Rejected,
    Missing,
    NotChecked,
}

/// Whether the cooperating writers' lock was held while the report was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PidLockObservation {
    Free,
    Held,
    Unknown,
}

/// The one-line answer a person can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DaemonVerdict {
    /// A live process answered health, accepted the token and named the same
    /// config directory. Still not cryptographic proof of identity (#22).
    Healthy,
    /// A live process matches the record but the health endpoint does not answer.
    RunningUnhealthy,
    /// A process exists but identity cannot be established, or the endpoint
    /// disagrees with the record.
    Unverified,
    /// The record names a process the OS says is absent.
    Stale,
    /// The record itself cannot be used (malformed, unreadable, not a file).
    Damaged,
    /// No record and no endpoint.
    NotRunning,
}

impl DaemonVerdict {
    /// A short, stable label for CLI and desktop text. The `serde` form uses
    /// the same words, so a script and a screenshot agree.
    pub fn as_str(self) -> &'static str {
        match self {
            DaemonVerdict::Healthy => "healthy",
            DaemonVerdict::RunningUnhealthy => "running-unhealthy",
            DaemonVerdict::Unverified => "unverified",
            DaemonVerdict::Stale => "stale",
            DaemonVerdict::Damaged => "damaged",
            DaemonVerdict::NotRunning => "not-running",
        }
    }
}

/// A read-only diagnosis of the local daemon record. Nothing here mutates a
/// file, signals a process, or prints the admin token.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DaemonStatusReport {
    pub config_dir: String,
    pub pid_path: String,
    pub token_path: String,
    pub log_path: String,
    pub pid_lock: PidLockObservation,
    pub record: DaemonPidRecord,
    pub observation: Option<PidObservation>,
    pub health: HealthObservation,
    pub token: TokenObservation,
    pub verdict: DaemonVerdict,
    pub next_step: String,
}

/// A short, escaped preview of a damaged record. PID files contain digits, so
/// anything else is already unexpected; escaping keeps control characters out
/// of a terminal and a JSON consumer alike.
fn record_preview(raw: &str) -> String {
    let trimmed = raw.trim_end_matches(['\r', '\n']);
    let mut preview: String = trimmed
        .chars()
        .take(40)
        .flat_map(char::escape_debug)
        .collect();
    if trimmed.chars().count() > 40 {
        preview.push('…');
    }
    preview
}

pub fn read_daemon_pid_record(path: &Path) -> DaemonPidRecord {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return DaemonPidRecord::Missing
        }
        Err(error) => {
            return DaemonPidRecord::Unreadable {
                reason: error.to_string(),
            }
        }
    };
    if !metadata.file_type().is_file() {
        return DaemonPidRecord::NotARegularFile;
    }
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) => {
            return DaemonPidRecord::Unreadable {
                reason: error.to_string(),
            }
        }
    };
    match raw.trim().parse::<u32>().ok().filter(|pid| *pid != 0) {
        Some(pid) => DaemonPidRecord::Pid { pid },
        None => DaemonPidRecord::Malformed {
            raw: record_preview(&raw),
            reason: "not a positive integer".to_string(),
        },
    }
}

/// Ask the OS about a PID without turning "could not look" into "is gone".
///
/// Stop decisions use [`observe_process`] instead, because they answer a
/// different question — "is this still the process I verified" — and need an
/// expected start time. A diagnosis must answer "what did the OS say", so this
/// keeps access-denied, zombie and live-with-start-time apart instead of
/// collapsing them into exited/unknown.
pub fn observe_pid(pid: u32) -> PidObservation {
    // The record protocol accepts only positive PIDs. In particular, never
    // pass zero to kill(2), where it denotes a process group rather than a
    // single process. Represent that invalid record value as absent.
    if pid == 0 {
        return PidObservation::Absent;
    }
    let sys_pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sys_pid]), true);
    if let Some(process) = system.process(sys_pid) {
        if process.status() == sysinfo::ProcessStatus::Zombie {
            return PidObservation::Zombie;
        }
        if process.start_time() != 0 {
            return PidObservation::Live {
                start_time: process.start_time(),
            };
        }
    }
    if pid_definitely_missing(pid) {
        return PidObservation::Absent;
    }
    match pid_access_error(pid) {
        Some(reason) => PidObservation::Denied { reason },
        None => PidObservation::Unknown {
            reason: "process not observable through the process table".to_string(),
        },
    }
}

/// `Some(reason)` only when the OS explicitly refused access, which is a
/// different fact from a PID that does not exist.
#[cfg(windows)]
fn pid_access_error(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle != 0 {
            CloseHandle(handle);
            return None;
        }
        let code = GetLastError();
        (code == ERROR_ACCESS_DENIED).then(|| format!("OpenProcess denied (error {code})"))
    }
}

#[cfg(unix)]
fn pid_access_error(pid: u32) -> Option<String> {
    let Ok(pid) = i32::try_from(pid) else {
        return None;
    };
    if pid <= 0 {
        return None;
    }
    let result = unsafe { libc::kill(pid, 0) };
    if result == 0 {
        return None;
    }
    let errno = std::io::Error::last_os_error().raw_os_error();
    (errno == Some(libc::EPERM)).then(|| "kill(pid, 0) denied (EPERM)".to_string())
}

#[cfg(not(any(unix, windows)))]
fn pid_access_error(_pid: u32) -> Option<String> {
    None
}

fn observe_health(expected_pid: Option<u32>) -> HealthObservation {
    match health_identity() {
        None => HealthObservation::Inactive,
        Some(identity) => HealthObservation::Reporting {
            pid: identity.pid,
            same_pid: expected_pid == Some(identity.pid),
            config_dir_matches: identity.config_dir.map(|reported| {
                expected_config_dir()
                    .map(|expected| reported == expected)
                    .unwrap_or(false)
            }),
        },
    }
}

#[cfg(not(target_os = "macos"))]
fn observe_token() -> TokenObservation {
    let Some(token) = crate::daemon_activity::read_admin_token() else {
        return TokenObservation::Missing;
    };
    match crate::daemon_activity::local_daemon_request(
        "GET",
        "/gate",
        Some(&token),
        Duration::from_millis(500),
    ) {
        Ok(response) if response.status == 200 => TokenObservation::Accepted,
        Ok(_) => TokenObservation::Rejected,
        // No answer is not a rejection; the endpoint did not speak.
        Err(_) => TokenObservation::NotChecked,
    }
}

fn observe_pid_lock(dir: &Path) -> PidLockObservation {
    let path = dir.join(DAEMON_PID_LOCK);
    let file = match OpenOptions::new().read(true).write(true).open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return PidLockObservation::Free
        }
        Err(_) => return PidLockObservation::Unknown,
    };
    // `std::fs::File` grew advisory locks of its own; a shared attempt answers
    // "is a writer holding `.daemon-pid.lock` right now" without blocking and
    // without creating the file.
    match file.try_lock_shared() {
        Ok(()) => {
            let _ = file.unlock();
            PidLockObservation::Free
        }
        Err(std::fs::TryLockError::WouldBlock) => PidLockObservation::Held,
        Err(_) => PidLockObservation::Unknown,
    }
}

fn classify_daemon(
    record: &DaemonPidRecord,
    observation: Option<&PidObservation>,
    health: &HealthObservation,
    token: TokenObservation,
) -> DaemonVerdict {
    // An endpoint that names a different process is the strongest evidence
    // available here: the record and the answering daemon are not the same
    // instance, whatever the process table says about the recorded PID.
    if let HealthObservation::Reporting {
        same_pid: false, ..
    } = health
    {
        return DaemonVerdict::Unverified;
    }
    // The endpoint claims the recorded PID while the process table cannot
    // confirm it is live. That contradiction is not a stale record: something
    // answered, so erasing the record could erase the only pointer to it.
    if let (HealthObservation::Reporting { same_pid: true, .. }, Some(observation)) =
        (health, observation)
    {
        if !matches!(observation, PidObservation::Live { .. }) {
            return DaemonVerdict::Unverified;
        }
    }
    match record {
        DaemonPidRecord::Missing => match health {
            HealthObservation::Inactive => DaemonVerdict::NotRunning,
            HealthObservation::Reporting { .. } => DaemonVerdict::Unverified,
        },
        DaemonPidRecord::Malformed { .. }
        | DaemonPidRecord::Unreadable { .. }
        | DaemonPidRecord::NotARegularFile => DaemonVerdict::Damaged,
        DaemonPidRecord::Pid { .. } => match observation {
            Some(PidObservation::Absent) | Some(PidObservation::Zombie) => DaemonVerdict::Stale,
            Some(PidObservation::Denied { .. }) | Some(PidObservation::Unknown { .. }) | None => {
                DaemonVerdict::Unverified
            }
            Some(PidObservation::Live { .. }) => match health {
                HealthObservation::Inactive => DaemonVerdict::RunningUnhealthy,
                HealthObservation::Reporting {
                    same_pid: true,
                    config_dir_matches,
                    ..
                } => {
                    if *config_dir_matches == Some(false) {
                        DaemonVerdict::Unverified
                    } else if token == TokenObservation::Accepted {
                        DaemonVerdict::Healthy
                    } else {
                        DaemonVerdict::Unverified
                    }
                }
                HealthObservation::Reporting {
                    same_pid: false, ..
                } => DaemonVerdict::Unverified,
            },
        },
    }
}

fn daemon_next_step(
    record: &DaemonPidRecord,
    observation: Option<&PidObservation>,
    health: &HealthObservation,
    verdict: DaemonVerdict,
) -> String {
    match verdict {
        DaemonVerdict::Healthy => {
            "Nothing to do. Health and the admin token agree with the record, but process identity is still unbound (see #22).".to_string()
        }
        DaemonVerdict::RunningUnhealthy => "A process holds the record but /health does not answer. Inspect daemon.log and the listening port; do not signal the PID from this report.".to_string(),
        DaemonVerdict::Stale => {
            let pid = match record {
                DaemonPidRecord::Pid { pid } => *pid,
                _ => 0,
            };
            format!("Run 'agent2ssh daemon clear-stale --expect-pid {pid}' to remove the record only if it still matches and the PID stays absent. No process is signalled.")
        }
        DaemonVerdict::Damaged => "The PID file is not a usable record. Inspect it by hand; clear-stale refuses malformed, unreadable and non-file records.".to_string(),
        DaemonVerdict::NotRunning => "No daemon record and no endpoint. Start one with 'agent2ssh daemon start'.".to_string(),
        DaemonVerdict::Unverified => match (record, observation, health) {
            (DaemonPidRecord::Missing, _, HealthObservation::Reporting { pid, .. }) => format!(
                "An endpoint reports PID {pid} but no PID record exists. Do not create or delete a record from this observation alone."
            ),
            (_, Some(PidObservation::Denied { .. }), _) => "The OS denied process inspection. That is not evidence the process exited: re-run as the owning user or inspect the process out of band.".to_string(),
            (_, _, HealthObservation::Reporting { same_pid: false, pid, .. }) => format!(
                "The endpoint reports PID {pid}, which is not the recorded PID and/or not this config directory. Inspect both before stopping anything."
            ),
            _ => "The record and the endpoint do not prove the same instance. Inspect daemon.log and the process out of band; clear-stale only removes a record it can prove absent.".to_string(),
        },
    }
}

/// Read-only diagnosis of the local daemon record.
pub fn daemon_status_report() -> Result<DaemonStatusReport> {
    let dir = config_dir_absolute()?;
    let pid_path = dir.join("daemon.pid");
    let record = read_daemon_pid_record(&pid_path);
    let recorded_pid = match &record {
        DaemonPidRecord::Pid { pid } => Some(*pid),
        _ => None,
    };
    let observation = recorded_pid.map(observe_pid);
    let health = observe_health(recorded_pid);
    #[cfg(not(target_os = "macos"))]
    let token = match (recorded_pid, observation.as_ref()) {
        (Some(_), Some(PidObservation::Live { .. })) => observe_token(),
        _ => TokenObservation::NotChecked,
    };
    // A PID and HTTP health response cannot bind this endpoint to the daemon.
    // Do not disclose the admin token while diagnosing an unbound macOS peer.
    #[cfg(target_os = "macos")]
    let token = TokenObservation::NotChecked;
    let verdict = classify_daemon(&record, observation.as_ref(), &health, token);
    #[cfg(target_os = "macos")]
    let ipc_evidence = {
        let socket_dir = dir.join(".daemon-ipc");
        let unsafe_dir = match std::fs::symlink_metadata(&socket_dir) {
            Ok(metadata) => !metadata.file_type().is_dir(),
            Err(e) => e.kind() != std::io::ErrorKind::NotFound,
        };
        unsafe_dir
            || ["daemon.instance.json", ".daemon-ipc/s"]
                .iter()
                .any(|name| match std::fs::symlink_metadata(dir.join(name)) {
                    Ok(_) => true,
                    Err(e) => e.kind() != std::io::ErrorKind::NotFound,
                })
    };
    #[cfg(target_os = "macos")]
    let verdict = if verdict == DaemonVerdict::NotRunning {
        DaemonVerdict::Unverified
    } else {
        verdict
    };
    let next_step = daemon_next_step(&record, observation.as_ref(), &health, verdict);
    #[cfg(target_os = "macos")]
    let next_step = if ipc_evidence
        && matches!(verdict, DaemonVerdict::Stale | DaemonVerdict::Unverified)
    {
        let prefix = if matches!(record, DaemonPidRecord::Missing) {
            "There is no PID record. "
        } else {
            ""
        };
        format!("{prefix}IPC identity artifacts exist or cannot be inspected. Inspect them before recovery; a complete stale instance requires clear-stale --expect-pid <PID> --expect-instance <instance-id>. No process is signalled.")
    } else if matches!(record, DaemonPidRecord::Missing) && verdict == DaemonVerdict::Unverified {
        "No daemon PID record; daemon identity and state is unknown. Inspect the endpoint and processes before recovery.".to_string()
    } else {
        next_step
    };
    Ok(DaemonStatusReport {
        config_dir: dir.display().to_string(),
        pid_path: pid_path.display().to_string(),
        token_path: dir.join("daemon.token").display().to_string(),
        log_path: dir.join("daemon.log").display().to_string(),
        pid_lock: observe_pid_lock(&dir),
        record,
        observation,
        health,
        token,
        verdict,
        next_step,
    })
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

/// Rotate daemon.log if it has grown past `max_size_bytes`, keeping three prior
/// generations (`daemon.log.1`, `daemon.log.2`, `daemon.log.3` — the loop below
/// shifts `.2 → .3` and `.1 → .2` before moving the live file to `.1`). The
/// daemon writes via a redirected stdout/stderr handle that we cannot rotate
/// while it runs, so this runs at (re)start time — bounding unbounded growth
/// across restarts. Unlike app.log, which the core rotates inline on every
/// write, daemon.log is only pruned here.
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
                #[cfg(target_os = "macos")]
                return Err(anyhow!("daemon PID {pid} appears absent, but its identity is unbound; PID file retained. Inspect it and use 'agent2ssh daemon clear-stale --expect-pid {pid}' to remove only the stale record"));
                #[cfg(not(target_os = "macos"))]
                {
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
/// The start time separates a marker left behind by an earlier daemon from a
/// later process that gets the same pid, but only while the two start times
/// differ. It is whole-second resolution (see [`observable_live_start_time`]),
/// so a recycled pid that starts in the same second the marker recorded is
/// indistinguishable from the process the marker named.
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

#[cfg(any(not(target_os = "macos"), test))]
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
/// daemon gives each WebSocket two seconds to finish, then waits up to four
/// seconds for its drain registry to empty. The two waits run in sequence, so
/// the worst case is about six seconds and a shorter constant would cut a
/// graceful stop short. Those numbers are literals in `agent2ssh-daemon.rs` —
/// the per-connection grace at `:5014` (`/terminal`) and `:5442`
/// (`/exec/stream`), the registry wait at `:5951` — so a change there has to be
/// reflected here rather than compiled against; exit is observed on a 50 ms poll
/// (`wait_for_exit`), which is why the margin over six seconds is measured in
/// seconds and not milliseconds.
#[cfg(not(target_os = "macos"))]
const GRACEFUL_STOP_WAIT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(any(not(target_os = "macos"), test))]
enum ProcessObservation {
    Exited,
    Running,
    Unknown,
}

#[cfg(any(not(target_os = "macos"), test))]
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
#[cfg(any(not(target_os = "macos"), test))]
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
#[cfg(any(not(target_os = "macos"), test))]
fn child_has_exited(_pid: u32) -> bool {
    false
}

// A Windows process object remains openable while another process holds its
// handle, even after it exits, so an openable handle is not evidence of life.
// A signaled handle proves termination without consuming the Child's exit
// status; a failed or denied query is not evidence of exit either.
#[cfg(windows)]
#[cfg(any(not(target_os = "macos"), test))]
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
#[cfg(any(not(target_os = "macos"), test))]
fn process_handle_is_signaled(_pid: u32) -> bool {
    false
}

/// `GetProcessTimes().ftCreationTime` as Unix seconds.
///
/// Measured against `sysinfo::Process::start_time` on Windows 11 26300: equal
/// in every sample (24/24 across four runs), because sysinfo divides the same
/// tick count by 10^7. So a value read from a handle is directly comparable to
/// [`OwnerMarker::start_time`] without a conversion table — see
/// `docs/reports/CNB-ISSUE-24-WINDOWS-PRIMITIVES-2026-11-12.md`.
#[cfg(windows)]
fn creation_time_secs(handle: isize) -> Option<u64> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetProcessTimes;
    /// Seconds between the FILETIME epoch (1601-01-01) and the Unix epoch.
    const FILETIME_TO_UNIX_SECS: u64 = 11_644_473_600;
    unsafe {
        let mut creation: FILETIME = std::mem::zeroed();
        let mut exit: FILETIME = std::mem::zeroed();
        let mut kernel: FILETIME = std::mem::zeroed();
        let mut user: FILETIME = std::mem::zeroed();
        if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) == 0 {
            return None;
        }
        let ticks = ((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64;
        Some(ticks / 10_000_000 - FILETIME_TO_UNIX_SECS)
    }
}

/// The Windows process object a stop is allowed to touch.
///
/// `OpenProcess(pid)` only ever answers "the object holding this PID right
/// now", and the PID can change hands between that call and the one that
/// signals. Opening once and keeping the handle is what makes verification,
/// waiting and termination act on the same object: a signaled handle means
/// *that* process ended, not that whoever took the PID next is alive.
///
/// The handle is only trusted after its own creation time matches the record.
/// It is seconds, not the 100 ns `GetProcessTimes` returns, because the record
/// stores whole seconds — and two processes created within the same second are
/// indistinguishable at that precision (measured, 4/4 runs; report §3.3). So
/// this is strictly stronger than re-opening per poll, not omniscient: it
/// narrows the reuse window to the instant of `open` and removes it for the
/// 8 s that follow.
#[cfg(windows)]
struct BoundProcess {
    handle: isize,
    pid: u32,
    /// The creation time this binding was accepted for. Kept so termination can
    /// refuse a handle opened for a different process.
    start_time: u64,
}

#[cfg(windows)]
impl BoundProcess {
    /// Open the object behind `pid` and keep the handle.
    ///
    /// `None` means "not bound": the process is gone, the access was denied, or
    /// the creation time is not the recorded one. None of those is evidence of
    /// exit, so every caller falls back to the existing observation path rather
    /// than treating it as one. The name whitelist is not re-checked here
    /// because `expected_start` was read from a process that already passed it
    /// ([`daemon_process_start_time`]).
    ///
    /// Only the rights needed to *observe* are requested here. A caller that may
    /// not terminate the process can still bind and wait; [`terminate`] asks for
    /// the extra right when a forced stop is actually requested.
    fn open(pid: u32, expected_start: u64) -> Option<Self> {
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        };
        let handle = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        };
        if handle == 0 {
            return None;
        }
        match creation_time_secs(handle) {
            Some(start) if start == expected_start => Some(Self {
                handle,
                pid,
                start_time: expected_start,
            }),
            _ => {
                unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
                None
            }
        }
    }

    /// Has the object this handle names terminated?
    fn is_signaled(&self) -> bool {
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        unsafe { WaitForSingleObject(self.handle, 0) == WAIT_OBJECT_0 }
    }

    /// Terminate the process this binding verified.
    ///
    /// The binding only asked for `SYNCHRONIZE | QUERY_LIMITED_INFORMATION`, so
    /// a caller that may wait but not terminate (another account, a restricted
    /// token) can still bind and observe. Termination asks for `PROCESS_TERMINATE`
    /// only when it is actually needed, and re-checks the creation time on the
    /// newly opened handle before signalling — a reused PID is refused instead
    /// of killed.
    ///
    /// This is the one place that re-opens by PID; it is not the old fallback,
    /// because a failure here returns an error rather than trying another
    /// mechanism. Whether a partially privileged token can bind but not
    /// terminate is inferred from the documented access rights, not measured on
    /// such an account.
    fn terminate(&self) -> Result<()> {
        use windows_sys::Win32::System::Threading::{
            OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        };
        let handle = unsafe {
            OpenProcess(
                PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                self.pid,
            )
        };
        if handle == 0 {
            return Err(anyhow!(
                "could not open daemon PID {} for termination ({}); no signal was sent",
                self.pid,
                std::io::Error::last_os_error()
            ));
        }
        if creation_time_secs(handle) != Some(self.start_time) {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
            return Err(anyhow!(
                "daemon PID {} was reused before it could be terminated; no signal was sent",
                self.pid
            ));
        }
        let sent = unsafe { TerminateProcess(handle, 1) };
        unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
        if sent == 0 {
            return Err(anyhow!(
                "failed to terminate daemon process {}: {}",
                self.pid,
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for BoundProcess {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle) };
    }
}

/// Placeholder so the stop path reads the same on every platform.
///
/// [`BoundProcess::open`] always answers `None` here, so the handle path is
/// never taken and the Unix observers keep deciding.
#[cfg(all(not(windows), any(not(target_os = "macos"), test)))]
struct BoundProcess;

#[cfg(all(not(windows), any(not(target_os = "macos"), test)))]
impl BoundProcess {
    fn open(_pid: u32, _expected_start: u64) -> Option<Self> {
        None
    }
}

/// Wait for exit using the bound handle when there is one.
///
/// A bound handle cannot answer `Unknown`: it either signals (that object
/// ended) or it does not (that object is running). That is the whole point of
/// binding it — the three-way guess exists because a PID can be reinterpreted,
/// and a handle cannot.
#[cfg(any(not(target_os = "macos"), test))]
fn wait_for_bound_exit(
    bound: Option<&BoundProcess>,
    pid: u32,
    expected_start: u64,
    within: Duration,
) -> ProcessObservation {
    #[cfg(windows)]
    if let Some(bound) = bound {
        let start = Instant::now();
        while start.elapsed() < within {
            if bound.is_signaled() {
                return ProcessObservation::Exited;
            }
            thread::sleep(Duration::from_millis(50));
        }
        return if bound.is_signaled() {
            ProcessObservation::Exited
        } else {
            ProcessObservation::Running
        };
    }
    let _ = bound;
    wait_for_exit(pid, expected_start, within)
}

/// Force the daemon down.
///
/// On Windows this only acts through the handle opened before the shutdown
/// request (#24). Re-opening the process by PID here would hand the
/// termination to whatever holds that PID now — exactly the fallback the issue
/// forbids. `None` means the object was denied, already gone, or already
/// replaced; the caller gets an error and neither the process nor the PID file
/// is touched.
///
/// Elsewhere the PID-based recheck (`start_time` + name) is still the only
/// mechanism available, and it is not atomic with the signal.
#[cfg(not(target_os = "macos"))]
fn terminate_for_stop(bound: Option<&BoundProcess>, pid: u32, expected_start: u64) -> Result<()> {
    #[cfg(windows)]
    {
        let _ = expected_start;
        let bound = bound.ok_or_else(|| {
            anyhow!(
                "daemon PID {pid} did not exit after a guarded shutdown and its process object \
                 could not be bound (access denied, already exited, or the PID was reused); \
                 no signal was sent and the PID file is retained"
            )
        })?;
        bound.terminate()
    }
    #[cfg(not(windows))]
    {
        let _ = bound;
        signal_daemon(pid, expected_start, true)
    }
}

#[cfg(any(not(target_os = "macos"), test))]
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

#[cfg(any(not(target_os = "macos"), test))]
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
//
// Windows never calls this from the stop path any more: a forced stop must go
// through the handle bound before the shutdown request (#24). Keeping the
// function out of that build also keeps the forbidden fallback unreachable.
#[cfg(all(not(target_os = "macos"), not(windows)))]
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
#[cfg(not(target_os = "macos"))]
enum ShutdownRequest {
    Accepted,
    /// The daemon says it is a different PID than the one asked for.
    Refused {
        actual: u32,
    },
    /// Refused or unreadable for another reason, with something to print.
    NotAccepted(String),
    /// A `200` to a request that named a PID — the only arm a `200` can reach
    /// in that case, because the guarded arm above requires
    /// `expected_pid.is_none()`. It therefore also catches a current daemon
    /// that compared the PID and matched, so the answer says only "the request
    /// was accepted", never "the check did or did not happen". Telling those
    /// apart needs a receipt the daemon does not send yet (K14).
    AcceptedUnchecked,
}

/// Post `/shutdown`, naming the process this stop is meant to end.
///
/// `expected_pid` is `None` only for [`request_graceful_shutdown`], which has no
/// PID to name: a real stop always names its target, so a daemon that moved
/// under the caller's feet can refuse instead of exiting.
#[cfg(not(target_os = "macos"))]
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
#[cfg(any(not(target_os = "macos"), test))]
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
/// On non-macOS platforms this requests shutdown; there is no `SIGTERM` branch to
/// fall back to. Windows is why the route exists at all — signalling there means
/// `TerminateProcess`, which skips the drain that closes WebSocket connections
/// cleanly and removes the pid file — but the stop path sends this request on
/// Unix too, because it is what carries `expect_pid`. On macOS the default is
/// refusal without sending a request.
///
/// Without a PID to name this sends the old, unchecked request, which is all it
/// can do. [`stop_daemon_process`] — the path every real stop takes — names its
/// target instead.
pub fn request_graceful_shutdown() -> bool {
    #[cfg(target_os = "macos")]
    {
        false
    }
    #[cfg(not(target_os = "macos"))]
    {
        matches!(request_shutdown(None), ShutdownRequest::Accepted)
    }
}

/// On macOS, refuse to stop an unbound daemon without sending a request or signal.
#[cfg(target_os = "macos")]
pub fn stop_daemon_process(pid: u32) -> std::result::Result<(), DaemonStopError> {
    Err(DaemonStopError::UnverifiedIdentity(format!(
        "daemon PID {pid} is not bound to an authenticated process object; no shutdown or signal was sent and this client did not remove the PID file. Inspect the process and stop it out of band, or explicitly try 'agent2ssh daemon stop --allow-unbound-graceful' (may stop the wrong process and expose the admin token)"
    )))
}

/// Stop the daemon with this `pid` and wait until it is gone (non-macOS).
///
/// Authenticate the local daemon and request shutdown through its admin route,
/// then signal it by force if it has not exited within the graceful-stop timeout.
/// Returning only once the process has exited is what makes a restart safe — the
/// old daemon still holds the listening port while it drains, so starting the
/// new one straight away would fail to bind.
#[cfg(not(target_os = "macos"))]
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
    // Bind the process object before asking for a shutdown, so the grace period
    // and any forced termination both address the object that was verified here
    // — not whatever holds the PID eight seconds later. `None` (denied, gone, or
    // already reused) is not evidence that the process exited: the observers
    // below still decide whether it is running, and a forced stop refuses
    // rather than re-opening the PID (see [`terminate_for_stop`]).
    let bound = BoundProcess::open(pid, start_time);
    // A matching executable name is not proof that this is our daemon: an
    // unrelated process can reuse the PID or spoof argv[0]. Require the health
    // PID to match, the daemon to serve *this* config directory, and this
    // config's admin token to be accepted before asking for anything.
    //
    // This is the second identity entry point in this file, not a reuse of
    // [`daemon_pid_state`]: that one is consulted only in the `None` branch
    // above (no observable start time) and answers from a `/health` + `/gate`
    // pair without ever looking at the config directory, while this path
    // authenticates with `/shutdown` itself and refuses on a `config_dir`
    // mismatch. Both criteria are deliberate, and changing one will not turn
    // the other's tests red — change them together.
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
        // Every `200` to a stop that named a PID lands here, a current daemon
        // that compared the PID and matched included: the guard on the first
        // arm requires `expected_pid.is_none()`, which a real stop never is. So
        // this arm cannot say that the daemon is old — only that its answer
        // carried no receipt. It is still the daemon that health-checked a
        // moment ago (its config directory matched), so continuing is the
        // previous behaviour, not a hole this opened; claiming the check
        // happened would be the lie. The warning therefore reports the missing
        // receipt and says the version is undeterminable, rather than
        // attributing it to an older daemon (#20).
        ShutdownRequest::AcceptedUnchecked => {
            let _ = crate::diagnostics::append_diagnostic_log(
                "warn",
                "daemon_control",
                "the daemon accepted the shutdown without a verifiable expect_pid receipt; its version cannot be determined from this response",
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
    match wait_for_bound_exit(bound.as_ref(), pid, start_time, GRACEFUL_STOP_WAIT) {
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
            terminate_for_stop(bound.as_ref(), pid, start_time).map_err(DaemonStopError::Other)?;
            match wait_for_bound_exit(bound.as_ref(), pid, start_time, Duration::from_secs(2)) {
                ProcessObservation::Exited => {}
                ProcessObservation::Running => return Err(Refused::StillRunning(pid)),
                ProcessObservation::Unknown => return Err(Refused::UncertainExit(pid)),
            }
        }
    }
    clear_daemon_owner_if(pid, start_time)?;
    Ok(())
}

/// Explicit, unbound legacy compatibility attempt. Acceptance is not proof of
/// process identity or exit. Never wait, signal, retry, or clean a PID record.
#[cfg(target_os = "macos")]
pub fn request_unbound_graceful_shutdown(pid: u32) -> Result<()> {
    let addr = crate::local_daemon_connect_addr();
    let socket: std::net::SocketAddr = addr.parse().map_err(|_| {
        anyhow!("unbound shutdown requires a numeric loopback address; no request sent")
    })?;
    if !socket.ip().is_loopback() {
        anyhow::bail!("unbound shutdown requires a loopback address; no request sent");
    }
    let read_record = || -> Result<String> {
        let _guard = crate::store::lock_config_file(DAEMON_PID_LOCK)?;
        let path = daemon_pid_path()?;
        if !std::fs::symlink_metadata(&path)
            .context("cannot inspect daemon PID file; no request sent")?
            .file_type()
            .is_file()
        {
            anyhow::bail!("daemon PID path is not a regular file; no request sent");
        }
        let raw = std::fs::read_to_string(path)
            .context("cannot read daemon PID file; no request sent")?;
        if raw.trim().parse::<u32>().ok() != Some(pid) {
            anyhow::bail!("daemon PID record does not match; no request sent");
        }
        Ok(raw)
    };
    let original_record = read_record()?;
    if daemon_process_start_time(pid).is_none() {
        anyhow::bail!(
            "daemon PID {pid} cannot be observed as a daemon; no request sent; this client did not remove the PID file"
        );
    }
    let health = health_identity().ok_or_else(|| {
        anyhow!("daemon health is unavailable; no request sent; this client did not remove the PID file")
    })?;
    if health.pid != pid
        || health.config_dir != expected_config_dir()
        || health.config_dir.is_none()
    {
        anyhow::bail!("daemon PID or configuration directory does not match; no request sent; this client did not remove the PID file");
    }
    if read_record()? != original_record {
        anyhow::bail!("daemon PID record changed during discovery; no request sent");
    }
    let token = crate::daemon_activity::read_admin_token()
        .ok_or_else(|| anyhow!("admin token unavailable; no request sent"))?;
    let _ = crate::diagnostics::append_diagnostic_log(
        "warn",
        "daemon_control",
        "explicit unbound HTTP shutdown requested; target identity is not verified",
        Some(serde_json::json!({"pid": pid, "address": addr})),
    );
    let response = crate::daemon_activity::local_daemon_request_with_body(
        "POST",
        "/shutdown",
        Some(&token),
        &serde_json::json!({"expect_pid": pid}),
        Duration::from_secs(2),
    )
    .map_err(|error| {
        anyhow!(
            "shutdown outcome is unknown ({error}); this client did not remove the PID file, but the daemon may have exited; do not automatically retry"
        )
    })?;
    if response.status != 200 {
        anyhow::bail!(
            "shutdown returned HTTP {}; process outcome is unknown and this client did not remove the PID file",
            response.status
        );
    }
    let accepted = serde_json::from_str::<serde_json::Value>(&response.body)
        .ok()
        .and_then(|body| body.get("ok").and_then(|ok| ok.as_bool()))
        == Some(true);
    if !accepted {
        anyhow::bail!("shutdown response was not a valid acceptance; process outcome is unknown and this client did not remove the PID file");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn stale_cleanup_rejects_same_pid_if_record_bytes_changed() {
        let _config = crate::store::TestConfigDir::new("daemon-stale-snapshot");
        let path = daemon_pid_path().unwrap();
        std::fs::write(&path, "2147483647\n").unwrap();
        let error = clear_stale_daemon_pid_with_snapshot(2147483647, "2147483647").unwrap_err();
        assert!(error.to_string().contains("changed during cleanup"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "2147483647\n");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn pid_publication_does_not_follow_a_dangling_symlink() {
        let _config = crate::store::TestConfigDir::new("daemon-pid-symlink");
        let path = daemon_pid_path().unwrap();
        let target = path.with_file_name("uncreated-target");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let error = write_daemon_pid(std::process::id()).unwrap_err();
        assert!(error.to_string().contains("existing path retained"));
        assert!(!target.exists());
        assert!(std::fs::symlink_metadata(path)
            .unwrap()
            .file_type()
            .is_symlink());
    }

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
        #[cfg(target_os = "macos")]
        {
            assert!(write_daemon_pid(me)
                .unwrap_err()
                .to_string()
                .contains("automatic replacement"));
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                (u32::MAX - 1).to_string()
            );
        }
        #[cfg(not(target_os = "macos"))]
        {
            write_daemon_pid(me).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), me.to_string());
        }
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

    /// Read the second `GetProcessTimes` reports, the same way `open` does, so a
    /// test can name the value it expects to be accepted.
    #[cfg(windows)]
    fn creation_time_of(pid: u32) -> Option<u64> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle == 0 {
            return None;
        }
        let secs = creation_time_secs(handle);
        unsafe { CloseHandle(handle) };
        secs
    }

    #[cfg(windows)]
    fn spawn_sleeper() -> std::process::Child {
        Command::new("cmd.exe")
            .args(["/C", "ping", "-n", "20", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sleeper")
    }

    /// A PID is not an object. Two processes can share one over time, and the
    /// only way to notice is to refuse a handle whose creation time is not the
    /// recorded one — which is what `open` does before the stop is allowed to
    /// trust the handle.
    #[cfg(windows)]
    #[test]
    fn a_bound_handle_refuses_a_creation_time_it_did_not_open() {
        let child = spawn_sleeper();
        let pid = child.id();
        let start = creation_time_of(pid).expect("read creation time");
        let bound = BoundProcess::open(pid, start);
        assert!(
            bound.is_some(),
            "the recorded second opened this process, so it must bind"
        );
        assert!(
            BoundProcess::open(pid, start + 1).is_none(),
            "a different second names a different object, and must not bind"
        );
        drop(bound);
        let mut child = child;
        let _ = child.kill();
        let _ = child.wait();
    }

    /// The handle has to answer for its own object across the exit, otherwise
    /// the stop path would be back to re-opening a PID it no longer controls.
    #[cfg(windows)]
    #[test]
    fn a_bound_handle_tracks_its_object_through_exit() {
        let mut child = spawn_sleeper();
        let pid = child.id();
        let start = creation_time_of(pid).expect("read creation time");
        let bound = BoundProcess::open(pid, start).expect("bind live child");
        assert!(!bound.is_signaled(), "a live child is not signaled");
        assert_eq!(
            wait_for_bound_exit(Some(&bound), pid, start, Duration::ZERO),
            ProcessObservation::Running,
            "a bound handle never answers Unknown while the object lives"
        );
        let _ = child.kill();
        let _ = child.wait();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !bound.is_signaled() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            bound.is_signaled(),
            "the bound object must report its own termination"
        );
        assert_eq!(
            wait_for_bound_exit(Some(&bound), pid, start, Duration::ZERO),
            ProcessObservation::Exited
        );
    }

    /// Termination goes through the handle, so it ends that object and reports
    /// failure instead of silently falling back to a PID.
    #[cfg(windows)]
    #[test]
    fn terminating_a_bound_handle_ends_the_object_it_opened() {
        let mut target = spawn_sleeper();
        let bystander = spawn_sleeper();
        let pid = target.id();
        let start = creation_time_of(pid).expect("read creation time");
        let bound = BoundProcess::open(pid, start).expect("bind");
        terminate_for_stop(Some(&bound), pid, start).expect("terminate bound object");
        let status = target.wait().expect("reap terminated child");
        assert!(
            !status.success(),
            "TerminateProcess exits 1, which must not look like a clean exit"
        );
        assert!(bound.is_signaled());
        assert!(
            !process_handle_is_signaled(bystander.id()),
            "a neighbouring process on another PID must be untouched"
        );
        let mut bystander = bystander;
        let _ = bystander.kill();
        let _ = bystander.wait();
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
        // pid for something else. The reverse is not covered here and cannot be
        // told apart by `daemon_is_owned`: a recycled pid whose start time lands
        // in the same whole second as the recorded one matches the marker.
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
        #[cfg(target_os = "macos")]
        assert!(stop_daemon_process(u32::MAX - 1)
            .unwrap_err()
            .to_string()
            .contains("no shutdown or signal"));
        #[cfg(not(target_os = "macos"))]
        stop_daemon_process(u32::MAX - 1).expect("nothing to stop");
    }

    /// #24: a forced stop must not fall back to re-opening the PID. The old
    /// fallback treated "no such process" as success, so restoring it makes this
    /// assertion fail instead of passing quietly.
    #[cfg(windows)]
    #[test]
    fn an_unbound_force_stop_refuses_instead_of_reopening_by_pid() {
        let error = terminate_for_stop(None, u32::MAX - 1, 0)
            .expect_err("an unbound force stop must not report success");
        assert!(error.to_string().contains("could not be bound"), "{error}");
    }

    /// The binding is only valid for the creation time it was opened for; a
    /// different one is a reused PID, not the same process.
    #[cfg(windows)]
    #[test]
    fn a_binding_is_accepted_only_for_the_creation_time_it_was_opened_for() {
        let pid = std::process::id();
        let start = observable_live_start_time(pid).expect("this process has a start time");
        assert!(
            BoundProcess::open(pid, start).is_some(),
            "the same creation time binds"
        );
        assert!(
            BoundProcess::open(pid, start + 1).is_none(),
            "a different creation time must not bind"
        );
    }
}
