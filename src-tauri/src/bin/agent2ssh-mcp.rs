#![recursion_limit = "512"]

use agent2ssh::{
    add_host_core, classify_risk, connect_host, disconnect_host, exec_multi_core, exec_ssh_core,
    export_team_config, forward_add_core, forward_list_core, forward_remove_core,
    import_ssh_config_core, import_team_config, list_active_connections, list_audit_core,
    list_hosts_core, list_playbooks_core, ping_hosts_core, remove_host_core, run_playbook_core,
    session_close_core, session_list_core, session_open_core, session_read_core,
    session_write_core, sftp_download_core, sftp_ls_core, sftp_mkdir_core, sftp_stat_core,
    sftp_upload_core, AuditFilter, ExecRequest, ForwardDirection, HostProfile, RiskLevel,
    SftpDownloadRequest, SftpUploadRequest, TeamConfigExport,
};
use agent2ssh::approval::{approval_list, approval_respond};
use agent2ssh::notify::{load_webhook_config, save_webhook_config};
use agent2ssh::remote::{get_daemon, list_daemons_core};
use agent2ssh::risk_config::classify_with_user_rules;
use agent2ssh::store::audit_path;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
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
                            "daemon_alias":     { "type": "string", "description": "Forward this exec to a remote daemon by alias (omit or 'localhost' for local)." }
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
                    "description": "Run the same command on multiple hosts concurrently. Returns an array of per-host results.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["hosts", "command"],
                        "properties": {
                            "hosts":        { "type": "array", "items": { "type": "string" }, "description": "List of host profile aliases." },
                            "command":      { "type": "string" },
                            "force":        { "type": "boolean" },
                            "timeout_secs": { "type": "integer" }
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
                            "until":      { "type": "string", "description": "ISO-8601 upper bound." }
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
                    "description": "List all open PTY sessions in this MCP server process.",
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
                    "description": "Run a named playbook (sequence of SSH commands) against a target host. Steps execute sequentially; halts on first failure.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["playbook", "host"],
                        "properties": {
                            "playbook": { "type": "string", "description": "Name of the playbook to run." },
                            "host":     { "type": "string", "description": "Target host profile alias." },
                            "force":    { "type": "boolean", "description": "Set true to allow high-risk steps within the playbook." }
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
                    "name": "ssh_doctor",
                    "description": "Run diagnostic checks on the agent2ssh environment: verify ssh/ssh-keygen binaries, config directory, hosts.json, daemon.token permissions, daemon health, optional config files, and audit log size.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_metrics",
                    "description": "Retrieve basic metrics from the local agent2ssh daemon (requests, execs, blocked commands, durations, approvals). Reads from GET /metrics on 127.0.0.1:7722.",
                    "inputSchema": { "type": "object", "properties": {} }
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
        "ssh_list_daemons" => {
            serde_json::to_value(list_daemons_core().map_err(McpError::from)?)?
        }
        "ssh_add_host" => {
            let host: HostProfile =
                serde_json::from_value(args).map_err(|e| McpError::internal(e))?;
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

            let risk = classify_risk(&command);
            let max_output_bytes = args["max_output_bytes"].as_u64().map(|v| v as usize);
            let request = ExecRequest { host, command, force, timeout_secs, stdin, max_output_bytes };

            // If daemon_alias is set and not "localhost", forward to remote daemon
            if let Some(ref alias) = daemon_alias {
                if alias != "localhost" {
                    let (url, remote_token) = get_daemon(alias)
                        .map_err(|e| McpError::internal(format!("daemon lookup failed: {e}")))?;
                    let token = remote_token
                        .ok_or_else(|| McpError::internal(format!("no token configured for daemon '{alias}'")))?;

                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(request.timeout_secs.unwrap_or(60) + 10))
                        .build()
                        .map_err(|e| McpError::internal(e))?;

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

                    let result: agent2ssh::types::ExecResult = resp.json().await
                        .map_err(|e| McpError::internal(format!("invalid response from remote: {e}")))?;
                    serde_json::to_value(result)?
                } else {
                    let result = exec_ssh_core(request).await.map_err(|e| {
                        McpError::internal(format!("{e} (risk_level={risk})"))
                    })?;
                    serde_json::to_value(result)?
                }
            } else {
                let result = exec_ssh_core(request).await.map_err(|e| {
                    McpError::internal(format!("{e} (risk_level={risk})"))
                })?;
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
            let results = exec_multi_core(hosts, command, force, timeout_secs, None).await;
            serde_json::to_value(results)?
        }
        "ssh_sftp_ls" => {
            let host = args["host"].as_str().ok_or_else(|| McpError::internal("host required"))?;
            let path = args["path"].as_str().ok_or_else(|| McpError::internal("path required"))?;
            let timeout_secs = args["timeout_secs"].as_u64();
            serde_json::to_value(sftp_ls_core(host, path, timeout_secs).await.map_err(McpError::from)?)?
        }
        "ssh_sftp_stat" => {
            let host = args["host"].as_str().ok_or_else(|| McpError::internal("host required"))?;
            let path = args["path"].as_str().ok_or_else(|| McpError::internal("path required"))?;
            let timeout_secs = args["timeout_secs"].as_u64();
            serde_json::to_value(sftp_stat_core(host, path, timeout_secs).await.map_err(McpError::from)?)?
        }
        "ssh_sftp_mkdir" => {
            let host = args["host"].as_str().ok_or_else(|| McpError::internal("host required"))?;
            let path = args["path"].as_str().ok_or_else(|| McpError::internal("path required"))?;
            let timeout_secs = args["timeout_secs"].as_u64();
            serde_json::to_value(sftp_mkdir_core(host, path, timeout_secs).await.map_err(McpError::from)?)?
        }
        "ssh_audit" => {
            let risk_level = args["risk_level"].as_str().and_then(|s| serde_json::from_value::<RiskLevel>(serde_json::Value::String(s.to_string())).ok());
            let filter = AuditFilter {
                host: args["host"].as_str().map(str::to_string),
                risk_level,
                exit_code: args["exit_code"].as_i64().map(|v| v as i32),
                since: args["since"].as_str().map(str::to_string),
                until: args["until"].as_str().map(str::to_string),
                limit: args["limit"].as_u64().unwrap_or(20) as usize,
            };
            serde_json::to_value(list_audit_core(filter).map_err(McpError::from)?)?
        }
        "ssh_sftp_upload" => {
            let request: SftpUploadRequest =
                serde_json::from_value(args).map_err(McpError::internal)?;
            serde_json::to_value(sftp_upload_core(request).await.map_err(McpError::from)?)?
        }
        "ssh_sftp_download" => {
            let request: SftpDownloadRequest =
                serde_json::from_value(args).map_err(McpError::internal)?;
            serde_json::to_value(sftp_download_core(request).await.map_err(McpError::from)?)?
        }
        "ssh_session_open" => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
            let id = session_open_core(host).await.map_err(McpError::from)?;
            json!({ "session_id": id.to_string(), "host": host })
        }
        "ssh_session_write" => {
            let id: uuid::Uuid = args["session_id"]
                .as_str()
                .ok_or_else(|| McpError::internal("session_id required"))?
                .parse()
                .map_err(|e| McpError::internal(format!("invalid session_id: {e}")))?;
            let input = args["input"]
                .as_str()
                .ok_or_else(|| McpError::internal("input required"))?;
            session_write_core(id, input).await.map_err(McpError::from)?;
            json!({ "ok": true })
        }
        "ssh_session_read" => {
            let id: uuid::Uuid = args["session_id"]
                .as_str()
                .ok_or_else(|| McpError::internal("session_id required"))?
                .parse()
                .map_err(|e| McpError::internal(format!("invalid session_id: {e}")))?;
            let timeout_ms = args["timeout_ms"].as_u64().unwrap_or(2000);
            let output = session_read_core(id, timeout_ms).await.map_err(McpError::from)?;
            json!({ "output": output })
        }
        "ssh_session_close" => {
            let id: uuid::Uuid = args["session_id"]
                .as_str()
                .ok_or_else(|| McpError::internal("session_id required"))?
                .parse()
                .map_err(|e| McpError::internal(format!("invalid session_id: {e}")))?;
            session_close_core(id).await.map_err(McpError::from)?;
            json!({ "closed": id.to_string() })
        }
        "ssh_session_list" => {
            let sessions = session_list_core().await;
            json!(sessions.iter().map(|(id, host)| json!({ "session_id": id.to_string(), "host": host })).collect::<Vec<_>>())
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
                .ok_or_else(|| McpError::internal("bind_port required"))? as u16;
            let target_host = args["target_host"]
                .as_str()
                .ok_or_else(|| McpError::internal("target_host required"))?;
            let target_port = args["target_port"]
                .as_u64()
                .ok_or_else(|| McpError::internal("target_port required"))? as u16;
            let rule = forward_add_core(host, direction, bind_port, target_host, target_port)
                .await
                .map_err(McpError::from)?;
            serde_json::to_value(rule)?
        }
        "ssh_forward_list" => {
            serde_json::to_value(forward_list_core().await)?
        }
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
            let base = classify_risk(command);
            let user_risk = classify_with_user_rules(command).await;
            let final_risk = if let Some(ur) = &user_risk {
                match (ur, &base) {
                    (RiskLevel::Blocked, _) => RiskLevel::Blocked,
                    (RiskLevel::High, RiskLevel::Blocked) => RiskLevel::Blocked,
                    (ur, _) => *ur,
                }
            } else { base };
            json!({
                "command": command,
                "risk_level": final_risk,
                "matched_user_rule": user_risk.is_some(),
            })
        }
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
            let uuid: uuid::Uuid = id.parse()
                .map_err(|e| McpError::internal(format!("invalid id: {e}")))?;
            approval_respond(uuid, approved).await.map_err(McpError::from)?;
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
            let result = run_playbook_core(playbook, host, force).await.map_err(McpError::from)?;
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
            connect_host(host).await.map_err(McpError::from)?;
            json!({ "ok": true, "host": host })
        }
        "ssh_disconnect" => {
            let host = args["host"]
                .as_str()
                .ok_or_else(|| McpError::internal("host required"))?;
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
                    save_webhook_config(&config)
                        .map_err(|e| McpError::internal(format!("failed to save webhook config: {e}")))?;
                    serde_json::to_value(config)?
                }
                other => {
                    return Err(McpError::internal(format!("unknown action '{}', expected 'get' or 'set'", other)));
                }
            }
        }
        "ssh_config_export" => {
            let export = export_team_config().map_err(McpError::from)?;
            serde_json::to_value(export)?
        }
        "ssh_config_import" => {
            let config_value = args.get("config")
                .ok_or_else(|| McpError::internal("config object required"))?;
            let export: TeamConfigExport = serde_json::from_value(config_value.clone())
                .map_err(|e| McpError::internal(format!("invalid config object: {e}")))?;
            let result = import_team_config(&export).map_err(McpError::from)?;
            serde_json::to_value(result)?
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
                && std::fs::read_to_string(&hosts_path).ok()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).is_some();
            checks.push(json!({"name": "hosts.json", "status": if hosts_path.exists() && hosts_ok {"pass"} else if hosts_path.exists() {"fail"} else {"warn"}, "detail": if !hosts_path.exists() {"not configured"} else if hosts_ok {"valid"} else {"invalid JSON"}}));

            // daemon.token
            let token_path = config_dir.join("daemon.token");
            if token_path.exists() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = std::fs::metadata(&token_path).map(|m| m.permissions().mode() & 0o777).unwrap_or(0o777);
                    checks.push(json!({"name": "daemon.token", "status": if mode == 0o600 {"pass"} else {"warn"}, "detail": format!("permissions 0{:o}", mode)}));
                }
                #[cfg(not(unix))]
                { checks.push(json!({"name": "daemon.token", "status": "pass", "detail": "exists"})); }
            } else {
                checks.push(json!({"name": "daemon.token", "status": "warn", "detail": "not found"}));
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
            for (filename, label) in &[("risk_rules.toml","risk rules"),("playbooks.toml","playbooks"),("remotes.toml","remote daemons"),("webhook.toml","webhook config")] {
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
                return Err(McpError::internal(format!("daemon returned status {}", resp.status())));
            }
            let metrics: Value = resp.json().await
                .map_err(|e| McpError::internal(format!("invalid JSON from /metrics: {e}")))?;
            metrics
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
