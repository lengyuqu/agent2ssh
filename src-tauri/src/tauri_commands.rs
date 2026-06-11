use crate::{
    core::{
        add_host_core, classify_risk, exec_multi_core, exec_ssh_core, import_ssh_config_core,
        list_audit_core, list_hosts_core, ping_hosts_core, remove_host_core, sftp_download_core,
        sftp_ls_core, sftp_mkdir_core, sftp_stat_core, sftp_upload_core,
    },
    forward::{forward_add_core, forward_list_core, forward_remove_core},
    session::{
        session_close_core, session_list_core, session_open_core, session_read_core,
        session_write_core,
    },
    types::{
        AuditEntry, AuditFilter, ExecMultiResult, ExecRequest, ExecResult, ForwardDirection,
        ForwardRule, HostProfile, PingResult, RiskLevel, SftpDownloadRequest, SftpResult,
        SftpUploadRequest,
    },
};
use crate::keys::{delete_key_core, generate_key_core, import_key_core, list_keys_core, SshKeyInfo};
use uuid::Uuid;

// ── Host management ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_hosts() -> Result<Vec<HostProfile>, String> {
    list_hosts_core().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_host(host: HostProfile) -> Result<HostProfile, String> {
    add_host_core(host).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_host(name: String) -> Result<(), String> {
    remove_host_core(&name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_ssh_config(path: Option<String>) -> Result<Vec<HostProfile>, String> {
    import_ssh_config_core(path.as_deref()).map_err(|e| e.to_string())
}

// ── Command execution ────────────────────────────────────────────────────────

#[tauri::command]
pub fn classify_command_risk(command: String) -> Result<RiskLevel, String> {
    Ok(classify_risk(&command))
}

#[tauri::command]
pub async fn exec_ssh(request: ExecRequest) -> Result<ExecResult, String> {
    exec_ssh_core(request).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn exec_multi(
    hosts: Vec<String>,
    command: String,
    force: bool,
    timeout_secs: Option<u64>,
    tags: Option<Vec<String>>,
) -> Result<Vec<ExecMultiResult>, String> {
    Ok(exec_multi_core(hosts, command, force, timeout_secs, tags).await)
}

#[tauri::command]
pub async fn ping_hosts(
    hosts: Vec<String>,
    timeout_secs: Option<u64>,
) -> Result<Vec<PingResult>, String> {
    Ok(ping_hosts_core(hosts, timeout_secs).await)
}

// ── SFTP ─────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn sftp_upload(request: SftpUploadRequest) -> Result<SftpResult, String> {
    sftp_upload_core(request).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sftp_download(request: SftpDownloadRequest) -> Result<SftpResult, String> {
    sftp_download_core(request).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sftp_ls(
    host: String,
    path: String,
    timeout_secs: Option<u64>,
) -> Result<ExecResult, String> {
    sftp_ls_core(&host, &path, timeout_secs)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sftp_stat(
    host: String,
    path: String,
    timeout_secs: Option<u64>,
) -> Result<ExecResult, String> {
    sftp_stat_core(&host, &path, timeout_secs)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sftp_mkdir(
    host: String,
    path: String,
    timeout_secs: Option<u64>,
) -> Result<ExecResult, String> {
    sftp_mkdir_core(&host, &path, timeout_secs)
        .await
        .map_err(|e| e.to_string())
}

// ── Sessions ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn session_open(host: String) -> Result<String, String> {
    session_open_core(&host)
        .await
        .map(|id| id.to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn session_write(id: String, input: String) -> Result<(), String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    session_write_core(uuid, &input)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn session_read(id: String, timeout_ms: Option<u64>) -> Result<String, String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    session_read_core(uuid, timeout_ms.unwrap_or(2000))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn session_close(id: String) -> Result<(), String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    session_close_core(uuid).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn session_list() -> Result<Vec<(String, String)>, String> {
    Ok(session_list_core()
        .await
        .into_iter()
        .map(|(id, host)| (id.to_string(), host))
        .collect())
}

// ── Port forwarding ──────────────────────────────────────────────────────────

#[tauri::command]
pub async fn forward_add(
    host: String,
    direction: ForwardDirection,
    bind_port: u16,
    target_host: String,
    target_port: u16,
) -> Result<ForwardRule, String> {
    forward_add_core(&host, direction, bind_port, &target_host, target_port)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn forward_list() -> Result<Vec<ForwardRule>, String> {
    Ok(forward_list_core().await)
}

#[tauri::command]
pub async fn forward_remove(id: String) -> Result<(), String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    forward_remove_core(uuid).await.map_err(|e| e.to_string())
}

// ── Audit ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_audit(filter: Option<AuditFilter>) -> Result<Vec<AuditEntry>, String> {
    let filter = filter.unwrap_or(AuditFilter {
        limit: 50,
        ..Default::default()
    });
    list_audit_core(filter).map_err(|e| e.to_string())
}

// ── Daemon helpers (for desktop approval polling) ───────────────────────────

/// Read the daemon bearer token from ~/.agent2ssh/daemon.token
#[tauri::command]
pub fn get_daemon_token() -> Result<String, String> {
    let config_dir = crate::store::config_dir().map_err(|e| e.to_string())?;
    let token_path = config_dir.join("daemon.token");
    if token_path.exists() {
        std::fs::read_to_string(&token_path)
            .map(|s| s.trim().to_string())
            .map_err(|e| e.to_string())
    } else {
        Err("daemon token not found (daemon not started?)".into())
    }
}

// ── SSH Key management ──────────────────────────────────────────────────────

#[tauri::command]
pub fn list_keys() -> Result<Vec<SshKeyInfo>, String> {
    list_keys_core().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn generate_key(name: String, comment: Option<String>) -> Result<SshKeyInfo, String> {
    generate_key_core(&name, comment.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_key(source_path: String, name: Option<String>) -> Result<SshKeyInfo, String> {
    import_key_core(&source_path, name.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_key(name: String) -> Result<(), String> {
    delete_key_core(&name).map_err(|e| e.to_string())
}

// ── Bootstrap ────────────────────────────────────────────────────────────────

pub fn run_tauri() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            // Host management
            list_hosts,
            add_host,
            remove_host,
            import_ssh_config,
            // Execution
            classify_command_risk,
            exec_ssh,
            exec_multi,
            ping_hosts,
            // SFTP
            sftp_upload,
            sftp_download,
            sftp_ls,
            sftp_stat,
            sftp_mkdir,
            // Sessions
            session_open,
            session_write,
            session_read,
            session_close,
            session_list,
            // Port forwarding
            forward_add,
            forward_list,
            forward_remove,
            // Audit
            list_audit,
            // Daemon helpers
            get_daemon_token,
            // SSH Keys
            list_keys,
            generate_key,
            import_key,
            delete_key,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
