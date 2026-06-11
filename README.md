# Agent2SSH

Agent2SSH is a local SSH capability layer for general-purpose agents. It exposes the same SSH core through:

- a Tauri desktop app (`agent2ssh-app`)
- an `agent2ssh` CLI
- an `agent2ssh-mcp` stdio MCP server
- a Codex/agent skill prompt in `skills/agent2ssh/SKILL.md`

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

## Implemented Features

### Host Management
- Host profile CRUD (`host add / list / rm`)
- Import from `~/.ssh/config` (`host import-config`)
- Jump host / ProxyJump support

### Command Execution
- Non-interactive SSH exec with JSON output (stdout, stderr, exit code, duration)
- Configurable timeout and stdin piping
- Multi-host concurrent exec (`exec-multi`)
- Connectivity check (`ping`) with latency reporting

### Risk Scoring
- Every command is classified as `low / medium / high / blocked`
- High-risk commands require `--force` / `force: true`
- Blocked commands are always rejected
- Risk level recorded in audit log

### File Transfer (SFTP)
- Upload: `sftp put` / `ssh_sftp_upload`
- Download: `sftp get` / `ssh_sftp_download`
- Remote directory listing: `sftp ls` / `ssh_sftp_ls`
- Remote stat: `sftp stat` / `ssh_sftp_stat`
- Remote mkdir: `sftp mkdir` / `ssh_sftp_mkdir`

### Persistent PTY Sessions
- Open an interactive shell session (`session open`)
- Send input (`session write`) and read buffered output (`session read`)
- List and close sessions
- Useful for REPLs and long-running interactive programs

### Port Forwarding
- Local forward (`-L`) and remote forward (`-R`) tunnels
- Manage by ID: add / list / remove

### Audit Log
- All executions written to `~/.agent2ssh/audit.jsonl`
- Filterable by host, risk level, exit code, and time range

### Desktop UI
- Host management panel
- Command execution with risk badge and force checkbox
- Audit log viewer with risk badges

## MCP Tools (21)

| Tool | Description |
|------|-------------|
| `ssh_list_hosts` | List configured host profiles |
| `ssh_import_config` | Import from `~/.ssh/config` |
| `ssh_add_host` | Create or update a host profile |
| `ssh_remove_host` | Remove a host profile |
| `ssh_ping` | Check reachability and latency |
| `ssh_exec` | Run a command (with risk, force, timeout, stdin) |
| `ssh_exec_multi` | Run a command on multiple hosts concurrently |
| `ssh_audit` | Query the execution audit log |
| `ssh_sftp_upload` | Upload a file via scp |
| `ssh_sftp_download` | Download a file via scp |
| `ssh_sftp_ls` | List a remote directory |
| `ssh_sftp_stat` | Stat a remote file |
| `ssh_sftp_mkdir` | Create a remote directory |
| `ssh_session_open` | Open a persistent PTY session |
| `ssh_session_write` | Send input to a PTY session |
| `ssh_session_read` | Read buffered output from a PTY session |
| `ssh_session_close` | Close a PTY session |
| `ssh_session_list` | List open PTY sessions |
| `ssh_forward_add` | Start a port forward tunnel |
| `ssh_forward_list` | List active tunnels |
| `ssh_forward_remove` | Stop a tunnel |

## Roadmap

Target platforms: **Windows / Linux / macOS** (Tauri desktop app runs on all three).

- HTTP daemon API — lets the web console connect to the local core
- Approval gates — desktop pop-up for high-risk commands before dispatch
- Configurable risk rules — user-defined blocked/high/medium patterns, per-host overrides
- Web console — browser-based audit log viewer, host manager, and live exec panel
