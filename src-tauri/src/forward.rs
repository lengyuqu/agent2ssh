use anyhow::{anyhow, Context, Result};
use std::{
    collections::HashMap,
    sync::OnceLock,
};
use tokio::{process::Child, sync::Mutex};
use uuid::Uuid;

use crate::{
    store::load_config,
    types::{ForwardDirection, ForwardRule, HostProfile},
};

struct ForwardHandle {
    rule: ForwardRule,
    _child: Child,
}

static FORWARDS: OnceLock<Mutex<HashMap<Uuid, ForwardHandle>>> = OnceLock::new();

fn forwards() -> &'static Mutex<HashMap<Uuid, ForwardHandle>> {
    FORWARDS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn resolve_host(name: &str) -> Result<HostProfile> {
    load_config()?
        .hosts
        .into_iter()
        .find(|h| h.name == name)
        .ok_or_else(|| anyhow!("unknown host profile: {name}"))
}

fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).display().to_string();
        }
    }
    path.to_string()
}

fn ssh_target(host: &HostProfile) -> String {
    match &host.user {
        Some(u) if !u.trim().is_empty() => format!("{}@{}", u, host.host),
        _ => host.host.clone(),
    }
}

pub async fn forward_add_core(
    host_name: &str,
    direction: ForwardDirection,
    bind_port: u16,
    target_host: &str,
    target_port: u16,
) -> Result<ForwardRule> {
    let host = resolve_host(host_name)?;

    // -L bind_port:target_host:target_port  (local → remote)
    // -R bind_port:target_host:target_port  (remote → local)
    let tunnel_arg = format!("{bind_port}:{target_host}:{target_port}");
    let direction_flag = match direction {
        ForwardDirection::Local => "-L",
        ForwardDirection::Remote => "-R",
    };

    let mut cmd = tokio::process::Command::new("ssh");
    cmd.arg("-N") // don't execute a remote command
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new")
        .arg("-p")
        .arg(host.port.unwrap_or(22).to_string())
        .arg(direction_flag)
        .arg(&tunnel_arg);

    if let Some(key_path) = &host.key_path {
        if !key_path.trim().is_empty() {
            cmd.arg("-i").arg(expand_tilde(key_path));
        }
    }

    cmd.arg(ssh_target(&host))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    let child = cmd.spawn().context("failed to spawn ssh forward")?;

    let rule = ForwardRule {
        id: Uuid::new_v4(),
        host: host_name.to_string(),
        direction,
        bind_port,
        target_host: target_host.to_string(),
        target_port,
    };

    let handle = ForwardHandle {
        rule: rule.clone(),
        _child: child,
    };

    forwards().lock().await.insert(rule.id, handle);
    Ok(rule)
}

pub async fn forward_list_core() -> Vec<ForwardRule> {
    forwards()
        .lock()
        .await
        .values()
        .map(|h| h.rule.clone())
        .collect()
}

pub async fn forward_remove_core(id: Uuid) -> Result<()> {
    let mut store = forwards().lock().await;
    if store.remove(&id).is_none() {
        return Err(anyhow!("unknown forward: {id}"));
    }
    // ForwardHandle drop kills the SSH process
    Ok(())
}
