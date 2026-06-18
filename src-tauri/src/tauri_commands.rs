use crate::keys::{
    delete_key_core, generate_key_core, import_key_core, list_keys_core, SshKeyInfo,
};
use crate::notify::{load_webhook_config, save_webhook_config, WebhookConfig};
use crate::{
    connection::{connect_host, disconnect_host, list_active_connections},
    core::{
        add_host_core, apply_risk_override, delete_host_group_core, exec_multi_core, exec_ssh_core,
        export_team_config, import_ssh_config_core, import_team_config, list_audit_core,
        list_host_groups_core, list_hosts_core, ping_hosts_core, remove_host_core,
        save_host_group_core, sftp_download_core_with_source, sftp_ls_core_with_source,
        sftp_mkdir_core_with_source, sftp_stat_core_with_source, sftp_upload_core_with_source,
        update_host_core, ExecMultiRequest, ImportResult, TeamConfigExport,
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
        ExecResult, ForwardDirection, ForwardRule, HostGroup, HostProfile, PingResult, RiskLevel,
        SftpDownloadRequest, SftpResult, SftpUploadRequest,
    },
};
use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Instant,
};
use tauri::AppHandle;
use tokio::sync::Mutex;
use uuid::Uuid;

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

#[derive(Debug, Clone, Copy)]
enum McpConfigFormat {
    Json,
    Toml,
}

#[derive(Debug, Clone)]
struct McpAgentCandidate {
    id: &'static str,
    name: &'static str,
    source: &'static str,
    config_path: PathBuf,
    format: McpConfigFormat,
    detection_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpAgentConfigStatus {
    pub id: String,
    pub name: String,
    pub source: String,
    pub config_path: String,
    pub detected: bool,
    pub configured: bool,
    pub status: String,
    pub command: Option<String>,
    pub configured_source: Option<String>,
    pub recommended_command: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpAgentConfigureResult {
    pub id: String,
    pub config_path: String,
    pub backup_path: Option<String>,
    pub command: String,
    pub source: String,
    pub message: String,
}

fn home_path(relative: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(relative)
}

fn mcp_agent_candidates() -> Vec<McpAgentCandidate> {
    vec![
        McpAgentCandidate {
            id: "codex",
            name: "Codex",
            source: "codex",
            config_path: home_path(".codex/config.toml"),
            format: McpConfigFormat::Toml,
            detection_paths: vec![home_path(".codex"), home_path(".codex/config.toml")],
        },
        McpAgentCandidate {
            id: "claude_desktop",
            name: "Claude Desktop",
            source: "claude_desktop",
            config_path: home_path("Library/Application Support/Claude/claude_desktop_config.json"),
            format: McpConfigFormat::Json,
            detection_paths: vec![
                home_path("Library/Application Support/Claude"),
                PathBuf::from("/Applications/Claude.app"),
            ],
        },
        McpAgentCandidate {
            id: "cursor",
            name: "Cursor",
            source: "cursor",
            config_path: home_path(".cursor/mcp.json"),
            format: McpConfigFormat::Json,
            detection_paths: vec![
                home_path(".cursor"),
                PathBuf::from("/Applications/Cursor.app"),
            ],
        },
        McpAgentCandidate {
            id: "windsurf",
            name: "Windsurf",
            source: "windsurf",
            config_path: home_path(".codeium/windsurf/mcp_config.json"),
            format: McpConfigFormat::Json,
            detection_paths: vec![
                home_path(".codeium/windsurf"),
                PathBuf::from("/Applications/Windsurf.app"),
            ],
        },
        McpAgentCandidate {
            id: "workbuddy",
            name: "WorkBuddy",
            source: "workbuddy",
            config_path: home_path(".workbuddy/mcp.json"),
            format: McpConfigFormat::Json,
            detection_paths: vec![
                home_path(".workbuddy"),
                home_path("Library/Application Support/WorkBuddy"),
                home_path("Library/Application Support/@genie/workbuddy-desktop"),
                PathBuf::from("/Applications/WorkBuddy.app"),
            ],
        },
        McpAgentCandidate {
            id: "qoder_work",
            name: "Qoder Work",
            source: "qoder_work",
            config_path: home_path("Library/Application Support/Qoder/SharedClientCache/mcp.json"),
            format: McpConfigFormat::Json,
            detection_paths: vec![
                home_path("Library/Application Support/Qoder/SharedClientCache/mcp.json"),
                home_path("Library/Application Support/QoderWork"),
                home_path("Library/Application Support/Qoder"),
                home_path(".qoderwork"),
                home_path(".qoder"),
                PathBuf::from("/Applications/QoderWork.app"),
                PathBuf::from("/Applications/Qoder.app"),
            ],
        },
        McpAgentCandidate {
            id: "trae",
            name: "Trae",
            source: "trae",
            config_path: home_path("Library/Application Support/Trae/User/mcp.json"),
            format: McpConfigFormat::Json,
            detection_paths: vec![
                home_path("Library/Application Support/Trae"),
                home_path(".trae"),
                home_path(".trae-cn"),
                PathBuf::from("/Applications/Trae.app"),
            ],
        },
        McpAgentCandidate {
            id: "trae_solo",
            name: "Trae Solo",
            source: "trae_solo",
            config_path: home_path("Library/Application Support/TRAE SOLO/User/mcp.json"),
            format: McpConfigFormat::Json,
            detection_paths: vec![
                home_path("Library/Application Support/TRAE SOLO"),
                home_path(".trae"),
                home_path(".trae-cn"),
                PathBuf::from("/Applications/TRAE SOLO.app"),
            ],
        },
    ]
}

fn resolve_path_command(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn bundled_mcp_binary_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe
        .parent()
        .ok_or_else(|| "failed to resolve current executable directory".to_string())?;
    let candidate = dir.join(format!("agent2ssh-mcp{}", std::env::consts::EXE_SUFFIX));
    if candidate.exists() {
        return Ok(candidate);
    }
    if let Some(path) = resolve_path_command("agent2ssh-mcp") {
        return Ok(path);
    }
    Err(format!("agent2ssh-mcp not found near {}", exe.display()))
}

fn agent_detected(candidate: &McpAgentCandidate) -> bool {
    candidate.config_path.exists() || candidate.detection_paths.iter().any(|path| path.exists())
}

fn configured_json(path: &Path) -> Result<(bool, Option<String>, Option<String>), String> {
    if !path.exists() {
        return Ok((false, None, None));
    }
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let agent = value
        .get("mcpServers")
        .and_then(|servers| servers.get("agent2ssh"));
    let command = agent
        .and_then(|server| server.get("command"))
        .and_then(|command| command.as_str())
        .map(ToString::to_string);
    let source = agent
        .and_then(|server| server.get("env"))
        .and_then(|env| env.get("AGENT2SSH_SOURCE"))
        .and_then(|source| source.as_str())
        .map(ToString::to_string);
    Ok((command.is_some(), command, source))
}

fn configured_toml(path: &Path) -> Result<(bool, Option<String>, Option<String>), String> {
    if !path.exists() {
        return Ok((false, None, None));
    }
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let value: toml::Value = toml::from_str(&raw).map_err(|e| e.to_string())?;
    let agent = value
        .get("mcp_servers")
        .and_then(|servers| servers.get("agent2ssh"));
    let command = agent
        .and_then(|server| server.get("command"))
        .and_then(|command| command.as_str())
        .map(ToString::to_string);
    let source = agent
        .and_then(|server| server.get("env"))
        .and_then(|env| env.get("AGENT2SSH_SOURCE"))
        .and_then(|source| source.as_str())
        .map(ToString::to_string);
    Ok((command.is_some(), command, source))
}

fn mcp_configured(
    candidate: &McpAgentCandidate,
) -> Result<(bool, Option<String>, Option<String>), String> {
    match candidate.format {
        McpConfigFormat::Json => configured_json(&candidate.config_path),
        McpConfigFormat::Toml => configured_toml(&candidate.config_path),
    }
}

fn backup_config(path: &Path) -> Result<Option<PathBuf>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let stamp = Utc::now().format("%Y%m%d%H%M%S");
    let backup = path.with_extension(format!(
        "{}.bak-{stamp}",
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("config")
    ));
    std::fs::copy(path, &backup).map_err(|e| e.to_string())?;
    Ok(Some(backup))
}

fn configure_json(path: &Path, command: &str, source: &str) -> Result<(), String> {
    let mut value = if path.exists() {
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str::<serde_json::Value>(&raw).map_err(|e| e.to_string())?
    } else {
        serde_json::json!({})
    };
    let Some(root) = value.as_object_mut() else {
        return Err("config root must be a JSON object".into());
    };
    let servers = root
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    let Some(servers) = servers.as_object_mut() else {
        return Err("mcpServers must be a JSON object".into());
    };
    let server = servers
        .entry("agent2ssh")
        .or_insert_with(|| serde_json::json!({}));
    let Some(server) = server.as_object_mut() else {
        return Err("mcpServers.agent2ssh must be a JSON object".into());
    };
    server.insert("command".into(), serde_json::json!(command));
    server.insert("args".into(), serde_json::json!([]));
    let env = server.entry("env").or_insert_with(|| serde_json::json!({}));
    let Some(env) = env.as_object_mut() else {
        return Err("mcpServers.agent2ssh.env must be a JSON object".into());
    };
    env.insert("AGENT2SSH_SOURCE".into(), serde_json::json!(source));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    std::fs::write(path, format!("{raw}\n")).map_err(|e| e.to_string())
}

fn table_mut<'a>(
    table: &'a mut toml::map::Map<String, toml::Value>,
    key: &str,
) -> Result<&'a mut toml::map::Map<String, toml::Value>, String> {
    let value = table
        .entry(key.to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    value
        .as_table_mut()
        .ok_or_else(|| format!("{key} must be a TOML table"))
}

fn configure_toml(path: &Path, command: &str, source: &str) -> Result<(), String> {
    let mut value = if path.exists() {
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        toml::from_str::<toml::Value>(&raw).map_err(|e| e.to_string())?
    } else {
        toml::Value::Table(toml::map::Map::new())
    };
    let Some(root) = value.as_table_mut() else {
        return Err("config root must be a TOML table".into());
    };
    let servers = table_mut(root, "mcp_servers")?;
    let agent = table_mut(servers, "agent2ssh")?;
    agent.insert("command".into(), toml::Value::String(command.to_string()));
    agent.insert("args".into(), toml::Value::Array(Vec::new()));
    let env = table_mut(agent, "env")?;
    env.insert(
        "AGENT2SSH_SOURCE".into(),
        toml::Value::String(source.to_string()),
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = toml::to_string_pretty(&value).map_err(|e| e.to_string())?;
    std::fs::write(path, raw).map_err(|e| e.to_string())
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
pub fn list_mcp_agent_configs() -> Result<Vec<McpAgentConfigStatus>, String> {
    let command = bundled_mcp_binary_path()?.display().to_string();
    Ok(mcp_agent_candidates()
        .into_iter()
        .map(|candidate| {
            let detected = agent_detected(&candidate);
            let (configured, existing_command, configured_source, status) =
                match mcp_configured(&candidate) {
                    Ok((configured, command, source)) => {
                        let status = if configured {
                            "configured"
                        } else if detected {
                            "detected"
                        } else {
                            "not_detected"
                        };
                        (configured, command, source, status.to_string())
                    }
                    Err(error) => (false, None, None, format!("invalid_config: {error}")),
                };
            McpAgentConfigStatus {
                id: candidate.id.to_string(),
                name: candidate.name.to_string(),
                source: candidate.source.to_string(),
                config_path: candidate.config_path.display().to_string(),
                detected,
                configured,
                status,
                command: existing_command,
                configured_source,
                recommended_command: command.clone(),
            }
        })
        .collect())
}

#[tauri::command]
pub fn configure_mcp_agent(agent_id: String) -> Result<McpAgentConfigureResult, String> {
    let candidate = mcp_agent_candidates()
        .into_iter()
        .find(|candidate| candidate.id == agent_id)
        .ok_or_else(|| format!("unknown MCP agent: {agent_id}"))?;
    let command = bundled_mcp_binary_path()?.display().to_string();
    let backup = backup_config(&candidate.config_path)?;
    match candidate.format {
        McpConfigFormat::Json => {
            configure_json(&candidate.config_path, &command, candidate.source)?
        }
        McpConfigFormat::Toml => {
            configure_toml(&candidate.config_path, &command, candidate.source)?
        }
    }
    Ok(McpAgentConfigureResult {
        id: candidate.id.to_string(),
        config_path: candidate.config_path.display().to_string(),
        backup_path: backup.map(|path| path.display().to_string()),
        command,
        source: candidate.source.to_string(),
        message: format!(
            "{} MCP config updated with source '{}'. Restart the agent client to load agent2ssh.",
            candidate.name, candidate.source
        ),
    })
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
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            // Host management
            list_hosts,
            add_host,
            update_host,
            remove_host,
            list_host_groups,
            save_host_group,
            delete_host_group,
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
            daemon_status,
            daemon_start,
            daemon_stop,
            daemon_restart,
            list_mcp_agent_configs,
            configure_mcp_agent,
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
