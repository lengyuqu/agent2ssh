use agent2ssh::{
    add_host_core, exec_ssh_core, list_audit_core, list_hosts_core, remove_host_core, ExecRequest,
    HostProfile,
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
                    "name": "ssh_add_host",
                    "description": "Create or update an SSH host profile.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["name", "host"],
                        "properties": {
                            "name":     { "type": "string" },
                            "host":     { "type": "string" },
                            "user":     { "type": "string" },
                            "port":     { "type": "integer" },
                            "key_path": { "type": "string" }
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
                    "description": "Run a non-interactive command over SSH and return stdout, stderr, exit code, and timing.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["host", "command"],
                        "properties": {
                            "host":    { "type": "string" },
                            "command": { "type": "string" }
                        }
                    }
                },
                {
                    "name": "ssh_audit",
                    "description": "Return recent SSH execution audit log entries.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "default": 20 }
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
        "ssh_exec" => {
            let request: ExecRequest =
                serde_json::from_value(args).map_err(|e| McpError::internal(e))?;
            serde_json::to_value(exec_ssh_core(request).await.map_err(McpError::from)?)?
        }
        "ssh_audit" => {
            let limit = args["limit"].as_u64().unwrap_or(20) as usize;
            serde_json::to_value(list_audit_core(limit).map_err(McpError::from)?)?
        }
        unknown => return Err(McpError::internal(anyhow!("unknown tool: {unknown}"))),
    };

    Ok(json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&payload)? }],
        "structuredContent": payload
    }))
}
