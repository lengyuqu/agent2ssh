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
    let pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    system
        .process(pid)
        .map(process_looks_like_daemon)
        .unwrap_or(false)
}

fn process_looks_like_daemon(process: &sysinfo::Process) -> bool {
    let name = process.name().to_string_lossy();
    if name.contains("agent2ssh-daemon") {
        return true;
    }
    process
        .cmd()
        .iter()
        .any(|part| part.to_string_lossy().contains("agent2ssh-daemon"))
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
