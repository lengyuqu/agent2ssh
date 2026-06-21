#![recursion_limit = "2048"]

use agent2ssh::approval::build_approval_context_with_effective_risk;
use agent2ssh::approval::{
    approval_list, approval_respond, check_approval_required, list_approval_policies,
};
use agent2ssh::events::subscribe_events;
use agent2ssh::execution_control::command_authorization_target;
use agent2ssh::notify::{load_webhook_config, save_webhook_config};
use agent2ssh::remote::{
    check_daemon_scope, check_daemon_version, diagnose_daemon, get_daemon, get_daemon_with_scope,
    get_daemons_unified_view, list_daemons_core, tags_for_remote_scope_check,
};
use agent2ssh::risk_config::classify_with_user_rules;
use agent2ssh::store::{audit_path, compute_metrics_trend, TrendPeriod};
use agent2ssh::{
    add_host_core, append_diagnostic_log, collect_health_snapshot, compare_exec_results,
    compare_ssh_configs, connect_host, disconnect_host, dry_run_playbook, effective_command_risk,
    exec_multi_core, exec_multi_with_strategy, exec_ssh_core, export_audit_csv, export_audit_jsonl,
    export_team_config, export_to_ssh_config, forward_add_core, forward_list_core,
    forward_remove_core, import_ssh_config_core, import_team_config, list_active_connections,
    list_audit_core, list_hosts_core, list_playbooks_core, ping_hosts_core, preview_exec,
    preview_exec_multi, preview_team_config_import, remove_host_core,
    run_playbook_core_with_source_and_approved_steps, session_close_core, session_list_core,
    session_open_core, session_read_core, session_write_core, sftp_download_core_with_source,
    sftp_ls_core_with_source, sftp_mkdir_core_with_source, sftp_stat_core_with_source,
    sftp_upload_core_with_source, AuditFilter, ExecMultiBatchRequest, ExecMultiRequest,
    ExecRequest, ForwardDirection, HostProfile, RiskLevel, SftpDownloadRequest, SftpUploadRequest,
    TeamConfigExport,
};
use anyhow::Result;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};

#[path = "agent2ssh_mcp/auth.rs"]
mod agent2ssh_mcp_auth;
#[path = "agent2ssh_mcp/tools.rs"]
mod agent2ssh_mcp_tools;

use agent2ssh_mcp_auth::{
    authorize_local_mcp_exec_request, authorize_local_mcp_exec_targets,
    authorize_local_mcp_operation, authorize_local_mcp_playbook_run,
};
use agent2ssh_mcp_tools::{McpTool, ToolCall};

const MCP_SERVER_NAME: &str = "agent2ssh-mcp";
const MCP_SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

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

#[derive(Deserialize)]
struct DaemonForwardRule {
    id: uuid::Uuid,
    host: String,
    direction: ForwardDirection,
    bind_port: u16,
    target_host: String,
    target_port: u16,
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

fn local_daemon_client() -> std::result::Result<Option<(reqwest::Client, String, String)>, McpError>
{
    let (url, token) = get_daemon("localhost")
        .map_err(|e| McpError::internal(format!("local daemon lookup failed: {e}")))?;
    let Some(token) = token else {
        return Ok(None);
    };
    let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30));
    // Propagate the MCP process's correlation id so daemon-side logs for forwarded
    // requests stay linked to the originating agent operation.
    if let Some(trace_id) = agent2ssh::current_trace_id() {
        if let Ok(value) = reqwest::header::HeaderValue::from_str(&trace_id) {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert("x-agent2ssh-trace-id", value);
            builder = builder.default_headers(headers);
        }
    }
    let client = builder.build().map_err(McpError::internal)?;
    Ok(Some((client, url.trim_end_matches('/').to_string(), token)))
}

fn response_preview(body: &str) -> String {
    let normalized = body
        .chars()
        .map(|ch| {
            if ch.is_control() && ch != '\n' && ch != '\t' {
                ' '
            } else {
                ch
            }
        })
        .collect::<String>()
        .trim()
        .to_string();
    const LIMIT: usize = 2048;
    if normalized.chars().count() <= LIMIT {
        return normalized;
    }
    let mut out = normalized.chars().take(LIMIT).collect::<String>();
    out.push_str("...");
    out
}

fn response_error(context: &str, status: reqwest::StatusCode, body: &str) -> McpError {
    let preview = response_preview(body);
    let remote_message = serde_json::from_str::<Value>(body).ok().and_then(|value| {
        value
            .get("error")
            .and_then(Value::as_str)
            .or_else(|| value.get("message").and_then(Value::as_str))
            .map(str::to_string)
    });
    let detail = remote_message
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| {
            if preview.is_empty() {
                "<empty response body>".to_string()
            } else {
                preview
            }
        });
    McpError::internal(format!("{context} returned HTTP {status}: {detail}"))
}

async fn response_text(
    response: reqwest::Response,
    context: &str,
) -> std::result::Result<String, McpError> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| McpError::internal(format!("{context} response body read failed: {e}")))?;
    if !status.is_success() {
        return Err(response_error(context, status, &body));
    }
    Ok(body)
}

async fn response_json<T: DeserializeOwned>(
    response: reqwest::Response,
    context: &str,
) -> std::result::Result<T, McpError> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| McpError::internal(format!("{context} response body read failed: {e}")))?;
    if !status.is_success() {
        return Err(response_error(context, status, &body));
    }
    serde_json::from_str(&body).map_err(|e| {
        let preview = response_preview(&body);
        let detail = if preview.is_empty() {
            "<empty response body>".to_string()
        } else {
            preview
        };
        McpError::internal(format!(
            "{context} returned invalid JSON (HTTP {status}): {e}; body: {detail}"
        ))
    })
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
    let body: DaemonIdBody = response_json(response, "local daemon /sessions").await?;
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
    let mut value: Value = response_json(response, "local daemon /gate").await?;
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
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.map_err(|e| {
            McpError::internal(format!(
                "local daemon session write response body read failed: {e}"
            ))
        })?;
        if can_fallback_session_error(&body) {
            return Ok(DaemonAttempt::Fallback);
        }
        return Err(response_error("local daemon session write", status, &body));
    }
    let _ = response_text(response, "local daemon session write").await?;
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
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.map_err(|e| {
            McpError::internal(format!(
                "local daemon session read response body read failed: {e}"
            ))
        })?;
        if can_fallback_session_error(&body) {
            return Ok(DaemonAttempt::Fallback);
        }
        return Err(response_error("local daemon session read", status, &body));
    }
    let body: DaemonOutputBody = response_json(response, "local daemon session read").await?;
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
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.map_err(|e| {
            McpError::internal(format!(
                "local daemon session close response body read failed: {e}"
            ))
        })?;
        if can_fallback_session_error(&body) {
            return Ok(DaemonAttempt::Fallback);
        }
        return Err(response_error("local daemon session close", status, &body));
    }
    let _ = response_text(response, "local daemon session close").await?;
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
    let sessions: Vec<DaemonSessionListItem> =
        response_json(response, "local daemon /sessions").await?;
    Ok(DaemonAttempt::Handled(
        sessions
            .into_iter()
            .map(|s| json!({ "session_id": s.id, "host": s.host, "backend": "daemon" }))
            .collect(),
    ))
}

async fn try_daemon_forward_add(
    host: &str,
    direction: ForwardDirection,
    bind_port: u16,
    target_host: &str,
    target_port: u16,
) -> std::result::Result<DaemonAttempt<Value>, McpError> {
    let Some((client, base_url, token)) = local_daemon_client()? else {
        return Ok(DaemonAttempt::Fallback);
    };
    let response = match client
        .post(format!("{base_url}/forwards"))
        .bearer_auth(token)
        .json(&json!({
            "host": host,
            "direction": direction,
            "bind_port": bind_port,
            "target_host": target_host,
            "target_port": target_port,
        }))
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return Ok(DaemonAttempt::Fallback),
    };
    let mut value: Value = response_json(response, "local daemon /forwards").await?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert("backend".into(), Value::String("daemon".into()));
    }
    Ok(DaemonAttempt::Handled(value))
}

async fn try_daemon_forward_list() -> std::result::Result<DaemonAttempt<Vec<Value>>, McpError> {
    let Some((client, base_url, token)) = local_daemon_client()? else {
        return Ok(DaemonAttempt::Fallback);
    };
    let response = match client
        .get(format!("{base_url}/forwards"))
        .bearer_auth(token)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return Ok(DaemonAttempt::Fallback),
    };
    let rules: Vec<DaemonForwardRule> = response_json(response, "local daemon /forwards").await?;
    Ok(DaemonAttempt::Handled(
        rules
            .into_iter()
            .map(|rule| {
                json!({
                    "id": rule.id,
                    "host": rule.host,
                    "direction": rule.direction,
                    "bind_port": rule.bind_port,
                    "target_host": rule.target_host,
                    "target_port": rule.target_port,
                    "backend": "daemon",
                })
            })
            .collect(),
    ))
}

async fn try_daemon_forward_remove(
    id: uuid::Uuid,
) -> std::result::Result<DaemonAttempt<Value>, McpError> {
    let Some((client, base_url, token)) = local_daemon_client()? else {
        return Ok(DaemonAttempt::Fallback);
    };
    let response = match client
        .delete(format!("{base_url}/forwards/{id}"))
        .bearer_auth(token)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return Ok(DaemonAttempt::Fallback),
    };
    let _ = response_text(response, "local daemon forward remove").await?;
    Ok(DaemonAttempt::Handled(
        json!({ "removed": id.to_string(), "backend": "daemon" }),
    ))
}

#[tokio::main]
async fn main() -> Result<()> {
    agent2ssh::install_panic_hook("mcp");
    agent2ssh::seed_trace_id_from_env();
    // K1: migrate any legacy plaintext passwords into the app-managed encrypted store (no-op once clean).
    if let Err(e) = agent2ssh::migrate_plaintext_secrets() {
        eprintln!("warning: secret migration skipped: {e}");
    }

    let mut args = std::env::args().skip(1);
    if let Some(arg) = args.next() {
        match arg.as_str() {
            "-V" | "--version" => {
                println!("{MCP_SERVER_NAME} {MCP_SERVER_VERSION}");
                return Ok(());
            }
            "-h" | "--help" => {
                println!("{MCP_SERVER_NAME} {MCP_SERVER_VERSION}");
                println!("Agent2SSH MCP stdio server");
                println!();
                println!("Usage: {MCP_SERVER_NAME}");
                println!();
                println!("Configure this binary as an MCP stdio server in your agent client.");
                println!("It reads newline-delimited JSON-RPC requests from stdin and writes responses to stdout.");
                println!("Use tools/list after initialize to discover available ssh_* tools.");
                return Ok(());
            }
            other => {
                eprintln!("unknown argument: {other}");
                eprintln!("Run '{MCP_SERVER_NAME} --help' for usage.");
                std::process::exit(2);
            }
        }
    }

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
            Err(err) => {
                // Persist the failure so MCP tool errors are observable in app.log
                // rather than only surfacing as a JSON-RPC error to the caller.
                let method = request.get("method").and_then(Value::as_str).unwrap_or("");
                let tool = request
                    .get("params")
                    .and_then(|params| params.get("name"))
                    .and_then(Value::as_str);
                let _ = append_diagnostic_log(
                    "error",
                    "mcp",
                    &err.message,
                    Some(json!({ "method": method, "tool": tool, "code": err.code })),
                );
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": err.code, "message": err.message }
                })
            }
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
            "capabilities": { "tools": {}, "resources": {} },
            "serverInfo": { "name": MCP_SERVER_NAME, "version": MCP_SERVER_VERSION }
        })),
        "ping" => Ok(json!({})),
        "resources/list" => Ok(json!({ "resources": [] })),
        "resources/templates/list" => Ok(json!({ "resourceTemplates": [] })),
        "tools/list" => Ok(json!({ "tools": agent2ssh_mcp_tools::tools_list() })),
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
            let call = agent2ssh_mcp_tools::resolve_tool_call(name, args)?;
            call_tool(call).await
        }
        other => Err(McpError::method_not_found(other)),
    }
}

async fn call_tool(call: ToolCall) -> std::result::Result<Value, McpError> {
    let args = call.args;
    let payload = match call.tool {
        McpTool::SshImportConfig => {
            let path = args["path"].as_str();
            serde_json::to_value(import_ssh_config_core(path).map_err(McpError::from)?)?
        }
        McpTool::SshListHosts => serde_json::to_value(list_hosts_core().map_err(McpError::from)?)?,
        McpTool::SshListDaemons => {
            serde_json::to_value(list_daemons_core().map_err(McpError::from)?)?
        }
        McpTool::SshAddHost => {
            let host: HostProfile = serde_json::from_value(args).map_err(McpError::internal)?;
            serde_json::to_value(add_host_core(host).map_err(McpError::from)?)?
        }
        McpTool::SshRemoveHost => {
            let host_name = args["name"]
                .as_str()
                .ok_or_else(|| McpError::internal("name required"))?;
            remove_host_core(host_name).map_err(McpError::from)?;
            json!({ "removed": host_name })
        }
        McpTool::SshPing => {
            let hosts: Vec<String> = args["hosts"]
                .as_array()
                .ok_or_else(|| McpError::internal("hosts array required"))?
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            let timeout_secs = args["timeout_secs"].as_u64();
            serde_json::to_value(ping_hosts_core(hosts, timeout_secs).await)?
        }
        McpTool::SshExec => {
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
                    let token = remote_token.ok_or_else(|| {
                        McpError::internal(format!("no token configured for daemon '{alias}'"))
                    })?;
                    let local_tags = mcp_host_tags(&request.host);
                    let remote_tags = tags_for_remote_scope_check(
                        &scope,
                        &url,
                        &token,
                        &request.host,
                        local_tags,
                    )
                    .await
                    .map_err(McpError::internal)?;
                    check_daemon_scope(&scope, &request.host, &remote_tags, &request.command)
                        .map_err(McpError::internal)?;

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

                    let context = format!("remote daemon '{alias}' /exec");
                    let result: agent2ssh::types::ExecResult =
                        response_json(resp, &context).await?;
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
        McpTool::SshExecMulti => {
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
            let approved_hosts = authorize_local_mcp_exec_targets(
                &hosts,
                &tags,
                &command,
                force,
                reason.clone(),
                change_id.clone(),
                &source,
            )
            .await?;

            let batch_result = exec_multi_with_strategy(ExecMultiBatchRequest {
                request: ExecMultiRequest {
                    hosts,
                    command,
                    force,
                    approved_hosts,
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
        McpTool::SshExecCompare => {
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
            let approved_hosts = authorize_local_mcp_exec_targets(
                &hosts, &tags, &command, force, None, None, &source,
            )
            .await?;

            let results = exec_multi_core(ExecMultiRequest {
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
            .await;
            let comparison = compare_exec_results(&results);
            serde_json::to_value(comparison)?
        }
        McpTool::SshSftpLs => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
            let path = args["path"]
                .as_str()
                .ok_or_else(|| McpError::internal("path required"))?;
            let timeout_secs = args["timeout_secs"].as_u64();
            let source = mcp_source();
            let command = format!("sftp ls {}", path);
            authorize_local_mcp_operation(host, &command, false, &source).await?;
            serde_json::to_value(
                sftp_ls_core_with_source(host, path, timeout_secs, Some(source))
                    .await
                    .map_err(McpError::from)?,
            )?
        }
        McpTool::SshSftpStat => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
            let path = args["path"]
                .as_str()
                .ok_or_else(|| McpError::internal("path required"))?;
            let timeout_secs = args["timeout_secs"].as_u64();
            let source = mcp_source();
            let command = format!("sftp stat {}", path);
            authorize_local_mcp_operation(host, &command, false, &source).await?;
            serde_json::to_value(
                sftp_stat_core_with_source(host, path, timeout_secs, Some(source))
                    .await
                    .map_err(McpError::from)?,
            )?
        }
        McpTool::SshSftpMkdir => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
            let path = args["path"]
                .as_str()
                .ok_or_else(|| McpError::internal("path required"))?;
            let timeout_secs = args["timeout_secs"].as_u64();
            let source = mcp_source();
            let command = format!("sftp mkdir {}", path);
            authorize_local_mcp_operation(host, &command, false, &source).await?;
            serde_json::to_value(
                sftp_mkdir_core_with_source(host, path, timeout_secs, Some(source))
                    .await
                    .map_err(McpError::from)?,
            )?
        }
        McpTool::SshAudit => {
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
        McpTool::SshAuditExport => {
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
        McpTool::SshSftpUpload => {
            let request: SftpUploadRequest =
                serde_json::from_value(args).map_err(McpError::internal)?;
            let source = mcp_source();
            let command = format!(
                "sftp upload {} -> {}",
                request.local_path, request.remote_path
            );
            authorize_local_mcp_operation(&request.host, &command, false, &source).await?;
            serde_json::to_value(
                sftp_upload_core_with_source(request, Some(source))
                    .await
                    .map_err(McpError::from)?,
            )?
        }
        McpTool::SshSftpDownload => {
            let request: SftpDownloadRequest =
                serde_json::from_value(args).map_err(McpError::internal)?;
            let source = mcp_source();
            let command = format!(
                "sftp download {} -> {}",
                request.remote_path, request.local_path
            );
            authorize_local_mcp_operation(&request.host, &command, false, &source).await?;
            serde_json::to_value(
                sftp_download_core_with_source(request, Some(source))
                    .await
                    .map_err(McpError::from)?,
            )?
        }
        McpTool::SshSessionOpen => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
            match try_daemon_session_open(host).await? {
                DaemonAttempt::Handled(value) => value,
                DaemonAttempt::Fallback => {
                    let source = mcp_source();
                    authorize_local_mcp_operation(host, "session_open", false, &source).await?;
                    let id = session_open_core(host).await.map_err(McpError::from)?;
                    json!({ "session_id": id.to_string(), "host": host, "backend": "process", "source": source })
                }
            }
        }
        McpTool::SshSessionWrite => {
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
        McpTool::SshSessionRead => {
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
        McpTool::SshSessionClose => {
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
        McpTool::SshSessionList => {
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
        McpTool::SshForwardAdd => {
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
            authorize_local_mcp_operation(host, &command, false, &source).await?;
            match try_daemon_forward_add(host, direction, bind_port, target_host, target_port)
                .await?
            {
                DaemonAttempt::Handled(value) => value,
                DaemonAttempt::Fallback => {
                    let mut value = serde_json::to_value(
                        forward_add_core(host, direction, bind_port, target_host, target_port)
                            .await
                            .map_err(McpError::from)?,
                    )?;
                    if let Some(obj) = value.as_object_mut() {
                        obj.insert("backend".into(), Value::String("process".into()));
                    }
                    value
                }
            }
        }
        McpTool::SshForwardList => {
            let mut items = match try_daemon_forward_list().await? {
                DaemonAttempt::Handled(items) => items,
                DaemonAttempt::Fallback => Vec::new(),
            };
            items.extend(forward_list_core().await.into_iter().map(|rule| {
                json!({
                    "id": rule.id,
                    "host": rule.host,
                    "direction": rule.direction,
                    "bind_port": rule.bind_port,
                    "target_host": rule.target_host,
                    "target_port": rule.target_port,
                    "backend": "process",
                })
            }));
            json!(items)
        }
        McpTool::SshForwardRemove => {
            let id: uuid::Uuid = args["forward_id"]
                .as_str()
                .ok_or_else(|| McpError::internal("forward_id required"))?
                .parse()
                .map_err(|e| McpError::internal(format!("invalid forward_id: {e}")))?;
            match try_daemon_forward_remove(id).await? {
                DaemonAttempt::Handled(value) => value,
                DaemonAttempt::Fallback => {
                    forward_remove_core(id).await.map_err(McpError::from)?;
                    json!({ "removed": id.to_string(), "backend": "process" })
                }
            }
        }
        McpTool::SshRiskCheck => {
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
        McpTool::SshGateStatus => try_daemon_gate_status().await?,
        McpTool::SshApprovalList => {
            let approvals = approval_list().await;
            serde_json::to_value(approvals)?
        }
        McpTool::SshApprovalRespond => {
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
        McpTool::SshPlaybookList => {
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
        McpTool::SshPlaybookRun => {
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
            let empty_params = HashMap::new();
            let params_for_auth = params_map.as_ref().unwrap_or(&empty_params);
            let approved_steps = authorize_local_mcp_playbook_run(
                playbook,
                host,
                force,
                params_for_auth,
                reason.clone(),
                change_id.clone(),
                &source,
            )
            .await?;
            let result = run_playbook_core_with_source_and_approved_steps(
                playbook,
                host,
                force,
                params_map.as_ref(),
                reason,
                change_id,
                Some(source),
                &approved_steps,
            )
            .await
            .map_err(McpError::from)?;
            serde_json::to_value(result)?
        }
        McpTool::SshPlaybookDryRun => {
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
        McpTool::SshConnectionStatus => {
            let statuses = list_active_connections().await;
            serde_json::to_value(statuses)?
        }
        McpTool::SshConnect => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
            let source = mcp_source();
            authorize_local_mcp_operation(host, "connect", false, &source).await?;
            connect_host(host).await.map_err(McpError::from)?;
            json!({ "ok": true, "host": host })
        }
        McpTool::SshDisconnect => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
            let source = mcp_source();
            authorize_local_mcp_operation(host, "disconnect", false, &source).await?;
            disconnect_host(host).await.map_err(McpError::from)?;
            json!({ "ok": true, "host": host })
        }
        McpTool::SshWebhookConfig => {
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
        McpTool::SshConfigExport => {
            let export = export_team_config().map_err(McpError::from)?;
            serde_json::to_value(export)?
        }
        McpTool::SshConfigImport => {
            let config_value = args
                .get("config")
                .ok_or_else(|| McpError::internal("config object required"))?;
            let export: TeamConfigExport = serde_json::from_value(config_value.clone())
                .map_err(|e| McpError::internal(format!("invalid config object: {e}")))?;
            let result = import_team_config(&export).map_err(McpError::from)?;
            serde_json::to_value(result)?
        }
        McpTool::SshConfigImportPreview => {
            let config_value = args
                .get("config")
                .ok_or_else(|| McpError::internal("config object required"))?;
            let export: TeamConfigExport = serde_json::from_value(config_value.clone())
                .map_err(|e| McpError::internal(format!("invalid config object: {e}")))?;
            let preview = preview_team_config_import(&export).map_err(McpError::from)?;
            serde_json::to_value(preview)?
        }
        McpTool::SshDoctor => {
            let mut checks: Vec<Value> = Vec::new();

            checks.push(json!({"name": "embedded SSH transport", "status": "pass", "detail": "exec, SFTP, terminal, sessions, jump hosts, connections, and forwards use the Rust backend"}));
            checks.push(json!({"name": "embedded key generation", "status": "pass", "detail": "Ed25519 keys are generated with the Rust backend and system CSPRNG"}));

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
                Ok(client) => match client
                    .get(format!("{}/health", agent2ssh::local_daemon_url()))
                    .send()
                    .await
                {
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
        McpTool::SshMetrics => {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(3))
                .build()
                .map_err(|e| McpError::internal(format!("client build failed: {e}")))?;
            let resp = client
                .get(format!("{}/metrics", agent2ssh::local_daemon_url()))
                .send()
                .await
                .map_err(|e| McpError::internal(format!("daemon /metrics unreachable: {e}")))?;
            let metrics: Value = response_json(resp, "local daemon /metrics").await?;
            metrics
        }
        McpTool::SshPreviewExec => {
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
        McpTool::SshApprovalPoliciesList => {
            let policies = list_approval_policies().map_err(McpError::from)?;
            serde_json::to_value(policies)?
        }
        McpTool::SshApprovalCheck => {
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
            let context =
                build_approval_context_with_effective_risk(host, command, "mcp", risk, None).ok();

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
        McpTool::SshHealthSnapshot => {
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
        McpTool::SshDaemonDiagnose => {
            let alias = args["alias"]
                .as_str()
                .ok_or_else(|| McpError::internal("alias required"))?;
            let diagnostic = diagnose_daemon(alias)
                .await
                .map_err(|e| McpError::internal(format!("diagnose_daemon failed: {e}")))?;
            serde_json::to_value(diagnostic)?
        }
        McpTool::SshDaemonVersionCheck => {
            let alias = args["alias"]
                .as_str()
                .ok_or_else(|| McpError::internal("alias required"))?;
            let compat = check_daemon_version(alias)
                .await
                .map_err(|e| McpError::internal(format!("version check failed: {e}")))?;
            serde_json::to_value(compat)?
        }
        McpTool::SshDaemonsView => {
            let view = get_daemons_unified_view()
                .await
                .map_err(|e| McpError::internal(format!("unified view failed: {e}")))?;
            serde_json::to_value(view)?
        }
        McpTool::SshMetricsTrend => {
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
        McpTool::SshEventsSubscribe => {
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
        McpTool::SshSyncDiff => {
            let path = args["path"].as_str();
            let diff = compare_ssh_configs(path)
                .map_err(|e| McpError::internal(format!("ssh sync diff failed: {e}")))?;
            serde_json::to_value(diff)?
        }
        McpTool::SshSyncExport => {
            let path = args["path"].as_str();
            let (out_path, count) = export_to_ssh_config(path, None)
                .map_err(|e| McpError::internal(format!("ssh sync export failed: {e}")))?;
            json!({ "path": out_path, "hosts_exported": count })
        }
    };

    Ok(json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&payload)? }],
        "structuredContent": payload
    }))
}
