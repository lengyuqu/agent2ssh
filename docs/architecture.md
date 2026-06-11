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

- `src-tauri/src/lib.rs`: shared core for host config, SSH exec, audit logging, and Tauri commands.
- `src-tauri/src/bin/agent2ssh.rs`: CLI for shell scripts and skill-driven agents.
- `src-tauri/src/bin/agent2ssh-mcp.rs`: MCP stdio server for MCP-capable agents.
- `src/App.tsx`: desktop console for host management, execution, and audit review.
- `skills/agent2ssh/SKILL.md`: operational guidance for agents using the CLI.

## Three-End Plan

The Tauri app is the desktop control center for macOS, Windows, and Linux.

The same local core should next be promoted into a daemon API:

```text
Desktop App  -> local HTTP/WebSocket daemon
Web Console  -> local daemon or team relay
Mobile App   -> approvals, monitoring, and emergency cancel
```

Desktop owns local credentials and terminal visibility. Web owns team administration and shared audit review. Mobile owns approvals and lightweight incident response.

## Safety Model

The MVP records all executions in `~/.agent2ssh/audit.jsonl`.

The next safety layer should classify commands before execution:

- low: read-only inspection commands
- medium: restart, upload, config edit
- high: sudo, deletion, process kill, production deploy
- blocked: disk formatting, broad system deletion, destructive shutdown

High-risk actions should require a desktop or mobile approval before dispatch.
