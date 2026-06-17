use crate::keys::{
    delete_key_core, generate_key_core, import_key_core, list_keys_core, SshKeyInfo,
};
use crate::notify::{load_webhook_config, save_webhook_config, WebhookConfig};
use crate::{
    connection::{connect_host, disconnect_host, list_active_connections},
    core::{
        add_host_core, exec_multi_core, exec_ssh_core, export_team_config, import_ssh_config_core,
        import_team_config, list_audit_core, list_hosts_core, ping_hosts_core, remove_host_core,
        sftp_download_core_with_source, sftp_ls_core_with_source, sftp_mkdir_core_with_source,
        sftp_stat_core_with_source, sftp_upload_core_with_source, ExecMultiRequest, ImportResult,
        TeamConfigExport,
    },
    execution_control::{
        append_rejected_exec_audit, authorize_command_with_approval, command_authorization_target,
        effective_command_risk, expand_exec_authorization_targets, CommandAuthorizationError,
        CommandAuthorizationInput,
    },
    forward::{forward_add_core, forward_list_core, forward_remove_core},
    playbook::{
        dry_run_playbook, list_playbooks_core, run_playbook_core_with_source, Playbook,
        PlaybookRunResult,
    },
    remote::{list_daemons_core, DaemonInfo},
    session::{
        session_close_core, session_list_core, session_open_core, session_read_core,
        session_write_core,
    },
    types::{
        source_from_env, AuditEntry, AuditFilter, ConnectionStatus, ExecMultiResult, ExecRequest,
        ExecResult, ForwardDirection, ForwardRule, HostProfile, PingResult, RiskLevel,
        SftpDownloadRequest, SftpResult, SftpUploadRequest,
    },
};
use std::collections::HashMap;
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
pub async fn classify_command_risk(command: String) -> Result<RiskLevel, String> {
    Ok(effective_command_risk(&command).await)
}

#[tauri::command]
pub async fn exec_ssh(mut request: ExecRequest) -> Result<ExecResult, String> {
    if request.source.is_none() {
        request.source = Some(source_from_env("desktop"));
    }
    authorize_desktop_exec_request(&mut request).await?;
    exec_ssh_core(request).await.map_err(|e| e.to_string())
}

async fn authorize_desktop_exec_request(request: &mut ExecRequest) -> Result<RiskLevel, String> {
    let target = command_authorization_target(&request.host);
    let source = request.source.as_deref().unwrap_or("desktop").to_string();
    let auth_scope = None;
    let result = authorize_command_with_approval(
        CommandAuthorizationInput {
            auth_scope: &auth_scope,
            source: &source,
            host: &request.host,
            tags: &target.tags,
            risk_override: target.risk_override,
            command: &request.command,
            force: request.force,
            reason: request.reason.clone(),
            change_id: request.change_id.clone(),
        },
        |prompt| async move {
            let message = "approval required but no desktop approval handler is available";
            append_rejected_exec_audit(
                &prompt.source,
                &prompt.host,
                &prompt.command,
                prompt.risk,
                message,
                prompt.change_id.as_deref(),
            );
            Err(format!("{message}; run through the daemon approval flow"))
        },
    )
    .await
    .map_err(command_authorization_error)?;
    if result.approved && result.risk == RiskLevel::High {
        request.force = true;
    }
    Ok(result.risk)
}

fn command_authorization_error(error: CommandAuthorizationError) -> String {
    match error {
        CommandAuthorizationError::ScopeDenied(message) => message,
        CommandAuthorizationError::Blocked { message, .. } => message,
        CommandAuthorizationError::ApprovalRejected => "command rejected by approver".to_string(),
        CommandAuthorizationError::ApprovalTimedOut => "approval request timed out".to_string(),
        CommandAuthorizationError::Internal(message) => message,
    }
}

async fn authorize_desktop_exec_targets(
    hosts: &[String],
    tags: &Option<Vec<String>>,
    command: &str,
    force: bool,
    reason: Option<String>,
    change_id: Option<String>,
    source: &str,
) -> Result<bool, String> {
    let targets = expand_exec_authorization_targets(hosts, tags).map_err(|e| e.to_string())?;
    let auth_scope = None;
    let mut high_risk_approved = false;
    for target in targets {
        let result = authorize_command_with_approval(
            CommandAuthorizationInput {
                auth_scope: &auth_scope,
                source,
                host: &target.host,
                tags: &target.tags,
                risk_override: target.risk_override,
                command,
                force: force || high_risk_approved,
                reason: reason.clone(),
                change_id: change_id.clone(),
            },
            |prompt| async move {
                let message = "approval required but no desktop approval handler is available";
                append_rejected_exec_audit(
                    &prompt.source,
                    &prompt.host,
                    &prompt.command,
                    prompt.risk,
                    message,
                    prompt.change_id.as_deref(),
                );
                Err(format!("{message}; run through the daemon approval flow"))
            },
        )
        .await
        .map_err(command_authorization_error)?;
        if result.approved && result.risk == RiskLevel::High {
            high_risk_approved = true;
        }
    }
    Ok(high_risk_approved)
}

async fn authorize_desktop_playbook_run(
    playbook: &str,
    host: &str,
    force: bool,
    params: &HashMap<String, String>,
    source: &str,
) -> Result<bool, String> {
    let dry_run = dry_run_playbook(playbook, params).map_err(|e| e.to_string())?;
    let target = command_authorization_target(host);
    let playbook_risk_override = list_playbooks_core()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|item| item.name == playbook)
        .and_then(|item| item.risk_override);
    let risk_override = playbook_risk_override.or(target.risk_override);
    let auth_scope = None;
    let mut high_risk_approved = false;

    for step in dry_run.steps {
        let result = authorize_command_with_approval(
            CommandAuthorizationInput {
                auth_scope: &auth_scope,
                source,
                host,
                tags: &target.tags,
                risk_override,
                command: &step.command_resolved,
                force: force || high_risk_approved,
                reason: None,
                change_id: None,
            },
            |prompt| async move {
                let message = "approval required but no desktop approval handler is available";
                append_rejected_exec_audit(
                    &prompt.source,
                    &prompt.host,
                    &prompt.command,
                    prompt.risk,
                    message,
                    prompt.change_id.as_deref(),
                );
                Err(format!("{message}; run through the daemon approval flow"))
            },
        )
        .await
        .map_err(command_authorization_error)?;
        if result.approved && result.risk == RiskLevel::High {
            high_risk_approved = true;
        }
    }

    Ok(high_risk_approved)
}

async fn authorize_desktop_operation(
    host: &str,
    command: &str,
    force: bool,
    source: &str,
) -> Result<(), String> {
    let target = command_authorization_target(host);
    let auth_scope = None;
    authorize_command_with_approval(
        CommandAuthorizationInput {
            auth_scope: &auth_scope,
            source,
            host,
            tags: &target.tags,
            risk_override: target.risk_override,
            command,
            force,
            reason: None,
            change_id: None,
        },
        |prompt| async move {
            let message = "approval required but no desktop approval handler is available";
            append_rejected_exec_audit(
                &prompt.source,
                &prompt.host,
                &prompt.command,
                prompt.risk,
                message,
                prompt.change_id.as_deref(),
            );
            Err(format!("{message}; run through the daemon approval flow"))
        },
    )
    .await
    .map_err(command_authorization_error)?;
    Ok(())
}

#[tauri::command]
pub async fn exec_multi(
    hosts: Vec<String>,
    command: String,
    force: bool,
    timeout_secs: Option<u64>,
    tags: Option<Vec<String>>,
) -> Result<Vec<ExecMultiResult>, String> {
    let source = source_from_env("desktop");
    let mut force = force;
    if authorize_desktop_exec_targets(&hosts, &tags, &command, force, None, None, &source).await? {
        force = true;
    }
    Ok(exec_multi_core(ExecMultiRequest {
        hosts,
        command,
        force,
        timeout_secs,
        tags,
        reason: None,
        change_id: None,
        source: Some(source),
    })
    .await)
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
    let source = source_from_env("desktop");
    let command = format!(
        "sftp upload {} -> {}",
        request.local_path, request.remote_path
    );
    authorize_desktop_operation(&request.host, &command, true, &source).await?;
    sftp_upload_core_with_source(request, Some(source))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sftp_download(request: SftpDownloadRequest) -> Result<SftpResult, String> {
    let source = source_from_env("desktop");
    let command = format!(
        "sftp download {} -> {}",
        request.remote_path, request.local_path
    );
    authorize_desktop_operation(&request.host, &command, true, &source).await?;
    sftp_download_core_with_source(request, Some(source))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sftp_ls(
    host: String,
    path: String,
    timeout_secs: Option<u64>,
) -> Result<ExecResult, String> {
    let source = source_from_env("desktop");
    let command = format!("sftp ls {}", path);
    authorize_desktop_operation(&host, &command, true, &source).await?;
    sftp_ls_core_with_source(&host, &path, timeout_secs, Some(source))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sftp_stat(
    host: String,
    path: String,
    timeout_secs: Option<u64>,
) -> Result<ExecResult, String> {
    let source = source_from_env("desktop");
    let command = format!("sftp stat {}", path);
    authorize_desktop_operation(&host, &command, true, &source).await?;
    sftp_stat_core_with_source(&host, &path, timeout_secs, Some(source))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sftp_mkdir(
    host: String,
    path: String,
    timeout_secs: Option<u64>,
) -> Result<ExecResult, String> {
    let source = source_from_env("desktop");
    let command = format!("sftp mkdir {}", path);
    authorize_desktop_operation(&host, &command, true, &source).await?;
    sftp_mkdir_core_with_source(&host, &path, timeout_secs, Some(source))
        .await
        .map_err(|e| e.to_string())
}

// ── Sessions ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn session_open(host: String) -> Result<String, String> {
    let source = source_from_env("desktop");
    authorize_desktop_operation(&host, "session_open", true, &source).await?;
    session_open_core(&host)
        .await
        .map(|id| id.to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn session_write(id: String, input: String) -> Result<(), String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let source = source_from_env("desktop");
    let host = session_list_core()
        .await
        .into_iter()
        .find(|(session_id, _)| *session_id == uuid)
        .map(|(_, host)| host)
        .unwrap_or_else(|| format!("session:{id}"));
    authorize_desktop_operation(&host, &input, false, &source).await?;
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
    let source = source_from_env("desktop");
    let command = format!(
        "forward {} {}:{} -> {}:{}",
        direction, bind_port, target_host, host, target_port
    );
    authorize_desktop_operation(&host, &command, true, &source).await?;
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

/// List all configured daemons (localhost + remotes from ~/.agent2ssh/remotes.toml)
#[tauri::command]
pub fn list_daemons() -> Result<Vec<DaemonInfo>, String> {
    list_daemons_core().map_err(|e| e.to_string())
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

// ── Connection pool ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn connection_status() -> Result<Vec<ConnectionStatus>, String> {
    Ok(list_active_connections().await)
}

#[tauri::command]
pub async fn ssh_connect(host: String) -> Result<(), String> {
    let source = source_from_env("desktop");
    authorize_desktop_operation(&host, "connect", true, &source).await?;
    connect_host(&host).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ssh_disconnect(host: String) -> Result<(), String> {
    let source = source_from_env("desktop");
    authorize_desktop_operation(&host, "disconnect", true, &source).await?;
    disconnect_host(&host).await.map_err(|e| e.to_string())
}

// ── Playbooks ────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_playbooks() -> Result<Vec<Playbook>, String> {
    list_playbooks_core().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn run_playbook(
    playbook: String,
    host: String,
    force: bool,
) -> Result<PlaybookRunResult, String> {
    let source = source_from_env("desktop");
    let params = HashMap::new();
    let mut force = force;
    if authorize_desktop_playbook_run(&playbook, &host, force, &params, &source).await? {
        force = true;
    }
    run_playbook_core_with_source(&playbook, &host, force, None, None, None, Some(source))
        .await
        .map_err(|e| e.to_string())
}

// ── Webhook config ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_webhook_config() -> Result<WebhookConfig, String> {
    Ok(load_webhook_config().unwrap_or_default())
}

#[tauri::command]
pub fn set_webhook_config(config: WebhookConfig) -> Result<(), String> {
    save_webhook_config(&config).map_err(|e| e.to_string())
}

// ── Audit rotation ───────────────────────────────────────────────────────────

/// Rotate the audit log if it exceeds 10 MB.
#[tauri::command]
pub fn rotate_audit() -> Result<(), String> {
    crate::store::rotate_audit_core().map_err(|e| e.to_string())
}

// ── Team Config Export/Import ────────────────────────────────────────────────

#[tauri::command]
pub fn export_team_config_cmd() -> Result<TeamConfigExport, String> {
    export_team_config().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_team_config_cmd(config: TeamConfigExport) -> Result<ImportResult, String> {
    import_team_config(&config).map_err(|e| e.to_string())
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
            list_daemons,
            // SSH Keys
            list_keys,
            generate_key,
            import_key,
            delete_key,
            // Connection pool
            connection_status,
            ssh_connect,
            ssh_disconnect,
            // Playbooks
            list_playbooks,
            run_playbook,
            // Webhook config
            get_webhook_config,
            set_webhook_config,
            // Team config export/import
            export_team_config_cmd,
            import_team_config_cmd,
            // Audit rotation
            rotate_audit,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
