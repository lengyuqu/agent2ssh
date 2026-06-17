#![recursion_limit = "2048"]

use agent2ssh::approval::build_approval_context;
use agent2ssh::approval::{
    approval_list, approval_respond, check_approval_required, list_approval_policies,
};
use agent2ssh::events::subscribe_events;
use agent2ssh::execution_control::{
    append_rejected_exec_audit, authorize_command_with_approval, command_authorization_target,
    expand_exec_authorization_targets, CommandAuthorizationError, CommandAuthorizationInput,
};
use agent2ssh::notify::{load_webhook_config, save_webhook_config};
use agent2ssh::remote::{
    check_daemon_scope, check_daemon_version, diagnose_daemon, get_daemon, get_daemon_with_scope,
    get_daemons_unified_view, list_daemons_core,
};
use agent2ssh::risk_config::classify_with_user_rules;
use agent2ssh::store::{audit_path, compute_metrics_trend, TrendPeriod};
use agent2ssh::{
    add_host_core, collect_health_snapshot, compare_exec_results, compare_ssh_configs,
    connect_host, disconnect_host, dry_run_playbook, effective_command_risk, exec_multi_core,
    exec_multi_with_strategy, exec_ssh_core, export_audit_csv, export_audit_jsonl,
    export_team_config, export_to_ssh_config, forward_add_core, forward_list_core,
    forward_remove_core, import_ssh_config_core, import_team_config, list_active_connections,
    list_audit_core, list_hosts_core, list_playbooks_core, ping_hosts_core, preview_exec,
    preview_exec_multi, preview_team_config_import, remove_host_core,
    run_playbook_core_with_source, session_close_core, session_list_core, session_open_core,
    session_read_core, session_write_core, sftp_download_core_with_source,
    sftp_ls_core_with_source, sftp_mkdir_core_with_source, sftp_stat_core_with_source,
    sftp_upload_core_with_source, AuditFilter, ExecMultiBatchRequest, ExecMultiRequest,
    ExecRequest, ForwardDirection, HostProfile, RiskLevel, SftpDownloadRequest, SftpUploadRequest,
    TeamConfigExport,
};
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};

struct McpError {
    code: i32,
    message: String,
}

impl McpError {
    fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("Method not found: {method}"),
        }
    }
    fn internal(msg: impl ToString) -> Self {
        Self {
            code: -32000,
            message: msg.to_string(),
        }
    }
}

impl From<anyhow::Error> for McpError {
    fn from(err: anyhow::Error) -> Self {
        Self::internal(err)
    }
}

impl From<serde_json::Error> for McpError {
    fn from(err: serde_json::Error) -> Self {
        Self::internal(err)
    }
}

#[derive(Deserialize)]
struct DaemonIdBody {
    id: String,
}

#[derive(Deserialize)]
struct DaemonOutputBody {
    output: String,
}

#[derive(Deserialize)]
struct DaemonSessionListItem {
    id: String,
    host: String,
}

enum DaemonAttempt<T> {
    Handled(T),
    Fallback,
}

fn mcp_source() -> String {
    std::env::var("AGENT2SSH_SOURCE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "mcp".to_string())
}

fn mcp_host_tags(host: &str) -> Vec<String> {
    list_hosts_core()
        .unwrap_or_default()
        .into_iter()
        .find(|h| h.name == host)
        .map(|h| h.tags)
        .unwrap_or_default()
}

async fn authorize_local_mcp_exec_request(
    request: &mut ExecRequest,
) -> std::result::Result<RiskLevel, McpError> {
    let target = command_authorization_target(&request.host);
    let source = request.source.as_deref().unwrap_or("mcp").to_string();
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
            let message = "approval required but no local MCP approval handler is available";
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
    .map_err(mcp_authorization_error)?;
    if result.approved && result.risk == RiskLevel::High {
        request.force = true;
    }
    Ok(result.risk)
}

async fn authorize_local_mcp_exec_targets(
    hosts: &[String],
    tags: &Option<Vec<String>>,
    command: &str,
    force: bool,
    reason: Option<String>,
    change_id: Option<String>,
    source: &str,
) -> std::result::Result<bool, McpError> {
    let targets = expand_exec_authorization_targets(hosts, tags).map_err(McpError::from)?;
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
                let message = "approval required but no local MCP approval handler is available";
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
        .map_err(mcp_authorization_error)?;
        if result.approved && result.risk == RiskLevel::High {
            high_risk_approved = true;
        }
    }
    Ok(high_risk_approved)
}

async fn authorize_local_mcp_playbook_run(
    playbook: &str,
    host: &str,
    force: bool,
    params: &HashMap<String, String>,
    reason: Option<String>,
    change_id: Option<String>,
    source: &str,
) -> std::result::Result<bool, McpError> {
    let dry_run = dry_run_playbook(playbook, params).map_err(McpError::from)?;
    let target = command_authorization_target(host);
    let playbook_risk_override = list_playbooks_core()
        .map_err(McpError::from)?
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
                reason: reason.clone(),
                change_id: change_id.clone(),
            },
            |prompt| async move {
                let message = "approval required but no local MCP approval handler is available";
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
        .map_err(mcp_authorization_error)?;
        if result.approved && result.risk == RiskLevel::High {
            high_risk_approved = true;
        }
    }

    Ok(high_risk_approved)
}

async fn authorize_local_mcp_operation(
    host: &str,
    command: &str,
    force: bool,
    source: &str,
) -> std::result::Result<(), McpError> {
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
            let message = "approval required but no local MCP approval handler is available";
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
    .map_err(mcp_authorization_error)?;
    Ok(())
}

fn mcp_authorization_error(error: CommandAuthorizationError) -> McpError {
    match error {
        CommandAuthorizationError::ScopeDenied(message) => McpError::internal(message),
        CommandAuthorizationError::Blocked { message, .. } => McpError::internal(message),
        CommandAuthorizationError::ApprovalRejected => {
            McpError::internal("command rejected by approver")
        }
        CommandAuthorizationError::ApprovalTimedOut => {
            McpError::internal("approval request timed out")
        }
        CommandAuthorizationError::Internal(message) => McpError::internal(message),
    }
}

fn local_daemon_client() -> std::result::Result<Option<(reqwest::Client, String, String)>, McpError>
{
    let (url, token) = get_daemon("localhost")
        .map_err(|e| McpError::internal(format!("local daemon lookup failed: {e}")))?;
    let Some(token) = token else {
        return Ok(None);
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(McpError::internal)?;
    Ok(Some((client, url.trim_end_matches('/').to_string(), token)))
}

fn daemon_error_body(status: reqwest::StatusCode, body: String) -> McpError {
    McpError::internal(format!("local daemon request failed ({status}): {body}"))
}

fn can_fallback_session_error(body: &str) -> bool {
    body.contains("unknown session")
}

async fn try_daemon_session_open(
    host: &str,
) -> std::result::Result<DaemonAttempt<Value>, McpError> {
    let Some((client, base_url, token)) = local_daemon_client()? else {
        return Ok(DaemonAttempt::Fallback);
    };
    let source = mcp_source();
    let response = match client
        .post(format!("{base_url}/sessions"))
        .bearer_auth(token)
        .json(&json!({ "host": host, "source": source }))
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return Ok(DaemonAttempt::Fallback),
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(daemon_error_body(status, body));
    }
    let body: DaemonIdBody = response
        .json()
        .await
        .map_err(|e| McpError::internal(format!("invalid daemon session response: {e}")))?;
    Ok(DaemonAttempt::Handled(json!({
        "session_id": body.id,
        "host": host,
        "backend": "daemon",
        "source": source,
    })))
}

async fn try_daemon_gate_status() -> std::result::Result<Value, McpError> {
    let Some((client, base_url, token)) = local_daemon_client()? else {
        return Ok(json!({
            "reachable": false,
            "mode": "unknown",
            "error": "local daemon token not found"
        }));
    };
    let response = client
        .get(format!("{base_url}/gate"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| McpError::internal(format!("local daemon gate status failed: {e}")))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(daemon_error_body(status, body));
    }
    let mut value: Value = serde_json::from_str(&body).map_err(McpError::internal)?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert("reachable".into(), Value::Bool(true));
    }
    Ok(value)
}

async fn try_daemon_session_write(
    session_id: &str,
    input: &str,
) -> std::result::Result<DaemonAttempt<Value>, McpError> {
    let Some((client, base_url, token)) = local_daemon_client()? else {
        return Ok(DaemonAttempt::Fallback);
    };
    let source = mcp_source();
    let response = match client
        .post(format!("{base_url}/sessions/{session_id}/write"))
        .bearer_auth(token)
        .json(&json!({ "input": input, "source": source }))
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return Ok(DaemonAttempt::Fallback),
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        if can_fallback_session_error(&body) {
            return Ok(DaemonAttempt::Fallback);
        }
        return Err(daemon_error_body(status, body));
    }
    Ok(DaemonAttempt::Handled(
        json!({ "ok": true, "backend": "daemon", "source": source }),
    ))
}

async fn try_daemon_session_read(
    session_id: &str,
    timeout_ms: u64,
) -> std::result::Result<DaemonAttempt<Value>, McpError> {
    let Some((client, base_url, token)) = local_daemon_client()? else {
        return Ok(DaemonAttempt::Fallback);
    };
    let source = mcp_source();
    let response = match client
        .get(format!("{base_url}/sessions/{session_id}/read"))
        .query(&[
            ("timeout_ms", timeout_ms.to_string()),
            ("source", source.clone()),
        ])
        .bearer_auth(token)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return Ok(DaemonAttempt::Fallback),
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        if can_fallback_session_error(&body) {
            return Ok(DaemonAttempt::Fallback);
        }
        return Err(daemon_error_body(status, body));
    }
    let body: DaemonOutputBody = response
        .json()
        .await
        .map_err(|e| McpError::internal(format!("invalid daemon session output response: {e}")))?;
    Ok(DaemonAttempt::Handled(
        json!({ "output": body.output, "backend": "daemon", "source": source }),
    ))
}

async fn try_daemon_session_close(
    session_id: &str,
) -> std::result::Result<DaemonAttempt<Value>, McpError> {
    let Some((client, base_url, token)) = local_daemon_client()? else {
        return Ok(DaemonAttempt::Fallback);
    };
    let source = mcp_source();
    let response = match client
        .delete(format!("{base_url}/sessions/{session_id}"))
        .query(&[("source", source.clone())])
        .bearer_auth(token)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return Ok(DaemonAttempt::Fallback),
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        if can_fallback_session_error(&body) {
            return Ok(DaemonAttempt::Fallback);
        }
        return Err(daemon_error_body(status, body));
    }
    Ok(DaemonAttempt::Handled(
        json!({ "closed": session_id, "backend": "daemon", "source": source }),
    ))
}

async fn try_daemon_session_list() -> std::result::Result<DaemonAttempt<Vec<Value>>, McpError> {
    let Some((client, base_url, token)) = local_daemon_client()? else {
        return Ok(DaemonAttempt::Fallback);
    };
    let response = match client
        .get(format!("{base_url}/sessions"))
        .bearer_auth(token)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return Ok(DaemonAttempt::Fallback),
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(daemon_error_body(status, body));
    }
    let sessions: Vec<DaemonSessionListItem> = response
        .json()
        .await
        .map_err(|e| McpError::internal(format!("invalid daemon session list response: {e}")))?;
    Ok(DaemonAttempt::Handled(
        sessions
            .into_iter()
            .map(|s| json!({ "session_id": s.id, "host": s.host, "backend": "daemon" }))
            .collect(),
    ))
}

#[tokio::main]
async fn main() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let request: Value = serde_json::from_str(&line)?;

        // JSON-RPC notifications have no "id" — never send a response.
        let id = match request.get("id").cloned() {
            Some(id) => id,
            None => continue,
        };

        let response = match handle_request(&request).await {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(err) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": err.code, "message": err.message }
            }),
        };

        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }

    Ok(())
}

async fn handle_request(request: &Value) -> std::result::Result<Value, McpError> {
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| McpError::internal("missing method"))?;

    match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "agent2ssh-mcp", "version": "0.1.0" }
        })),
        "tools/list" => Ok(json!({
            "tools": [
                {
                    "name": "ssh_list_hosts",
                    "description": "List configured SSH host profiles.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_list_daemons",
                    "description": "List all configured daemons (localhost + remote daemons from ~/.agent2ssh/remotes.toml). Returns alias, url, and connected status for each.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_import_config",
                    "description": "Import SSH host profiles from ~/.ssh/config (or a custom path). Skips aliases that already exist.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Path to ssh config (default: ~/.ssh/config)." }
                        }
                    }
                },
                {
                    "name": "ssh_add_host",
                    "description": "Create or update an SSH host profile.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["name", "host"],
                        "properties": {
                            "name":      { "type": "string" },
                            "host":      { "type": "string" },
                            "user":      { "type": "string" },
                            "port":      { "type": "integer" },
                            "key_path":  { "type": "string" },
                            "password":  { "type": "string", "description": "SSH password for password-based authentication. Prefer key_path for production." },
                            "jump_host": { "type": "string", "description": "Host profile alias to use as ProxyJump bastion." },
                            "tags":      { "type": "array", "items": { "type": "string" } },
                            "env":       { "type": "string", "description": "Environment label for grouping hosts." },
                            "role":      { "type": "string", "description": "Role label for grouping hosts." },
                            "owner":     { "type": "string", "description": "Owner label for grouping hosts." }
                        }
                    }
                },
                {
                    "name": "ssh_remove_host",
                    "description": "Remove a configured SSH host profile by alias.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["name"],
                        "properties": {
                            "name": { "type": "string", "description": "The host alias to remove." }
                        }
                    }
                },
                {
                    "name": "ssh_exec",
                    "description": "Run a non-interactive command over SSH. Returns stdout, stderr, exit code, timing, and risk_level. High-risk commands require force=true; blocked commands always fail. Optionally forward to a remote daemon via daemon_alias.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["host", "command"],
                        "properties": {
                            "host":         { "type": "string" },
                            "command":      { "type": "string" },
                            "force":        { "type": "boolean", "description": "Set true to run high-risk commands." },
                            "timeout_secs":     { "type": "integer", "description": "Kill the command after N seconds (default 60)." },
                            "stdin":            { "type": "string", "description": "Pipe this string into the remote command's stdin." },
                            "max_output_bytes": { "type": "integer", "description": "Truncate stdout to this many bytes (default 4 MiB)." },
                            "daemon_alias":     { "type": "string", "description": "Forward this exec to a remote daemon by alias (omit or 'localhost' for local)." },
                            "reason":           { "type": "string", "description": "Optional reason/note for this operation (audit trail)." },
                            "change_id":        { "type": "string", "description": "Optional change/ticket ID for this operation (audit trail)." }
                        }
                    }
                },
                {
                    "name": "ssh_ping",
                    "description": "Check SSH reachability of one or more hosts. Returns reachable status and latency for each.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["hosts"],
                        "properties": {
                            "hosts":        { "type": "array", "items": { "type": "string" } },
                            "timeout_secs": { "type": "integer", "default": 5 }
                        }
                    }
                },
                {
                    "name": "ssh_exec_multi",
                    "description": "Run the same command on multiple hosts concurrently. Returns an array of per-host results. Supports optional batch strategy for concurrency limits, failure thresholds, and batched rollout.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["hosts", "command"],
                        "properties": {
                            "hosts":        { "type": "array", "items": { "type": "string" }, "description": "List of host profile aliases." },
                            "command":      { "type": "string" },
                            "force":        { "type": "boolean" },
                            "timeout_secs": { "type": "integer" },
                            "tags":         { "type": "array", "items": { "type": "string" }, "description": "Expand hosts by tag." },
                            "strategy":     {
                                "type": "object",
                                "description": "Optional batch execution strategy.",
                                "properties": {
                                    "concurrency":                { "type": "integer", "description": "Max concurrent hosts (0 = unlimited)." },
                                    "max_failures":               { "type": "integer", "description": "Stop after this many failures (0 = never stop)." },
                                    "batch_size":                 { "type": "integer", "description": "Execute in batches of this size." },
                                    "pause_between_batches_secs": { "type": "integer", "description": "Pause between batches (seconds)." }
                                }
                            },
                            "reason":       { "type": "string", "description": "Optional reason/note for this operation (audit trail)." },
                            "change_id":    { "type": "string", "description": "Optional change/ticket ID for this operation (audit trail)." }
                        }
                    }
                },
                {
                    "name": "ssh_exec_compare",
                    "description": "Compare execution results across multiple hosts. Groups by exit code and highlights stdout/stderr differences. Provide either results directly or run a command on multiple hosts.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "hosts":        { "type": "array", "items": { "type": "string" }, "description": "List of host profile aliases to execute and compare." },
                            "command":      { "type": "string", "description": "Command to run on all hosts." },
                            "force":        { "type": "boolean" },
                            "timeout_secs": { "type": "integer" },
                            "tags":         { "type": "array", "items": { "type": "string" }, "description": "Expand hosts by tag." }
                        }
                    }
                },
                {
                    "name": "ssh_audit",
                    "description": "Return recent SSH execution audit log entries with optional filtering.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "limit":      { "type": "integer", "default": 20 },
                            "host":       { "type": "string", "description": "Filter by host alias." },
                            "risk_level": { "type": "string", "enum": ["low","medium","high","blocked"] },
                            "exit_code":  { "type": "integer", "description": "Filter by exit code." },
                            "since":      { "type": "string", "description": "ISO-8601 lower bound." },
                            "until":      { "type": "string", "description": "ISO-8601 upper bound." },
                            "search":     { "type": "string", "description": "Full-text search across command and host fields." },
                            "command_pattern": { "type": "string", "description": "Command pattern (glob-style: *, ?)." },
                            "host_env":   { "type": "string", "description": "Filter by host environment label." },
                            "host_role":  { "type": "string", "description": "Filter by host role label." },
                            "host_owner": { "type": "string", "description": "Filter by host owner label." }
                        }
                    }
                },
                {
                    "name": "ssh_audit_export",
                    "description": "Export audit log entries as JSONL or CSV format with optional filtering. Redaction is applied at write time so exported data preserves redaction.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["format"],
                        "properties": {
                            "format":     { "type": "string", "enum": ["jsonl", "csv"], "description": "Export format: jsonl (one JSON per line) or csv." },
                            "limit":      { "type": "integer", "default": 20 },
                            "host":       { "type": "string", "description": "Filter by host alias." },
                            "risk_level": { "type": "string", "enum": ["low","medium","high","blocked"] },
                            "exit_code":  { "type": "integer", "description": "Filter by exit code." },
                            "since":      { "type": "string", "description": "ISO-8601 lower bound." },
                            "until":      { "type": "string", "description": "ISO-8601 upper bound." },
                            "search":     { "type": "string", "description": "Full-text search across command and host fields." },
                            "command_pattern": { "type": "string", "description": "Command pattern (glob-style: *, ?)." },
                            "host_env":   { "type": "string", "description": "Filter by host environment label." },
                            "host_role":  { "type": "string", "description": "Filter by host role label." },
                            "host_owner": { "type": "string", "description": "Filter by host owner label." }
                        }
                    }
                },
                {
                    "name": "ssh_sftp_ls",
                    "description": "List a remote directory (runs ls -la on the remote host).",
                    "inputSchema": {
                        "type": "object",
                        "required": ["host", "path"],
                        "properties": {
                            "host":         { "type": "string" },
                            "path":         { "type": "string" },
                            "timeout_secs": { "type": "integer" }
                        }
                    }
                },
                {
                    "name": "ssh_sftp_stat",
                    "description": "Stat a remote file or directory (runs stat on the remote host).",
                    "inputSchema": {
                        "type": "object",
                        "required": ["host", "path"],
                        "properties": {
                            "host":         { "type": "string" },
                            "path":         { "type": "string" },
                            "timeout_secs": { "type": "integer" }
                        }
                    }
                },
                {
                    "name": "ssh_sftp_mkdir",
                    "description": "Create a directory on a remote host (runs mkdir -p).",
                    "inputSchema": {
                        "type": "object",
                        "required": ["host", "path"],
                        "properties": {
                            "host":         { "type": "string" },
                            "path":         { "type": "string" },
                            "timeout_secs": { "type": "integer" }
                        }
                    }
                },
                {
                    "name": "ssh_sftp_upload",
                    "description": "Upload a local file to a remote host via scp.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["host", "local_path", "remote_path"],
                        "properties": {
                            "host":        { "type": "string", "description": "Host profile alias." },
                            "local_path":  { "type": "string", "description": "Local file path to upload." },
                            "remote_path": { "type": "string", "description": "Destination path on the remote host." }
                        }
                    }
                },
                {
                    "name": "ssh_sftp_download",
                    "description": "Download a file from a remote host via scp.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["host", "remote_path", "local_path"],
                        "properties": {
                            "host":        { "type": "string", "description": "Host profile alias." },
                            "remote_path": { "type": "string", "description": "Remote file path to download." },
                            "local_path":  { "type": "string", "description": "Local destination path." }
                        }
                    }
                },
                {
                    "name": "ssh_session_open",
                    "description": "Open a persistent interactive PTY session to a host. Returns a session_id for subsequent write/read/close calls.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["host"],
                        "properties": {
                            "host": { "type": "string", "description": "Host profile alias." }
                        }
                    }
                },
                {
                    "name": "ssh_session_write",
                    "description": "Send input to an open PTY session (e.g. a command followed by \\n).",
                    "inputSchema": {
                        "type": "object",
                        "required": ["session_id", "input"],
                        "properties": {
                            "session_id": { "type": "string" },
                            "input":      { "type": "string", "description": "Text to write to session stdin. Include \\n to submit a command." }
                        }
                    }
                },
                {
                    "name": "ssh_session_read",
                    "description": "Read buffered output from a PTY session. Returns whatever arrived since the last read.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["session_id"],
                        "properties": {
                            "session_id": { "type": "string" },
                            "timeout_ms": { "type": "integer", "default": 2000, "description": "How long to wait for output before returning." }
                        }
                    }
                },
                {
                    "name": "ssh_session_close",
                    "description": "Close and terminate a PTY session.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["session_id"],
                        "properties": {
                            "session_id": { "type": "string" }
                        }
                    }
                },
                {
                    "name": "ssh_session_list",
                    "description": "List open PTY sessions. Defaults to the local daemon registry and includes MCP process-local fallback sessions when present.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_forward_add",
                    "description": "Start an SSH port forward tunnel (-L local or -R remote). Returns a forward_id.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["host", "bind_port", "target_host", "target_port"],
                        "properties": {
                            "host":        { "type": "string" },
                            "direction":   { "type": "string", "enum": ["local", "remote"], "default": "local" },
                            "bind_port":   { "type": "integer" },
                            "target_host": { "type": "string" },
                            "target_port": { "type": "integer" }
                        }
                    }
                },
                {
                    "name": "ssh_forward_list",
                    "description": "List active SSH port forward tunnels.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_forward_remove",
                    "description": "Stop and remove an SSH port forward by ID.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["forward_id"],
                        "properties": {
                            "forward_id": { "type": "string" }
                        }
                    }
                },
                {
                    "name": "ssh_risk_check",
                    "description": "Check the risk level of a command using built-in rules and user-defined risk_rules.toml.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["command"],
                        "properties": {
                            "command": { "type": "string", "description": "The command to check." },
                            "host":    { "type": "string", "description": "Optional host alias to check per-host overrides." }
                        }
                    }
                },
                {
                    "name": "ssh_gate_status",
                    "description": "Read the local daemon execution gate status. When paused, non-desktop daemon execution is rejected until resumed from CLI or desktop.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_approval_list",
                    "description": "List all pending and recent approval requests (for high-risk command authorization).",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_approval_respond",
                    "description": "Approve or reject a pending approval request by ID.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["id", "approved"],
                        "properties": {
                            "id":       { "type": "string", "description": "The approval request UUID." },
                            "approved": { "type": "boolean", "description": "true to approve, false to reject." }
                        }
                    }
                },
                {
                    "name": "ssh_playbook_list",
                    "description": "List all configured playbooks (command templates) from ~/.agent2ssh/playbooks.toml.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_playbook_run",
                    "description": "Run a named playbook (sequence of SSH commands) against a target host. Steps execute sequentially; halts on first failure. Supports template parameters via the params object.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["playbook", "host"],
                        "properties": {
                            "playbook": { "type": "string", "description": "Name of the playbook to run." },
                            "host":     { "type": "string", "description": "Target host profile alias." },
                            "force":    { "type": "boolean", "description": "Set true to allow high-risk steps within the playbook." },
                            "params":   { "type": "object", "description": "Key-value parameters to substitute into step command templates ({{param_name}} syntax)." },
                            "reason":   { "type": "string", "description": "Optional reason/note for this operation (audit trail)." },
                            "change_id": { "type": "string", "description": "Optional change/ticket ID for this operation (audit trail)." }
                        }
                    }
                },
                {
                    "name": "ssh_playbook_dry_run",
                    "description": "Preview a playbook without executing. Resolves template parameters and returns the commands that would be run.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["playbook"],
                        "properties": {
                            "playbook": { "type": "string", "description": "Name of the playbook to preview." },
                            "params":   { "type": "object", "description": "Key-value parameters to substitute into step command templates." }
                        }
                    }
                },
                {
                    "name": "ssh_connection_status",
                    "description": "List all configured hosts and their current ControlMaster connection status (connected/disconnected, socket path).",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_connect",
                    "description": "Manually establish a persistent ControlMaster connection to a specific host.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["host"],
                        "properties": {
                            "host": { "type": "string", "description": "Host profile alias to connect to." }
                        }
                    }
                },
                {
                    "name": "ssh_disconnect",
                    "description": "Manually close an existing ControlMaster connection to a specific host.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["host"],
                        "properties": {
                            "host": { "type": "string", "description": "Host profile alias to disconnect from." }
                        }
                    }
                },
                {
                    "name": "ssh_webhook_config",
                    "description": "Get or set webhook notification configuration. Use action='get' to retrieve current config, or action='set' with url/events/secret to update.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["action"],
                        "properties": {
                            "action": { "type": "string", "enum": ["get", "set"], "description": "Use 'get' to read current config or 'set' to update." },
                            "url":    { "type": "string", "description": "Webhook URL to POST events to." },
                            "events": { "type": "array", "items": { "type": "string", "enum": ["approval_required", "exec_blocked", "exec_completed"] }, "description": "Event types to subscribe to." },
                            "secret": { "type": "string", "description": "HMAC-SHA256 signing secret for X-Agent2SSH-Signature header." }
                        }
                    }
                },
                {
                    "name": "ssh_config_export",
                    "description": "Export team configuration (hosts without private key paths, risk rules, and playbooks). Returns a JSON object suitable for sharing within a team.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_config_import",
                    "description": "Import team configuration from a JSON object. Merges hosts (skips duplicates by name), and overwrites risk rules and playbooks if provided.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["config"],
                        "properties": {
                            "config": {
                                "type": "object",
                                "description": "Team config export object with hosts, risk_rules, and playbooks fields.",
                                "properties": {
                                    "hosts":      { "type": "array", "items": { "type": "object" } },
                                    "risk_rules":  { "type": "string", "description": "Raw TOML content of risk rules." },
                                    "playbooks":   { "type": "string", "description": "Raw TOML content of playbooks." }
                                }
                            }
                        }
                    }
                },
                {
                    "name": "ssh_config_import_preview",
                    "description": "Preview what a team config import will change without actually importing. Shows hosts to add, skip, update, and risk rules/playbook changes.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["config"],
                        "properties": {
                            "config": {
                                "type": "object",
                                "description": "Team config export object with hosts, risk_rules, and playbooks fields.",
                                "properties": {
                                    "hosts":      { "type": "array", "items": { "type": "object" } },
                                    "risk_rules":  { "type": "string", "description": "Raw TOML content of risk rules." },
                                    "playbooks":   { "type": "string", "description": "Raw TOML content of playbooks." }
                                }
                            }
                        }
                    }
                },
                {
                    "name": "ssh_doctor",
                    "description": "Run diagnostic checks on the agent2ssh environment: verify ssh/ssh-keygen binaries, config directory, hosts.json, daemon.token permissions, daemon health, optional config files, and audit log size.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_metrics",
                    "description": "Retrieve basic metrics from the local agent2ssh daemon (requests, execs, blocked commands, durations, approvals). Reads from GET /metrics on 127.0.0.1:7722.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_preview_exec",
                    "description": "Preview what an execution will do before running it. Returns target hosts, commands, risk levels, warnings, and whether approval is required. Supports single-host and multi-host preview.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["command"],
                        "properties": {
                            "host":         { "type": "string", "description": "Single target host profile alias." },
                            "hosts":        { "type": "array", "items": { "type": "string" }, "description": "Multiple target host profile aliases (for multi-host preview)." },
                            "command":      { "type": "string", "description": "The command to preview." },
                            "timeout_secs": { "type": "integer", "description": "Timeout that would be used for execution (default 60)." },
                            "tags":         { "type": "array", "items": { "type": "string" }, "description": "Tags to expand into host names for multi-host preview." }
                        }
                    }
                },
                {
                    "name": "ssh_approval_policies_list",
                    "description": "List all configured approval policies. Each policy specifies when approval is required based on host, tags, risk level, and command pattern.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_approval_check",
                    "description": "Check if running a command on a specific host requires approval based on configured policies. Returns the matching policy name and whether approval is needed.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["host", "command"],
                        "properties": {
                            "host":    { "type": "string", "description": "Host profile alias." },
                            "command": { "type": "string", "description": "The command to check." }
                        }
                    }
                },
                {
                    "name": "ssh_health_snapshot",
                    "description": "Collect health snapshot (uptime, disk, memory, load, SSH latency) for configured hosts. Returns per-host health data collected concurrently via SSH.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "hosts":        { "type": "array", "items": { "type": "string" }, "description": "Host aliases to collect health from (default: all configured hosts)." },
                            "timeout_secs": { "type": "integer", "description": "SSH connection timeout in seconds (default 10)." }
                        }
                    }
                },
                {
                    "name": "ssh_daemon_diagnose",
                    "description": "Run connection diagnostics on a remote daemon: checks TCP connectivity, TLS handshake, token configuration, authentication, version compatibility, and latency. Returns a detailed diagnostic report.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["alias"],
                        "properties": {
                            "alias": { "type": "string", "description": "The remote daemon alias from ~/.agent2ssh/remotes.toml (e.g. 'prod')." }
                        }
                    }
                },
                {
                    "name": "ssh_daemon_version_check",
                    "description": "Check version compatibility between this build and a remote daemon. Returns local version, remote version, compatibility status, and a human-readable message.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["alias"],
                        "properties": {
                            "alias": { "type": "string", "description": "The remote daemon alias from ~/.agent2ssh/remotes.toml (e.g. 'prod')." }
                        }
                    }
                },
                {
                    "name": "ssh_daemons_view",
                    "description": "Get a unified view of all daemons (localhost + remotes) with their health, metrics, and host counts.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_metrics_trend",
                    "description": "Show execution metrics trends: volume, failure rate, risk distribution, top hosts, and hourly breakdown. Supports period selection.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "period": { "type": "string", "enum": ["24h", "7d", "30d", "all"], "default": "24h", "description": "Time period for the trend report." }
                        }
                    }
                },
                {
                    "name": "ssh_events_subscribe",
                    "description": "Subscribe to the real-time event stream. Returns the latest events from the event bus. Note: for continuous streaming, use the daemon's SSE endpoint GET /events/stream.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_sync_diff",
                    "description": "Compare Agent2SSH hosts with ~/.ssh/config. Shows hosts only in one side and conflicts.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Path to SSH config file (default: ~/.ssh/config)" }
                        }
                    }
                },
                {
                    "name": "ssh_sync_export",
                    "description": "Export Agent2SSH hosts to SSH config format file.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Path to write SSH config (default: ~/.ssh/config.d/agent2ssh.conf)" }
                        }
                    }
                }
            ]
        })),
        "tools/call" => {
            let params = request
                .get("params")
                .ok_or_else(|| McpError::internal("missing params"))?;
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| McpError::internal("missing tool name"))?;
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            call_tool(name, args).await
        }
        other => Err(McpError::method_not_found(other)),
    }
}

async fn call_tool(name: &str, args: Value) -> std::result::Result<Value, McpError> {
    let payload = match name {
        "ssh_import_config" => {
            let path = args["path"].as_str();
            serde_json::to_value(import_ssh_config_core(path).map_err(McpError::from)?)?
        }
        "ssh_list_hosts" => serde_json::to_value(list_hosts_core().map_err(McpError::from)?)?,
        "ssh_list_daemons" => serde_json::to_value(list_daemons_core().map_err(McpError::from)?)?,
        "ssh_add_host" => {
            let host: HostProfile = serde_json::from_value(args).map_err(McpError::internal)?;
            serde_json::to_value(add_host_core(host).map_err(McpError::from)?)?
        }
        "ssh_remove_host" => {
            let host_name = args["name"]
                .as_str()
                .ok_or_else(|| McpError::internal("name required"))?;
            remove_host_core(host_name).map_err(McpError::from)?;
            json!({ "removed": host_name })
        }
        "ssh_ping" => {
            let hosts: Vec<String> = args["hosts"]
                .as_array()
                .ok_or_else(|| McpError::internal("hosts array required"))?
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            let timeout_secs = args["timeout_secs"].as_u64();
            serde_json::to_value(ping_hosts_core(hosts, timeout_secs).await)?
        }
        "ssh_exec" => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?
                .to_string();
            let command = args["command"]
                .as_str()
                .ok_or_else(|| McpError::internal("command required"))?
                .to_string();
            let force = args["force"].as_bool().unwrap_or(false);
            let timeout_secs = args["timeout_secs"].as_u64();
            let stdin = args["stdin"].as_str().map(str::to_string);
            let daemon_alias = args["daemon_alias"].as_str().map(str::to_string);
            let reason = args["reason"].as_str().map(str::to_string);
            let change_id = args["change_id"].as_str().map(str::to_string);

            let max_output_bytes = args["max_output_bytes"].as_u64().map(|v| v as usize);
            let mut request = ExecRequest {
                host,
                command,
                force,
                timeout_secs,
                stdin,
                max_output_bytes,
                reason,
                change_id,
                source: Some(mcp_source()),
            };

            // If daemon_alias is set and not "localhost", forward to remote daemon
            if let Some(ref alias) = daemon_alias {
                if alias != "localhost" {
                    let (url, remote_token, scope) = get_daemon_with_scope(alias)
                        .map_err(|e| McpError::internal(format!("daemon lookup failed: {e}")))?;
                    let tags = mcp_host_tags(&request.host);
                    check_daemon_scope(&scope, &request.host, &tags, &request.command)
                        .map_err(McpError::internal)?;
                    let token = remote_token.ok_or_else(|| {
                        McpError::internal(format!("no token configured for daemon '{alias}'"))
                    })?;

                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(
                            request.timeout_secs.unwrap_or(60) + 10,
                        ))
                        .build()
                        .map_err(McpError::internal)?;

                    let resp = client
                        .post(format!("{}/exec", url.trim_end_matches('/')))
                        .bearer_auth(&token)
                        .json(&request)
                        .send()
                        .await
                        .map_err(|e| McpError::internal(format!("remote exec failed: {e}")))?;

                    if !resp.status().is_success() {
                        let body = resp.text().await.unwrap_or_default();
                        return Err(McpError::internal(format!("remote daemon error: {body}")));
                    }

                    let result: agent2ssh::types::ExecResult = resp.json().await.map_err(|e| {
                        McpError::internal(format!("invalid response from remote: {e}"))
                    })?;
                    serde_json::to_value(result)?
                } else {
                    let risk = authorize_local_mcp_exec_request(&mut request).await?;
                    let result = exec_ssh_core(request)
                        .await
                        .map_err(|e| McpError::internal(format!("{e} (risk_level={risk})")))?;
                    serde_json::to_value(result)?
                }
            } else {
                let risk = authorize_local_mcp_exec_request(&mut request).await?;
                let result = exec_ssh_core(request)
                    .await
                    .map_err(|e| McpError::internal(format!("{e} (risk_level={risk})")))?;
                serde_json::to_value(result)?
            }
        }
        "ssh_exec_multi" => {
            let hosts: Vec<String> = args["hosts"]
                .as_array()
                .ok_or_else(|| McpError::internal("hosts array required"))?
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            let command = args["command"]
                .as_str()
                .ok_or_else(|| McpError::internal("command required"))?
                .to_string();
            let force = args["force"].as_bool().unwrap_or(false);
            let timeout_secs = args["timeout_secs"].as_u64();
            let tags: Option<Vec<String>> = args["tags"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            });
            let reason = args["reason"].as_str().map(str::to_string);
            let change_id = args["change_id"].as_str().map(str::to_string);

            // Parse optional strategy
            let strategy: Option<agent2ssh::types::BatchStrategy> = args["strategy"]
                .as_object()
                .map(|obj| agent2ssh::types::BatchStrategy {
                    concurrency: obj
                        .get("concurrency")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as usize),
                    max_failures: obj
                        .get("max_failures")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as usize),
                    batch_size: obj
                        .get("batch_size")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as usize),
                    pause_between_batches_secs: obj
                        .get("pause_between_batches_secs")
                        .and_then(|v| v.as_u64()),
                });

            let source = mcp_source();
            let mut force = force;
            if authorize_local_mcp_exec_targets(
                &hosts,
                &tags,
                &command,
                force,
                reason.clone(),
                change_id.clone(),
                &source,
            )
            .await?
            {
                force = true;
            }

            let batch_result = exec_multi_with_strategy(ExecMultiBatchRequest {
                request: ExecMultiRequest {
                    hosts,
                    command,
                    force,
                    timeout_secs,
                    tags,
                    reason,
                    change_id,
                    source: Some(source),
                },
                strategy,
            })
            .await;
            serde_json::to_value(batch_result)?
        }
        "ssh_exec_compare" => {
            let hosts: Vec<String> = args["hosts"]
                .as_array()
                .ok_or_else(|| McpError::internal("hosts array required"))?
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            let command = args["command"]
                .as_str()
                .ok_or_else(|| McpError::internal("command required"))?
                .to_string();
            let force = args["force"].as_bool().unwrap_or(false);
            let timeout_secs = args["timeout_secs"].as_u64();
            let tags: Option<Vec<String>> = args["tags"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            });

            let source = mcp_source();
            let mut force = force;
            if authorize_local_mcp_exec_targets(&hosts, &tags, &command, force, None, None, &source)
                .await?
            {
                force = true;
            }

            let results = exec_multi_core(ExecMultiRequest {
                hosts,
                command,
                force,
                timeout_secs,
                tags,
                reason: None,
                change_id: None,
                source: Some(source),
            })
            .await;
            let comparison = compare_exec_results(&results);
            serde_json::to_value(comparison)?
        }
        "ssh_sftp_ls" => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
            let path = args["path"]
                .as_str()
                .ok_or_else(|| McpError::internal("path required"))?;
            let timeout_secs = args["timeout_secs"].as_u64();
            let source = mcp_source();
            let command = format!("sftp ls {}", path);
            authorize_local_mcp_operation(host, &command, true, &source).await?;
            serde_json::to_value(
                sftp_ls_core_with_source(host, path, timeout_secs, Some(source))
                    .await
                    .map_err(McpError::from)?,
            )?
        }
        "ssh_sftp_stat" => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
            let path = args["path"]
                .as_str()
                .ok_or_else(|| McpError::internal("path required"))?;
            let timeout_secs = args["timeout_secs"].as_u64();
            let source = mcp_source();
            let command = format!("sftp stat {}", path);
            authorize_local_mcp_operation(host, &command, true, &source).await?;
            serde_json::to_value(
                sftp_stat_core_with_source(host, path, timeout_secs, Some(source))
                    .await
                    .map_err(McpError::from)?,
            )?
        }
        "ssh_sftp_mkdir" => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
            let path = args["path"]
                .as_str()
                .ok_or_else(|| McpError::internal("path required"))?;
            let timeout_secs = args["timeout_secs"].as_u64();
            let source = mcp_source();
            let command = format!("sftp mkdir {}", path);
            authorize_local_mcp_operation(host, &command, true, &source).await?;
            serde_json::to_value(
                sftp_mkdir_core_with_source(host, path, timeout_secs, Some(source))
                    .await
                    .map_err(McpError::from)?,
            )?
        }
        "ssh_audit" => {
            let risk_level = args["risk_level"].as_str().and_then(|s| {
                serde_json::from_value::<RiskLevel>(serde_json::Value::String(s.to_string())).ok()
            });
            let filter = AuditFilter {
                host: args["host"].as_str().map(str::to_string),
                risk_level,
                exit_code: args["exit_code"].as_i64().map(|v| v as i32),
                since: args["since"].as_str().map(str::to_string),
                until: args["until"].as_str().map(str::to_string),
                limit: args["limit"].as_u64().unwrap_or(20) as usize,
                search: args["search"].as_str().map(str::to_string),
                command_pattern: args["command_pattern"].as_str().map(str::to_string),
                host_env: args["host_env"].as_str().map(str::to_string),
                host_role: args["host_role"].as_str().map(str::to_string),
                host_owner: args["host_owner"].as_str().map(str::to_string),
            };
            serde_json::to_value(list_audit_core(filter).map_err(McpError::from)?)?
        }
        "ssh_audit_export" => {
            let format = args["format"]
                .as_str()
                .ok_or_else(|| McpError::internal("format required: 'jsonl' or 'csv'"))?;
            let risk_level = args["risk_level"].as_str().and_then(|s| {
                serde_json::from_value::<RiskLevel>(serde_json::Value::String(s.to_string())).ok()
            });
            let filter = AuditFilter {
                host: args["host"].as_str().map(str::to_string),
                risk_level,
                exit_code: args["exit_code"].as_i64().map(|v| v as i32),
                since: args["since"].as_str().map(str::to_string),
                until: args["until"].as_str().map(str::to_string),
                limit: args["limit"].as_u64().unwrap_or(20) as usize,
                search: args["search"].as_str().map(str::to_string),
                command_pattern: args["command_pattern"].as_str().map(str::to_string),
                host_env: args["host_env"].as_str().map(str::to_string),
                host_role: args["host_role"].as_str().map(str::to_string),
                host_owner: args["host_owner"].as_str().map(str::to_string),
            };
            let exported = match format {
                "jsonl" => export_audit_jsonl(&filter).map_err(McpError::from)?,
                "csv" => export_audit_csv(&filter).map_err(McpError::from)?,
                other => {
                    return Err(McpError::internal(format!(
                        "unsupported format '{}', expected 'jsonl' or 'csv'",
                        other
                    )))
                }
            };
            json!({ "format": format, "data": exported })
        }
        "ssh_sftp_upload" => {
            let request: SftpUploadRequest =
                serde_json::from_value(args).map_err(McpError::internal)?;
            let source = mcp_source();
            let command = format!(
                "sftp upload {} -> {}",
                request.local_path, request.remote_path
            );
            authorize_local_mcp_operation(&request.host, &command, true, &source).await?;
            serde_json::to_value(
                sftp_upload_core_with_source(request, Some(source))
                    .await
                    .map_err(McpError::from)?,
            )?
        }
        "ssh_sftp_download" => {
            let request: SftpDownloadRequest =
                serde_json::from_value(args).map_err(McpError::internal)?;
            let source = mcp_source();
            let command = format!(
                "sftp download {} -> {}",
                request.remote_path, request.local_path
            );
            authorize_local_mcp_operation(&request.host, &command, true, &source).await?;
            serde_json::to_value(
                sftp_download_core_with_source(request, Some(source))
                    .await
                    .map_err(McpError::from)?,
            )?
        }
        "ssh_session_open" => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
            match try_daemon_session_open(host).await? {
                DaemonAttempt::Handled(value) => value,
                DaemonAttempt::Fallback => {
                    let source = mcp_source();
                    authorize_local_mcp_operation(host, "session_open", true, &source).await?;
                    let id = session_open_core(host).await.map_err(McpError::from)?;
                    json!({ "session_id": id.to_string(), "host": host, "backend": "process", "source": source })
                }
            }
        }
        "ssh_session_write" => {
            let session_id = args["session_id"]
                .as_str()
                .ok_or_else(|| McpError::internal("session_id required"))?;
            let input = args["input"]
                .as_str()
                .ok_or_else(|| McpError::internal("input required"))?;
            match try_daemon_session_write(session_id, input).await? {
                DaemonAttempt::Handled(value) => value,
                DaemonAttempt::Fallback => {
                    let id: uuid::Uuid = session_id
                        .parse()
                        .map_err(|e| McpError::internal(format!("invalid session_id: {e}")))?;
                    let source = mcp_source();
                    let host = session_list_core()
                        .await
                        .into_iter()
                        .find(|(open_id, _)| *open_id == id)
                        .map(|(_, host)| host)
                        .unwrap_or_else(|| format!("session:{session_id}"));
                    authorize_local_mcp_operation(&host, input, false, &source).await?;
                    session_write_core(id, input)
                        .await
                        .map_err(McpError::from)?;
                    json!({ "ok": true, "backend": "process", "source": source })
                }
            }
        }
        "ssh_session_read" => {
            let session_id = args["session_id"]
                .as_str()
                .ok_or_else(|| McpError::internal("session_id required"))?;
            let timeout_ms = args["timeout_ms"].as_u64().unwrap_or(2000);
            match try_daemon_session_read(session_id, timeout_ms).await? {
                DaemonAttempt::Handled(value) => value,
                DaemonAttempt::Fallback => {
                    let id: uuid::Uuid = session_id
                        .parse()
                        .map_err(|e| McpError::internal(format!("invalid session_id: {e}")))?;
                    let output = session_read_core(id, timeout_ms)
                        .await
                        .map_err(McpError::from)?;
                    json!({ "output": output, "backend": "process", "source": mcp_source() })
                }
            }
        }
        "ssh_session_close" => {
            let session_id = args["session_id"]
                .as_str()
                .ok_or_else(|| McpError::internal("session_id required"))?;
            match try_daemon_session_close(session_id).await? {
                DaemonAttempt::Handled(value) => value,
                DaemonAttempt::Fallback => {
                    let id: uuid::Uuid = session_id
                        .parse()
                        .map_err(|e| McpError::internal(format!("invalid session_id: {e}")))?;
                    session_close_core(id).await.map_err(McpError::from)?;
                    json!({ "closed": id.to_string(), "backend": "process", "source": mcp_source() })
                }
            }
        }
        "ssh_session_list" => {
            let mut items = match try_daemon_session_list().await? {
                DaemonAttempt::Handled(items) => items,
                DaemonAttempt::Fallback => Vec::new(),
            };
            items.extend(
                session_list_core()
                    .await
                    .iter()
                    .map(|(id, host)| json!({ "session_id": id.to_string(), "host": host, "backend": "process" })),
            );
            json!(items)
        }
        "ssh_forward_add" => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
            let direction = match args["direction"].as_str().unwrap_or("local") {
                "remote" => ForwardDirection::Remote,
                _ => ForwardDirection::Local,
            };
            let bind_port = args["bind_port"]
                .as_u64()
                .ok_or_else(|| McpError::internal("bind_port required"))?
                as u16;
            let target_host = args["target_host"]
                .as_str()
                .ok_or_else(|| McpError::internal("target_host required"))?;
            let target_port = args["target_port"]
                .as_u64()
                .ok_or_else(|| McpError::internal("target_port required"))?
                as u16;
            let source = mcp_source();
            let command = format!(
                "forward {} {}:{} -> {}:{}",
                direction, bind_port, target_host, host, target_port
            );
            authorize_local_mcp_operation(host, &command, true, &source).await?;
            let rule = forward_add_core(host, direction, bind_port, target_host, target_port)
                .await
                .map_err(McpError::from)?;
            serde_json::to_value(rule)?
        }
        "ssh_forward_list" => serde_json::to_value(forward_list_core().await)?,
        "ssh_forward_remove" => {
            let id: uuid::Uuid = args["forward_id"]
                .as_str()
                .ok_or_else(|| McpError::internal("forward_id required"))?
                .parse()
                .map_err(|e| McpError::internal(format!("invalid forward_id: {e}")))?;
            forward_remove_core(id).await.map_err(McpError::from)?;
            json!({ "removed": id.to_string() })
        }
        "ssh_risk_check" => {
            let command = args["command"]
                .as_str()
                .ok_or_else(|| McpError::internal("command required"))?;
            let user_risk = classify_with_user_rules(command).await;
            let final_risk = if let Some(host) = args["host"].as_str() {
                let target = command_authorization_target(host);
                agent2ssh::core::apply_risk_override(
                    effective_command_risk(command).await,
                    target.risk_override,
                )
            } else {
                effective_command_risk(command).await
            };
            json!({
                "command": command,
                "risk_level": final_risk,
                "matched_user_rule": user_risk.is_some(),
            })
        }
        "ssh_gate_status" => try_daemon_gate_status().await?,
        "ssh_approval_list" => {
            let approvals = approval_list().await;
            serde_json::to_value(approvals)?
        }
        "ssh_approval_respond" => {
            let id = args["id"]
                .as_str()
                .ok_or_else(|| McpError::internal("id required"))?;
            let approved = args["approved"]
                .as_bool()
                .ok_or_else(|| McpError::internal("approved boolean required"))?;
            let uuid: uuid::Uuid = id
                .parse()
                .map_err(|e| McpError::internal(format!("invalid id: {e}")))?;
            approval_respond(uuid, approved)
                .await
                .map_err(McpError::from)?;
            json!({ "ok": true, "id": id, "approved": approved })
        }
        "ssh_playbook_list" => {
            let playbooks = list_playbooks_core().map_err(McpError::from)?;
            let summaries: Vec<Value> = playbooks
                .iter()
                .map(|p| {
                    json!({
                        "name": p.name,
                        "description": p.description,
                        "step_count": p.steps.len(),
                        "tags": p.tags,
                    })
                })
                .collect();
            serde_json::to_value(summaries)?
        }
        "ssh_playbook_run" => {
            let playbook = args["playbook"]
                .as_str()
                .ok_or_else(|| McpError::internal("playbook required"))?;
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
            let force = args["force"].as_bool().unwrap_or(false);
            let params_map: Option<HashMap<String, String>> =
                args["params"].as_object().map(|obj| {
                    obj.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                });
            let reason = args["reason"].as_str().map(str::to_string);
            let change_id = args["change_id"].as_str().map(str::to_string);
            let source = mcp_source();
            let mut force = force;
            let empty_params = HashMap::new();
            let params_for_auth = params_map.as_ref().unwrap_or(&empty_params);
            if authorize_local_mcp_playbook_run(
                playbook,
                host,
                force,
                params_for_auth,
                reason.clone(),
                change_id.clone(),
                &source,
            )
            .await?
            {
                force = true;
            }
            let result = run_playbook_core_with_source(
                playbook,
                host,
                force,
                params_map.as_ref(),
                reason,
                change_id,
                Some(source),
            )
            .await
            .map_err(McpError::from)?;
            serde_json::to_value(result)?
        }
        "ssh_playbook_dry_run" => {
            let playbook = args["playbook"]
                .as_str()
                .ok_or_else(|| McpError::internal("playbook required"))?;
            let params_map: HashMap<String, String> = args["params"]
                .as_object()
                .map(|obj| {
                    obj.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            let result = dry_run_playbook(playbook, &params_map).map_err(McpError::from)?;
            serde_json::to_value(result)?
        }
        "ssh_connection_status" => {
            let statuses = list_active_connections().await;
            serde_json::to_value(statuses)?
        }
        "ssh_connect" => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
            let source = mcp_source();
            authorize_local_mcp_operation(host, "connect", true, &source).await?;
            connect_host(host).await.map_err(McpError::from)?;
            json!({ "ok": true, "host": host })
        }
        "ssh_disconnect" => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
            let source = mcp_source();
            authorize_local_mcp_operation(host, "disconnect", true, &source).await?;
            disconnect_host(host).await.map_err(McpError::from)?;
            json!({ "ok": true, "host": host })
        }
        "ssh_webhook_config" => {
            let action = args["action"]
                .as_str()
                .ok_or_else(|| McpError::internal("action required: 'get' or 'set'"))?;
            match action {
                "get" => {
                    let config = load_webhook_config().unwrap_or_default();
                    serde_json::to_value(config)?
                }
                "set" => {
                    let mut config = load_webhook_config().unwrap_or_default();
                    if let Some(url) = args["url"].as_str() {
                        config.url = Some(url.to_string());
                    }
                    if let Some(events) = args["events"].as_array() {
                        config.events = events
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect();
                    }
                    if let Some(secret) = args["secret"].as_str() {
                        config.secret = Some(secret.to_string());
                    }
                    save_webhook_config(&config).map_err(|e| {
                        McpError::internal(format!("failed to save webhook config: {e}"))
                    })?;
                    serde_json::to_value(config)?
                }
                other => {
                    return Err(McpError::internal(format!(
                        "unknown action '{}', expected 'get' or 'set'",
                        other
                    )));
                }
            }
        }
        "ssh_config_export" => {
            let export = export_team_config().map_err(McpError::from)?;
            serde_json::to_value(export)?
        }
        "ssh_config_import" => {
            let config_value = args
                .get("config")
                .ok_or_else(|| McpError::internal("config object required"))?;
            let export: TeamConfigExport = serde_json::from_value(config_value.clone())
                .map_err(|e| McpError::internal(format!("invalid config object: {e}")))?;
            let result = import_team_config(&export).map_err(McpError::from)?;
            serde_json::to_value(result)?
        }
        "ssh_config_import_preview" => {
            let config_value = args
                .get("config")
                .ok_or_else(|| McpError::internal("config object required"))?;
            let export: TeamConfigExport = serde_json::from_value(config_value.clone())
                .map_err(|e| McpError::internal(format!("invalid config object: {e}")))?;
            let preview = preview_team_config_import(&export).map_err(McpError::from)?;
            serde_json::to_value(preview)?
        }
        "ssh_doctor" => {
            let mut checks: Vec<Value> = Vec::new();

            // ssh binary
            let ssh_ok = which_check("ssh");
            checks.push(json!({"name": "ssh binary", "status": if ssh_ok {"pass"} else {"fail"}, "detail": if ssh_ok {"ssh found in PATH"} else {"ssh binary not found"}}));

            // ssh-keygen
            let keygen_ok = which_check("ssh-keygen");
            checks.push(json!({"name": "ssh-keygen binary", "status": if keygen_ok {"pass"} else {"warn"}, "detail": if keygen_ok {"ssh-keygen found"} else {"ssh-keygen not found"}}));

            // config directory
            let config_dir = agent2ssh::config_dir().map_err(McpError::from)?;
            let dir_ok = config_dir.exists();
            checks.push(json!({"name": "config directory", "status": if dir_ok {"pass"} else {"fail"}, "detail": format!("{}", config_dir.display())}));

            // hosts.json
            let hosts_path = config_dir.join("hosts.json");
            let hosts_ok = hosts_path.exists()
                && std::fs::read_to_string(&hosts_path)
                    .ok()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .is_some();
            checks.push(json!({"name": "hosts.json", "status": if hosts_path.exists() && hosts_ok {"pass"} else if hosts_path.exists() {"fail"} else {"warn"}, "detail": if !hosts_path.exists() {"not configured"} else if hosts_ok {"valid"} else {"invalid JSON"}}));

            // daemon.token
            let token_path = config_dir.join("daemon.token");
            if token_path.exists() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = std::fs::metadata(&token_path)
                        .map(|m| m.permissions().mode() & 0o777)
                        .unwrap_or(0o777);
                    checks.push(json!({"name": "daemon.token", "status": if mode == 0o600 {"pass"} else {"warn"}, "detail": format!("permissions 0{:o}", mode)}));
                }
                #[cfg(not(unix))]
                {
                    checks.push(
                        json!({"name": "daemon.token", "status": "pass", "detail": "exists"}),
                    );
                }
            } else {
                checks
                    .push(json!({"name": "daemon.token", "status": "warn", "detail": "not found"}));
            }

            // daemon health
            let daemon_ok = match reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(2))
                .build()
            {
                Ok(client) => match client.get("http://127.0.0.1:7722/health").send().await {
                    Ok(resp) => resp.status().is_success(),
                    Err(_) => false,
                },
                Err(_) => false,
            };
            checks.push(json!({"name": "daemon health", "status": if daemon_ok {"pass"} else {"warn"}, "detail": if daemon_ok {"healthy"} else {"not reachable"}}));

            // optional config files
            for (filename, label) in &[
                ("risk_rules.toml", "risk rules"),
                ("playbooks.toml", "playbooks"),
                ("remotes.toml", "remote daemons"),
                ("webhook.toml", "webhook config"),
            ] {
                let exists = config_dir.join(filename).exists();
                checks.push(json!({"name": format!("{filename} ({label})"), "status": if exists {"pass"} else {"warn"}, "detail": if exists {"present"} else {"not found"}}));
            }

            // audit log
            if let Ok(audit_p) = audit_path() {
                if audit_p.exists() {
                    let size = std::fs::metadata(&audit_p).map(|m| m.len()).unwrap_or(0);
                    let size_mb = size as f64 / (1024.0 * 1024.0);
                    checks.push(json!({"name": "audit log", "status": if size_mb > 10.0 {"warn"} else {"pass"}, "detail": format!("{:.2} MB", size_mb)}));
                } else {
                    checks.push(json!({"name": "audit log", "status": "pass", "detail": "no audit log yet"}));
                }
            }

            json!(checks)
        }
        "ssh_metrics" => {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(3))
                .build()
                .map_err(|e| McpError::internal(format!("client build failed: {e}")))?;
            let resp = client
                .get("http://127.0.0.1:7722/metrics")
                .send()
                .await
                .map_err(|e| McpError::internal(format!("daemon /metrics unreachable: {e}")))?;
            if !resp.status().is_success() {
                return Err(McpError::internal(format!(
                    "daemon returned status {}",
                    resp.status()
                )));
            }
            let metrics: Value = resp
                .json()
                .await
                .map_err(|e| McpError::internal(format!("invalid JSON from /metrics: {e}")))?;
            metrics
        }
        "ssh_preview_exec" => {
            let command = args["command"]
                .as_str()
                .ok_or_else(|| McpError::internal("command required"))?;
            let timeout_secs = args["timeout_secs"].as_u64();

            // Check if multi-host or single-host
            let hosts_array: Option<Vec<String>> = args["hosts"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            });

            let tags: Option<Vec<String>> = args["tags"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            });

            if let Some(hosts) = hosts_array {
                let plan = preview_exec_multi(hosts, command, tags, timeout_secs)
                    .await
                    .map_err(McpError::from)?;
                serde_json::to_value(plan)?
            } else {
                let host = args["host"]
                    .as_str()
                    .ok_or_else(|| McpError::internal("host or hosts required"))?;
                let plan = preview_exec(host, command, timeout_secs)
                    .await
                    .map_err(McpError::from)?;
                serde_json::to_value(plan)?
            }
        }
        "ssh_approval_policies_list" => {
            let policies = list_approval_policies().map_err(McpError::from)?;
            serde_json::to_value(policies)?
        }
        "ssh_approval_check" => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
            let command = args["command"]
                .as_str()
                .ok_or_else(|| McpError::internal("command required"))?;

            let target = command_authorization_target(host);

            let risk = agent2ssh::core::apply_risk_override(
                effective_command_risk(command).await,
                target.risk_override,
            );
            let result = check_approval_required(host, &target.tags, command, risk)
                .map_err(McpError::from)?;

            // Build approval context for richer response
            let context = build_approval_context(host, command, "mcp").ok();

            match result {
                Some(policy) => json!({
                    "requires_approval": true,
                    "matched_policy": policy.name,
                    "host": host,
                    "command": command,
                    "risk_level": risk,
                    "ttl_secs": policy.ttl_secs,
                    "context": context,
                }),
                None => json!({
                    "requires_approval": false,
                    "host": host,
                    "command": command,
                    "risk_level": risk,
                    "context": context,
                }),
            }
        }
        "ssh_health_snapshot" => {
            let hosts: Option<Vec<String>> = args["hosts"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            });
            let timeout_secs = args["timeout_secs"].as_u64();

            let target_hosts = match hosts {
                Some(h) if !h.is_empty() => h,
                _ => {
                    // Collect health for ALL configured hosts
                    list_hosts_core()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|h| h.name)
                        .collect()
                }
            };

            let snapshot = collect_health_snapshot(target_hosts, timeout_secs).await;
            serde_json::to_value(snapshot)?
        }
        "ssh_daemon_diagnose" => {
            let alias = args["alias"]
                .as_str()
                .ok_or_else(|| McpError::internal("alias required"))?;
            let diagnostic = diagnose_daemon(alias)
                .await
                .map_err(|e| McpError::internal(format!("diagnose_daemon failed: {e}")))?;
            serde_json::to_value(diagnostic)?
        }
        "ssh_daemon_version_check" => {
            let alias = args["alias"]
                .as_str()
                .ok_or_else(|| McpError::internal("alias required"))?;
            let compat = check_daemon_version(alias)
                .await
                .map_err(|e| McpError::internal(format!("version check failed: {e}")))?;
            serde_json::to_value(compat)?
        }
        "ssh_daemons_view" => {
            let view = get_daemons_unified_view()
                .await
                .map_err(|e| McpError::internal(format!("unified view failed: {e}")))?;
            serde_json::to_value(view)?
        }
        "ssh_metrics_trend" => {
            let period_str = args["period"].as_str().unwrap_or("24h");
            let period = match period_str {
                "24h" | "last24h" => TrendPeriod::Last24h,
                "7d" | "last7d" => TrendPeriod::Last7d,
                "30d" | "last30d" => TrendPeriod::Last30d,
                "all" => TrendPeriod::All,
                other => {
                    return Err(McpError::internal(format!(
                        "unknown period '{}'. Use: 24h, 7d, 30d, or all",
                        other
                    )))
                }
            };
            let trend = compute_metrics_trend(period)
                .map_err(|e| McpError::internal(format!("metrics trend failed: {e}")))?;
            serde_json::to_value(trend)?
        }
        "ssh_events_subscribe" => {
            // Return a snapshot of recent events from the bus.
            // For continuous streaming, use the daemon SSE endpoint.
            let mut rx = subscribe_events();
            let mut events = Vec::new();
            // Drain any currently buffered events (non-blocking)
            while let Ok(event) = rx.try_recv() {
                events.push(serde_json::to_value(event).unwrap_or_default());
            }
            json!({
                "events": events,
                "hint": "For real-time streaming, use the daemon SSE endpoint: GET /events/stream"
            })
        }
        "ssh_sync_diff" => {
            let path = args["path"].as_str();
            let diff = compare_ssh_configs(path)
                .map_err(|e| McpError::internal(format!("ssh sync diff failed: {e}")))?;
            serde_json::to_value(diff)?
        }
        "ssh_sync_export" => {
            let path = args["path"].as_str();
            let (out_path, count) = export_to_ssh_config(path, None)
                .map_err(|e| McpError::internal(format!("ssh sync export failed: {e}")))?;
            json!({ "path": out_path, "hosts_exported": count })
        }
        unknown => return Err(McpError::internal(anyhow!("unknown tool: {unknown}"))),
    };

    Ok(json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&payload)? }],
        "structuredContent": payload
    }))
}

fn which_check(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
