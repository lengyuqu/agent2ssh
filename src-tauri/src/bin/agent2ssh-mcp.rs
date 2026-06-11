use agent2ssh::{
    add_host_core, classify_risk, exec_multi_core, exec_ssh_core, forward_add_core,
    forward_list_core, forward_remove_core, import_ssh_config_core, list_audit_core,
    list_hosts_core, ping_hosts_core, remove_host_core, session_close_core, session_list_core,
    session_open_core, session_read_core, session_write_core, sftp_download_core, sftp_ls_core,
    sftp_mkdir_core, sftp_stat_core, sftp_upload_core, AuditFilter, ExecRequest, ForwardDirection,
    HostProfile, RiskLevel, SftpDownloadRequest, SftpUploadRequest,
};
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
                            "jump_host": { "type": "string", "description": "Host profile alias to use as ProxyJump bastion." }
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
                    "description": "Run a non-interactive command over SSH. Returns stdout, stderr, exit code, timing, and risk_level. High-risk commands require force=true; blocked commands always fail.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["host", "command"],
                        "properties": {
                            "host":         { "type": "string" },
                            "command":      { "type": "string" },
                            "force":        { "type": "boolean", "description": "Set true to run high-risk commands." },
                            "timeout_secs":     { "type": "integer", "description": "Kill the command after N seconds (default 60)." },
                            "stdin":            { "type": "string", "description": "Pipe this string into the remote command's stdin." },
                            "max_output_bytes": { "type": "integer", "description": "Truncate stdout to this many bytes (default 4 MiB)." }
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

            let risk = classify_risk(&command);
            let max_output_bytes = args["max_output_bytes"].as_u64().map(|v| v as usize);
            let request = ExecRequest { host, command, force, timeout_secs, stdin, max_output_bytes };
            let result = exec_ssh_core(request).await.map_err(|e| {
                McpError::internal(format!("{e} (risk_level={risk})"))
            })?;
            serde_json::to_value(result)?
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
            let results = exec_multi_core(hosts, command, force, timeout_secs).await;
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
        unknown => return Err(McpError::internal(anyhow!("unknown tool: {unknown}"))),
    };

    Ok(json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&payload)? }],
        "structuredContent": payload
    }))
}
