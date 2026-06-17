# Agent2SSH MCP Tools Reference

Agent2SSH exposes 51 tools via the Model Context Protocol (MCP) stdio server.

## Tool List

| # | Tool | Description |
|---|------|-------------|
| 1 | `ssh_list_hosts` | List configured SSH host profiles |
| 2 | `ssh_add_host` | Create or update an SSH host profile |
| 3 | `ssh_remove_host` | Remove a host profile by alias |
| 4 | `ssh_import_config` | Import hosts from ~/.ssh/config |
| 5 | `ssh_exec` | Run a non-interactive command over SSH (supports `daemon_alias` for remote routing) |
| 6 | `ssh_exec_multi` | Run the same command on multiple hosts concurrently |
| 7 | `ssh_ping` | Check SSH reachability of hosts |
| 8 | `ssh_audit` | Query execution audit log entries |
| 9 | `ssh_sftp_ls` | List a remote directory |
| 10 | `ssh_sftp_stat` | Stat a remote file or directory |
| 11 | `ssh_sftp_mkdir` | Create a remote directory |
| 12 | `ssh_sftp_upload` | Upload a local file via scp |
| 13 | `ssh_sftp_download` | Download a remote file via scp |
| 14 | `ssh_session_open` | Open a persistent PTY session |
| 15 | `ssh_session_write` | Send input to an open session |
| 16 | `ssh_session_read` | Read output from a session |
| 17 | `ssh_session_close` | Close a PTY session |
| 18 | `ssh_session_list` | List open PTY sessions |
| 19 | `ssh_forward_add` | Start a port forward tunnel |
| 20 | `ssh_forward_list` | List active port forwards |
| 21 | `ssh_forward_remove` | Stop a port forward |
| 22 | `ssh_risk_check` | Check risk level of a command |
| 23 | `ssh_gate_status` | Read the daemon execution gate status |
| 24 | `ssh_approval_list` | List pending approval requests |
| 25 | `ssh_approval_respond` | Approve or reject an approval request |
| 26 | `ssh_connection_status` | List ControlMaster connection states for all hosts |
| 27 | `ssh_connect` | Manually establish a ControlMaster connection to a host |
| 28 | `ssh_disconnect` | Manually close a ControlMaster connection |
| 29 | `ssh_webhook_config` | Get or set webhook notification configuration |
| 30 | `ssh_playbook_list` | List all configured playbooks |
| 31 | `ssh_playbook_run` | Execute a playbook on a host with optional params, reason, and change ID |
| 32 | `ssh_list_daemons` | List configured daemon instances with connectivity status |
| 33 | `ssh_config_export` | Export team config (hosts without keys, risk rules, playbooks) |
| 34 | `ssh_config_import` | Import team config from JSON |
| 35 | `ssh_doctor` | Run diagnostic checks on SSH, config, and daemon |
| 36 | `ssh_metrics` | Get daemon request/execution/approval counters |
| 37 | `ssh_health_snapshot` | Collect host health data (uptime, disk, memory, load) |
| 38 | `ssh_playbook_dry_run` | Preview playbook steps with resolved parameters |
| 39 | `ssh_preview_exec` | Preview execution plan before running (risk, warnings) |
| 40 | `ssh_approval_policies_list` | List all approval policy rules |
| 41 | `ssh_approval_check` | Check if a host+command requires approval |
| 42 | `ssh_config_import_preview` | Preview team config import diff |
| 43 | `ssh_exec_compare` | Compare execution results across multiple hosts |
| 44 | `ssh_daemon_diagnose` | Run diagnostic checks on a remote daemon |
| 45 | `ssh_daemon_version_check` | Check version compatibility with a remote daemon |
| 46 | `ssh_audit_export` | Export audit log as JSONL or CSV |
| 47 | `ssh_daemons_view` | Unified view of all daemons with health and metrics |
| 48 | `ssh_metrics_trend` | Compute execution trends (24h, 7d, 30d, all) |
| 49 | `ssh_events_subscribe` | Subscribe to the real-time event stream |
| 50 | `ssh_sync_diff` | Compare Agent2SSH hosts with ~/.ssh/config |
| 51 | `ssh_sync_export` | Export Agent2SSH hosts to SSH config format |

## Risk Levels

Commands are classified into four risk levels:

- **low** — safe read-only commands (ls, cat, whoami, etc.)
- **medium** — commands that modify state (apt install, git push, sed -i, etc.)
- **high** — potentially destructive commands (sudo, rm -rf, chmod 777, etc.)
- **blocked** — unconditionally dangerous commands (mkfs, rm -rf /, shutdown, fork bomb, etc.)

High-risk commands require `force: true` in the MCP call. Blocked commands are always rejected.

## Approval Flow

When the daemon is running, high-risk commands can be routed through an approval gate:

1. MCP `ssh_exec` with a high-risk command creates an approval request
2. Query pending approvals with `ssh_approval_list`
3. Approve or reject with `ssh_approval_respond`

## Policy-as-Code (Recommended)

Define unified security policies in `~/.agent2ssh/policy.toml`. This file supports execution gate rules, execution limits, and risk rules in a single place:

```toml
[risk_rules]
[risk_rules.blocked]
patterns = ["kubectl delete namespace", "terraform destroy"]

[risk_rules.high]
patterns = ["docker system prune", "git push --force"]

[risk_rules.medium]
patterns = []

[gate]
max_concurrent = 5          # cap concurrent exec-multi tasks
max_daily_ops = 500         # cap daily operations per host
emergency_stop = false      # set true to pause all execution

[anomaly]
min_threshold = 10          # minimum events to trigger anomaly scoring
```

All risk rules support glob patterns with `*`. User rules are checked before built-in rules.

> Legacy `~/.agent2ssh/risk_rules.toml` is still supported for backward compatibility, but `policy.toml` is the recommended approach for new deployments.

## Per-Host Risk Override

Set `risk_override` on a host profile to override the risk level for commands on that host. For example, setting `risk_override: "low"` on a sandbox host allows non-blocked commands to run without confirmation. Commands classified as `blocked` by built-in or user-defined rules are never downgraded by overrides.

## SSH Connection Pool (ControlMaster)

Agent2SSH automatically manages SSH ControlMaster connections for faster repeated execution:

- First command to a host establishes the ControlMaster socket (`~/.agent2ssh/cm_<host>.sock`)
- Subsequent commands reuse the existing connection, skipping SSH handshake (~500ms → ~10ms)
- Use `ssh_connection_status` to see which hosts have active connections
- Use `ssh_connect` to pre-establish a connection, `ssh_disconnect` to tear one down
- ControlPersist=600 keeps idle connections alive for 10 minutes

## Webhook Notifications

Configure `~/.agent2ssh/webhook.toml` to receive event notifications:

```toml
url = "https://hooks.slack.com/services/T.../B.../..."
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
```

Use `ssh_list_daemons` to see configured daemons with connectivity status. Pass `daemon_alias` to `ssh_exec` to route commands through a remote daemon. The CLI supports `--daemon <alias>` as a global flag.
