use anyhow::{anyhow, Context, Result};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::store::config_dir;

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
    system.process(pid).is_some()
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
    let Ok(mut stream) = TcpStream::connect_timeout(
        &"127.0.0.1:7722".parse().expect("valid daemon address"),
        Duration::from_millis(500),
    ) else {
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

fn append_log_file() -> Result<File> {
    let log_path = daemon_log_path()?;
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
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
    let child = Command::new(daemon_bin)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err_log))
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
