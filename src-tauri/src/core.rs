use anyhow::{anyhow, Context, Result};
use std::{process::Stdio, time::Instant};
use tokio::process::Command;

use crate::{
    store::{append_audit, hosts_lock, list_audit_raw, load_config, save_config},
    types::{AuditEntry, ExecRequest, ExecResult, HostProfile},
};

pub fn list_hosts_core() -> Result<Vec<HostProfile>> {
    Ok(load_config()?.hosts)
}

pub fn add_host_core(host: HostProfile) -> Result<HostProfile> {
    validate_host(&host)?;
    let _guard = hosts_lock().lock().unwrap();
    let mut config = load_config()?;
    if let Some(existing) = config.hosts.iter_mut().find(|item| item.name == host.name) {
        *existing = host.clone();
    } else {
        config.hosts.push(host.clone());
    }
    config.hosts.sort_by(|a, b| a.name.cmp(&b.name));
    save_config(&config)?;
    Ok(host)
}

pub fn remove_host_core(name: &str) -> Result<()> {
    let _guard = hosts_lock().lock().unwrap();
    let mut config = load_config()?;
    let before = config.hosts.len();
    config.hosts.retain(|h| h.name != name);
    if config.hosts.len() == before {
        return Err(anyhow!("no host profile named '{name}'"));
    }
    save_config(&config)
}

pub fn list_audit_core(limit: usize) -> Result<Vec<AuditEntry>> {
    list_audit_raw(limit)
}

pub async fn exec_ssh_core(request: ExecRequest) -> Result<ExecResult> {
    let host = resolve_host(&request.host)?;
    let started = Instant::now();
    let mut command = Command::new("ssh");
    command
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new")
        .arg("-p")
        .arg(host.port.unwrap_or(22).to_string());

    if let Some(key_path) = &host.key_path {
        if !key_path.trim().is_empty() {
            command.arg("-i").arg(expand_tilde(key_path));
        }
    }

    command
        .arg(ssh_target(&host))
        .arg(&request.command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = command.output().await.context("failed to spawn ssh")?;
    let result = ExecResult {
        host: request.host,
        command: request.command,
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        duration_ms: started.elapsed().as_millis(),
    };
    append_audit(&result)?;
    Ok(result)
}

fn validate_host(host: &HostProfile) -> Result<()> {
    if host.name.trim().is_empty() {
        return Err(anyhow!("host alias is required"));
    }
    if host.host.trim().is_empty() {
        return Err(anyhow!("host address is required"));
    }
    Ok(())
}

fn resolve_host(name: &str) -> Result<HostProfile> {
    load_config()?
        .hosts
        .into_iter()
        .find(|host| host.name == name)
        .ok_or_else(|| anyhow!("unknown host profile: {name}"))
}

fn ssh_target(host: &HostProfile) -> String {
    match &host.user {
        Some(user) if !user.trim().is_empty() => format!("{user}@{}", host.host),
        _ => host.host.clone(),
    }
}

fn expand_tilde(path: &str) -> String {
    if path == "~" {
        return dirs::home_dir()
            .map(|home| home.display().to_string())
            .unwrap_or_else(|| path.to_string());
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).display().to_string();
        }
    }
    path.to_string()
}
