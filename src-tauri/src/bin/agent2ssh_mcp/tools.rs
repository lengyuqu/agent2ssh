use serde_json::{json, Value};

use super::McpError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub(super) enum McpTool {
    SshListHosts,
    SshListDaemons,
    SshImportConfig,
    SshAddHost,
    SshRemoveHost,
    SshExec,
    SshPing,
    SshExecMulti,
    SshExecCompare,
    SshAudit,
    SshAuditExport,
    SshSftpLs,
    SshSftpStat,
    SshSftpMkdir,
    SshSftpRename,
    SshSftpRm,
    SshSftpRmdir,
    SshSftpRmRf,
    SshSftpUpload,
    SshSftpDownload,
    SshSessionOpen,
    SshSessionWrite,
    SshSessionRead,
    SshSessionClose,
    SshSessionList,
    SshForwardAdd,
    SshForwardList,
    SshForwardRemove,
    SshRiskCheck,
    SshGateStatus,
    SshApprovalList,
    SshApprovalRespond,
    SshPlaybookList,
    SshPlaybookRun,
    SshPlaybookDryRun,
    SshSnippetList,
    SshSnippetSave,
    SshSnippetDelete,
    SshConnectionStatus,
    SshConnect,
    SshDisconnect,
    SshWebhookConfig,
    SshConfigExport,
    SshConfigImport,
    SshConfigImportPreview,
    SshDoctor,
    SshMetrics,
    SshPreviewExec,
    SshApprovalPoliciesList,
    SshApprovalCheck,
    SshHealthSnapshot,
    SshDaemonDiagnose,
    SshDaemonVersionCheck,
    SshDaemonsView,
    SshMetricsTrend,
    SshEventsSubscribe,
    SshSyncDiff,
    SshSyncExport,
    SshRedactRuleList,
    SshRedactRuleAdd,
    SshRedactRuleUpdate,
    SshRedactRuleRemove,
    SshRedactRuleReset,
    SshHighlightRuleList,
    SshHighlightRuleAdd,
    SshHighlightRuleUpdate,
    SshHighlightRuleRemove,
    SshHighlightRuleReset,
    SshAlgoPrefsGet,
    SshAlgoPrefsSet,
    SshAlgoPrefsClear,
}

pub(super) struct ToolCall {
    pub(super) tool: McpTool,
    pub(super) args: Value,
}

pub(super) fn tools_list() -> Vec<Value> {
    tool_definitions()
}

pub(super) fn resolve_tool_call(name: &str, args: Value) -> Result<ToolCall, McpError> {
    let definitions = tool_definitions();
    let definition = definitions
        .iter()
        .find(|definition| definition.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| McpError::method_not_found(name))?;
    validate_required_args(definition, &args)?;
    let tool = tool_kind(name).ok_or_else(|| McpError::method_not_found(name))?;
    Ok(ToolCall { tool, args })
}

fn tool_kind(name: &str) -> Option<McpTool> {
    match name {
        "ssh_list_hosts" => Some(McpTool::SshListHosts),
        "ssh_list_daemons" => Some(McpTool::SshListDaemons),
        "ssh_import_config" => Some(McpTool::SshImportConfig),
        "ssh_add_host" => Some(McpTool::SshAddHost),
        "ssh_remove_host" => Some(McpTool::SshRemoveHost),
        "ssh_exec" => Some(McpTool::SshExec),
        "ssh_ping" => Some(McpTool::SshPing),
        "ssh_exec_multi" => Some(McpTool::SshExecMulti),
        "ssh_exec_compare" => Some(McpTool::SshExecCompare),
        "ssh_audit" => Some(McpTool::SshAudit),
        "ssh_audit_export" => Some(McpTool::SshAuditExport),
        "ssh_sftp_ls" => Some(McpTool::SshSftpLs),
        "ssh_sftp_stat" => Some(McpTool::SshSftpStat),
        "ssh_sftp_mkdir" => Some(McpTool::SshSftpMkdir),
        "ssh_sftp_rename" => Some(McpTool::SshSftpRename),
        "ssh_sftp_rm" => Some(McpTool::SshSftpRm),
        "ssh_sftp_rmdir" => Some(McpTool::SshSftpRmdir),
        "ssh_sftp_rm_rf" => Some(McpTool::SshSftpRmRf),
        "ssh_sftp_upload" => Some(McpTool::SshSftpUpload),
        "ssh_sftp_download" => Some(McpTool::SshSftpDownload),
        "ssh_session_open" => Some(McpTool::SshSessionOpen),
        "ssh_session_write" => Some(McpTool::SshSessionWrite),
        "ssh_session_read" => Some(McpTool::SshSessionRead),
        "ssh_session_close" => Some(McpTool::SshSessionClose),
        "ssh_session_list" => Some(McpTool::SshSessionList),
        "ssh_forward_add" => Some(McpTool::SshForwardAdd),
        "ssh_forward_list" => Some(McpTool::SshForwardList),
        "ssh_forward_remove" => Some(McpTool::SshForwardRemove),
        "ssh_risk_check" => Some(McpTool::SshRiskCheck),
        "ssh_gate_status" => Some(McpTool::SshGateStatus),
        "ssh_approval_list" => Some(McpTool::SshApprovalList),
        "ssh_approval_respond" => Some(McpTool::SshApprovalRespond),
        "ssh_playbook_list" => Some(McpTool::SshPlaybookList),
        "ssh_playbook_run" => Some(McpTool::SshPlaybookRun),
        "ssh_playbook_dry_run" => Some(McpTool::SshPlaybookDryRun),
        "ssh_snippet_list" => Some(McpTool::SshSnippetList),
        "ssh_snippet_save" => Some(McpTool::SshSnippetSave),
        "ssh_snippet_delete" => Some(McpTool::SshSnippetDelete),
        "ssh_connection_status" => Some(McpTool::SshConnectionStatus),
        "ssh_connect" => Some(McpTool::SshConnect),
        "ssh_disconnect" => Some(McpTool::SshDisconnect),
        "ssh_webhook_config" => Some(McpTool::SshWebhookConfig),
        "ssh_config_export" => Some(McpTool::SshConfigExport),
        "ssh_config_import" => Some(McpTool::SshConfigImport),
        "ssh_config_import_preview" => Some(McpTool::SshConfigImportPreview),
        "ssh_doctor" => Some(McpTool::SshDoctor),
        "ssh_metrics" => Some(McpTool::SshMetrics),
        "ssh_preview_exec" => Some(McpTool::SshPreviewExec),
        "ssh_approval_policies_list" => Some(McpTool::SshApprovalPoliciesList),
        "ssh_approval_check" => Some(McpTool::SshApprovalCheck),
        "ssh_health_snapshot" => Some(McpTool::SshHealthSnapshot),
        "ssh_daemon_diagnose" => Some(McpTool::SshDaemonDiagnose),
        "ssh_daemon_version_check" => Some(McpTool::SshDaemonVersionCheck),
        "ssh_daemons_view" => Some(McpTool::SshDaemonsView),
        "ssh_metrics_trend" => Some(McpTool::SshMetricsTrend),
        "ssh_events_subscribe" => Some(McpTool::SshEventsSubscribe),
        "ssh_sync_diff" => Some(McpTool::SshSyncDiff),
        "ssh_sync_export" => Some(McpTool::SshSyncExport),
        "ssh_redact_rule_list" => Some(McpTool::SshRedactRuleList),
        "ssh_redact_rule_add" => Some(McpTool::SshRedactRuleAdd),
        "ssh_redact_rule_update" => Some(McpTool::SshRedactRuleUpdate),
        "ssh_redact_rule_remove" => Some(McpTool::SshRedactRuleRemove),
        "ssh_redact_rule_reset" => Some(McpTool::SshRedactRuleReset),
        "ssh_highlight_rule_list" => Some(McpTool::SshHighlightRuleList),
        "ssh_highlight_rule_add" => Some(McpTool::SshHighlightRuleAdd),
        "ssh_highlight_rule_update" => Some(McpTool::SshHighlightRuleUpdate),
        "ssh_highlight_rule_remove" => Some(McpTool::SshHighlightRuleRemove),
        "ssh_highlight_rule_reset" => Some(McpTool::SshHighlightRuleReset),
        "ssh_algo_prefs_get" => Some(McpTool::SshAlgoPrefsGet),
        "ssh_algo_prefs_set" => Some(McpTool::SshAlgoPrefsSet),
        "ssh_algo_prefs_clear" => Some(McpTool::SshAlgoPrefsClear),
        _ => None,
    }
}

fn validate_required_args(definition: &Value, args: &Value) -> Result<(), McpError> {
    let Some(required) = definition
        .get("inputSchema")
        .and_then(|schema| schema.get("required"))
        .and_then(Value::as_array)
    else {
        return Ok(());
    };
    if required.is_empty() {
        return Ok(());
    }
    let object = args
        .as_object()
        .ok_or_else(|| McpError::internal("arguments must be an object"))?;
    for field in required.iter().filter_map(Value::as_str) {
        if object.get(field).is_none_or(Value::is_null) {
            return Err(McpError::internal(format!("{field} required")));
        }
    }
    Ok(())
}

fn tool_definitions() -> Vec<Value> {
    json!([
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
                            "change_id":        { "type": "string", "description": "Optional change/ticket ID for this operation (audit trail)." },
                            "side_effect":      { "type": "string", "description": "Optional side-effect description for the approval card and audit trail (e.g. 'triggers a Full GC, 100-300ms pause')." }
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
                    "description": "List a remote directory via embedded SFTP.",
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
                    "description": "Stat a remote file or directory via embedded SFTP.",
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
                    "description": "Create a remote directory recursively via embedded SFTP.",
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
                    "name": "ssh_sftp_rename",
                    "description": "Rename or move a remote file or directory via embedded SFTP. Rated medium risk.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["host", "old_path", "new_path"],
                        "properties": {
                            "host":         { "type": "string" },
                            "old_path":     { "type": "string" },
                            "new_path":     { "type": "string" },
                            "timeout_secs": { "type": "integer" }
                        }
                    }
                },
                {
                    "name": "ssh_sftp_rm",
                    "description": "Delete a single remote file via embedded SFTP. Rated low risk, but host approval policies may still require approval.",
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
                    "name": "ssh_sftp_rmdir",
                    "description": "Remove an empty remote directory via embedded SFTP. Fails when the directory is not empty; use ssh_sftp_rm_rf for a recursive delete. Rated low risk.",
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
                    "name": "ssh_sftp_rm_rf",
                    "description": "Recursively delete a remote directory tree via embedded SFTP (the remote equivalent of `rm -rf`). Rated high risk, and blocked outright when the target is `/`, `/*` or `/.`; other targets may still require approval. This cannot be undone.",
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
                    "description": "Upload a local file to a remote host via embedded SFTP.",
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
                    "description": "Download a file from a remote host via embedded SFTP.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["host", "remote_path", "local_path"],
                        "properties": {
                            "host":        { "type": "string", "description": "Host profile alias." },
                            "remote_path": { "type": "string", "description": "Remote file path to download." },
                            "local_path":  { "type": "string", "description": "Local destination path." },
                            "max_mb":      { "type": "integer", "description": "Hard cap on the remote file size in MiB (default 100). Larger files are rejected with guidance to use scp/rsync." }
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
                    "name": "ssh_snippet_list",
                    "description": "List reusable command snippets from ~/.agent2ssh/snippets.json.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_snippet_save",
                    "description": "Create or replace a reusable command snippet. Names are unique and commands are stored locally; this does not execute the command.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["name", "command"],
                        "properties": {
                            "name": { "type": "string", "description": "Unique snippet name (maximum 128 characters)." },
                            "command": { "type": "string", "description": "Command text to save; it is not executed." },
                            "description": { "type": "string", "description": "Optional human-readable note." }
                        }
                    }
                },
                {
                    "name": "ssh_snippet_delete",
                    "description": "Delete a reusable command snippet by name. This does not execute the saved command.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["name"],
                        "properties": {
                            "name": { "type": "string", "description": "Snippet name to delete." }
                        }
                    }
                },
                {
                    "name": "ssh_connection_status",
                    "description": "List all configured hosts and their current embedded SSH connection status.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_connect",
                    "description": "Manually establish and retain an embedded SSH connection to a specific host.",
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
                    "description": "Manually close a retained embedded SSH connection to a specific host.",
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
                    "description": "Run diagnostic checks on the agent2ssh environment: verify embedded SSH/keygen capability, config directory, hosts.json, daemon.token permissions, daemon health, optional config files, and audit log size.",
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
                },
                {
                    "name": "ssh_redact_rule_list",
                    "description": "List the sensitive-data redaction rules this instance applies before it writes an audit record, webhook notification, playbook result or diagnostic export. Each rule is marked when it is one of the built-in defaults. Read-only.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_redact_rule_add",
                    "description": "Add a redaction rule. A rule's pattern is its identity, so adding a pattern that is already present fails instead of overwriting it, and an invalid regex is rejected before anything is written. The replacement is literal.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["pattern", "replacement"],
                        "properties": {
                            "pattern":     { "type": "string", "description": "Regular expression to match; also the rule's identity." },
                            "replacement": { "type": "string", "description": "Literal replacement text, used verbatim (capture groups are not expanded)." }
                        }
                    }
                },
                {
                    "name": "ssh_redact_rule_update",
                    "description": "Replace the redaction rule identified by pattern. Omit new_pattern to keep the current pattern, which is the usual case of changing only the replacement.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["pattern", "replacement"],
                        "properties": {
                            "pattern":     { "type": "string", "description": "The pattern that identifies the rule to replace." },
                            "replacement": { "type": "string", "description": "New literal replacement text." },
                            "new_pattern": { "type": "string", "description": "Rename the rule to this pattern (default: keep pattern)." }
                        }
                    }
                },
                {
                    "name": "ssh_redact_rule_remove",
                    "description": "Remove a redaction rule. This cannot be undone by re-adding the pattern as it was, and it stops that class of secret being redacted everywhere this instance writes, so force must be true; a call without it is rejected and changes nothing. ssh_redact_rule_reset restores the built-in set.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["pattern", "force"],
                        "properties": {
                            "pattern": { "type": "string", "description": "The pattern that identifies the rule to remove." },
                            "force":   { "type": "boolean", "description": "Must be true. Removing a rule means that class of secret stops being redacted in every audit record, webhook notification, playbook result and diagnostic export." }
                        }
                    }
                },
                {
                    "name": "ssh_redact_rule_reset",
                    "description": "Restore the built-in redaction rule set, discarding every edit you have made. This is the way back from a removal, because it only ever adds rules back. Modifies configuration.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_highlight_rule_list",
                    "description": "List the terminal highlight rules the UI paints matches with. Read-only.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_highlight_rule_add",
                    "description": "Add a terminal highlight rule. A rule's keyword is its identity, so a duplicate fails instead of overwriting. Rules are stored as regular expressions: pass literal true to escape the keyword first so it matches as plain text.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["keyword", "name", "color"],
                        "properties": {
                            "keyword":        { "type": "string",  "description": "Pattern to match, or literal text when literal is true; also the rule's identity." },
                            "name":           { "type": "string",  "description": "Human-readable label shown in the settings panel." },
                            "color":          { "type": "string",  "description": "Hex color like #FF6B6B." },
                            "case_sensitive": { "type": "boolean", "description": "Match case-sensitively (default false)." },
                            "literal":        { "type": "boolean", "description": "Escape keyword so it matches literally instead of as a regular expression (default false)." }
                        }
                    }
                },
                {
                    "name": "ssh_highlight_rule_update",
                    "description": "Replace the terminal highlight rule identified by keyword. Every field you omit keeps its current value. This changes only what the terminal paints, never what the app writes.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["keyword"],
                        "properties": {
                            "keyword":        { "type": "string",  "description": "The keyword that identifies the rule to replace." },
                            "new_keyword":    { "type": "string",  "description": "Rename the rule to this pattern (default: keep keyword)." },
                            "name":           { "type": "string",  "description": "New label (default: keep)." },
                            "color":          { "type": "string",  "description": "New hex color (default: keep)." },
                            "case_sensitive": { "type": "boolean", "description": "New case-sensitivity (default: keep)." },
                            "literal":        { "type": "boolean", "description": "Escape new_keyword so it matches literally; meaningful only together with new_keyword." }
                        }
                    }
                },
                {
                    "name": "ssh_highlight_rule_remove",
                    "description": "Remove a terminal highlight rule. This hides nothing the app writes, but it still requires force so a mistyped keyword cannot quietly drop a rule you meant to keep. ssh_highlight_rule_reset restores the built-in set.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["keyword", "force"],
                        "properties": {
                            "keyword": { "type": "string",  "description": "The keyword that identifies the rule to remove." },
                            "force":   { "type": "boolean", "description": "Must be true." }
                        }
                    }
                },
                {
                    "name": "ssh_highlight_rule_reset",
                    "description": "Restore the built-in terminal highlight rules, discarding every edit. Modifies configuration.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_algo_prefs_get",
                    "description": "Show the SSH algorithm preferences in effect for the next handshake, whether they are an override or the built-in defaults, and the defaults they would fall back to. Read-only.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "ssh_algo_prefs_set",
                    "description": "Replace one or more SSH algorithm lists; the ones you leave out keep their current value. Each list is checked before it is stored: it must not be empty, must not repeat an entry, and every entry has to look like an algorithm name, so a list that could not complete a handshake is rejected rather than written. Takes effect on the next handshake; already-open sessions keep the algorithms they negotiated.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "kex":       { "type": "string", "description": "Comma-separated key exchange algorithms, most preferred first." },
                            "hostkey":   { "type": "string", "description": "Comma-separated host key algorithms." },
                            "cipher_cs": { "type": "string", "description": "Comma-separated client-to-server ciphers." },
                            "cipher_sc": { "type": "string", "description": "Comma-separated server-to-client ciphers." },
                            "mac_cs":    { "type": "string", "description": "Comma-separated client-to-server MACs." },
                            "mac_sc":    { "type": "string", "description": "Comma-separated server-to-client MACs." },
                            "comp_cs":   { "type": "string", "description": "Comma-separated client-to-server compression." },
                            "comp_sc":   { "type": "string", "description": "Comma-separated server-to-client compression." }
                        }
                    }
                },
                {
                    "name": "ssh_algo_prefs_clear",
                    "description": "Drop the SSH algorithm overrides and go back to the built-in safe defaults. Modifies configuration.",
                    "inputSchema": { "type": "object", "properties": {} }
                }
            ])
        .as_array()
        .expect("MCP tool definitions must be an array")
        .clone()
}
