use crate::keys::{
    delete_key_core, generate_key_core, import_key_core, list_keys_core, SshKeyInfo,
};
use crate::notify::{load_webhook_config, save_webhook_config, WebhookConfig};
use crate::{
    connection::{connect_host, disconnect_host, list_active_connections},
    core::{
        add_host_core, apply_risk_override, delete_host_group_core, delete_proxy_core,
        exec_multi_core, exec_ssh_core, export_team_config, import_ssh_config_core,
        import_team_config, list_audit_core, list_host_groups_core, list_hosts_core,
        list_proxies_core, ping_hosts_core, remove_host_core, save_host_group_core,
        save_proxy_core, sftp_download_core_with_source, sftp_ls_core_with_source,
        sftp_mkdir_core_with_source, sftp_stat_core_with_source, sftp_upload_core_with_source,
        sftp_walk_core_with_source, update_host_core, ExecMultiRequest, ImportResult,
        TeamConfigExport,
    },
    diagnostics::{
        append_diagnostic_log, clear_diagnostic_logs as clear_diagnostic_logs_core,
        export_diagnostic_bundle as export_diagnostic_bundle_core,
        list_diagnostic_logs as list_diagnostic_logs_core, DiagnosticLogEntry,
    },
    execution_control::{
        append_rejected_exec_audit, authorize_command_with_approval, command_authorization_target,
        effective_command_risk, expand_exec_authorization_targets, CommandAuthorizationError,
        CommandAuthorizationInput,
    },
    forward::{forward_add_core, forward_list_core, forward_remove_core},
    playbook::{
        delete_playbook_core, dry_run_playbook, list_playbooks_core,
        run_playbook_core_with_source_and_approved_steps, save_playbook_core, Playbook,
        PlaybookRunResult,
    },
    remote::{list_daemons_core, DaemonInfo},
    session::{
        session_close_core, session_list_core, session_open_core, session_read_core,
        session_write_core,
    },
    store::append_audit,
    types::{
        source_from_env, AuditEntry, AuditFilter, ConnectionStatus, ExecMultiResult, ExecRequest,
        ExecResult, ForwardDirection, ForwardRule, HostGroup, HostProfile, PingResult,
        ProxyProfile, RiskLevel, SftpDownloadRequest, SftpExchangeRequest, SftpExchangeResult,
        SftpResult, SftpUploadRequest, WalkEntry,
    },
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Instant,
};
use tauri::{AppHandle, Manager, WindowEvent};
use tokio::sync::Mutex;
use uuid::Uuid;

mod mcp_agent_config;
pub use mcp_agent_config::{
    configure_mcp_agent, list_mcp_agent_configs, McpAgentConfigStatus, McpAgentConfigureResult,
};

static DESKTOP_SESSION_INPUT_BUFFERS: OnceLock<Mutex<HashMap<Uuid, String>>> = OnceLock::new();

fn desktop_session_input_buffers() -> &'static Mutex<HashMap<Uuid, String>> {
    DESKTOP_SESSION_INPUT_BUFFERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn append_operation_audit(
    source: &str,
    host: &str,
    command: &str,
    risk: RiskLevel,
    exit_code: Option<i32>,
    duration_ms: u128,
    reason: Option<&str>,
) {
    let result = ExecResult {
        host: host.to_string(),
        command: command.to_string(),
        exit_code,
        stdout: String::new(),
        stderr: reason.unwrap_or_default().to_string(),
        duration_ms,
        risk_level: risk,
        truncated: false,
    };
    let _ = append_audit(&result, risk, reason, None, Some(source));
}

#[derive(Debug, Clone, Serialize)]
pub struct DaemonControlResult {
    pub running: bool,
    pub pid: Option<u32>,
    pub message: String,
}

fn bundled_daemon_binary_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe
        .parent()
        .ok_or_else(|| "failed to resolve current executable directory".to_string())?;
    let candidate = dir.join(format!("agent2ssh-daemon{}", std::env::consts::EXE_SUFFIX));
    if candidate.exists() {
        return Ok(candidate);
    }
    Err(format!(
        "daemon sidecar not found at {}",
        candidate.display()
    ))
}

fn split_completed_session_commands(pending: &str, input: &str) -> (Vec<String>, String) {
    let mut combined = String::with_capacity(pending.len() + input.len());
    combined.push_str(pending);
    combined.push_str(input);

    let mut commands = Vec::new();
    let mut start = 0usize;
    for (idx, ch) in combined.char_indices() {
        if ch == '\n' || ch == '\r' {
            let command = combined[start..idx].trim();
            if !command.is_empty() {
                commands.push(command.to_string());
            }
            start = idx + ch.len_utf8();
        }
    }

    (commands, combined[start..].to_string())
}

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
pub fn update_host(original_name: String, host: HostProfile) -> Result<HostProfile, String> {
    update_host_core(&original_name, host).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_host(name: String) -> Result<(), String> {
    remove_host_core(&name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_host_groups() -> Result<Vec<HostGroup>, String> {
    list_host_groups_core().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_host_group(group: HostGroup) -> Result<HostGroup, String> {
    save_host_group_core(group).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_host_group(id: String) -> Result<bool, String> {
    delete_host_group_core(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_proxies() -> Result<Vec<ProxyProfile>, String> {
    list_proxies_core().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_proxy(proxy: ProxyProfile) -> Result<ProxyProfile, String> {
    save_proxy_core(proxy).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_proxy(id: String) -> Result<bool, String> {
    delete_proxy_core(&id).map_err(|e| e.to_string())
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
pub async fn classify_command_risk_for_host(
    command: String,
    host: Option<String>,
) -> Result<RiskLevel, String> {
    let host_override = host
        .as_deref()
        .and_then(|host| command_authorization_target(host).risk_override);
    Ok(apply_risk_override(
        effective_command_risk(&command).await,
        host_override,
    ))
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
) -> Result<Vec<String>, String> {
    let targets = expand_exec_authorization_targets(hosts, tags).map_err(|e| e.to_string())?;
    let auth_scope = None;
    let mut approved_hosts = Vec::new();
    for target in targets {
        let result = authorize_command_with_approval(
            CommandAuthorizationInput {
                auth_scope: &auth_scope,
                source,
                host: &target.host,
                tags: &target.tags,
                risk_override: target.risk_override,
                command,
                force,
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
            approved_hosts.push(target.host);
        }
    }
    Ok(approved_hosts)
}

async fn authorize_desktop_playbook_run(
    playbook: &str,
    host: &str,
    force: bool,
    params: &HashMap<String, String>,
    source: &str,
) -> Result<Vec<usize>, String> {
    let dry_run = dry_run_playbook(playbook, params).map_err(|e| e.to_string())?;
    let target = command_authorization_target(host);
    let playbook_risk_override = list_playbooks_core()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|item| item.name == playbook)
        .and_then(|item| item.risk_override);
    let risk_override = playbook_risk_override.or(target.risk_override);
    let auth_scope = None;
    let mut approved_steps = Vec::new();

    for step in dry_run.steps {
        let result = authorize_command_with_approval(
            CommandAuthorizationInput {
                auth_scope: &auth_scope,
                source,
                host,
                tags: &target.tags,
                risk_override,
                command: &step.command_resolved,
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
        if result.approved && result.risk == RiskLevel::High {
            approved_steps.push(step.step);
        }
    }

    Ok(approved_steps)
}

async fn authorize_desktop_operation(
    host: &str,
    command: &str,
    force: bool,
    source: &str,
) -> Result<RiskLevel, String> {
    let target = command_authorization_target(host);
    let auth_scope = None;
    let result = authorize_command_with_approval(
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
    Ok(result.risk)
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
    let approved_hosts =
        authorize_desktop_exec_targets(&hosts, &tags, &command, force, None, None, &source).await?;
    Ok(exec_multi_core(ExecMultiRequest {
        hosts,
        command,
        force,
        approved_hosts,
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

fn build_sftp_exchange_temp_path(source_path: &str) -> String {
    let source_file_name = Path::new(source_path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("agent2ssh-transfer.bin");
    let safe_file_name: String = source_file_name
        .chars()
        .map(|character| {
            if matches!(
                character,
                '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            ) {
                '_'
            } else {
                character
            }
        })
        .collect();
    let file_name = if safe_file_name.trim().is_empty() {
        "agent2ssh-transfer.bin".to_string()
    } else {
        safe_file_name
    };
    std::env::temp_dir()
        .join(format!("agent2ssh-sftp-{}-{}", Uuid::new_v4(), file_name))
        .display()
        .to_string()
}

// ── SFTP ─────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn sftp_upload(request: SftpUploadRequest) -> Result<SftpResult, String> {
    let source = source_from_env("desktop");
    let command = format!(
        "sftp upload {} -> {}",
        request.local_path, request.remote_path
    );
    authorize_desktop_operation(&request.host, &command, false, &source).await?;
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
    authorize_desktop_operation(&request.host, &command, false, &source).await?;
    sftp_download_core_with_source(request, Some(source))
        .await
        .map_err(|e| e.to_string())
}

/// K6: request cancellation of an in-flight SFTP transfer by its id. Returns
/// true if a matching transfer was registered (false if it already finished or
/// the id is unknown).
#[tauri::command]
pub async fn sftp_cancel(transfer_id: String) -> Result<bool, String> {
    Ok(crate::sftp_transfer::cancel_transfer(&transfer_id))
}

#[tauri::command]
pub async fn sftp_exchange(request: SftpExchangeRequest) -> Result<SftpExchangeResult, String> {
    let source = source_from_env("desktop");
    let local_temp = build_sftp_exchange_temp_path(&request.source_path);
    let download_command = format!("sftp download {} -> {}", request.source_path, local_temp);
    authorize_desktop_operation(&request.source_host, &download_command, false, &source).await?;

    let upload_command = format!("sftp upload {} -> {}", local_temp, request.destination_path);
    authorize_desktop_operation(&request.destination_host, &upload_command, false, &source).await?;

    let started = Instant::now();
    let downloaded = sftp_download_core_with_source(
        SftpDownloadRequest {
            host: request.source_host.clone(),
            remote_path: request.source_path.clone(),
            local_path: local_temp.clone(),
            resume: false,
            transfer_id: None,
        },
        Some(source.clone()),
    )
    .await
    .map_err(|e| {
        let _ = fs::remove_file(&local_temp);
        e.to_string()
    })?;

    let uploaded = sftp_upload_core_with_source(
        SftpUploadRequest {
            host: request.destination_host,
            local_path: local_temp.clone(),
            remote_path: request.destination_path,
            resume: false,
            transfer_id: None,
        },
        Some(source),
    )
    .await
    .map_err(|e| {
        let _ = fs::remove_file(&local_temp);
        e.to_string()
    })?;

    let _ = fs::remove_file(&local_temp);

    Ok(SftpExchangeResult {
        downloaded,
        uploaded,
        local_path: local_temp,
        duration_ms: started.elapsed().as_millis(),
    })
}

#[tauri::command]
pub async fn sftp_ls(
    host: String,
    path: String,
    timeout_secs: Option<u64>,
) -> Result<ExecResult, String> {
    let source = source_from_env("desktop");
    let command = format!("sftp ls {}", path);
    authorize_desktop_operation(&host, &command, false, &source).await?;
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
    authorize_desktop_operation(&host, &command, false, &source).await?;
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
    authorize_desktop_operation(&host, &command, false, &source).await?;
    sftp_mkdir_core_with_source(&host, &path, timeout_secs, Some(source))
        .await
        .map_err(|e| e.to_string())
}

/// One entry in a local filesystem directory listing for the desktop file
/// transfer panel.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LocalDirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    /// Last-modified time as a Unix epoch second, when available.
    pub modified_unix: Option<u64>,
}

/// A resolved local directory listing: the canonical path that was listed, its
/// parent (if any), the user's home directory (for a default/home shortcut), and
/// the sorted entries.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LocalDirListing {
    pub path: String,
    pub parent: Option<String>,
    pub home: String,
    pub entries: Vec<LocalDirEntry>,
}

fn expand_local_path(path: Option<String>) -> std::path::PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/"));
    let raw = path.unwrap_or_default();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return home;
    }
    if trimmed == "~" {
        return home;
    }
    // K7: accept both POSIX (`~/`) and Windows (`~\`) home-relative prefixes.
    if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        return home.join(rest);
    }
    std::path::PathBuf::from(trimmed)
}

fn local_ls_inner(path: Option<String>) -> anyhow::Result<LocalDirListing> {
    let requested = expand_local_path(path);
    // Canonicalize so the displayed path and parent navigation are stable; fall
    // back to the requested path if canonicalization fails (e.g. permissions).
    let dir = std::fs::canonicalize(&requested).unwrap_or(requested);
    let metadata = std::fs::metadata(&dir)
        .map_err(|e| anyhow::anyhow!("cannot access {}: {e}", dir.display()))?;
    if !metadata.is_dir() {
        return Err(anyhow::anyhow!("{} is not a directory", dir.display()));
    }

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&dir)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", dir.display()))?
    {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        // file_type() avoids following symlinks for the dir flag; size/mtime come
        // from metadata when reachable.
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        let (size, modified_unix) = match entry.metadata() {
            Ok(meta) => (
                meta.len(),
                meta.modified().ok().and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_secs())
                }),
            ),
            Err(_) => (0, None),
        };
        entries.push(LocalDirEntry {
            name,
            is_dir,
            size,
            modified_unix,
        });
    }
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/"));
    Ok(LocalDirListing {
        parent: dir.parent().map(|p| p.to_string_lossy().to_string()),
        path: dir.to_string_lossy().to_string(),
        home: home.to_string_lossy().to_string(),
        entries,
    })
}

/// List a local directory for the desktop file-transfer panel. `path` may use a
/// leading `~`; an empty/None path defaults to the user's home directory. This
/// makes the "this computer" side of the panel browsable just like a remote
/// host, so upload/download targets can be chosen by clicking instead of typing.
#[tauri::command]
pub async fn local_ls(path: Option<String>) -> Result<LocalDirListing, String> {
    local_ls_inner(path).map_err(|e| e.to_string())
}

/// Maximum recursion depth for a local directory walk — a hard guard against
/// symlink loops or pathologically deep trees. (J4)
const MAX_LOCAL_WALK_DEPTH: usize = 64;

fn local_walk_inner(
    root: &Path,
    rel_prefix: &str,
    depth: usize,
    out: &mut Vec<WalkEntry>,
) -> std::io::Result<()> {
    if depth >= MAX_LOCAL_WALK_DEPTH {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(root)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        // Skip symlinks: avoids loops and "open a link-to-dir as a file" errors.
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let rel = if rel_prefix.is_empty() {
            name.clone()
        } else {
            format!("{rel_prefix}/{name}")
        };
        let is_dir = file_type.is_dir();
        let size = if is_dir {
            0
        } else {
            entry.metadata().map(|m| m.len()).unwrap_or(0)
        };
        out.push(WalkEntry {
            rel_path: rel.clone(),
            is_dir,
            size,
        });
        if is_dir {
            local_walk_inner(&entry.path(), &rel, depth + 1, out)?;
        }
    }
    Ok(())
}

/// Recursively enumerate a local directory tree for the file-transfer panel
/// (J4). Returns descendants as `/`-joined paths relative to `root`, parents
/// before children, so the same layout can be recreated on a remote host.
#[tauri::command]
pub async fn local_walk(root: String) -> Result<Vec<WalkEntry>, String> {
    let path = expand_local_path(Some(root));
    let mut out = Vec::new();
    local_walk_inner(&path, "", 0, &mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Create a local directory (and parents) — used when recreating a remote
/// directory tree under a local download target. (J4)
#[tauri::command]
pub async fn local_mkdir(path: String) -> Result<(), String> {
    let path = expand_local_path(Some(path));
    fs::create_dir_all(&path).map_err(|e| e.to_string())
}

/// Recursively enumerate a remote directory tree over SFTP. (J4)
#[tauri::command]
pub async fn sftp_walk(host: String, root: String) -> Result<Vec<WalkEntry>, String> {
    let source = source_from_env("desktop");
    let command = format!("sftp walk {root}");
    authorize_desktop_operation(&host, &command, false, &source).await?;
    sftp_walk_core_with_source(&host, &root, None, Some(source))
        .await
        .map_err(|e| e.to_string())
}

// ── Sessions ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn session_open(host: String) -> Result<String, String> {
    let source = source_from_env("desktop");
    let risk = authorize_desktop_operation(&host, "session_open", false, &source).await?;
    let started = Instant::now();
    match session_open_core(&host).await {
        Ok(id) => {
            desktop_session_input_buffers()
                .lock()
                .await
                .insert(id, String::new());
            append_operation_audit(
                &source,
                &host,
                "session_open",
                risk,
                Some(0),
                started.elapsed().as_millis(),
                None,
            );
            Ok(id.to_string())
        }
        Err(e) => {
            let message = e.to_string();
            append_operation_audit(
                &source,
                &host,
                "session_open",
                risk,
                None,
                started.elapsed().as_millis(),
                Some(&message),
            );
            Err(message)
        }
    }
}

#[tauri::command]
pub async fn session_write(id: String, input: String, force: Option<bool>) -> Result<(), String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let source = source_from_env("desktop");
    let host = session_list_core()
        .await
        .into_iter()
        .find(|(session_id, _)| *session_id == uuid)
        .map(|(_, host)| host)
        .unwrap_or_else(|| format!("session:{id}"));
    let (completed_commands, next_pending) = {
        let buffers = desktop_session_input_buffers().lock().await;
        let pending = buffers.get(&uuid).cloned().unwrap_or_default();
        split_completed_session_commands(&pending, &input)
    };

    let mut completed_risks = Vec::new();
    for command in &completed_commands {
        let risk =
            authorize_desktop_operation(&host, command, force.unwrap_or(false), &source).await?;
        completed_risks.push((command.clone(), risk));
    }

    let started = Instant::now();
    match session_write_core(uuid, &input).await {
        Ok(()) => {
            desktop_session_input_buffers()
                .lock()
                .await
                .insert(uuid, next_pending);
            if completed_risks.is_empty() {
                append_operation_audit(
                    &source,
                    &host,
                    &format!("session write {} bytes", input.len()),
                    RiskLevel::Low,
                    Some(0),
                    started.elapsed().as_millis(),
                    None,
                );
            } else {
                for (command, risk) in &completed_risks {
                    append_operation_audit(
                        &source,
                        &host,
                        &format!("session command {command}"),
                        *risk,
                        Some(0),
                        started.elapsed().as_millis(),
                        None,
                    );
                }
            }
            Ok(())
        }
        Err(e) => {
            let message = e.to_string();
            append_operation_audit(
                &source,
                &host,
                &format!("session write {} bytes", input.len()),
                completed_risks
                    .iter()
                    .map(|(_, risk)| *risk)
                    .fold(RiskLevel::Low, RiskLevel::max_severity),
                None,
                started.elapsed().as_millis(),
                Some(&message),
            );
            Err(message)
        }
    }
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
    let source = source_from_env("desktop");
    let host = session_list_core()
        .await
        .into_iter()
        .find(|(session_id, _)| *session_id == uuid)
        .map(|(_, host)| host)
        .unwrap_or_else(|| format!("session:{id}"));
    let risk = authorize_desktop_operation(&host, "session_close", false, &source).await?;
    let started = Instant::now();
    match session_close_core(uuid).await {
        Ok(()) => {
            desktop_session_input_buffers().lock().await.remove(&uuid);
            append_operation_audit(
                &source,
                &host,
                "session_close",
                risk,
                Some(0),
                started.elapsed().as_millis(),
                None,
            );
            Ok(())
        }
        Err(e) => {
            let message = e.to_string();
            append_operation_audit(
                &source,
                &host,
                "session_close",
                risk,
                None,
                started.elapsed().as_millis(),
                Some(&message),
            );
            Err(message)
        }
    }
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
    let risk = authorize_desktop_operation(&host, &command, false, &source).await?;
    let started = Instant::now();
    match forward_add_core(&host, direction, bind_port, &target_host, target_port).await {
        Ok(rule) => {
            append_operation_audit(
                &source,
                &host,
                &command,
                risk,
                Some(0),
                started.elapsed().as_millis(),
                None,
            );
            Ok(rule)
        }
        Err(e) => {
            let message = e.to_string();
            append_operation_audit(
                &source,
                &host,
                &command,
                risk,
                None,
                started.elapsed().as_millis(),
                Some(&message),
            );
            Err(message)
        }
    }
}

#[tauri::command]
pub async fn forward_list() -> Result<Vec<ForwardRule>, String> {
    Ok(forward_list_core().await)
}

#[tauri::command]
pub async fn forward_remove(id: String) -> Result<(), String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let source = source_from_env("desktop");
    if let Some(rule) = forward_list_core()
        .await
        .into_iter()
        .find(|rule| rule.id == uuid)
    {
        let command = format!(
            "forward remove {} {}:{} -> {}:{}",
            rule.direction, rule.bind_port, rule.target_host, rule.host, rule.target_port
        );
        let risk = authorize_desktop_operation(&rule.host, &command, false, &source).await?;
        let started = Instant::now();
        return match forward_remove_core(uuid).await {
            Ok(()) => {
                append_operation_audit(
                    &source,
                    &rule.host,
                    &command,
                    risk,
                    Some(0),
                    started.elapsed().as_millis(),
                    None,
                );
                Ok(())
            }
            Err(e) => {
                let message = e.to_string();
                append_operation_audit(
                    &source,
                    &rule.host,
                    &command,
                    risk,
                    None,
                    started.elapsed().as_millis(),
                    Some(&message),
                );
                Err(message)
            }
        };
    }
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

#[tauri::command]
pub fn list_diagnostic_logs(limit: Option<usize>) -> Result<Vec<DiagnosticLogEntry>, String> {
    list_diagnostic_logs_core(limit.unwrap_or(200)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn write_diagnostic_log(
    level: String,
    component: String,
    message: String,
    fields: Option<Value>,
) -> Result<DiagnosticLogEntry, String> {
    append_diagnostic_log(&level, &component, &message, fields).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_diagnostic_logs() -> Result<(), String> {
    clear_diagnostic_logs_core().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn export_diagnostic_bundle() -> Result<String, String> {
    export_diagnostic_bundle_core()
        .map(|path| path.display().to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn daemon_status() -> Result<DaemonControlResult, String> {
    match crate::daemon_control::read_daemon_pid().map_err(|e| e.to_string())? {
        Some(pid)
            if crate::daemon_control::process_is_alive(pid)
                && crate::daemon_control::daemon_health_ok() =>
        {
            let _ = append_diagnostic_log(
                "debug",
                "tauri",
                "daemon status healthy",
                Some(serde_json::json!({ "pid": pid })),
            );
            Ok(DaemonControlResult {
                running: true,
                pid: Some(pid),
                message: format!("Daemon is running (pid={pid})."),
            })
        }
        Some(pid) => {
            crate::daemon_control::remove_daemon_pid_file();
            let _ = append_diagnostic_log(
                "warn",
                "tauri",
                "removed stale daemon pid file",
                Some(serde_json::json!({ "pid": pid })),
            );
            Ok(DaemonControlResult {
                running: false,
                pid: Some(pid),
                message: format!("Removed stale daemon PID file (pid={pid})."),
            })
        }
        None => {
            let _ = append_diagnostic_log("debug", "tauri", "daemon status not running", None);
            Ok(DaemonControlResult {
                running: false,
                pid: None,
                message: "Daemon is not running.".into(),
            })
        }
    }
}

#[tauri::command]
pub fn daemon_start(app: AppHandle) -> Result<DaemonControlResult, String> {
    let _ = app;
    if let Some(pid) = crate::daemon_control::read_daemon_pid().map_err(|e| e.to_string())? {
        if crate::daemon_control::process_is_alive(pid) && crate::daemon_control::daemon_health_ok()
        {
            let _ = append_diagnostic_log(
                "info",
                "tauri",
                "daemon start skipped because daemon is already healthy",
                Some(serde_json::json!({ "pid": pid })),
            );
            return Ok(DaemonControlResult {
                running: true,
                pid: Some(pid),
                message: format!("Daemon is already running (pid={pid})."),
            });
        }
        let _ = append_diagnostic_log(
            "warn",
            "tauri",
            "daemon start removing stale pid",
            Some(serde_json::json!({ "pid": pid })),
        );
        crate::daemon_control::remove_daemon_pid_file();
    }

    let daemon_bin = bundled_daemon_binary_path()?;
    let started =
        crate::daemon_control::start_daemon_background(&daemon_bin).map_err(|e| e.to_string())?;
    let _ = append_diagnostic_log(
        "info",
        "tauri",
        "daemon start succeeded",
        Some(serde_json::json!({
            "pid": started.pid,
            "log_path": started.log_path.display().to_string(),
        })),
    );

    Ok(DaemonControlResult {
        running: true,
        pid: Some(started.pid),
        message: format!(
            "Daemon started (pid={}); log: {}.",
            started.pid,
            started.log_path.display()
        ),
    })
}

#[tauri::command]
pub fn daemon_stop() -> Result<DaemonControlResult, String> {
    match crate::daemon_control::read_daemon_pid().map_err(|e| e.to_string())? {
        Some(pid) if crate::daemon_control::process_is_alive(pid) => {
            crate::daemon_control::terminate_process(pid).map_err(|e| e.to_string())?;
            crate::daemon_control::remove_daemon_pid_file();
            let _ = append_diagnostic_log(
                "info",
                "tauri",
                "daemon stopped",
                Some(serde_json::json!({ "pid": pid })),
            );
            Ok(DaemonControlResult {
                running: false,
                pid: Some(pid),
                message: format!("Daemon stopped (pid={pid})."),
            })
        }
        Some(pid) => {
            crate::daemon_control::remove_daemon_pid_file();
            let _ = append_diagnostic_log(
                "warn",
                "tauri",
                "daemon stop removed stale pid",
                Some(serde_json::json!({ "pid": pid })),
            );
            Ok(DaemonControlResult {
                running: false,
                pid: Some(pid),
                message: format!("Removed stale daemon PID file (pid={pid})."),
            })
        }
        None => {
            let _ =
                append_diagnostic_log("info", "tauri", "daemon stop skipped; not running", None);
            Ok(DaemonControlResult {
                running: false,
                pid: None,
                message: "Daemon is not running.".into(),
            })
        }
    }
}

#[tauri::command]
pub fn daemon_restart(app: AppHandle) -> Result<DaemonControlResult, String> {
    if let Some(pid) = crate::daemon_control::read_daemon_pid().map_err(|e| e.to_string())? {
        if crate::daemon_control::process_is_alive(pid) {
            crate::daemon_control::terminate_process(pid).map_err(|e| e.to_string())?;
            let _ = append_diagnostic_log(
                "info",
                "tauri",
                "daemon restart terminated existing process",
                Some(serde_json::json!({ "pid": pid })),
            );
        }
        crate::daemon_control::remove_daemon_pid_file();
    }
    daemon_start(app)
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    let _ = daemon_stop();
    app.exit(0);
}

const TRAY_ID: &str = "agent2ssh-tray";
const TRAY_MENU_OPEN_ID: &str = "tray-open";
const TRAY_MENU_QUIT_ID: &str = "tray-quit";

fn reveal_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn quit_from_tray(app: &AppHandle) {
    let _ = daemon_stop();
    app.exit(0);
}

fn build_system_tray(
    app: &AppHandle,
    open_label: &str,
    quit_label: &str,
    tooltip: &str,
) -> Result<(), String> {
    let tray_menu = tauri::menu::Menu::with_items(
        app,
        &[
            &tauri::menu::MenuItemBuilder::with_id(TRAY_MENU_OPEN_ID, open_label)
                .build(app)
                .map_err(|e| e.to_string())?,
            &tauri::menu::MenuItemBuilder::with_id(TRAY_MENU_QUIT_ID, quit_label)
                .build(app)
                .map_err(|e| e.to_string())?,
        ],
    )
    .map_err(|e| e.to_string())?;

    let mut tray_builder = tauri::tray::TrayIconBuilder::with_id(TRAY_ID)
        .menu(&tray_menu)
        .tooltip(tooltip)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            if event.id().as_ref() == TRAY_MENU_OPEN_ID {
                reveal_main_window(&app.app_handle());
            }

            if event.id().as_ref() == TRAY_MENU_QUIT_ID {
                quit_from_tray(&app.app_handle());
            }
        })
        .on_tray_icon_event(|app, event| match event {
            tauri::tray::TrayIconEvent::Click { button, .. }
                if button == tauri::tray::MouseButton::Left =>
            {
                reveal_main_window(&app.app_handle());
            }
            tauri::tray::TrayIconEvent::DoubleClick { .. } => {
                reveal_main_window(&app.app_handle());
            }
            _ => {}
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray_builder = tray_builder.icon(icon);
    }

    let _tray = tray_builder.build(app).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn set_tray_labels(
    app: AppHandle,
    open_label: String,
    quit_label: String,
    tooltip: Option<String>,
) {
    let _ = app.remove_tray_by_id(TRAY_ID);
    let _ = build_system_tray(
        &app,
        &open_label,
        &quit_label,
        tooltip.as_deref().unwrap_or("Agent2SSH"),
    );
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

// ── Embedded SSH connection retention ───────────────────────────────────────

#[tauri::command]
pub async fn connection_status() -> Result<Vec<ConnectionStatus>, String> {
    Ok(list_active_connections().await)
}

/// K10: read the opt-in telemetry setting (off by default, local-only).
#[tauri::command]
pub async fn get_telemetry_enabled() -> Result<bool, String> {
    Ok(crate::telemetry::telemetry_enabled())
}

/// K10: toggle the opt-in telemetry setting.
#[tauri::command]
pub async fn set_telemetry_enabled(enabled: bool) -> Result<(), String> {
    crate::telemetry::save_telemetry_config(enabled).map_err(|e| e.to_string())
}

/// K1: state of the app-managed credential store for the desktop unlock flow.
#[derive(Serialize)]
pub struct SecretsStatus {
    /// A master password has been set (the encrypted store exists).
    pub initialized: bool,
    /// The store is unlocked in this process.
    pub unlocked: bool,
}

/// K1: report whether the credential store is initialized/unlocked, so the
/// desktop knows whether to show an unlock dialog, a "set master password"
/// prompt, or nothing.
#[tauri::command]
pub async fn secrets_status() -> Result<SecretsStatus, String> {
    Ok(SecretsStatus {
        initialized: crate::secrets::is_initialized(),
        unlocked: crate::secrets::is_unlocked(),
    })
}

/// K1: unlock the credential store (or initialize it with `password` if no master
/// password has been set yet), then migrate any leftover plaintext into it.
#[tauri::command]
pub async fn secrets_unlock(password: String) -> Result<(), String> {
    crate::secrets::unlock_or_init(&password).map_err(|e| e.to_string())?;
    // Now that we have a key, encrypt any plaintext passwords still on disk.
    if let Err(e) = crate::store::migrate_plaintext_secrets() {
        eprintln!("warning: secret migration after unlock failed: {e}");
    }
    Ok(())
}

/// K1: change the master password (re-encrypt the store under a new key).
/// Requires the store to be unlocked.
#[tauri::command]
pub async fn secrets_change_password(new_password: String) -> Result<(), String> {
    crate::secrets::change_master_password(&new_password).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ssh_connect(host: String) -> Result<(), String> {
    let source = source_from_env("desktop");
    let risk = authorize_desktop_operation(&host, "connect", false, &source).await?;
    let started = Instant::now();
    match connect_host(&host).await {
        Ok(()) => {
            append_operation_audit(
                &source,
                &host,
                "connect",
                risk,
                Some(0),
                started.elapsed().as_millis(),
                None,
            );
            Ok(())
        }
        Err(e) => {
            let message = e.to_string();
            append_operation_audit(
                &source,
                &host,
                "connect",
                risk,
                None,
                started.elapsed().as_millis(),
                Some(&message),
            );
            Err(message)
        }
    }
}

#[tauri::command]
pub async fn ssh_disconnect(host: String) -> Result<(), String> {
    let source = source_from_env("desktop");
    let risk = authorize_desktop_operation(&host, "disconnect", false, &source).await?;
    let started = Instant::now();
    match disconnect_host(&host).await {
        Ok(()) => {
            append_operation_audit(
                &source,
                &host,
                "disconnect",
                risk,
                Some(0),
                started.elapsed().as_millis(),
                None,
            );
            Ok(())
        }
        Err(e) => {
            let message = e.to_string();
            append_operation_audit(
                &source,
                &host,
                "disconnect",
                risk,
                None,
                started.elapsed().as_millis(),
                Some(&message),
            );
            Err(message)
        }
    }
}

// ── Playbooks ────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_playbooks() -> Result<Vec<Playbook>, String> {
    list_playbooks_core().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_playbook(playbook: Playbook) -> Result<Playbook, String> {
    save_playbook_core(playbook).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_playbook(name: String) -> Result<bool, String> {
    delete_playbook_core(&name).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn run_playbook(
    playbook: String,
    host: String,
    force: bool,
) -> Result<PlaybookRunResult, String> {
    let source = source_from_env("desktop");
    let params = HashMap::new();
    let approved_steps =
        authorize_desktop_playbook_run(&playbook, &host, force, &params, &source).await?;
    run_playbook_core_with_source_and_approved_steps(
        &playbook,
        &host,
        force,
        None,
        None,
        None,
        Some(source),
        &approved_steps,
    )
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
    crate::diagnostics::install_panic_hook("tauri");
    // K1: migrate any legacy plaintext passwords into the app-managed encrypted store (no-op once clean).
    if let Err(e) = crate::store::migrate_plaintext_secrets() {
        eprintln!("warning: secret migration skipped: {e}");
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        // K3: in-app updater (signature-verified). Endpoints + pubkey come from
        // tauri.conf.json; the JS `@tauri-apps/plugin-updater` API drives the
        // check/download/install flow from Settings.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            build_system_tray(app.handle(), "Open", "Quit", "Agent2SSH")?;
            Ok(())
        })
        .on_window_event(|app, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.minimize();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            // Host management
            list_hosts,
            add_host,
            update_host,
            remove_host,
            list_host_groups,
            save_host_group,
            delete_host_group,
            list_proxies,
            save_proxy,
            delete_proxy,
            import_ssh_config,
            // Execution
            classify_command_risk,
            classify_command_risk_for_host,
            exec_ssh,
            exec_multi,
            ping_hosts,
            // SFTP
            sftp_upload,
            sftp_download,
            sftp_cancel,
            sftp_exchange,
            sftp_ls,
            sftp_stat,
            sftp_mkdir,
            local_ls,
            local_walk,
            local_mkdir,
            sftp_walk,
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
            daemon_status,
            daemon_start,
            daemon_stop,
            daemon_restart,
            quit_app,
            set_tray_labels,
            mcp_agent_config::list_mcp_agent_configs,
            mcp_agent_config::configure_mcp_agent,
            // Diagnostics
            list_diagnostic_logs,
            write_diagnostic_log,
            clear_diagnostic_logs,
            export_diagnostic_bundle,
            // SSH Keys
            list_keys,
            generate_key,
            import_key,
            delete_key,
            // Embedded SSH connection retention
            connection_status,
            ssh_connect,
            ssh_disconnect,
            // Opt-in telemetry (K10)
            get_telemetry_enabled,
            set_telemetry_enabled,
            // App-managed credential store (K1)
            secrets_status,
            secrets_unlock,
            secrets_change_password,
            // Playbooks
            list_playbooks,
            save_playbook,
            delete_playbook,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_local_path_handles_tilde_and_empty() {
        let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/"));
        assert_eq!(expand_local_path(None), home);
        assert_eq!(expand_local_path(Some("  ".into())), home);
        assert_eq!(expand_local_path(Some("~".into())), home);
        assert_eq!(expand_local_path(Some("~/sub".into())), home.join("sub"));
        assert_eq!(
            expand_local_path(Some("/etc".into())),
            std::path::PathBuf::from("/etc")
        );
    }

    #[test]
    fn local_ls_inner_lists_dirs_first_with_metadata() {
        let dir = std::env::temp_dir().join(format!("a2s-localls-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("zsub")).unwrap();
        std::fs::write(dir.join("afile.txt"), b"hello").unwrap();

        let listing = local_ls_inner(Some(dir.to_string_lossy().to_string())).unwrap();
        // Directory is sorted ahead of the file even though its name sorts later.
        assert_eq!(listing.entries.len(), 2);
        assert!(listing.entries[0].is_dir && listing.entries[0].name == "zsub");
        let file = &listing.entries[1];
        assert!(!file.is_dir && file.name == "afile.txt");
        assert_eq!(file.size, 5);
        assert!(file.modified_unix.is_some());
        assert!(listing.parent.is_some());
        assert!(!listing.home.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_walk_inner_enumerates_tree_parents_first() {
        let dir = std::env::temp_dir().join(format!("a2s-walk-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("sub/deep")).unwrap();
        std::fs::write(dir.join("root.txt"), b"abc").unwrap();
        std::fs::write(dir.join("sub/inner.txt"), b"hello").unwrap();
        std::fs::write(dir.join("sub/deep/leaf.txt"), b"xy").unwrap();

        let mut out = Vec::new();
        local_walk_inner(&dir, "", 0, &mut out).unwrap();
        let rels: Vec<&str> = out.iter().map(|e| e.rel_path.as_str()).collect();

        // Files and dirs present with `/`-joined relative paths.
        assert!(rels.contains(&"root.txt"));
        assert!(rels.contains(&"sub"));
        assert!(rels.contains(&"sub/inner.txt"));
        assert!(rels.contains(&"sub/deep"));
        assert!(rels.contains(&"sub/deep/leaf.txt"));

        // A parent directory always appears before its children.
        let pos = |p: &str| rels.iter().position(|r| *r == p).unwrap();
        assert!(pos("sub") < pos("sub/inner.txt"));
        assert!(pos("sub") < pos("sub/deep"));
        assert!(pos("sub/deep") < pos("sub/deep/leaf.txt"));

        // Sizes are reported for files; directories report 0.
        let leaf = out
            .iter()
            .find(|e| e.rel_path == "sub/deep/leaf.txt")
            .unwrap();
        assert_eq!(leaf.size, 2);
        assert!(!leaf.is_dir);
        assert!(out.iter().find(|e| e.rel_path == "sub").unwrap().is_dir);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_ls_inner_rejects_a_file_path() {
        let dir = std::env::temp_dir().join(format!("a2s-localls-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("f.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(local_ls_inner(Some(file.to_string_lossy().to_string())).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
