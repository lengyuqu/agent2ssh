# Agent2SSH MCP Tools Reference

Agent2SSH exposes 24 tools via the Model Context Protocol (MCP) stdio server.

## Tool List

| # | Tool | Description |
|---|------|-------------|
| 1 | `ssh_list_hosts` | List configured SSH host profiles |
| 2 | `ssh_add_host` | Create or update an SSH host profile |
| 3 | `ssh_remove_host` | Remove a host profile by alias |
| 4 | `ssh_import_config` | Import hosts from ~/.ssh/config |
| 5 | `ssh_exec` | Run a non-interactive command over SSH |
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
| 23 | `ssh_approval_list` | List pending approval requests |
| 24 | `ssh_approval_respond` | Approve or reject an approval request |

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

## User-Defined Risk Rules

Custom rules can be added to `~/.agent2ssh/risk_rules.toml`:

```toml
[blocked]
patterns = ["kubectl delete namespace", "terraform destroy"]

[high]
patterns = ["docker system prune", "git push --force"]

[medium]
patterns = []
```

Rules support glob patterns with `*`. User rules are checked before built-in rules.

## Per-Host Risk Override

Set `risk_override` on a host profile to override the risk level for all commands on that host. For example, setting `risk_override: "low"` on a sandbox host allows running any command without confirmation.
