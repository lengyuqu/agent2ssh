# Agent2SSH

Agent2SSH is a local SSH capability layer for general-purpose agents. It exposes the same SSH core through:

- a Tauri desktop app
- an `agent2ssh` CLI
- an `agent2ssh-mcp` stdio MCP server
- a Codex/agent skill prompt in `skills/agent2ssh/SKILL.md`

The first implementation focuses on host profiles, non-interactive SSH execution, JSON output, and audit logs.

## Development

```bash
npm install
npm run build
cd src-tauri && cargo check
```

Run the desktop app:

```bash
npm run tauri:dev
```

Run the CLI:

```bash
cd src-tauri
cargo run --bin agent2ssh -- host add prod --host 10.0.0.12 --user ubuntu --key ~/.ssh/id_ed25519
cargo run --bin agent2ssh -- host list --json
cargo run --bin agent2ssh -- exec prod "uname -a" --json
```

Run the MCP server:

```bash
cd src-tauri
cargo run --bin agent2ssh-mcp
```

## Data

Local data is stored under:

```text
~/.agent2ssh/hosts.json
~/.agent2ssh/audit.jsonl
```

## Current Scope

Implemented:

- host profile create/update/list
- OpenSSH-backed command execution
- desktop UI for host management and command execution
- CLI JSON output for agent use
- MCP tools: `ssh_list_hosts`, `ssh_add_host`, `ssh_exec`
- audit log for executed commands

Next:

- persistent PTY sessions
- SFTP upload/download
- port forwarding
- risk scoring and approval gates
- web/mobile companion clients backed by the same local daemon API
