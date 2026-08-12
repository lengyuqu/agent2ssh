# Agent2SSH MCP Tools Reference

Agent2SSH exposes 54 tools via the Model Context Protocol (MCP) stdio server.

## Tool List

The numbering below matches the order returned by `tools/list` in
`src-tauri/src/bin/agent2ssh_mcp/tools.rs`, which is the contract enforced by
the `mcp_tools_match_skills_md_documentation` integration test.

| # | Tool | Description |
|---|------|-------------|
| 1 | `ssh_list_hosts` | List configured SSH host profiles. |
| 2 | `ssh_list_daemons` | List all configured daemons (localhost + remote daemons from `~/.agent2ssh/remotes.toml`). Returns alias, URL, and connected status. |
| 3 | `ssh_import_config` | Import SSH host profiles from `~/.ssh/config` (or a custom path). Skips aliases that already exist. |
| 4 | `ssh_add_host` | Create or update an SSH host profile. |
| 5 | `ssh_remove_host` | Remove a configured SSH host profile by alias. |
| 6 | `ssh_exec` | Run a non-interactive command over SSH. Returns stdout, stderr, exit code, timing, and risk level. High-risk commands require `force: true`; blocked commands always fail. Optionally forward to a remote daemon via `daemon_alias`. |
| 7 | `ssh_ping` | Check SSH reachability of one or more hosts. Returns reachable status and latency for each. |
| 8 | `ssh_exec_multi` | Run the same command on multiple hosts concurrently. Returns an array of per-host results. Supports optional batch strategy for concurrency limits, failure thresholds, and batched rollout. |
| 9 | `ssh_exec_compare` | Compare execution results across multiple hosts. Groups by exit code and highlights stdout/stderr differences. Provide either results directly or run a command on multiple hosts. |
| 10 | `ssh_audit` | Return recent SSH execution audit log entries with optional filtering. |
| 11 | `ssh_audit_export` | Export audit log entries as JSONL or CSV with optional filtering. Redaction is applied at write time so exported data preserves redaction. |
| 12 | `ssh_sftp_ls` | List a remote directory via embedded SFTP. |
| 13 | `ssh_sftp_stat` | Stat a remote file or directory via embedded SFTP. |
| 14 | `ssh_sftp_mkdir` | Create a remote directory recursively via embedded SFTP. |
| 15 | `ssh_sftp_upload` | Upload a local file to a remote host via embedded SFTP. |
| 16 | `ssh_sftp_download` | Download a file from a remote host via embedded SFTP. |
| 17 | `ssh_session_open` | Open a persistent interactive PTY session. Returns a `session_id` for subsequent write/read/close calls. |
| 18 | `ssh_session_write` | Send input to an open PTY session (e.g. a command followed by `\n`). |
| 19 | `ssh_session_read` | Read buffered output from a PTY session. Returns whatever arrived since the last read. |
| 20 | `ssh_session_close` | Close and terminate a PTY session. |
| 21 | `ssh_session_list` | List open PTY sessions. Defaults to the local daemon registry and includes MCP process-local fallback sessions when present. |
| 22 | `ssh_forward_add` | Start an SSH port forward tunnel (`-L` local or `-R` remote). Returns a `forward_id`. |
| 23 | `ssh_forward_list` | List active SSH port forward tunnels. |
| 24 | `ssh_forward_remove` | Stop and remove an SSH port forward by ID. |
| 25 | `ssh_risk_check` | Check the risk level of a command using built-in rules and user-defined `risk_rules.toml`. |
| 26 | `ssh_gate_status` | Read the local daemon execution gate status. When paused, non-desktop daemon execution is rejected until resumed from CLI or desktop. |
| 27 | `ssh_approval_list` | List all pending and recent approval requests (for high-risk command authorization). |
| 28 | `ssh_approval_respond` | Approve or reject a pending approval request by ID. |
| 29 | `ssh_playbook_list` | List all configured playbooks from `~/.agent2ssh/playbooks.toml`. |
| 30 | `ssh_playbook_run` | Run a named playbook (sequence of SSH commands) against a target host. Steps execute sequentially; halts on first failure. Supports template parameters via the `params` object. |
| 31 | `ssh_playbook_dry_run` | Preview a playbook without executing. Resolves template parameters and returns the commands that would be run. |
| 32 | `ssh_snippet_list` | List reusable command snippets from `~/.agent2ssh/snippets.json`. |
| 33 | `ssh_snippet_save` | Create or replace a reusable command snippet without executing it. |
| 34 | `ssh_snippet_delete` | Delete a reusable command snippet by name without executing it. |
| 35 | `ssh_connection_status` | List all configured hosts and their current embedded SSH connection status. |
| 36 | `ssh_connect` | Manually establish and retain an embedded SSH connection to a specific host. |
| 37 | `ssh_disconnect` | Manually close a retained embedded SSH connection to a specific host. |
| 38 | `ssh_webhook_config` | Get or set webhook notification configuration. Use `action: "get"` to retrieve current config, or `action: "set"` with `url`/`events`/`secret` to update. |
| 39 | `ssh_config_export` | Export team configuration (hosts without private key paths, risk rules, and playbooks). Returns a JSON object suitable for sharing within a team. |
| 40 | `ssh_config_import` | Import team configuration from a JSON object. Merges hosts (skips duplicates by name), and overwrites risk rules and playbooks if provided. |
| 41 | `ssh_config_import_preview` | Preview what a team config import will change without actually importing. Shows hosts to add, skip, update, and risk rules/playbook changes. |
| 42 | `ssh_doctor` | Run diagnostic checks on the agent2ssh environment: embedded SSH/keygen capability, config directory, `hosts.json`, daemon token permissions, daemon health, optional config files, and audit log size. |
| 43 | `ssh_metrics` | Retrieve basic metrics from the local agent2ssh daemon (requests, execs, blocked commands, durations, approvals). Reads from `GET /metrics` on `127.0.0.1:7722`. |
| 44 | `ssh_preview_exec` | Preview what an execution will do before running it. Returns target hosts, commands, risk levels, warnings, and whether approval is required. Supports single-host and multi-host preview. |
| 45 | `ssh_approval_policies_list` | List all configured approval policies. Each policy specifies when approval is required based on host, tags, risk level, and command pattern. |
| 46 | `ssh_approval_check` | Check whether running a command on a specific host requires approval based on configured policies. Returns the matching policy name and whether approval is needed. |
| 47 | `ssh_health_snapshot` | Collect a health snapshot (uptime, disk, memory, load, SSH latency) for configured hosts. Returns per-host data collected concurrently via SSH. |
| 48 | `ssh_daemon_diagnose` | Run connection diagnostics on a remote daemon: TCP connectivity, TLS handshake, token configuration, authentication, version compatibility, latency. Returns a detailed report. |
| 49 | `ssh_daemon_version_check` | Check version compatibility between this build and a remote daemon. Returns local version, remote version, compatibility status, and a human-readable message. |
| 50 | `ssh_daemons_view` | Get a unified view of all daemons (localhost + remotes) with their health, metrics, and host counts. |
| 51 | `ssh_metrics_trend` | Show execution metrics trends: volume, failure rate, risk distribution, top hosts, hourly breakdown. Supports `24h` / `7d` / `30d` / `all` period selection. |
| 52 | `ssh_events_subscribe` | Subscribe to the real-time event stream. Returns the latest events from the event bus. For continuous streaming, use the daemon's SSE endpoint `GET /events/stream`. |
| 53 | `ssh_sync_diff` | Compare Agent2SSH hosts with `~/.ssh/config`. Shows hosts only on one side and conflicts. |
| 54 | `ssh_sync_export` | Export Agent2SSH hosts to SSH config format file (default `~/.ssh/config.d/agent2ssh.conf`). |

## Risk Levels

Commands are classified into four risk levels:

- **low** — safe read-only commands (ls, cat, whoami, etc.)
- **medium** — commands that modify state (apt install, git push, sed -i, etc.)
- **high** — potentially destructive commands (sudo, rm -rf, chmod 777, etc.)
- **blocked** — unconditionally dangerous commands (mkfs, rm -rf /, shutdown, fork bomb, etc.)

High-risk commands require daemon approval or `force: true` when policy allows. Local MCP paths that do not have an approval handler fail closed and should be routed through the daemon approval flow. Blocked commands are always rejected.

## Execution Authorization

MCP exec, exec-multi, playbook, SFTP, session open/write, forward add, connect, and disconnect operations use the shared Agent2SSH authorization path where applicable:

- daemon or remote-token scope is checked before approval
- effective risk combines built-in rules and user policy rules
- user policy rules can only escalate built-in risk
- host/playbook `risk_override` can lower or raise non-blocked risk
- rejected attempts are written to the audit log with the MCP source

## Approval Flow

When the daemon is running, high-risk commands can be routed through an approval gate:

1. MCP `ssh_exec` or another daemon-routed operation with high-risk work creates an approval request
2. Query pending approvals with `ssh_approval_list`
3. Approve or reject with `ssh_approval_respond`

## Policy-as-Code (Recommended)

Define unified security policies in `~/.agent2ssh/policy.toml`. This file supports risk rules and approval policies in one versionable place. Execution gate, execution limits, and anomaly detection use their own files: `execution_gate.toml`, `execution_limits.toml`, and `anomaly.toml`.

```toml
[risk.blocked]
patterns = ["kubectl delete namespace*", "terraform destroy*"]

[risk.high]
patterns = ["docker system prune*", "git push *force*"]

[risk.medium]
patterns = ["apt install*", "yum install*"]

[[approval.policies]]
name = "production high risk"
tags = ["production"]
min_risk = "high"
requires_approval = true
ttl_secs = 300

[[approval.policies]]
name = "sandbox auto approve"
hosts = ["sandbox"]
requires_approval = false
```

All risk rules support glob patterns with `*`. User rules are merged with built-in classification and can only raise risk; they cannot downgrade an internally classified high or blocked command.

> Legacy `~/.agent2ssh/risk_rules.toml` is still supported for backward compatibility, but `policy.toml` is the recommended approach for new deployments.

## Per-Host Risk Override

Set `risk_override` on a host profile to override the risk level for non-blocked commands on that host. For example, setting `risk_override: "low"` on a sandbox host lowers non-blocked commands before approval checks. Commands classified as `blocked` by built-in or user-defined rules are never downgraded by overrides.

## SSH Connection Retention

Agent2SSH can retain embedded SSH sessions for faster connection checks and preconnect workflows:

- Use `ssh_connection_status` to see which hosts have active connections
- Use `ssh_connect` to pre-establish a connection, `ssh_disconnect` to tear one down
- Jump-host connections use the same embedded direct-tcpip bastion channel as exec, SFTP, terminal, sessions, and forwards

## Webhook Notifications

Configure `~/.agent2ssh/webhook.toml` to receive event notifications:

```toml
url = "https://example.com/agent2ssh-webhook"
events = ["approval_required", "exec_blocked", "exec_completed"]
secret = ""  # Optional HMAC-SHA256 signing key
```

When the URL contains `hooks.slack.com`, messages are automatically formatted as Slack Block Kit messages. Use `ssh_webhook_config` to get or set configuration programmatically.

## Playbooks

Define reusable command sequences in `~/.agent2ssh/playbooks.toml`:

```toml
[[playbooks]]
name = "deploy-web"
description = "Pull latest code and restart web service"
steps = [
  "cd /opt/app && git pull",
  "systemctl restart nginx",
]
tags = ["production", "web"]
risk_override = "high"
```

Use `ssh_playbook_list` to list all playbooks, `ssh_playbook_run` to execute one on a target host. Steps run sequentially and halt on first failure. Provide `reason` and `change_id` when the resulting audit entries need operational context.

## Remote Daemon

Connect to agent2ssh-daemon instances running on other machines via `~/.agent2ssh/remotes.toml`:

```toml
[[remotes]]
alias = "ci-server"
url = "http://192.168.1.100:7722"
token_env = "AGENT2SSH_PROD_TOKEN"

[remotes.scope]
allowed_tags = ["staging"]
allowed_commands = ["uptime", "df *", "git *"]
denied_commands = ["rm *"]
```

Use `ssh_list_daemons` to see configured daemons with connectivity status. Pass `daemon_alias` to `ssh_exec` to route commands through a remote daemon. The CLI supports `--daemon <alias>` as a global flag. `remotes.toml` scope is enforced client-side before forwarding; the remote daemon can also enforce server-side scoped tokens via `daemon_tokens.toml`.
