use anyhow::{anyhow, Context, Result};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::{process::Command, sync::Mutex};

use crate::{store::config_dir, types::HostProfile};

// Per-host creation lock: ensures only one goroutine establishes ControlMaster per host.
static HOST_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

fn host_locks() -> &'static Mutex<HashMap<String, Arc<Mutex<()>>>> {
    HOST_LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn per_host_lock(host_name: &str) -> Arc<Mutex<()>> {
    let mut map = host_locks().lock().await;
    map.entry(host_name.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn socket_path(host: &HostProfile) -> Result<PathBuf> {
    let safe: String = host
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    Ok(config_dir()?.join(format!("cm_{safe}.sock")))
}

fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).display().to_string();
        }
    }
    path.to_string()
}

pub fn ssh_target(host: &HostProfile) -> String {
    match &host.user {
        Some(u) if !u.trim().is_empty() => format!("{}@{}", u, host.host),
        _ => host.host.clone(),
    }
}

async fn socket_alive(socket: &PathBuf, target: &str) -> bool {
    if !socket.exists() {
        return false;
    }
    Command::new("ssh")
        .arg("-S").arg(socket)
        .arg("-O").arg("check")
        .arg(target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

async fn create_master(host: &HostProfile, socket: &PathBuf) -> Result<()> {
    let target = ssh_target(host);
    let mut cmd = Command::new("ssh");
    cmd.arg("-M")
        .arg("-S").arg(socket)
        .arg("-N")
        .arg("-f")
        .arg("-o").arg("ControlPersist=600")
        .arg("-o").arg("BatchMode=yes")
        .arg("-o").arg("StrictHostKeyChecking=accept-new")
        .arg("-p").arg(host.port.unwrap_or(22).to_string());

    if let Some(key_path) = &host.key_path {
        if !key_path.trim().is_empty() {
            cmd.arg("-i").arg(expand_tilde(key_path));
        }
    }
    cmd.arg(&target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());

    let output = tokio::time::timeout(Duration::from_secs(15), cmd.output())
        .await
        .map_err(|_| anyhow!("timed out establishing ControlMaster to '{}'", host.name))?
        .context("failed to spawn ControlMaster")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("ControlMaster failed for '{}': {stderr}", host.name));
    }
    Ok(())
}

/// Return a live ControlMaster socket path, creating one if needed.
/// Returns `None` if setup fails — callers fall back to a direct connection.
/// Per-host locking prevents multiple concurrent callers from racing on first connect.
pub async fn get_or_create_socket(host: &HostProfile) -> Option<PathBuf> {
    let socket = socket_path(host).ok()?;
    let target = ssh_target(host);

    // Fast path: socket already alive (no locking needed for reads)
    if socket_alive(&socket, &target).await {
        return Some(socket);
    }

    // Slow path: acquire per-host lock before creating
    let lock = per_host_lock(&host.name).await;
    let _guard = lock.lock().await;

    // Re-check after acquiring the lock (another task may have created it)
    if socket_alive(&socket, &target).await {
        return Some(socket);
    }

    match create_master(host, &socket).await {
        Ok(()) => Some(socket),
        Err(_) => None, // silently fall back to direct connection
    }
}

/// Apply ControlMaster reuse flags to an existing `ssh` Command.
pub fn apply_socket(cmd: &mut Command, socket: &PathBuf) {
    cmd.arg("-S").arg(socket).arg("-o").arg("ControlMaster=no");
}
