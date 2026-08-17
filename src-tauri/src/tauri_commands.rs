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
        sftp_mkdir_core_with_source, sftp_read_text_core_with_source, sftp_stat_core_with_source,
        sftp_upload_core_with_source, sftp_walk_core_with_source, update_host_core,
        validate_command_length, ExecMultiRequest, ImportResult, TeamConfigExport,
        MAX_COMMAND_BYTES,
    },
    diagnostics::{
        append_diagnostic_log, clear_diagnostic_logs as clear_diagnostic_logs_core,
        export_diagnostic_bundle as export_diagnostic_bundle_core,
        generate_system_report as generate_system_report_core,
        list_diagnostic_logs as list_diagnostic_logs_core, DiagnosticLogEntry,
    },
    embedded_ssh::{
        get_host_fingerprint_status_core, import_known_hosts_from_ssh, trust_host_fingerprint_core,
        HostFingerprintStatus, KnownHostImportSummary,
    },
    execution_control::{
        append_rejected_exec_audit, authorize_command_with_approval, command_authorization_target,
        effective_command_risk, expand_exec_authorization_targets, CommandAuthorizationError,
        CommandAuthorizationInput,
    },
    forward::{
        forward_add_core_via, forward_list_core, forward_remove_core, forward_start_core,
        forward_stats_core, forward_stop_core, RuleStats,
    },
    playbook::{
        delete_playbook_core, dry_run_playbook, list_playbooks_core,
        run_playbook_core_with_source_and_approved_steps, save_playbook_core, Playbook,
        PlaybookRunResult,
    },
    recording::{
        delete_recording as delete_recording_core, list_recordings as list_recordings_core,
        load_recording_config, read_recording as read_recording_core, save_recording_config,
        RecordingConfig, RecordingContent, RecordingInfo,
    },
    remote::{list_daemons_core, DaemonInfo},
    session::{
        session_close_core, session_list_core, session_open_core, session_read_core,
        session_write_core,
    },
    snippets::{add_snippet, load_snippets, remove_snippet, Snippet},
    store::{append_audit, config_dir, lock_config_file, restrict_file_to_owner},
    types::{
        source_from_transport, AuditEntry, AuditFilter, ConnectionStatus, ExecMultiResult,
        ExecRequest, ExecResult, ForwardDirection, ForwardRule, HostGroup, HostProfile, PingResult,
        ProxyProfile, RiskLevel, SftpDownloadRequest, SftpExchangeRequest, SftpExchangeResult,
        SftpResult, SftpUploadRequest, WalkEntry,
    },
    webdav_sync::{
        apply_config_template, create_named_snapshot, delete_config_snapshot,
        list_config_snapshots, restore_config_snapshot, ConfigSnapshotInfo,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(windows)]
use std::process::Command;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};
use tauri::{AppHandle, Manager, WindowEvent};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::app_state::app_state;
#[cfg(not(windows))]
use crate::shell_profile::{build_shell_profile_line, SHELL_PROFILE_SENTINEL};

mod mcp_agent_config;
pub use mcp_agent_config::{
    agent_skill_status, configure_mcp_agent, install_agent_skill, list_mcp_agent_configs,
    uninstall_agent_skill, uninstall_mcp_agent, AgentSkillStatus, McpAgentConfigStatus,
    McpAgentConfigureResult, McpAgentUninstallResult,
};

fn desktop_session_input_buffers() -> &'static Mutex<HashMap<Uuid, String>> {
    &app_state().desktop_session_buffers
}

fn ensure_command_length(command: &str) -> Result<(), String> {
    validate_command_length(command)
        .map_err(|_| format!("command length exceeds maximum of {MAX_COMMAND_BYTES} bytes"))?;
    Ok(())
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
        dropped_bytes: 0,
        side_effect: None,
    };
    let _ = append_audit(&result, risk, reason, None, Some(source));
}

#[derive(Debug, Clone, Serialize)]
pub struct DaemonControlResult {
    pub running: bool,
    pub pid: Option<u32>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliPathStatus {
    pub cli_dir: String,
    pub cli_path: String,
    pub mcp_path: String,
    pub cli_exists: bool,
    pub mcp_exists: bool,
    pub in_process_path: bool,
    pub in_user_path: bool,
    pub installed: bool,
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

fn bundled_cli_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe
        .parent()
        .ok_or_else(|| "failed to resolve current executable directory".to_string())?;
    let cli = dir.join(format!("agent2ssh{}", std::env::consts::EXE_SUFFIX));
    let mcp = dir.join(format!("agent2ssh-mcp{}", std::env::consts::EXE_SUFFIX));
    if cli.exists() || mcp.exists() {
        return Ok(dir.to_path_buf());
    }
    Err(format!(
        "agent2ssh CLI binaries not found near {}",
        exe.display()
    ))
}

fn normalize_path_segment(path: &Path) -> String {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    path.to_string_lossy()
        .trim()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

fn path_contains_dir(raw_path: &str, dir: &Path) -> bool {
    let expected = normalize_path_segment(dir);
    raw_path
        .split(';')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .any(|segment| normalize_path_segment(Path::new(segment)) == expected)
}

fn current_process_path_contains(dir: &Path) -> bool {
    std::env::var("PATH")
        .map(|path| path_contains_dir(&path, dir))
        .unwrap_or(false)
}

#[cfg(windows)]
fn read_user_path_value() -> Result<String, String> {
    let output = Command::new("reg.exe")
        .args(["query", "HKCU\\Environment", "/v", "Path"])
        .output()
        .map_err(|e| format!("failed to query user PATH: {e}"))?;
    if !output.status.success() {
        return Ok(String::new());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim_start();
        if !trimmed
            .get(..4)
            .map(|prefix| prefix.eq_ignore_ascii_case("Path"))
            .unwrap_or(false)
        {
            continue;
        }
        let mut parts = trimmed
            .splitn(3, char::is_whitespace)
            .filter(|part| !part.is_empty());
        let _name = parts.next();
        let value_type = parts.next();
        let value = parts.next();
        if matches!(value_type, Some("REG_SZ") | Some("REG_EXPAND_SZ")) {
            return Ok(value.unwrap_or_default().trim().to_string());
        }
    }
    Ok(String::new())
}

#[cfg(not(windows))]
fn read_user_path_value() -> Result<String, String> {
    // B51: On Linux/macOS, the "user PATH" is assembled from shell profile files.
    // We read the current process PATH as a baseline, which reflects the
    // effective PATH after all profile scripts have run.
    Ok(std::env::var("PATH").unwrap_or_default())
}

#[cfg(not(windows))]
fn write_user_path_value(_path_value: &str) -> Result<(), String> {
    // B51: On non-Windows, we don't write a single PATH string to a registry.
    // Instead, install_cli_to_path / remove_cli_from_path use shell profile
    // modification directly via append_shell_profile_entry / remove_shell_profile_entry.
    // This function is kept for API compatibility but should not be called
    // on non-Windows. If it is, return an informative error.
    Err("Use install_cli_to_path / remove_cli_from_path on non-Windows".to_string())
}

#[cfg(windows)]
fn write_user_path_value(path_value: &str) -> Result<(), String> {
    let status = Command::new("reg.exe")
        .args([
            "add",
            "HKCU\\Environment",
            "/v",
            "Path",
            "/t",
            "REG_EXPAND_SZ",
            "/d",
            path_value,
            "/f",
        ])
        .status()
        .map_err(|e| format!("failed to write user PATH: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("reg.exe failed to write user PATH: {status}"))
    }
}

fn append_user_path_dir(path_value: &str, dir: &Path) -> String {
    let dir = dir.to_string_lossy();
    let trimmed = path_value.trim().trim_end_matches(';');
    if trimmed.is_empty() {
        dir.to_string()
    } else {
        format!("{trimmed};{dir}")
    }
}

fn remove_user_path_dir(path_value: &str, dir: &Path) -> String {
    let expected = normalize_path_segment(dir);
    path_value
        .split(';')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .filter(|segment| normalize_path_segment(Path::new(segment)) != expected)
        .collect::<Vec<_>>()
        .join(";")
}

fn cli_path_status_with_message(message: String) -> Result<CliPathStatus, String> {
    let cli_dir = bundled_cli_dir()?;
    let cli_path = cli_dir.join(format!("agent2ssh{}", std::env::consts::EXE_SUFFIX));
    let mcp_path = cli_dir.join(format!("agent2ssh-mcp{}", std::env::consts::EXE_SUFFIX));
    let user_path = read_user_path_value()?;
    let in_user_path = path_contains_dir(&user_path, &cli_dir);
    let in_process_path = current_process_path_contains(&cli_dir);
    let cli_exists = cli_path.exists();
    let mcp_exists = mcp_path.exists();

    // B51: On non-Windows, also check if the shell profile has the entry,
    // since the process PATH may not reflect newly added profile lines.
    #[cfg(not(windows))]
    let installed =
        cli_exists && mcp_exists && (in_user_path || in_process_path || shell_profile_has_entry());
    #[cfg(windows)]
    let installed = cli_exists && mcp_exists && in_user_path;

    Ok(CliPathStatus {
        cli_dir: cli_dir.display().to_string(),
        cli_path: cli_path.display().to_string(),
        mcp_path: mcp_path.display().to_string(),
        cli_exists,
        mcp_exists,
        in_process_path,
        in_user_path,
        installed,
        message,
    })
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
    list_hosts_core()
        .map(|hosts| {
            hosts
                .into_iter()
                .map(HostProfile::redacted_for_transport)
                .collect()
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_host(host: HostProfile) -> Result<HostProfile, String> {
    add_host_core(host)
        .map(HostProfile::redacted_for_transport)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_host(original_name: String, host: HostProfile) -> Result<HostProfile, String> {
    update_host_core(&original_name, host)
        .map(HostProfile::redacted_for_transport)
        .map_err(|e| e.to_string())
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
    ensure_command_length(&command)?;
    Ok(effective_command_risk(&command).await)
}

#[tauri::command]
pub async fn classify_command_risk_for_host(
    command: String,
    host: Option<String>,
) -> Result<RiskLevel, String> {
    ensure_command_length(&command)?;
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
        request.source = Some(source_from_transport());
    }
    ensure_command_length(&request.command)?;
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
            side_effect: request.side_effect.clone(),
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
                side_effect: None,
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
                side_effect: None,
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
            side_effect: None,
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
    ensure_command_length(&command)?;
    let source = source_from_transport();
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
    let source = source_from_transport();
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
    let source = source_from_transport();
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
    let source = source_from_transport();
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
            max_mb: None,
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
    let source = source_from_transport();
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
    let source = source_from_transport();
    let command = format!("sftp stat {}", path);
    authorize_desktop_operation(&host, &command, false, &source).await?;
    sftp_stat_core_with_source(&host, &path, timeout_secs, Some(source))
        .await
        .map_err(|e| e.to_string())
}

/// V3-1: read a remote file's content for the SFTP panel's inline text preview.
/// Callers gate this to files already known (from the directory listing) to be
/// small; the core function still enforces its own byte cap and rejects
/// non-UTF-8 content so a preview attempt on a binary/oversized file fails
/// cleanly instead of returning junk.
#[tauri::command]
pub async fn sftp_read_text(
    host: String,
    path: String,
    timeout_secs: Option<u64>,
) -> Result<ExecResult, String> {
    let source = source_from_transport();
    let command = format!("sftp read {}", path);
    authorize_desktop_operation(&host, &command, false, &source).await?;
    sftp_read_text_core_with_source(&host, &path, timeout_secs, Some(source))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sftp_mkdir(
    host: String,
    path: String,
    timeout_secs: Option<u64>,
) -> Result<ExecResult, String> {
    let source = source_from_transport();
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

// V3-1: same bound as the remote SFTP preview (core.rs::SFTP_PREVIEW_MAX_BYTES) —
// kept as a separate local constant since local reads don't go through core.rs.
const LOCAL_PREVIEW_MAX_BYTES: u64 = 1_048_577;

fn local_read_text_inner(path: String) -> anyhow::Result<String> {
    use std::io::Read;
    let resolved = expand_local_path(Some(path));
    let file = fs::File::open(&resolved)
        .map_err(|e| anyhow::anyhow!("cannot open {}: {e}", resolved.display()))?;
    let mut buf = Vec::new();
    file.take(LOCAL_PREVIEW_MAX_BYTES)
        .read_to_end(&mut buf)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", resolved.display()))?;
    if buf.len() as u64 >= LOCAL_PREVIEW_MAX_BYTES {
        return Err(anyhow::anyhow!(
            "file exceeds the {LOCAL_PREVIEW_MAX_BYTES}-byte preview limit"
        ));
    }
    String::from_utf8(buf).map_err(|_| anyhow::anyhow!("file is not valid UTF-8 text"))
}

/// V3-1: read a local file's content for the SFTP panel's inline text preview.
/// See `local_read_text_inner` for the size/UTF-8 gating.
#[tauri::command]
pub async fn local_read_text(path: String) -> Result<String, String> {
    local_read_text_inner(path).map_err(|e| e.to_string())
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
    let source = source_from_transport();
    let command = format!("sftp walk {root}");
    authorize_desktop_operation(&host, &command, false, &source).await?;
    sftp_walk_core_with_source(&host, &root, None, Some(source))
        .await
        .map_err(|e| e.to_string())
}

// ── Sessions ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn session_open(host: String) -> Result<String, String> {
    let source = source_from_transport();
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
    let source = source_from_transport();
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
    let source = source_from_transport();
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
    via: Option<String>,
) -> Result<ForwardRule, String> {
    let source = source_from_transport();
    let command = format!(
        "forward {} {}:{} -> {}:{}",
        direction, bind_port, target_host, host, target_port
    );
    let risk = authorize_desktop_operation(&host, &command, false, &source).await?;
    let started = Instant::now();
    match forward_add_core_via(
        &host,
        direction,
        bind_port,
        &target_host,
        target_port,
        via.as_deref(),
    )
    .await
    {
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
pub async fn forward_stats() -> Result<std::collections::HashMap<Uuid, RuleStats>, String> {
    Ok(forward_stats_core().await)
}

#[tauri::command]
pub async fn forward_remove(id: String) -> Result<(), String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let source = source_from_transport();
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

/// Finding 17: Stop a single forward rule by ID without removing it.
#[tauri::command]
pub async fn forward_stop(id: String) -> Result<(), String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let source = source_from_transport();
    if let Some(rule) = forward_list_core()
        .await
        .into_iter()
        .find(|rule| rule.id == uuid)
    {
        let command = format!(
            "forward stop {} {}:{} -> {}:{}",
            rule.direction, rule.bind_port, rule.target_host, rule.host, rule.target_port
        );
        let risk = authorize_desktop_operation(&rule.host, &command, false, &source).await?;
        let started = Instant::now();
        return match forward_stop_core(uuid).await {
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
    forward_stop_core(uuid).await.map_err(|e| e.to_string())
}

/// Finding 17: Restart a previously stopped forward rule by ID.
#[tauri::command]
pub async fn forward_start(id: String) -> Result<(), String> {
    let uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let source = source_from_transport();
    if let Some(rule) = forward_list_core()
        .await
        .into_iter()
        .find(|rule| rule.id == uuid)
    {
        let command = format!(
            "forward start {} {}:{} -> {}:{}",
            rule.direction, rule.bind_port, rule.target_host, rule.host, rule.target_port
        );
        let risk = authorize_desktop_operation(&rule.host, &command, false, &source).await?;
        let started = Instant::now();
        return match forward_start_core(uuid).await {
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
    forward_start_core(uuid).await.map_err(|e| e.to_string())
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
pub fn generate_system_report() -> Result<serde_json::Value, String> {
    generate_system_report_core().map_err(|e| e.to_string())
}

/// B43: Store a passphrase in the in-memory cache for a host.
#[tauri::command]
pub fn passphrase_cache_set(host_name: String, passphrase: String) -> Result<(), String> {
    let cache_key = format!("host:{host_name}");
    crate::embedded_ssh::passphrase_cache_set(&cache_key, &passphrase);
    Ok(())
}

/// B43: Evict a single host's passphrase from the cache.
#[tauri::command]
pub fn passphrase_cache_evict(host_name: String) -> Result<(), String> {
    let cache_key = format!("host:{host_name}");
    crate::embedded_ssh::passphrase_cache_evict(&cache_key);
    Ok(())
}

/// B43: Clear the entire passphrase cache (called on secrets lock).
#[tauri::command]
pub fn passphrase_cache_clear() -> Result<(), String> {
    crate::embedded_ssh::passphrase_cache_clear();
    Ok(())
}

/// G4: Redact sensitive text before it is placed on the clipboard, so a
/// copied command block never leaks tokens/keys. Reuses the same
/// `copy_redact_rules.json` rule set as exec/audit/export redaction.
#[tauri::command]
pub fn redact_for_clipboard(text: String) -> String {
    crate::copy_redact::redact_for_clipboard(&text)
}

/// B33: Discover Docker containers and Kubernetes pods that can be used
/// as exec targets.
#[tauri::command]
pub fn discover_containers(
) -> Result<Vec<crate::container_discovery::ContainerDiscoveryTarget>, String> {
    crate::container_discovery::discover_containers().map_err(|e| e.to_string())
}

/// Enumerate system fonts for terminal font selection.
#[tauri::command]
pub fn list_fonts() -> Result<Vec<crate::font_list::FontInfo>, String> {
    Ok(crate::font_list::list_fonts())
}

/// Enumerate locally installed shells.
#[tauri::command]
pub fn list_shells() -> Result<Vec<crate::shell_list::ShellInfo>, String> {
    Ok(crate::shell_list::list_shells())
}

/// Get the default shell for the current system.
#[tauri::command]
pub fn default_shell() -> Result<Option<crate::shell_list::ShellInfo>, String> {
    Ok(crate::shell_list::default_shell())
}

/// B24: List all terminal highlight rules.
#[tauri::command]
pub fn list_highlights() -> Result<Vec<crate::types::HighlightRule>, String> {
    Ok(crate::highlight::list_rules())
}

/// B24: Add a new highlight rule.
#[tauri::command]
pub fn add_highlight(
    rule: crate::types::HighlightRule,
) -> Result<Vec<crate::types::HighlightRule>, String> {
    crate::highlight::insert_rule(rule).map_err(|e| e.to_string())
}

/// B24: Remove a highlight rule by keyword.
#[tauri::command]
pub fn remove_highlight(keyword: String) -> Result<Vec<crate::types::HighlightRule>, String> {
    crate::highlight::delete_rule(&keyword).map_err(|e| e.to_string())
}

/// B24: Update an existing highlight rule. `old_keyword` identifies the rule.
#[tauri::command]
pub fn update_highlight(
    old_keyword: String,
    rule: crate::types::HighlightRule,
) -> Result<Vec<crate::types::HighlightRule>, String> {
    crate::highlight::update_rule(&old_keyword, rule).map_err(|e| e.to_string())
}

/// B24: Reset all highlight rules to defaults.
#[tauri::command]
pub fn reset_highlights() -> Result<Vec<crate::types::HighlightRule>, String> {
    crate::highlight::reset_defaults().map_err(|e| e.to_string())
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

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseWindowAction {
    MinimizeToTray,
    QuitApplication,
}

fn default_close_window_action() -> CloseWindowAction {
    CloseWindowAction::MinimizeToTray
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPreferences {
    #[serde(default = "default_close_window_action")]
    pub close_window_action: CloseWindowAction,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            close_window_action: default_close_window_action(),
        }
    }
}

fn app_preferences_path() -> Result<PathBuf, String> {
    config_dir()
        .map(|dir| dir.join("app_preferences.json"))
        .map_err(|e| e.to_string())
}

fn load_app_preferences_core() -> AppPreferences {
    let Ok(path) = app_preferences_path() else {
        return AppPreferences::default();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return AppPreferences::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_app_preferences_core(preferences: &AppPreferences) -> Result<(), String> {
    let path = app_preferences_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(preferences).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_app_preferences() -> AppPreferences {
    load_app_preferences_core()
}

#[tauri::command]
pub fn set_app_preferences(preferences: AppPreferences) -> Result<AppPreferences, String> {
    save_app_preferences_core(&preferences)?;
    Ok(preferences)
}

#[tauri::command]
pub fn get_recording_config() -> RecordingConfig {
    load_recording_config()
}

#[tauri::command]
pub fn set_recording_config(config: RecordingConfig) -> Result<RecordingConfig, String> {
    save_recording_config(&config).map_err(|error| error.to_string())?;
    Ok(config)
}

#[tauri::command]
pub fn list_recordings() -> Result<Vec<RecordingInfo>, String> {
    list_recordings_core().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn read_recording(id: String) -> Result<RecordingContent, String> {
    read_recording_core(&id).map_err(|error| error.to_string())
}

/// Deletion is deliberately two-step at the contract boundary. The desktop
/// must show the sensitive-data warning and then pass `confirmed: true`.
#[tauri::command]
pub fn delete_recording(id: String, confirmed: bool) -> Result<RecordingInfo, String> {
    let info = delete_recording_core(&id, confirmed).map_err(|error| error.to_string())?;
    append_operation_audit(
        "desktop_recording",
        &info.host,
        &format!("delete terminal recording {}", info.id),
        RiskLevel::Low,
        Some(0),
        0,
        Some("explicitly confirmed terminal recording deletion"),
    );
    Ok(info)
}

#[tauri::command]
pub fn get_cli_path_status() -> Result<CliPathStatus, String> {
    cli_path_status_with_message("CLI PATH status loaded.".to_string())
}

// ── B51: Cross-platform CLI install (Linux/macOS shell profile) ─────────────
// On Windows, PATH is managed via the registry (HKCU\Environment\Path).
// On Linux/macOS, we modify the user's shell profile to add/remove an
// `export PATH=...` line tagged with a sentinel comment.

#[cfg(not(windows))]
fn detect_shell_profiles() -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() {
        return Vec::new();
    }
    let home = PathBuf::from(home);

    let mut profiles = Vec::new();

    // Bash: ~/.bashrc (Linux), ~/.bash_profile (macOS login shell)
    profiles.push(home.join(".bashrc"));
    profiles.push(home.join(".bash_profile"));

    // Zsh: ~/.zshrc (most common on macOS)
    profiles.push(home.join(".zshrc"));

    // Fish: ~/.config/fish/config.fish
    let fish_config = home.join(".config").join("fish").join("config.fish");
    profiles.push(fish_config);

    // Filter to only existing files
    profiles.into_iter().filter(|p| p.exists()).collect()
}

#[cfg(not(windows))]
fn append_shell_profile_entry(dir: &Path) -> Result<usize, String> {
    let profiles = detect_shell_profiles();
    if profiles.is_empty() {
        return Err(
            "No shell profile files found (~/.bashrc, ~/.zshrc, ~/.config/fish/config.fish). \
             Please add the CLI directory to your PATH manually."
                .to_string(),
        );
    }
    let mut modified = 0;
    for profile in &profiles {
        let content = std::fs::read_to_string(profile)
            .map_err(|e| format!("failed to read {}: {e}", profile.display()))?;

        // Check if already present
        if content.lines().any(|l| l.contains(SHELL_PROFILE_SENTINEL)) {
            continue;
        }

        let line = build_shell_profile_line(profile, dir);

        // Append the PATH export line
        let new_content = if content.is_empty() || content.ends_with('\n') {
            format!("{content}{line}\n")
        } else {
            format!("{content}\n{line}\n")
        };

        std::fs::write(profile, new_content)
            .map_err(|e| format!("failed to write {}: {e}", profile.display()))?;
        modified += 1;
    }
    Ok(modified)
}

#[cfg(not(windows))]
fn remove_shell_profile_entry(_dir: &Path) -> Result<usize, String> {
    let profiles = detect_shell_profiles();
    if profiles.is_empty() {
        return Err("No shell profile files found.".to_string());
    }
    let mut modified = 0;
    for profile in &profiles {
        let content = std::fs::read_to_string(profile)
            .map_err(|e| format!("failed to read {}: {e}", profile.display()))?;

        // Remove lines containing the sentinel
        let new_content: String = content
            .lines()
            .filter(|line| !line.contains(SHELL_PROFILE_SENTINEL))
            .collect::<Vec<_>>()
            .join("\n");

        // Add trailing newline if we have content
        let new_content = if new_content.is_empty() {
            new_content
        } else {
            format!("{new_content}\n")
        };

        if new_content != content {
            std::fs::write(profile, new_content)
                .map_err(|e| format!("failed to write {}: {e}", profile.display()))?;
            modified += 1;
        }
    }
    Ok(modified)
}

#[cfg(not(windows))]
fn shell_profile_has_entry() -> bool {
    let profiles = detect_shell_profiles();
    profiles.iter().any(|profile| {
        std::fs::read_to_string(profile)
            .map(|content| content.contains(SHELL_PROFILE_SENTINEL))
            .unwrap_or(false)
    })
}

#[tauri::command]
pub fn install_cli_to_path() -> Result<CliPathStatus, String> {
    let cli_dir = bundled_cli_dir()?;

    #[cfg(windows)]
    {
        let user_path = read_user_path_value()?;
        if path_contains_dir(&user_path, &cli_dir) {
            return cli_path_status_with_message(
                "Agent2SSH CLI is already in user PATH.".to_string(),
            );
        }
        let next_path = append_user_path_dir(&user_path, &cli_dir);
        write_user_path_value(&next_path)?;
    }

    #[cfg(not(windows))]
    {
        if shell_profile_has_entry() {
            return cli_path_status_with_message(
                "Agent2SSH CLI is already in shell profile PATH.".to_string(),
            );
        }
        let modified = append_shell_profile_entry(&cli_dir)?;
        if modified == 0 {
            return Err("Failed to modify any shell profile files.".to_string());
        }
    }

    cli_path_status_with_message(
        "Agent2SSH CLI added to PATH. Restart terminals or Codex sessions to pick it up."
            .to_string(),
    )
}

#[tauri::command]
pub fn remove_cli_from_path() -> Result<CliPathStatus, String> {
    let cli_dir = bundled_cli_dir()?;

    #[cfg(windows)]
    {
        let user_path = read_user_path_value()?;
        if !path_contains_dir(&user_path, &cli_dir) {
            return cli_path_status_with_message("Agent2SSH CLI is not in user PATH.".to_string());
        }
        let next_path = remove_user_path_dir(&user_path, &cli_dir);
        write_user_path_value(&next_path)?;
    }

    #[cfg(not(windows))]
    {
        if !shell_profile_has_entry() {
            return cli_path_status_with_message(
                "Agent2SSH CLI is not in shell profile PATH.".to_string(),
            );
        }
        let _ = remove_shell_profile_entry(&cli_dir)?;
    }

    cli_path_status_with_message(
        "Agent2SSH CLI removed from PATH. Restart terminals or Codex sessions to pick it up."
            .to_string(),
    )
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavSyncConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default = "default_webdav_remote_path")]
    pub remote_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavSyncConfigView {
    pub enabled: bool,
    pub url: String,
    pub username: Option<String>,
    pub remote_path: String,
    pub password_configured: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavSyncSaveRequest {
    pub enabled: bool,
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub remote_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavSyncStatus {
    pub configured: bool,
    pub enabled: bool,
    pub last_action: Option<String>,
    pub last_success: Option<bool>,
    pub last_message: Option<String>,
    pub last_sync_at: Option<String>,
    pub last_uploaded_bytes: Option<u64>,
    pub last_remote_path: Option<String>,
    pub portable_digest: Option<String>,
    pub sync_state: String,
    pub sync_summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebDavSyncFile {
    path: String,
    content: String,
    bytes: u64,
}

const WEBDAV_SYNC_FILES: &[&str] = &[
    "hosts.json",
    "playbooks.toml",
    "risk_rules.toml",
    "policy.toml",
    "policy.json",
    "execution_limits.toml",
    "anomaly.toml",
    "webhook.toml",
    "app_preferences.json",
];

fn default_webdav_remote_path() -> String {
    "agent2ssh/agent2ssh-sync.json".to_string()
}

impl Default for WebDavSyncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            username: None,
            password: None,
            remote_path: default_webdav_remote_path(),
        }
    }
}

impl WebDavSyncConfig {
    fn configured(&self) -> bool {
        !self.url.trim().is_empty() && !self.remote_path.trim().is_empty()
    }

    fn view(&self) -> WebDavSyncConfigView {
        WebDavSyncConfigView {
            enabled: self.enabled,
            url: self.url.clone(),
            username: self.username.clone(),
            remote_path: self.remote_path.clone(),
            password_configured: self
                .password
                .as_deref()
                .map(|value| !value.is_empty())
                .unwrap_or(false),
        }
    }
}

impl WebDavSyncStatus {
    fn from_config(config: &WebDavSyncConfig) -> Self {
        let (portable_digest, sync_state, sync_summary) = portable_sync_summary();
        Self {
            configured: config.configured(),
            enabled: config.enabled,
            last_action: None,
            last_success: None,
            last_message: None,
            last_sync_at: None,
            last_uploaded_bytes: None,
            last_remote_path: Some(config.remote_path.clone()),
            portable_digest,
            sync_state,
            sync_summary,
        }
    }
}

fn portable_sync_summary() -> (Option<String>, String, String) {
    match crate::webdav_sync::current_portable_config_digest() {
        Ok(current) => match crate::webdav_sync::load_local_sync_marker() {
            Ok(Some(marker)) if crate::webdav_sync::sync_marker_digest(&marker) == current => (
                Some(current),
                "in_sync".to_string(),
                "Portable configuration matches the last applied sync snapshot.".to_string(),
            ),
            Ok(Some(_)) => (
                Some(current),
                "local_ahead".to_string(),
                "Portable configuration has local changes since the last sync.".to_string(),
            ),
            Ok(None) => (
                Some(current),
                "unknown".to_string(),
                "Portable configuration has not been synchronized by the versioned sync engine."
                    .to_string(),
            ),
            Err(error) => (
                Some(current),
                "unknown".to_string(),
                format!("Portable sync metadata is unreadable: {error}"),
            ),
        },
        Err(error) => (
            None,
            "unknown".to_string(),
            format!("Portable configuration digest is unavailable: {error}"),
        ),
    }
}

fn webdav_sync_config_path() -> Result<PathBuf, String> {
    config_dir()
        .map(|dir| dir.join("webdav_sync.json"))
        .map_err(|e| e.to_string())
}

fn webdav_sync_status_path() -> Result<PathBuf, String> {
    config_dir()
        .map(|dir| dir.join("webdav_sync_status.json"))
        .map_err(|e| e.to_string())
}

fn normalize_optional_field(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_remote_path(value: &str) -> String {
    let trimmed = value.trim().trim_matches('/').replace('\\', "/");
    if trimmed.is_empty() {
        default_webdav_remote_path()
    } else {
        trimmed
    }
}

fn validate_remote_path(remote_path: &str) -> Result<(), String> {
    if remote_path
        .split('/')
        .any(|segment| segment == "." || segment == "..")
    {
        return Err("WebDAV remote path cannot contain . or .. segments".to_string());
    }
    Ok(())
}

fn load_webdav_sync_config_core() -> WebDavSyncConfig {
    let Ok(path) = webdav_sync_config_path() else {
        return WebDavSyncConfig::default();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return WebDavSyncConfig::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_webdav_sync_config_core(config: &WebDavSyncConfig) -> Result<(), String> {
    let _guard = lock_config_file(".webdav_sync.lock").map_err(|e| e.to_string())?;
    let path = webdav_sync_config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(&path, raw).map_err(|e| e.to_string())?;
    restrict_file_to_owner(&path).map_err(|e| e.to_string())
}

fn load_webdav_sync_status_core(config: &WebDavSyncConfig) -> WebDavSyncStatus {
    let Ok(path) = webdav_sync_status_path() else {
        return WebDavSyncStatus::from_config(config);
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return WebDavSyncStatus::from_config(config);
    };
    let mut status: WebDavSyncStatus =
        serde_json::from_str(&raw).unwrap_or_else(|_| WebDavSyncStatus::from_config(config));
    status.configured = config.configured();
    status.enabled = config.enabled;
    status
}

fn save_webdav_sync_status_core(status: &WebDavSyncStatus) -> Result<(), String> {
    let _guard = lock_config_file(".webdav_sync_status.lock").map_err(|e| e.to_string())?;
    let path = webdav_sync_status_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(status).map_err(|e| e.to_string())?;
    fs::write(&path, raw).map_err(|e| e.to_string())?;
    restrict_file_to_owner(&path).map_err(|e| e.to_string())
}

fn webdav_failure_status(
    config: &WebDavSyncConfig,
    action: &str,
    message: String,
) -> WebDavSyncStatus {
    let (portable_digest, sync_state, sync_summary) = portable_sync_summary();
    WebDavSyncStatus {
        configured: config.configured(),
        enabled: config.enabled,
        last_action: Some(action.to_string()),
        last_success: Some(false),
        last_message: Some(message),
        last_sync_at: Some(chrono::Utc::now().to_rfc3339()),
        last_uploaded_bytes: None,
        last_remote_path: Some(config.remote_path.clone()),
        portable_digest,
        sync_state,
        sync_summary,
    }
}

fn require_webdav_config(config: &WebDavSyncConfig, require_enabled: bool) -> Result<(), String> {
    if require_enabled && !config.enabled {
        return Err("WebDAV sync is disabled".to_string());
    }
    if config.url.trim().is_empty() {
        return Err("WebDAV URL is required".to_string());
    }
    if config.remote_path.trim().is_empty() {
        return Err("WebDAV remote path is required".to_string());
    }
    validate_remote_path(&config.remote_path)
}

fn webdav_base_url(config: &WebDavSyncConfig) -> Result<reqwest::Url, String> {
    let mut raw = config.url.trim().trim_end_matches('/').to_string();
    raw.push('/');
    reqwest::Url::parse(&raw).map_err(|e| format!("invalid WebDAV URL: {e}"))
}

fn webdav_target_url(config: &WebDavSyncConfig) -> Result<reqwest::Url, String> {
    let base = webdav_base_url(config)?;
    base.join(config.remote_path.trim_start_matches('/'))
        .map_err(|e| format!("invalid WebDAV remote path: {e}"))
}

fn webdav_parent_collections(
    config: &WebDavSyncConfig,
) -> Result<Vec<(String, reqwest::Url)>, String> {
    let base = webdav_base_url(config)?;
    let segments: Vec<&str> = config
        .remote_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    let mut collections = Vec::new();
    if segments.len() <= 1 {
        return Ok(collections);
    }
    for index in 0..(segments.len() - 1) {
        let path = segments[..=index].join("/");
        let url = base
            .join(&format!("{path}/"))
            .map_err(|e| format!("invalid WebDAV collection path: {e}"))?;
        collections.push((path, url));
    }
    Ok(collections)
}

fn webdav_request_auth(
    request: reqwest::RequestBuilder,
    config: &WebDavSyncConfig,
) -> reqwest::RequestBuilder {
    if let Some(username) = config.username.as_deref() {
        request.basic_auth(username, config.password.clone())
    } else {
        request
    }
}

async fn ensure_webdav_collections(
    client: &reqwest::Client,
    config: &WebDavSyncConfig,
) -> Result<(), String> {
    let mkcol = reqwest::Method::from_bytes(b"MKCOL").map_err(|e| e.to_string())?;
    for (path, url) in webdav_parent_collections(config)? {
        let response = webdav_request_auth(client.request(mkcol.clone(), url.clone()), config)
            .send()
            .await
            .map_err(|e| format!("failed to create WebDAV collection {path}: {e}"))?;
        let status = response.status();
        if status.is_success() || status.as_u16() == 405 {
            continue;
        }
        return Err(format!(
            "failed to create WebDAV collection {path}: HTTP {status}"
        ));
    }
    Ok(())
}

async fn propfind_webdav_parent(
    client: &reqwest::Client,
    config: &WebDavSyncConfig,
) -> Result<(), String> {
    let propfind = reqwest::Method::from_bytes(b"PROPFIND").map_err(|e| e.to_string())?;
    let url = webdav_parent_collections(config)?
        .last()
        .map(|(_, url)| url.clone())
        .unwrap_or(webdav_base_url(config)?);
    let response = webdav_request_auth(client.request(propfind, url.clone()), config)
        .header("Depth", "0")
        .send()
        .await
        .map_err(|e| format!("WebDAV test request failed: {e}"))?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("WebDAV test failed: HTTP {status} at {url}"))
    }
}

async fn build_webdav_sync_payload() -> Result<(Vec<u8>, usize), String> {
    let dir = config_dir().map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for name in WEBDAV_SYNC_FILES {
        let path = dir.join(name);
        if !path.is_file() {
            continue;
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read sync file {name}: {e}"))?;
        files.push(WebDavSyncFile {
            path: (*name).to_string(),
            bytes: content.len() as u64,
            content,
        });
    }
    let keys_dir = dir.join("keys");
    if keys_dir.is_dir() {
        for entry in fs::read_dir(&keys_dir).map_err(|e| format!("failed to read keys dir: {e}"))? {
            let entry = entry.map_err(|e| format!("failed to read key entry: {e}"))?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("failed to read sync key file {name}: {e}"))?;
            files.push(WebDavSyncFile {
                path: format!("keys/{name}"),
                bytes: content.len() as u64,
                content,
            });
        }
    }
    let forwards = forward_list_core().await;
    let content = serde_json::to_string_pretty(&forwards).map_err(|e| e.to_string())?;
    files.push(WebDavSyncFile {
        path: "active_forwards.json".to_string(),
        bytes: content.len() as u64,
        content,
    });
    if files.is_empty() {
        return Err("no Agent2SSH configuration files are available to sync".to_string());
    }
    let file_count = files.len();
    let payload = serde_json::json!({
        "version": 1,
        "exportedAt": chrono::Utc::now().to_rfc3339(),
        "files": files,
    });
    let bytes = serde_json::to_vec_pretty(&payload).map_err(|e| e.to_string())?;
    Ok((bytes, file_count))
}

async fn test_webdav_sync_inner(config: &WebDavSyncConfig) -> Result<WebDavSyncStatus, String> {
    require_webdav_config(config, false)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    ensure_webdav_collections(&client, config).await?;
    propfind_webdav_parent(&client, config).await?;
    let (portable_digest, sync_state, sync_summary) = portable_sync_summary();
    Ok(WebDavSyncStatus {
        configured: config.configured(),
        enabled: config.enabled,
        last_action: Some("test".to_string()),
        last_success: Some(true),
        last_message: Some("WebDAV connection test succeeded.".to_string()),
        last_sync_at: Some(chrono::Utc::now().to_rfc3339()),
        last_uploaded_bytes: None,
        last_remote_path: Some(config.remote_path.clone()),
        portable_digest,
        sync_state,
        sync_summary,
    })
}

async fn push_webdav_sync_inner(config: &WebDavSyncConfig) -> Result<WebDavSyncStatus, String> {
    require_webdav_config(config, true)?;
    let (payload, file_count) = build_webdav_sync_payload().await?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .build()
        .map_err(|e| e.to_string())?;
    ensure_webdav_collections(&client, config).await?;
    let target_url = webdav_target_url(config)?;
    let uploaded_bytes = payload.len() as u64;
    let response = webdav_request_auth(client.put(target_url.clone()), config)
        .header("Content-Type", "application/json")
        .body(payload)
        .send()
        .await
        .map_err(|e| format!("WebDAV upload failed: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "WebDAV upload failed: HTTP {status} at {target_url}"
        ));
    }
    let (portable_digest, sync_state, sync_summary) = portable_sync_summary();
    Ok(WebDavSyncStatus {
        configured: config.configured(),
        enabled: config.enabled,
        last_action: Some("upload".to_string()),
        last_success: Some(true),
        last_message: Some(format!(
            "Uploaded {file_count} configuration files to WebDAV."
        )),
        last_sync_at: Some(chrono::Utc::now().to_rfc3339()),
        last_uploaded_bytes: Some(uploaded_bytes),
        last_remote_path: Some(config.remote_path.clone()),
        portable_digest,
        sync_state,
        sync_summary,
    })
}

#[tauri::command]
pub fn get_webdav_sync_config() -> WebDavSyncConfigView {
    load_webdav_sync_config_core().view()
}

#[tauri::command]
pub fn set_webdav_sync_config(
    config: WebDavSyncSaveRequest,
) -> Result<WebDavSyncConfigView, String> {
    let existing = load_webdav_sync_config_core();
    let password = match normalize_optional_field(config.password) {
        Some(password) => Some(password),
        None => existing.password,
    };
    let next = WebDavSyncConfig {
        enabled: config.enabled,
        url: config.url.trim().trim_end_matches('/').to_string(),
        username: normalize_optional_field(config.username),
        password,
        remote_path: normalize_remote_path(&config.remote_path),
    };
    if next.enabled {
        require_webdav_config(&next, false)?;
    } else {
        validate_remote_path(&next.remote_path)?;
    }
    save_webdav_sync_config_core(&next)?;
    let mut status = load_webdav_sync_status_core(&next);
    status.configured = next.configured();
    status.enabled = next.enabled;
    if status.last_remote_path.is_none() {
        status.last_remote_path = Some(next.remote_path.clone());
    }
    let _ = save_webdav_sync_status_core(&status);
    Ok(next.view())
}

#[tauri::command]
pub fn get_webdav_sync_status() -> WebDavSyncStatus {
    let config = load_webdav_sync_config_core();
    load_webdav_sync_status_core(&config)
}

#[tauri::command]
pub async fn test_webdav_sync() -> Result<WebDavSyncStatus, String> {
    let config = load_webdav_sync_config_core();
    let status = match test_webdav_sync_inner(&config).await {
        Ok(status) => status,
        Err(err) => webdav_failure_status(&config, "test", err),
    };
    save_webdav_sync_status_core(&status)?;
    Ok(status)
}

#[tauri::command]
pub async fn push_webdav_sync() -> Result<WebDavSyncStatus, String> {
    let config = load_webdav_sync_config_core();
    let status = match push_webdav_sync_inner(&config).await {
        Ok(status) => status,
        Err(err) => webdav_failure_status(&config, "upload", err),
    };
    save_webdav_sync_status_core(&status)?;
    Ok(status)
}

// ── Config snapshots (V4-3) ─────────────────────────────────────────────────
// Reuses the backup-directory mechanism `crate::webdav_sync` already builds
// for pre-sync safety nets (`create_sync_backup`, `SYNCABLE_FILES`) — this is
// a separate, independent capability from the desktop's own JSON-blob WebDAV
// sync above; a user never needs WebDAV configured to save/restore snapshots.

#[tauri::command]
pub fn list_config_snapshots_cmd() -> Result<Vec<ConfigSnapshotInfo>, String> {
    list_config_snapshots().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_config_snapshot(label: String) -> Result<ConfigSnapshotInfo, String> {
    let label = if label.trim().is_empty() {
        "manual".to_string()
    } else {
        label.trim().to_string()
    };
    create_named_snapshot(&label).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_config_snapshot_cmd(id: String) -> Result<ConfigSnapshotInfo, String> {
    restore_config_snapshot(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_config_snapshot_cmd(id: String) -> Result<(), String> {
    delete_config_snapshot(&id).map_err(|e| e.to_string())
}

/// `files` is `[(relative filename, content)]`, e.g. `[("policy.toml", "...")]`.
/// Rejects anything outside `SYNCABLE_FILES`; returns the pre-apply snapshot so
/// the caller can tell the user how to undo it.
#[tauri::command]
pub fn apply_config_template_cmd(
    files: Vec<(String, String)>,
) -> Result<ConfigSnapshotInfo, String> {
    apply_config_template(&files).map_err(|e| e.to_string())
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
    let source = source_from_transport();
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
    let source = source_from_transport();
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
pub fn get_host_fingerprint_status(host: String) -> Result<HostFingerprintStatus, String> {
    get_host_fingerprint_status_core(&host, 30).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustHostFingerprintRequest {
    pub host: String,
    pub expected_fingerprint_sha256: Option<String>,
    pub host_key_algorithm: String,
    pub fingerprint_sha256: String,
}

#[tauri::command]
pub fn trust_host_fingerprint(request: TrustHostFingerprintRequest) -> Result<(), String> {
    trust_host_fingerprint_core(
        &request.host,
        request.expected_fingerprint_sha256.as_deref(),
        &request.host_key_algorithm,
        &request.fingerprint_sha256,
        30,
    )
    .map_err(|e| e.to_string())
}

/// G13: Import trust from the system OpenSSH `~/.ssh/known_hosts`.
#[tauri::command]
pub fn import_known_hosts(path: Option<String>) -> Result<KnownHostImportSummary, String> {
    import_known_hosts_from_ssh(path.as_deref()).map_err(|e| e.to_string())
}

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
    let source = source_from_transport();
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

// ── B58: Wayland Compatibility ───────────────────────────────────────────────
// WebKitGTK (used by Tauri on Linux) has rendering issues on some Wayland
// compositors (NVIDIA/wlroots/Hyprland). These patches are applied before
// the Tauri builder runs and can be opted out via AGENT2SSH_DISABLE_WAYLAND_COMPAT.

#[cfg(target_os = "linux")]
fn apply_wayland_compat() {
    let session_type = std::env::var("XDG_SESSION_TYPE").ok();
    let decision = crate::wayland::decide_wayland_compat(
        std::env::var_os("AGENT2SSH_DISABLE_WAYLAND_COMPAT").is_some(),
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
        session_type.as_deref(),
        std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some(),
        std::env::var_os("AGENT2SSH_KEEP_GBM_BACKEND").is_some(),
    );
    if !decision.apply {
        return;
    }

    // 1. Disable DMABUF renderer — crashes on NVIDIA/wlroots before window creation.
    if decision.disable_dmabuf_renderer {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    // 2. Remove GBM_BACKEND — NVIDIA GBM causes "Failed to create GBM buffer"
    //    on Hyprland. Allow users to keep it via AGENT2SSH_KEEP_GBM_BACKEND.
    if decision.remove_gbm_backend {
        std::env::remove_var("GBM_BACKEND");
    }

    let _ = crate::diagnostics::append_diagnostic_log(
        "info",
        "wayland_compat",
        "applied wayland compatibility patches",
        None,
    );
}

#[cfg(not(target_os = "linux"))]
fn apply_wayland_compat() {}

// ── Command snippets ────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_snippets_command() -> Result<Vec<Snippet>, String> {
    load_snippets().map_err(|error| format!("failed to load snippets: {error}"))
}

#[tauri::command]
pub fn save_snippet_command(snippet: Snippet) -> Result<Vec<Snippet>, String> {
    add_snippet(
        &snippet.name,
        &snippet.command,
        snippet.description.as_deref(),
    )
    .map_err(|error| format!("failed to save snippet: {error}"))
}

#[tauri::command]
pub fn delete_snippet_command(name: String) -> Result<bool, String> {
    remove_snippet(&name).map_err(|error| format!("failed to delete snippet: {error}"))
}

// ── Bootstrap ────────────────────────────────────────────────────────────────

pub fn run_tauri() {
    crate::diagnostics::install_panic_hook("tauri");
    // B58: Apply Wayland compatibility patches on Linux before Tauri builder.
    apply_wayland_compat();
    // K1: migrate any legacy plaintext passwords into the app-managed encrypted store (no-op once clean).
    if let Err(e) = crate::store::migrate_plaintext_secrets() {
        eprintln!("warning: secret migration skipped: {e}");
    }
    // The transport is implicitly Tauri when the `tauri` feature is compiled
    // in (is_desktop() returns true under #[cfg(feature = "tauri")]). The
    // global Host stays at Host::Cli (the default) since per-command AppHandle
    // instances are passed directly to each Tauri command handler.
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            reveal_main_window(app);
        }))
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
                match load_app_preferences_core().close_window_action {
                    CloseWindowAction::MinimizeToTray => {
                        api.prevent_close();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.hide();
                        }
                    }
                    CloseWindowAction::QuitApplication => {
                        let _ = daemon_stop();
                    }
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
            get_webdav_sync_config,
            set_webdav_sync_config,
            get_webdav_sync_status,
            test_webdav_sync,
            push_webdav_sync,
            list_config_snapshots_cmd,
            create_config_snapshot,
            restore_config_snapshot_cmd,
            delete_config_snapshot_cmd,
            apply_config_template_cmd,
            // Command snippets
            list_snippets_command,
            save_snippet_command,
            delete_snippet_command,
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
            sftp_read_text,
            local_ls,
            local_walk,
            local_mkdir,
            local_read_text,
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
            forward_stats,
            forward_remove,
            forward_stop,
            forward_start,
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
            get_app_preferences,
            set_app_preferences,
            get_recording_config,
            set_recording_config,
            list_recordings,
            read_recording,
            delete_recording,
            get_cli_path_status,
            install_cli_to_path,
            remove_cli_from_path,
            set_tray_labels,
            mcp_agent_config::list_mcp_agent_configs,
            mcp_agent_config::configure_mcp_agent,
            mcp_agent_config::uninstall_mcp_agent,
            mcp_agent_config::agent_skill_status,
            mcp_agent_config::install_agent_skill,
            mcp_agent_config::uninstall_agent_skill,
            // Diagnostics
            list_diagnostic_logs,
            write_diagnostic_log,
            clear_diagnostic_logs,
            export_diagnostic_bundle,
            generate_system_report,
            passphrase_cache_set,
            passphrase_cache_evict,
            passphrase_cache_clear,
            // B24: Terminal Highlight
            list_highlights,
            add_highlight,
            remove_highlight,
            update_highlight,
            reset_highlights,
            // B33: Container Discovery
            discover_containers,
            // Font + Shell enumeration
            list_fonts,
            list_shells,
            default_shell,
            // G4: Copy-block redaction
            redact_for_clipboard,
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
            get_host_fingerprint_status,
            trust_host_fingerprint,
            import_known_hosts,
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

    #[test]
    fn local_read_text_inner_reads_small_text_file() {
        let dir = std::env::temp_dir().join(format!("a2s-readtext-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("note.txt");
        std::fs::write(&file, "hello world\n").unwrap();

        let content = local_read_text_inner(file.to_string_lossy().to_string()).unwrap();
        assert_eq!(content, "hello world\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_read_text_inner_rejects_oversized_file() {
        let dir = std::env::temp_dir().join(format!("a2s-readtext-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("big.txt");
        std::fs::write(&file, vec![b'a'; LOCAL_PREVIEW_MAX_BYTES as usize]).unwrap();

        assert!(local_read_text_inner(file.to_string_lossy().to_string()).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_read_text_inner_rejects_binary_content() {
        let dir = std::env::temp_dir().join(format!("a2s-readtext-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("bin.dat");
        std::fs::write(&file, [0xff_u8, 0xfe, 0x00, 0x01, 0x02]).unwrap();

        assert!(local_read_text_inner(file.to_string_lossy().to_string()).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
