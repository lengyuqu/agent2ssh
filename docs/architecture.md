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
| `src-tauri/src/execution_control.rs` | Shared execution authorization for scope, effective risk, approval, and rejected audit entries |
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
                 │ host CRUD · exec · authz · risk · SFTP · sessions  │
                 │ forwards · approval · audit · keys · playbooks     │
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

Execution entry points resolve an effective risk before running remote work:

| Level | Examples | Behaviour |
|-------|----------|-----------|
| `low` | `ls`, `cat`, `ps`, `df`, `grep` | Executes freely |
| `medium` | `apt install`, `sed -i`, `git push`, `chmod` | Executes; shown in UI with badge |
| `high` | `sudo`, `rm -rf`, `kill -9`, `iptables` | Requires `--force` / `force: true`, or daemon approval flow |
| `blocked` | `shutdown`, `mkfs`, `rm -rf /`, fork-bomb | Always rejected |

Effective risk is calculated from the built-in classifier plus user policy rules. User rules from `policy.toml` / `policy.json` or legacy `risk_rules.toml` can only raise the built-in risk. Host and playbook `risk_override` settings are trusted overrides for non-blocked commands, but `blocked` remains unconditional. Approval policies live in the same unified policy file, with legacy `approval_policies.toml` supported when no unified policy file exists.

Executions, completed mutation operations, and rejected attempts are appended to `~/.agent2ssh/audit.jsonl` with the risk level and source recorded. SFTP, PTY session open/write/close, port-forward add/remove, connection operations, and playbook steps are represented as operation command strings until they gain first-class policy types, so mutation paths pass through the same authorization and audit machinery. Read/list-style observation paths remain event/scope controlled and do not create operation audit entries by default.

PTY session writes are authorized at completed-line boundaries by combining any buffered pending input with the new write. Daemon-managed sessions and desktop-local sessions both use this line buffer and append operation-level audit entries for completed input. This blocks normal fragmented shell commands such as splitting `rm -rf /` across multiple writes, but it is not a full shell parser for arbitrary interactive terminal applications.

For multi-host execution and playbooks, high-risk approvals are applied only to the target host or playbook step that received approval. Explicit `force` still applies to the whole requested operation when the caller chooses it and policy permits it.

## Execution Control Flow

`execution_control.rs` centralizes the common authorization path used by CLI, MCP, Tauri, and daemon surfaces:

1. Resolve the host target, tags, and optional host/playbook `risk_override`.
2. Enforce daemon or remote-token scope restrictions before approval is requested.
3. Calculate effective risk from built-in rules, user rules, and trusted overrides.
4. Reject `blocked` work immediately and write a rejected audit entry.
5. Enforce approval policy or high-risk approval/force requirements.
6. Execute the operation with approved-host or approved-step force only where applicable, then append the final audit entry for exec/mutation paths.

The daemon approval handler creates an approval request and waits for approval, rejection, or timeout. Local CLI/MCP and desktop-local paths without an approval handler fail closed and instruct the caller to use the daemon approval flow or `--force` when policy permits. WebSocket exec streaming uses the same core SSH command builder as non-streaming exec, so password, key, jump-host, and ControlMaster behavior stay aligned.

## Control Plane

The current control-plane layer is enforced at the daemon/audit boundary rather than only in the desktop UI:

| Capability | Config / entry point | Behaviour |
|------------|----------------------|-----------|
| Execution gate | `agent2ssh pause/resume/status`, `execution_gate.toml` | Pauses non-desktop daemon mutation/execution paths and returns HTTP 423 for blocked sources |
| Execution limits | `execution_limits.toml` | Enforces per-source, per-host, and per-tag execution rate plus session concurrency limits; returns HTTP 429 on limit rejection |
| Policy dry-run | `policy.toml` / `policy.json`, `agent2ssh policy validate/test` | Validates policy-as-code and predicts `allow` / `approve` / `block` decisions |
| Anomaly detection | `anomaly.toml` | Detects source bursts, sensitive command patterns, and after-hours high-risk activity from audit windows |

The desktop Settings menu is the operator surface for local recovery and control. It exposes local daemon health from `/health` with version, PID, and last check time; shows execution gate status as active, paused, or unavailable; supports manual refresh; and links to the daemon Web Console URL. The desktop control-plane research record is in `docs/reports/r5-desktop-control-plane-research-report.md`.

The daemon event stream exposes `gate_rejected`, `limit_rejected`, and `anomaly_detected`, allowing Live Agent Activity and webhook consumers to react while the activity is still local and recent.

For remote daemon routing, client-side `remotes.toml` scope is checked before forwarding. Host allowlists and command patterns are local checks; tag-based scope fetches the target host metadata from the remote daemon so multi-node tag decisions use the remote daemon as the source of truth.

## Persistence And Locking

Host configuration and audit writes use cross-process lock files under `~/.agent2ssh/` plus atomic temp-file writes and rename. This keeps concurrent CLI, MCP, daemon, and desktop access from corrupting `hosts.json` or interleaving audit writes. The internal lock files are `.hosts.lock` and `.audit.lock`.

## Current Direction

The original daemon, approval, risk configuration, web console, Live Activity, session takeover, and G-stage control-plane milestones are implemented. R5 confirmed the desktop Settings menu as the local operator surface for daemon health, gate recovery, and console handoff. The next phase is release/adoption closure, cross-platform validation, and broader end-to-end testing.

```text
Desktop App (Windows / Linux / macOS)  →  local HTTP/WebSocket daemon
Web Console                            →  local or configured remote daemon
MCP / CLI                              →  local or configured remote daemon
```
