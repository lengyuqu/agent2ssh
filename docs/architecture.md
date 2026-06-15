# Agent2SSH Architecture

## Direction

Agent2SSH gives agents the ability to operate SSH without embedding SSH logic into each agent host.

```text
Agent / IDE / Automation
        |
        | Desktop / CLI / MCP / Skill / HTTP API
        v
Agent2SSH local capability layer
        |
        | OpenSSH, then native SSH later
        v
Remote hosts
```

## Components

| File | Role |
|------|------|
| `src-tauri/src/core.rs` | SSH exec, ping, exec-multi, SFTP wrappers, risk scoring |
| `src-tauri/src/store.rs` | Host profile persistence and audit log storage under `~/.agent2ssh` |
| `src-tauri/src/session.rs` | Persistent PTY sessions |
| `src-tauri/src/forward.rs` | SSH port forward tunnel management |
| `src-tauri/src/connection.rs` | SSH ControlMaster management and `~/.ssh/config` parser |
| `src-tauri/src/approval.rs` | Approval request queue and response handling |
| `src-tauri/src/risk_config.rs` | User-defined risk rules from `risk_rules.toml` |
| `src-tauri/src/keys.rs` | SSH key generation, import, listing, and deletion |
| `src-tauri/src/playbook.rs` | Playbook loading and sequential execution |
| `src-tauri/src/notify.rs` | Webhook configuration and delivery |
| `src-tauri/src/remote.rs` | Remote daemon registry and health probing |
| `src-tauri/src/types.rs` | Shared types: `HostProfile`, `ExecRequest`, `ExecResult`, `RiskLevel`, etc. |
| `src-tauri/src/tauri_commands.rs` | Tauri IPC commands wrapping the core |
| `src-tauri/src/bin/agent2ssh.rs` | CLI binary |
| `src-tauri/src/bin/agent2ssh-mcp.rs` | MCP stdio server (JSON-RPC 2.0, 50 tools) |
| `src-tauri/src/bin/agent2ssh-daemon.rs` | Local HTTP/WebSocket daemon and browser console server |
| `src/App.tsx` | Desktop console |
| `src-tauri/web/console.html` | Daemon-served browser console |
| `skills/agent2ssh/SKILL.md` | Operational guidance for agents using the CLI or MCP |

## Surfaces

```text
                 ┌────────────────────────────────────────────────────┐
                 │                Agent2SSH Core (Rust)               │
                 │ host CRUD · exec · risk · SFTP · sessions · audit  │
                 │ forwards · approval · keys · playbooks · remotes   │
                 └──────┬──────────────┬──────────────┬──────────────┘
                        │              │              │
                  Tauri IPC       CLI binary       MCP stdio
                  (desktop)     (agent2ssh)    (agent2ssh-mcp)
                        │              │              │
                    React UI      shell/users       agents
                        │
                        └──────── HTTP/WebSocket daemon
                                 (agent2ssh-daemon)
                                         │
                                  browser console
```

The desktop app, CLI, MCP server, and daemon share the same Rust core library. The MCP server exposes 50 tools and speaks JSON-RPC 2.0 over stdio, making it compatible with any MCP-capable agent host.

## Safety Model

Every command passes through `classify_risk()` before execution:

| Level | Examples | Behaviour |
|-------|----------|-----------|
| `low` | `ls`, `cat`, `ps`, `df`, `grep` | Executes freely |
| `medium` | `apt install`, `sed -i`, `git push`, `chmod` | Executes; shown in UI with badge |
| `high` | `sudo`, `rm -rf`, `kill -9`, `iptables` | Requires `--force` / `force: true`, or daemon approval flow |
| `blocked` | `shutdown`, `mkfs`, `rm -rf /`, fork-bomb | Always rejected |

User-defined risk rules can override built-in classification. All executions, including blocked attempts, are appended to `~/.agent2ssh/audit.jsonl` with the risk level recorded.

## Current Direction

The original daemon, approval, risk configuration, and web console milestones are implemented. The next phase is documentation accuracy, release validation, security hardening, and broader end-to-end testing.

```text
Desktop App (Windows / Linux / macOS)  →  local HTTP/WebSocket daemon
Web Console                            →  local or configured remote daemon
MCP / CLI                              →  local or configured remote daemon
```
