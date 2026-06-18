use anyhow::{anyhow, Result};
use ssh2::Session;
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};
use tokio::sync::Mutex;

use crate::{
    embedded_ssh::connect_embedded_ssh,
    store::load_config,
    types::{ConnectionStatus, HostProfile},
};

struct EmbeddedConnectionHandle {
    _session: Session,
}

static CONNECTIONS: OnceLock<Mutex<HashMap<String, EmbeddedConnectionHandle>>> = OnceLock::new();
static HOST_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

fn connections() -> &'static Mutex<HashMap<String, EmbeddedConnectionHandle>> {
    CONNECTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn host_locks() -> &'static Mutex<HashMap<String, Arc<Mutex<()>>>> {
    HOST_LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn per_host_lock(host_name: &str) -> Arc<Mutex<()>> {
    let mut map = host_locks().lock().await;
    map.entry(host_name.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn resolve_host(host_name: &str) -> Result<HostProfile> {
    load_config()?
        .hosts
        .into_iter()
        .find(|host| host.name == host_name)
        .ok_or_else(|| anyhow!("unknown host profile: {host_name}"))
}

/// List configured hosts and whether Agent2SSH currently holds an embedded SSH
/// session for them.
pub async fn list_active_connections() -> Vec<ConnectionStatus> {
    let config = match load_config() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let store = connections().lock().await;
    config
        .hosts
        .iter()
        .map(|host| ConnectionStatus {
            host: host.name.clone(),
            connected: store.contains_key(&host.name),
            socket_path: None,
        })
        .collect()
}

/// Manually establish and retain an embedded SSH connection to a specific host.
pub async fn connect_host(host_name: &str) -> Result<()> {
    let host = resolve_host(host_name)?;
    let lock = per_host_lock(&host.name).await;
    let _guard = lock.lock().await;

    if connections().lock().await.contains_key(&host.name) {
        return Ok(());
    }

    let host_for_task = host.clone();
    let session = tokio::task::spawn_blocking(move || connect_embedded_ssh(&host_for_task, 60))
        .await
        .map_err(|e| anyhow!("embedded connection task failed: {e}"))??;
    connections().lock().await.insert(
        host.name.clone(),
        EmbeddedConnectionHandle { _session: session },
    );
    Ok(())
}

/// Close a retained embedded SSH connection.
pub async fn disconnect_host(host_name: &str) -> Result<()> {
    resolve_host(host_name)?;
    connections().lock().await.remove(host_name);
    Ok(())
}
