# Agent2SSH Architecture

## Direction

Agent2SSH gives agents the ability to operate SSH instead of embedding agents into an SSH client.

```text
Agent / IDE / Automation
        |
        | CLI / MCP / Skill
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
| `src-tauri/src/store.rs` | Host profile persistence (`~/.agent2ssh/hosts.json`) and audit log (`~/.agent2ssh/audit.jsonl`) |
| `src-tauri/src/session.rs` | Persistent PTY sessions (process-local) |
| `src-tauri/src/forward.rs` | SSH port forward tunnel management (process-local) |
| `src-tauri/src/connection.rs` | `~/.ssh/config` parser for `host import-config` |
| `src-tauri/src/types.rs` | Shared types: `HostProfile`, `ExecRequest`, `ExecResult`, `RiskLevel`, etc. |
| `src-tauri/src/tauri_commands.rs` | Tauri IPC commands wrapping the core |
| `src-tauri/src/bin/agent2ssh.rs` | CLI binary |
| `src-tauri/src/bin/agent2ssh-mcp.rs` | MCP stdio server (JSON-RPC 2.0, 21 tools) |
| `src/App.tsx` | Desktop console: host management, execution, audit review |
| `skills/agent2ssh/SKILL.md` | Operational guidance for agents using the CLI or MCP |

## Three-Surface Design

```text
                  ┌──────────────────────────────────────────────────┐
                  │               Agent2SSH Core (Rust)              │
                  │  host CRUD · exec · risk scoring · SFTP          │
                  │  PTY sessions · port forwards · audit log         │
                  └────────┬────────────────┬────────────────┬───────┘
                           │                │                │
                    Tauri IPC          CLI binary       MCP stdio
                    (desktop)        (agent2ssh)    (agent2ssh-mcp)
                           │
                    React/TS UI
```

All three surfaces share the same Rust core library. The MCP server exposes 21 tools and speaks JSON-RPC 2.0 over stdio, making it compatible with any MCP-capable agent host.

## Safety Model

Every command passes through `classify_risk()` before execution:

| Level | Examples | Behaviour |
|-------|----------|-----------|
| `low` | `ls`, `cat`, `ps`, `df`, `grep` | Executes freely |
| `medium` | `apt install`, `sed -i`, `git push`, `chmod` | Executes; shown in UI with badge |
| `high` | `sudo`, `rm -rf`, `kill -9`, `iptables` | Requires `--force` / `force: true` |
| `blocked` | `shutdown`, `mkfs`, `rm -rf /`, fork-bomb | Always rejected |

All executions (including blocked attempts) are appended to `~/.agent2ssh/audit.jsonl` with the risk level recorded.

## Roadmap

The next safety layer is an approval gates UI: high-risk commands trigger a desktop pop-up that must be confirmed before dispatch. This pairs with a local HTTP daemon that lets the web console connect to the same local core.

```text
Desktop App (Windows / Linux / macOS)  →  local HTTP/WebSocket daemon
Web Console                            →  local daemon or team relay
```
