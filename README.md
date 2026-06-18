# Agent2SSH

Agent2SSH is a local SSH capability layer for general-purpose agents. It exposes one Rust core through:

- a Tauri desktop app (`agent2ssh-app`)
- an `agent2ssh` CLI
- an `agent2ssh-daemon` local HTTP/WebSocket server
- an `agent2ssh-mcp` stdio MCP server
- a Codex/agent skill prompt in `skills/agent2ssh/SKILL.md`

## Development

```bash
npm install
npm run build
cd src-tauri
cargo check --no-default-features --bin agent2ssh --bin agent2ssh-mcp
cargo check --no-default-features --features daemon --bin agent2ssh-daemon
```

Run the desktop app:

```bash
npm run tauri:dev
```

Run the CLI:

```bash
cd src-tauri
cargo run --no-default-features --bin agent2ssh -- host add prod --host 10.0.0.12 --user ubuntu --key ~/.ssh/id_ed25519
cargo run --no-default-features --bin agent2ssh -- host list --json
cargo run --no-default-features --bin agent2ssh -- exec prod "uname -a" --json
```

Run the MCP server:

```bash
cd src-tauri
cargo run --no-default-features --bin agent2ssh-mcp
```

Run the HTTP daemon:

```bash
cd src-tauri
cargo run --no-default-features --features daemon --bin agent2ssh-daemon
```

## Installation

### Homebrew (macOS)

```bash
brew tap lengyuqu/agent2ssh
brew install agent2ssh
```

### From source

```bash
git clone https://github.com/lengyuqu/agent2ssh.git
cd agent2ssh
npm install && npm run build
cd src-tauri
cargo build --release --no-default-features --bin agent2ssh --bin agent2ssh-mcp
cargo build --release --no-default-features --features daemon --bin agent2ssh-daemon
```

### Pre-built binaries

Download from [GitHub Releases](https://github.com/lengyuqu/agent2ssh/releases).

## Daemon

The daemon serves the local HTTP API and browser console on `127.0.0.1:7722`.

```bash
# Start the HTTP daemon
agent2ssh daemon start

# Check status
agent2ssh daemon status

# Stop the daemon
agent2ssh daemon stop

# Open web console
open http://127.0.0.1:7722/console
```

Authenticated requests use the bearer token stored at `~/.agent2ssh/daemon.token`.

## Data

Local data is stored under `~/.agent2ssh/`. See [docs/guides/configuration-guide.md](docs/guides/configuration-guide.md) for the full file layout, configuration format, and storage details.

## MCP Integration

Configure Agent2SSH as an MCP server in your agent's config:

```json
{
  "mcpServers": {
    "agent2ssh": {
      "command": "agent2ssh-mcp",
      "args": [],
      "env": {
        "AGENT2SSH_SOURCE": "workbuddy"
      }
    }
  }
}
```

Set `AGENT2SSH_SOURCE` to the client label you want to see in Activity, such as
`workbuddy`, `qoder_work`, `trae`, `codex`, or `claude_desktop`.

See [docs/skills.md](docs/skills.md) for the full list of 51 MCP tools.

For first-time external users, follow the [10-minute CLI and MCP setup guide](docs/guides/external-user-10min.md). It covers host import, low-risk command verification, Codex/Claude-style MCP configuration, and sanitized feedback submission.

## Implemented Features

### Host Management

- Host profile CRUD (`host add / list / rm`)
- Import from `~/.ssh/config` (`host import-config`)
- Jump host / ProxyJump support
- Per-host risk override
- Host tags for grouping and bulk execution
- SSH key association

### Command Execution

- Non-interactive SSH exec with JSON output
- Direct-host exec uses the embedded SSH transport; jump-host execution still falls back to system `ssh`
- Configurable timeout, stdin piping, and output truncation
- Multi-host concurrent exec (`exec-multi`)
- Multi-host execution by tag
- Connectivity check (`ping`) with latency reporting
- SSH ControlMaster connection pooling

### Safety

- CLI, MCP, Tauri, and daemon mutation entry points share the same execution authorization layer for exec, playbooks, SFTP, PTY session open/write/close, forwards, and connection operations
- Every command is classified as `low / medium / high / blocked`; user policy rules can only escalate built-in risk
- High-risk commands require daemon/UI approval or `--force` / `force: true` when policy allows
- Blocked commands are always rejected
- Host and playbook `risk_override` can lower or raise non-blocked risk in trusted scopes, but cannot unblock `blocked`
- Unified policy-as-code in `~/.agent2ssh/policy.toml` / `policy.json`, with compatibility for legacy `risk_rules.toml` and `approval_policies.toml`
- Approval queue and daemon approval endpoints for high-risk commands
- Multi-host and playbook approvals are scoped to the approved host or step; one approval does not automatically force later targets or steps
- Execution audit log with risk level, initiating source, completed operations, and rejected attempts recorded
- Daemon-level execution gate, execution rate/session limits, and audit-window anomaly detection

### File Transfer (SFTP)

- Upload: `sftp put` / `ssh_sftp_upload`
- Download: `sftp get` / `ssh_sftp_download`
- Remote directory listing: `sftp ls` / `ssh_sftp_ls`
- Remote stat: `sftp stat` / `ssh_sftp_stat`
- Remote mkdir: `sftp mkdir` / `ssh_sftp_mkdir`
- Daemon SFTP operations pass through the same scope, risk, approval, gate, limit, and audit checks as command execution; directory helpers also record an operation-level audit entry in addition to the underlying shell command result

### Sessions And Tunnels

- Persistent PTY sessions backed by the embedded SSH terminal worker for direct hosts
- Session open/write/read/list/close
- Browser/WebSocket interactive terminal with remote PTY resize forwarding
- Connection diagnostics include authentication method, host-key algorithm, server banner, and SHA256 host-key fingerprint
- Local and remote port forwarding
- Forward list/remove by ID
- PTY session writes use line-buffered authorization for completed shell input and operation-level audit entries across daemon and desktop-local paths. This is a conservative guard for normal command input, not a full shell parser for arbitrary interactive programs.

### Desktop And Web UI

- Desktop host manager, exec panel, audit viewer, approval dialog, key manager, playbooks, tunnels, sessions, and connection status
- Desktop command and session risk previews include host-level risk overrides before prompting
- Desktop Settings menu centralizes language, setup, SSH config import, daemon Web Console URL, and execution gate controls
- Desktop Settings menu shows local daemon health, version, PID, and last health check time using the daemon `/health` endpoint
- Desktop Settings menu can start, stop, and restart the bundled local daemon sidecar
- Desktop setup wizard can start the bundled local daemon during first-run setup
- Desktop execution gate status is tri-state: active, paused, or unavailable when the local daemon cannot be reached; the menu shows the last status check time and supports manual refresh
- Live Agent Activity panel for local visibility into daemon session activity, WebSocket exec streams, recent audit records from CLI/MCP/daemon operations, and anomaly alerts
- Browser console served by the daemon at `/console`
- Daemon REST API, WebSocket streaming exec endpoint, WebSocket interactive terminal endpoint, and authenticated SSE event stream

### Automation

- Webhook notifications for approval, blocked execution, completed execution, and anomaly events
- Reusable command playbooks from `~/.agent2ssh/playbooks.toml`
- Remote daemon registry from `~/.agent2ssh/remotes.toml`
- MCP tools for local and remote operation
- Bounded event previews for session input/output and streaming exec output, suitable for local agent activity monitoring

## MCP Tools (51)

Agent2SSH exposes **51 MCP tools** covering host management, command execution, SFTP, persistent sessions, port forwarding, playbooks, audit, approval workflows, execution gate visibility, daemon management, and more.

For the complete tool reference with descriptions and parameters, see [docs/skills.md](docs/skills.md).

## API Reference

The daemon API contract lives in [docs/api.yaml](docs/api.yaml).

## Security

### Daemon Token

The daemon authenticates requests using a Bearer token stored at `~/.agent2ssh/daemon.token`. The token file is automatically restricted to owner-only permissions (`0600`) on Unix systems. If the daemon detects overly permissive permissions on startup, it will fix them and log a warning.

For least-privilege daemon clients, add scoped tokens in `~/.agent2ssh/daemon_tokens.toml`. The legacy `daemon.token` remains the unrestricted local admin token.

```toml
[[tokens]]
name = "prod-readonly"
token_env = "AGENT2SSH_PROD_READONLY_TOKEN"

[tokens.scope]
allowed_hosts = ["prod-web-1"]
allowed_tags = ["production"]
allowed_commands = ["uptime", "df *", "journalctl -n *"]
denied_commands = ["rm *", "sudo *"]
```

### Remote Daemon Usage

When connecting to remote `agent2ssh-daemon` instances, follow these guidelines:

- **Token storage**: Prefer storing the remote token via `token_env` (environment variable reference) in `remotes.toml` rather than writing the plaintext token directly. For example:
  ```toml
  [[remotes]]
  alias = "prod"
  url = "https://daemon.example.com:7722"
  token_env = "AGENT2SSH_PROD_TOKEN"

  [remotes.scope]
  allowed_tags = ["production"]
  allowed_commands = ["uptime", "df *"]
  denied_commands = ["rm *"]
  ```
  This avoids committing secrets to version-controlled configuration files.

- **HTTPS in production**: Remote daemon connections should always use HTTPS in production environments. The daemon itself listens on HTTP by default (`127.0.0.1:7722`); place a TLS-terminating reverse proxy (e.g., Caddy, nginx) in front of it for remote access.

- **Token rotation**: To rotate the daemon token, stop the daemon, run `agent2ssh daemon rotate-token`, and restart. Update all clients (local `remotes.toml` or environment variables) with the new token. All clients must be updated before they can resume communication.

- **Network security**: The daemon should be placed behind a firewall and never exposed directly to the public internet. Use VPN, SSH tunnels, or IP allowlisting to restrict access. The daemon provides full SSH execution capabilities to any authenticated caller.

- **Trust model**: When you connect to a remote `agent2ssh-daemon`, you are trusting the remote machine's OS-level security. Anyone with root or admin access on the remote host can read the daemon token, intercept SSH keys, and observe or modify command execution. Treat remote daemon hosts with the same level of trust you would give to any machine holding your SSH credentials.

### SSH Key Permissions

All private keys managed by Agent2SSH (generated or imported) are automatically restricted to `0600` permissions on Unix systems. The SSH key directory is located at `~/.agent2ssh/keys/`.

### Risk Overrides

User risk rules from `policy.toml` or legacy `risk_rules.toml` can only raise a command's built-in risk. Host and playbook `risk_override` settings are the trusted downgrade/upgrade mechanism for non-blocked commands, but they cannot downgrade commands classified as `blocked`. Built-in or user-defined blocked rules always reject execution.

### Sensitive Command Redaction

Audit entries and webhook payloads redact common secret-bearing command arguments such as `--token`, `--password`, `--secret`, `--api-key`, and `key=value` variants before persistence or outbound delivery.

### Webhook Outbound

Webhook notifications are **non-blocking fire-and-forget**: failures (network errors, unreachable endpoints, DNS resolution failures) are logged to stderr but never propagate to the caller or block the main execution flow.

- **Timeout**: Outbound webhook requests use a 10-second HTTP client timeout to prevent slow or unresponsive endpoints from blocking the daemon. This timeout is hardcoded in the client and applies per-request.
- **HMAC-SHA256 signing**: Set the `secret` field in `~/.agent2ssh/webhook.toml` to enable payload signing. When configured, each outbound request includes an `X-Agent2SSH-Signature` header with the value `sha256=<hex>`, computed as HMAC-SHA256 over the raw JSON body using the configured secret. Receiving endpoints should verify this signature to authenticate the payload origin.
- **Event filtering**: Only events listed in the `events` array of `webhook.toml` trigger outbound HTTP calls. The default subscribed event is `approval_required`.

## Roadmap

The current codebase has completed the original MVP through packaging, tags, keys, playbooks, webhooks, remote daemons, daemon API, web console, and approval gates. The next work focuses on cross-platform validation, external dogfood, ecosystem reliability, and team scenarios when real multi-user demand appears. See [docs/plan.md](docs/plan.md).
