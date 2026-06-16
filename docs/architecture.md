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
| `src-tauri/src/policy.rs` | Unified policy-as-code loader for `policy.toml` / `policy.json` |
| `src-tauri/src/risk_config.rs` | Risk rule compatibility layer for legacy `risk_rules.toml` |
| `src-tauri/src/gate.rs` | Daemon execution gate state and source bypass rules |
| `src-tauri/src/limits.rs` | Daemon execution rate and session concurrency limits |
| `src-tauri/src/anomaly.rs` | Audit-window anomaly detection and anomaly event publishing |
| `src-tauri/src/keys.rs` | SSH key generation, import, listing, and deletion |
| `src-tauri/src/playbook.rs` | Playbook loading and sequential execution |
| `src-tauri/src/notify.rs` | Webhook configuration and delivery |
| `src-tauri/src/events.rs` | Local event bus for daemon SSE, activity monitoring, approvals, audit rotation, and SSH operation events |
| `src-tauri/src/remote.rs` | Remote daemon registry and health probing |
| `src-tauri/src/types.rs` | Shared types: `HostProfile`, `ExecRequest`, `ExecResult`, `RiskLevel`, etc. |
| `src-tauri/src/tauri_commands.rs` | Tauri IPC commands wrapping the core |
| `src-tauri/src/bin/agent2ssh.rs` | CLI binary |
| `src-tauri/src/bin/agent2ssh-mcp.rs` | MCP stdio server (JSON-RPC 2.0, 51 tools) |
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

The desktop app, CLI, MCP server, and daemon share the same Rust core library. The MCP server exposes 51 tools and speaks JSON-RPC 2.0 over stdio, making it compatible with any MCP-capable agent host.

## Local Activity Visibility

Agent2SSH is also a local observation surface for agent-driven SSH activity. The daemon exposes an authenticated SSE endpoint at `/events/stream`, and the desktop app subscribes to it through the Live Agent Activity panel.

Current live events cover daemon-managed PTY session open/write/read/close, WebSocket exec start/output/exit, approvals, audit rotation, execution gate changes/rejections, execution limit rejections, anomaly detections, and connection/config changes. The panel also polls recent audit records, so completed CLI/MCP execs that write to the same config directory are visible even when they did not originate from the desktop UI.

MCP PTY sessions route to the local daemon registry by default when the daemon is reachable and the local token is available. If the daemon is unavailable, MCP falls back to the process-local session store so basic PTY usage still works. The desktop Session panel also connects to the daemon session registry, so daemon-managed MCP sessions can be listed, attached, tailed, read, written to, and closed from the UI. Read-only attach and high-risk input confirmation provide a conservative default for observing externally created PTY sessions.

## Safety Model

Every command passes through `classify_risk()` before execution:

| Level | Examples | Behaviour |
|-------|----------|-----------|
| `low` | `ls`, `cat`, `ps`, `df`, `grep` | Executes freely |
| `medium` | `apt install`, `sed -i`, `git push`, `chmod` | Executes; shown in UI with badge |
| `high` | `sudo`, `rm -rf`, `kill -9`, `iptables` | Requires `--force` / `force: true`, or daemon approval flow |
| `blocked` | `shutdown`, `mkfs`, `rm -rf /`, fork-bomb | Always rejected |

Unified policy files (`policy.toml` / `policy.json`) can define custom risk rules and approval policies in one versionable file. Legacy `risk_rules.toml` and `approval_policies.toml` remain supported when no unified policy file exists. All executions, including blocked attempts, are appended to `~/.agent2ssh/audit.jsonl` with the risk level recorded.

## Control Plane

The current control-plane layer is enforced at the daemon/audit boundary rather than only in the desktop UI:

| Capability | Config / entry point | Behaviour |
|------------|----------------------|-----------|
| Execution gate | `agent2ssh pause/resume/status`, `execution_gate.toml` | Pauses non-desktop daemon execution and returns HTTP 423 for blocked sources |
| Execution limits | `execution_limits.toml` | Enforces per-source, per-host, and per-tag execution rate/session concurrency limits; returns HTTP 429 on limit rejection |
| Policy dry-run | `policy.toml` / `policy.json`, `agent2ssh policy validate/test` | Validates policy-as-code and predicts `allow` / `approve` / `block` decisions |
| Anomaly detection | `anomaly.toml` | Detects source bursts, sensitive command patterns, and after-hours high-risk activity from audit windows |

The daemon event stream exposes `gate_rejected`, `limit_rejected`, and `anomaly_detected`, allowing Live Agent Activity and webhook consumers to react while the activity is still local and recent.

## Current Direction

The original daemon, approval, risk configuration, web console, Live Activity, session takeover, and G-stage control-plane milestones are implemented. The next phase is release/adoption closure, cross-platform validation, and broader end-to-end testing.

```text
Desktop App (Windows / Linux / macOS)  →  local HTTP/WebSocket daemon
Web Console                            →  local or configured remote daemon
MCP / CLI                              →  local or configured remote daemon
```
