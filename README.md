# Agent2SSH

**A local SSH capability layer for general-purpose agents — one Rust core, five surfaces.**
**面向通用 Agent 的本地 SSH 能力层 —— 一个 Rust 核心，五种入口。**

[![CI](https://github.com/lengyuqu/agent2ssh/actions/workflows/ci.yml/badge.svg)](https://github.com/lengyuqu/agent2ssh/actions/workflows/ci.yml) [![Release](https://img.shields.io/github/v/release/lengyuqu/agent2ssh?sort=semver&label=release)](https://github.com/lengyuqu/agent2ssh/releases) [![MCP tools](https://img.shields.io/badge/MCP_tools-76-2DD4BF)](docs/skills.md) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**English** · [简体中文](#简体中文)

---

## English

**Contents:** [Overview](#overview) · [Use cases](#use-cases) · [Installation](#installation) · [Development](#development) · [Daemon](#daemon) · [MCP](#mcp-integration) · [Features](#implemented-features) · [Security](#security) · [Roadmap](#roadmap)

### Overview

Agent2SSH is a local SSH capability layer for general-purpose agents. It exposes one Rust core through:

- a Tauri desktop app (`agent2ssh-app`)
- an `agent2ssh` CLI
- an `agent2ssh-daemon` local HTTP/WebSocket server
- an `agent2ssh-mcp` stdio MCP server
- a Codex/agent skill prompt in `skills/agent2ssh/SKILL.md`

Host management, command execution, SFTP, interactive terminals, port forwarding, approval and audit all live in that one core, so every surface enforces the same rules.

### Use cases

- Give Codex, Claude Desktop or any MCP-capable agent local SSH tool capabilities
- Manage SSH hosts, keys, proxies, jump hosts and remote daemons in one place on your own machine
- Put high-risk commands, SFTP, terminal sessions and port forwards behind one approval, rate-limit, pause and audit layer
- Watch agent activity, approval requests, audit records, anomaly alerts and resident sessions from the desktop

### Installation

#### Homebrew (macOS)

```bash
brew tap lengyuqu/agent2ssh
brew install agent2ssh
```

Supported platforms are macOS on Apple Silicon, Linux x86_64, and Windows x86_64.
Intel Macs (`x86_64-apple-darwin`) are no longer supported: the next release
stops publishing binaries and bundles for that target, and the Homebrew formula
now fails with an explicit message instead of downloading a missing archive.
Building from source still works, but that architecture is no longer tested.

#### Pre-built binaries

Download from [GitHub Releases](https://github.com/lengyuqu/agent2ssh/releases).

#### From source

```bash
git clone https://github.com/lengyuqu/agent2ssh.git
cd agent2ssh
npm install && npm run build
cd src-tauri
cargo build --release --no-default-features --bin agent2ssh --bin agent2ssh-mcp
cargo build --release --no-default-features --features daemon --bin agent2ssh-daemon
```

### Development

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

### Daemon

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

`daemon stop` names the process it means to stop (`expect_pid` on `POST /shutdown`) and the daemon re-checks it, so a listener that was replaced by another daemon between the health check and the stop refuses instead of shutting down. If you see `the daemon listening now is pid B, not the pid A the stop was for`, the stop found a different daemon: run it again to address pid B.

### Data

Local data is stored under `~/.agent2ssh/`. See [docs/guides/configuration-guide.md](docs/guides/configuration-guide.md) for the full file layout, configuration format, and storage details.

### Documentation

Start with the [help overview](docs/guides/help.md) for desktop areas, common commands, and safety notes. First-time users can also follow the [CLI quickstart](docs/guides/cli-quickstart.md), [MCP quickstart](docs/guides/mcp-quickstart.md), or [10-minute CLI and MCP setup guide](docs/guides/external-user-10min.md). The [web console guide](docs/guides/web-console-guide.md) and [daemon API quickstart](docs/guides/daemon-api-quickstart.md) cover the browser and HTTP surfaces; [docs/README.md](docs/README.md) indexes everything.

### MCP Integration

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
`qoder`, `qoder_cn`, `trae`, `trae_cn`, `codex`, or `claude_desktop`.

See [docs/skills.md](docs/skills.md) for the full list of 76 MCP tools. `tools/list` offers the 26 core tools by default to keep the model's context small; set `AGENT2SSH_MCP_TOOLS=all` to list them all.

Ready-made client configs live in [docs/guides/mcp-client-templates.md](docs/guides/mcp-client-templates.md). For first-time external users, follow the [10-minute CLI and MCP setup guide](docs/guides/external-user-10min.md). It covers host import, low-risk command verification, Codex/Claude-style MCP configuration, and sanitized feedback submission.

### Implemented Features

#### Host Management

- Host profile CRUD (`host add / list / rm`)
- Import from `~/.ssh/config` (`host import-config`)
- Jump host / ProxyJump support
- HTTP CONNECT and SOCKS5 proxy profiles for SSH connections, managed from the desktop Proxies page or `hosts.json`
- Per-host risk override
- Host tags for grouping and bulk execution
- SSH key association

#### Command Execution

- Non-interactive SSH exec with JSON output
- Embedded SSH transport for exec, SFTP, terminal, sessions, jump-host proxying, and port forwards
- Configurable timeout, stdin piping, and output truncation
- Multi-host concurrent exec (`exec-multi`)
- Multi-host execution by tag
- Connectivity check (`ping`) with latency reporting
- Retained embedded SSH connections for connection status and preconnect workflows
- Local execution on the daemon's machine in a named environment (`local`, `local:pwsh`, `local:cmd`, `local:gitbash`, `local:vs`, `local:wsl[:<distro>]`, user-defined `local:<id>`, or `local:<runtime>:<container-id>` for capture exec inside a container), through the same authorization chain as an SSH exec
- Resident shell sessions that keep one `local:<id>` environment alive across commands, so cwd and exported variables persist; a command that outlives its timeout kills the session

#### SSH Backend Boundary

- SSH transport is implemented in the Rust/Tauri backend with `russh` (crypto backend `ring`) — pure Rust, no libssh2 and no OpenSSL.
- Exec, ping, health probes, SFTP, WebSocket exec streaming, WebSocket terminal, persistent PTY sessions, HTTP/SOCKS5 proxy dialing, jump-host / ProxyJump-style bastion channels, retained connections, and local/remote port forwards do not call system `ssh`, `scp`, or `sshpass`.
- Local SSH key generation is also implemented in Rust and uses the operating system CSPRNG directly; it does not call system `ssh-keygen`.
- Daemon process status/stop and health checks use Rust process/HTTP APIs, not shelling out to `kill`, `taskkill`, `tasklist`, or `curl`.
- SSH config import/export reads and writes local config text, but connection execution remains embedded.

#### Safety

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
- SSH host fingerprints are trusted automatically on first use in `~/.agent2ssh/known_hosts.json`; later fingerprint changes are rejected

#### File Transfer (SFTP)

- Upload: `sftp put` / `ssh_sftp_upload`
- Download: `sftp get` / `ssh_sftp_download`
- Remote directory listing: `sftp ls` / `ssh_sftp_ls`
- Remote stat: `sftp stat` / `ssh_sftp_stat`
- Remote mkdir: `sftp mkdir` / `ssh_sftp_mkdir`
- Remote rename / move: `sftp rename` / `ssh_sftp_rename`
- Remote delete file: `sftp rm` / `ssh_sftp_rm`
- Remote remove empty directory: `sftp rmdir` / `ssh_sftp_rmdir`
- Remote recursive delete: `sftp rm-rf` / `ssh_sftp_rm_rf` — rated high risk, and blocked outright for `/`, `/*` and `/.`
- Desktop Files page provides a two-pane SFTP file manager with path shortcuts, folder navigation, file selection, directory creation, copy-left/copy-right transfer actions, and a right-click menu for rename/delete on the remote side
- Daemon SFTP operations pass through the same scope, risk, approval, gate, limit, and audit checks as command execution; directory helpers also record an operation-level audit entry in addition to the underlying shell command result

#### Sessions And Tunnels

- Persistent PTY sessions backed by the embedded SSH terminal worker for direct hosts
- Session open/write/read/list/close
- Browser/WebSocket interactive terminal with remote PTY resize forwarding
- Marker-backed command blocks with color rails, search, navigation, safe plain-text copy, and structured block metadata
- Opt-in local asciicast v2 terminal recording with variable-speed desktop playback; recordings are treated as sensitive and excluded from sync
- Connection diagnostics include authentication method, host-key algorithm, SHA256 host-key fingerprint, and jump-host alias when present; the server banner is deliberately not among them
- Local and remote port forwarding with desktop tunnel creation, active tunnel list, host counts, and removal controls
- Forward list/remove by ID
- PTY session writes use line-buffered authorization for completed shell input and operation-level audit entries across daemon and desktop-local paths. This is a conservative guard for normal command input, not a full shell parser for arbitrary interactive programs.

#### Desktop And Web UI

- Desktop host manager, exec panel, audit viewer, approval dialog, key manager, playbooks, tunnels, sessions, and connection status
- Desktop Terminal holds the host tree, session tabs, split panes, broadcast, command blocks, Ctrl+R history, snippets, terminal themes and the focused host's SFTP drawer in one surface
- Desktop Resident Sessions panel listing the daemon's resident shells with a close action, and a Containers panel for container exec targets
- Desktop Proxies page for HTTP CONNECT and SOCKS5 proxy profiles, including optional username/password authentication
- Desktop Terminal supports selectable terminal themes including app-matched, GitHub Dark, Tokyo Night, Dracula, Nord, Solarized Light, High Contrast, and Amber
- Desktop Help page links bundled guides and shows first-run checklist, useful commands, and safety notes
- Desktop command and session risk previews include host-level risk overrides before prompting
- Desktop Settings menu centralizes language, setup, SSH config import, daemon Web Console URL, and execution gate controls
- Desktop Settings menu shows local daemon health, version, PID, and last health check time using the daemon `/health` endpoint
- Desktop Settings menu can start, stop, and restart the bundled local daemon sidecar
- Desktop setup wizard can start the bundled local daemon during first-run setup
- Desktop execution gate status is tri-state: active, paused, or unavailable when the local daemon cannot be reached; the menu shows the last status check time and supports manual refresh
- Windows release builds run as GUI apps without opening an extra console window
- Live Agent Activity panel for local visibility into daemon session activity, WebSocket exec streams, recent audit records from CLI/MCP/daemon operations, and anomaly alerts
- Browser console served by the daemon at `/console`
- Daemon REST API, WebSocket streaming exec endpoint, WebSocket interactive terminal endpoint, and authenticated SSE event stream

#### Automation

- Webhook notifications for approval, blocked execution, completed execution, and anomaly events
- Reusable command playbooks from `~/.agent2ssh/playbooks.toml`
- Remote daemon registry from `~/.agent2ssh/remotes.toml`
- MCP tools for local and remote operation
- Bounded event previews for session input/output and streaming exec output, suitable for local agent activity monitoring

### MCP Tools

Agent2SSH exposes **76 MCP tools** covering host management, command snippets, command execution, SFTP, persistent sessions, local execution (one-shot `local:<environment>` commands and long-lived resident shell sessions), port forwarding, playbooks, audit, approval workflows, execution gate visibility, daemon management, secret redaction rules, terminal highlight rules, and SSH algorithm preferences.

For the complete tool reference with descriptions and parameters, see [docs/skills.md](docs/skills.md).

### API Reference

The daemon API contract lives in [docs/api.yaml](docs/api.yaml).

### Security

#### Daemon Token

The daemon authenticates requests using a Bearer token stored at `~/.agent2ssh/daemon.token`. The token file is automatically restricted to owner-only permissions (`0600`) on Unix systems. If the daemon detects overly permissive permissions on startup, it will fix them and log a warning.

For least-privilege daemon clients, add scoped tokens in `~/.agent2ssh/daemon_tokens.toml`. The legacy `daemon.token` remains the unrestricted local admin token.

Scoped tokens are limited to host-bound ping, execution, SFTP, session, forwarding,
connection, and playbook authorization. Host lists, audit entries, health
snapshots, connections, sessions, forwards, and approvals are filtered by the
token's allowed hosts/tags. Global or sensitive surfaces such as audit export,
metrics trends, event streams, pending OTP prompts, SSH sync diff, and local
configuration mutations require the unrestricted admin token.

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

#### Remote Daemon Usage

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

#### SSH Key Permissions

All private keys managed by Agent2SSH (generated or imported) are automatically restricted to `0600` permissions on Unix systems. The SSH key directory is located at `~/.agent2ssh/keys/`.

#### Risk Overrides

User risk rules from `policy.toml` or legacy `risk_rules.toml` can only raise a command's built-in risk. Host and playbook `risk_override` settings are the trusted downgrade/upgrade mechanism for non-blocked commands, but they cannot downgrade commands classified as `blocked`. Built-in or user-defined blocked rules always reject execution.

#### Sensitive Command Redaction

Audit entries and webhook payloads redact common secret-bearing command arguments such as `--token`, `--password`, `--secret`, `--api-key`, and `key=value` variants before persistence or outbound delivery.

#### Webhook Outbound

Webhook notifications are **non-blocking fire-and-forget**: failures (network errors, unreachable endpoints, DNS resolution failures) are logged to stderr but never propagate to the caller or block the main execution flow.

- **Timeout**: Outbound webhook requests use a 10-second HTTP client timeout to prevent slow or unresponsive endpoints from blocking the daemon. This timeout is hardcoded in the client and applies per-request.
- **HMAC-SHA256 signing**: Set the `secret` field in `~/.agent2ssh/webhook.toml` to enable payload signing. When configured, each outbound request includes an `X-Agent2SSH-Signature` header with the value `sha256=<hex>`, computed as HMAC-SHA256 over the raw JSON body using the configured secret. Receiving endpoints should verify this signature to authenticate the payload origin.
- **Event filtering**: Only events listed in the `events` array of `webhook.toml` trigger outbound HTTP calls. The default subscribed event is `approval_required`.

### Roadmap

The current codebase has completed the original MVP through packaging, tags, keys, playbooks, webhooks, remote daemons, daemon API, web console, and approval gates. The next work focuses on cross-platform validation, external dogfood, ecosystem reliability, and team scenarios when real multi-user demand appears. See [docs/PLAN.md](docs/PLAN.md).

### License

Released under the [MIT License](LICENSE).

Copyright (c) 2026 lengyuqu

---

## 简体中文

**目录：** [概览](#概览) · [适用场景](#适用场景) · [安装](#安装) · [开发与构建](#开发与构建) · [Daemon 服务](#daemon-服务) · [MCP 集成](#mcp-集成) · [已实现功能](#已实现功能) · [安全](#安全) · [路线图](#路线图)

### 概览

Agent2SSH 是一个面向通用 Agent 的本地 SSH 能力层。它把同一个 Rust 核心通过五种入口暴露出来：

- Tauri 桌面应用（`agent2ssh-app`）
- `agent2ssh` 命令行工具
- `agent2ssh-daemon` 本地 HTTP/WebSocket 服务
- `agent2ssh-mcp` 基于 stdio 的 MCP server
- `skills/agent2ssh/SKILL.md` 中的 Codex/Agent Skill 提示词

SSH 主机管理、命令执行、SFTP、交互式终端、端口转发、审批和审计都统一在这一个核心里，因此每个入口执行的都是同一套规则。

### 适用场景

- 为 Codex、Claude Desktop 或其他支持 MCP 的 Agent 提供本地 SSH 工具能力
- 在本机集中管理 SSH 主机、密钥、代理、跳板机和远端 daemon
- 对高风险命令、SFTP、终端会话和端口转发做统一审批、限流、暂停和审计
- 在桌面端观察 Agent 活动、审批请求、审计记录、异常告警和驻留会话

### 安装

#### Homebrew 安装（macOS）

```bash
brew tap lengyuqu/agent2ssh
brew install agent2ssh
```

支持的平台为 macOS（Apple Silicon）、Linux x86_64 和 Windows x86_64。
**Intel Mac（`x86_64-apple-darwin`）已不再支持**：从下一版开始不再发布该平台的二进制与安装包，Homebrew 在 Intel Mac 上会直接给出明确报错，而不是去下载一个并不存在的压缩包。仍需要的话可以从源码构建，但官方不再测试该架构。

#### 预编译二进制

从 [GitHub Releases](https://github.com/lengyuqu/agent2ssh/releases) 下载二进制或桌面安装包。

#### 源码构建

```bash
git clone https://github.com/lengyuqu/agent2ssh.git
cd agent2ssh
npm install && npm run build
cd src-tauri
cargo build --release --no-default-features --bin agent2ssh --bin agent2ssh-mcp
cargo build --release --no-default-features --features daemon --bin agent2ssh-daemon
```

### 开发与构建

```bash
npm install
npm run build
cd src-tauri
cargo check --no-default-features --bin agent2ssh --bin agent2ssh-mcp
cargo check --no-default-features --features daemon --bin agent2ssh-daemon
```

运行桌面应用：

```bash
npm run tauri:dev
```

运行 CLI：

```bash
cd src-tauri
cargo run --no-default-features --bin agent2ssh -- host add prod --host 10.0.0.12 --user ubuntu --key ~/.ssh/id_ed25519
cargo run --no-default-features --bin agent2ssh -- host list --json
cargo run --no-default-features --bin agent2ssh -- exec prod "uname -a" --json
```

运行 MCP server：

```bash
cd src-tauri
cargo run --no-default-features --bin agent2ssh-mcp
```

运行 HTTP daemon：

```bash
cd src-tauri
cargo run --no-default-features --features daemon --bin agent2ssh-daemon
```

### Daemon 服务

daemon 在 `127.0.0.1:7722` 上提供本地 HTTP API 和浏览器控制台。

```bash
# 启动 HTTP daemon
agent2ssh daemon start

# 查看状态
agent2ssh daemon status

# 停止 daemon
agent2ssh daemon stop

# 打开 Web 控制台
open http://127.0.0.1:7722/console
```

需要认证的请求使用保存在 `~/.agent2ssh/daemon.token` 中的 Bearer token。

`daemon stop` 会指明它要停止的进程（`POST /shutdown` 上的 `expect_pid`），daemon 会再核对一次：如果在健康检查和停止之间监听者已被另一个 daemon 替换，它会拒绝关闭而不是误停。如果看到 `the daemon listening now is pid B, not the pid A the stop was for`，说明这次停止找到的是另一个 daemon：再执行一次即可指向 pid B。

### 数据位置

本地数据默认保存在 `~/.agent2ssh/`。完整文件布局、配置格式与存储细节见 [docs/guides/configuration-guide.md](docs/guides/configuration-guide.md)。

### 文档

建议先读[帮助总览](docs/guides/help.md)，了解桌面端功能区、常用命令与安全说明。首次使用可以跟随 [CLI 快速入门](docs/guides/cli-quickstart.md)、[MCP 快速入门](docs/guides/mcp-quickstart.md)或[10 分钟 CLI 与 MCP 接入指南](docs/guides/external-user-10min.md)。浏览器与 HTTP 接口分别见 [Web 控制台指南](docs/guides/web-console-guide.md)和 [Daemon API 快速入门](docs/guides/daemon-api-quickstart.md)；全部文档索引在 [docs/README.md](docs/README.md)。

### MCP 集成

在支持 MCP 的客户端配置中，把 Agent2SSH 配置为一个 MCP server：

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

建议设置 `AGENT2SSH_SOURCE` 为你希望在「活动」面板中看到的客户端标签，例如 `qoder`、`qoder_cn`、`trae`、`trae_cn`、`codex` 或 `claude_desktop`，便于在桌面 Live Activity 和审计日志中区分调用来源。

完整的 76 个 MCP 工具列表见 [docs/skills.md](docs/skills.md)。为控制模型上下文体积，`tools/list` 默认只列出 26 个核心工具；设置 `AGENT2SSH_MCP_TOOLS=all` 可列出全部工具（未列出的工具始终可按名调用）。

现成的客户端配置模板见 [docs/guides/mcp-client-templates.md](docs/guides/mcp-client-templates.md)。首次接入的外部用户建议跟随[10 分钟 CLI 与 MCP 接入指南](docs/guides/external-user-10min.md)，其中覆盖主机导入、低风险命令验证、Codex/Claude 风格的 MCP 配置，以及脱敏反馈提交。

### 已实现功能

#### 主机管理

- 主机配置增删改查（`host add / list / rm`）
- 从 `~/.ssh/config` 导入（`host import-config`）
- 跳板机 / ProxyJump 支持
- 用于 SSH 连接的 HTTP CONNECT 与 SOCKS5 代理配置，可在桌面「代理」页或 `hosts.json` 中管理
- 按主机的风险覆盖
- 主机标签，用于分组与批量执行
- SSH 密钥关联

#### 命令执行

- 非交互式 SSH exec，支持 JSON 输出
- exec、SFTP、终端、会话、跳板机代理和端口转发统一使用内嵌 SSH 传输
- 可配置超时、stdin 输入与输出截断
- 多主机并发执行（`exec-multi`）
- 按标签的多主机执行
- 连通性检查（`ping`）并报告延迟
- 保留内嵌 SSH 连接，用于连接状态查看与预连接流程
- 在 daemon 所在机器的命名环境中本地执行（`local`、`local:pwsh`、`local:cmd`、`local:gitbash`、`local:vs`、`local:wsl[:<发行版>]`、用户自定义的 `local:<id>`，或用于容器内捕获式执行的 `local:<runtime>:<container-id>`），走与 SSH exec 完全相同的授权链
- 驻留 shell 会话：让一个 `local:<id>` 环境跨命令保持存活，工作目录与导出变量得以保留；超出超时的命令会终止整个会话

#### SSH 后端边界

- SSH 传输在 Rust/Tauri 后端用 `russh` 实现（加密后端为 `ring`）—— 纯 Rust，不依赖 libssh2，也不依赖 OpenSSL。
- exec、ping、健康探测、SFTP、WebSocket exec 流、WebSocket 终端、持久 PTY 会话、HTTP/SOCKS5 代理拨号、跳板机 / ProxyJump 式堡垒通道、保留连接以及本地/远程端口转发，都不调用系统 `ssh`、`scp` 或 `sshpass`。
- 本地 SSH 密钥生成同样由 Rust 实现，直接使用操作系统 CSPRNG，不调用系统 `ssh-keygen`。
- daemon 进程状态/停止与健康检查使用 Rust 的进程/HTTP API，不外部调用 `kill`、`taskkill`、`tasklist` 或 `curl`。
- SSH 配置导入/导出会读写本地配置文本，但连接执行仍走内嵌实现。

#### 安全机制

- CLI、MCP、Tauri 和 daemon 的变更类入口共享同一执行授权层，覆盖 exec、剧本、SFTP、PTY 会话打开/写入/关闭、转发和连接操作
- 每条命令都会被分级为 `low / medium / high / blocked`；用户策略规则只能上调内置风险
- 高风险命令需要 daemon/UI 审批，或在策略允许时使用 `--force` / `force: true`
- 被阻止（`blocked`）的命令始终拒绝执行
- 主机与剧本的 `risk_override` 可在可信范围内下调或上调非 blocked 风险，但不能解除 `blocked`
- 统一的策略即代码：`~/.agent2ssh/policy.toml` / `policy.json`，并兼容旧的 `risk_rules.toml` 与 `approval_policies.toml`
- 高风险命令的审批队列与 daemon 审批端点
- 多主机与剧本审批按已批准的主机或步骤限定范围；一次审批不会自动放行后续目标或步骤
- 执行审计日志记录风险等级、发起来源、已完成操作与被拒绝的尝试
- daemon 级执行闸门、执行速率/会话数限制，以及审计窗口异常检测
- SSH 主机指纹在首次使用时自动信任并写入 `~/.agent2ssh/known_hosts.json`；之后指纹变化会被拒绝

#### 文件传输（SFTP）

- 上传：`sftp put` / `ssh_sftp_upload`
- 下载：`sftp get` / `ssh_sftp_download`
- 远端目录列举：`sftp ls` / `ssh_sftp_ls`
- 远端 stat：`sftp stat` / `ssh_sftp_stat`
- 远端新建目录：`sftp mkdir` / `ssh_sftp_mkdir`
- 远端重命名 / 移动：`sftp rename` / `ssh_sftp_rename`
- 远端删除文件：`sftp rm` / `ssh_sftp_rm`
- 远端删除空目录：`sftp rmdir` / `ssh_sftp_rmdir`
- 远端递归删除：`sftp rm-rf` / `ssh_sftp_rm_rf` —— 判定为高风险，且对 `/`、`/*` 和 `/.` 直接阻止
- 桌面「文件」页提供双栏 SFTP 文件管理器：路径快捷跳转、目录导航、文件选择、新建目录、向左/向右复制传输，以及远端侧的重命名/删除右键菜单
- daemon 的 SFTP 操作经过与命令执行相同的范围、风险、审批、闸门、限流与审计检查；目录类辅助操作除底层 shell 命令结果外，还会额外记录一条操作级审计

#### 会话与隧道

- 直连主机的持久 PTY 会话，由内嵌 SSH 终端 worker 承载
- 会话打开/写入/读取/列举/关闭
- 浏览器/WebSocket 交互式终端，支持远端 PTY resize 转发
- 基于标记的命令块，带颜色标尺、搜索、跳转、安全的纯文本复制与结构化块元数据
- 可选开启的本地 asciicast v2 终端录制，桌面端支持变速回放；录制被视为敏感数据，不参与同步
- 连接诊断包含认证方式、主机密钥算法、SHA256 主机密钥指纹，以及存在时的跳板机别名；服务端 banner 有意不在其中
- 本地与远程端口转发，桌面端可创建隧道、查看活动隧道列表与主机计数并移除
- 按 ID 列举/移除转发
- PTY 会话写入对已完成的 shell 输入采用行缓冲授权，并在 daemon 与桌面本地两条路径上记录操作级审计。这是面向常规命令输入的保守防护，不是针对任意交互式程序的完整 shell 解析器。

#### 桌面与 Web UI

- 桌面主机管理、执行面板、审计查看器、审批对话框、密钥管理、剧本、隧道、会话与连接状态
- 桌面「终端」把主机树、会话标签、分屏、广播、命令块、Ctrl+R 历史、命令片段、终端主题，以及当前聚焦主机的 SFTP 抽屉集中在同一个界面
- 桌面「驻留会话」面板列出 daemon 的驻留 shell 并可关闭；「容器」面板列出容器执行目标
- 桌面「代理」页管理 HTTP CONNECT 与 SOCKS5 代理配置，支持可选的用户名/密码认证
- 桌面终端支持可选主题：跟随应用、GitHub Dark、Tokyo Night、Dracula、Nord、Solarized Light、高对比度与琥珀
- 桌面「帮助」页链接内置指南，并展示首次运行清单、常用命令与安全说明
- 桌面命令与会话的风险预览会在提示前计入主机级风险覆盖
- 桌面「设置」菜单集中管理语言、初始化设置、SSH 配置导入、daemon Web 控制台地址与执行闸门控制
- 桌面「设置」菜单通过 daemon `/health` 端点展示本地 daemon 健康状态、版本、PID 与最近一次健康检查时间
- 桌面「设置」菜单可启动、停止和重启随应用分发的本地 daemon sidecar
- 桌面初始化向导可在首次运行时启动本地 daemon
- 桌面执行闸门状态为三态：已启用、已暂停，或本地 daemon 不可达时的不可用；菜单展示最近一次状态检查时间并支持手动刷新
- Windows 发布版以 GUI 应用运行，不会额外弹出控制台窗口
- Live Agent Activity 面板在本机呈现 daemon 会话活动、WebSocket exec 流、来自 CLI/MCP/daemon 操作的近期审计记录与异常告警
- daemon 在 `/console` 提供浏览器控制台
- daemon REST API、WebSocket 流式 exec 端点、WebSocket 交互式终端端点，以及需认证的 SSE 事件流

#### 自动化

- 审批、被阻止执行、执行完成与异常事件的 webhook 通知
- 来自 `~/.agent2ssh/playbooks.toml` 的可复用命令剧本
- 来自 `~/.agent2ssh/remotes.toml` 的远端 daemon 注册表
- 覆盖本地与远端操作的 MCP 工具
- 会话输入/输出与流式 exec 输出的有界事件预览，适合本地 Agent 活动监控

### MCP 工具

Agent2SSH 暴露 **76 个 MCP 工具**，覆盖主机管理、命令片段、命令执行、SFTP、持久会话、本地执行（一次性 `local:<environment>` 命令与长驻 shell 会话）、端口转发、剧本、审计、审批流程、执行闸门可见性、daemon 管理、敏感信息脱敏规则、终端高亮规则与 SSH 算法偏好。

带描述与参数的完整工具参考见 [docs/skills.md](docs/skills.md)。

### API 参考

daemon API 契约见 [docs/api.yaml](docs/api.yaml)。

### 安全

#### Daemon Token 认证

daemon 使用保存在 `~/.agent2ssh/daemon.token` 的 Bearer token 认证请求。在 Unix 系统上，token 文件会自动限制为仅属主可读写（`0600`）。如果 daemon 启动时发现权限过宽，会自动修正并记录告警。

需要最小权限的 daemon 客户端可在 `~/.agent2ssh/daemon_tokens.toml` 中添加受限 token。旧的 `daemon.token` 仍是无限制的本地管理员 token。

受限 token 只能用于绑定主机的 ping、执行、SFTP、会话、转发、连接与剧本授权。主机列表、审计记录、健康快照、连接、会话、转发与审批都会按该 token 允许的主机/标签过滤。全局或敏感接口——如审计导出、指标趋势、事件流、待处理 OTP 提示、SSH 同步 diff 以及本地配置变更——仍需无限制的管理员 token。

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

#### 远端 Daemon 使用建议

连接远端 `agent2ssh-daemon` 实例时，请遵循以下建议：

- **Token 存储**：优先在 `remotes.toml` 中用 `token_env`（环境变量引用）保存远端 token，而不是直接写明文。例如：
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
  这样可以避免把密钥提交进受版本控制的配置文件。

- **生产环境使用 HTTPS**：生产环境的远端 daemon 连接应始终使用 HTTPS。daemon 自身默认监听 HTTP（`127.0.0.1:7722`）；远程访问请在其前面放置终止 TLS 的反向代理（如 Caddy、nginx）。

- **Token 轮换**：轮换 daemon token 的步骤是先停止 daemon，执行 `agent2ssh daemon rotate-token`，再重启。随后更新所有客户端（本地 `remotes.toml` 或环境变量）中的 token；所有客户端更新完成后才能恢复通信。

- **网络安全**：daemon 应置于防火墙之后，切勿直接暴露到公网。使用 VPN、SSH 隧道或 IP 白名单限制访问。daemon 会向任何通过认证的调用方提供完整的 SSH 执行能力。

- **信任模型**：连接远端 `agent2ssh-daemon` 意味着你信任那台机器操作系统层面的安全性。任何在远端拥有 root 或管理员权限的人都能读取 daemon token、截获 SSH 密钥，并观察或篡改命令执行。请以对待任何持有你 SSH 凭据的机器同等的信任级别来对待远端 daemon 主机。

#### SSH 密钥权限

Agent2SSH 管理的所有私钥（生成或导入）在 Unix 系统上都会自动限制为 `0600` 权限。SSH 密钥目录位于 `~/.agent2ssh/keys/`。

#### 风险覆盖

来自 `policy.toml` 或旧 `risk_rules.toml` 的用户风险规则只能上调命令的内置风险。主机与剧本的 `risk_override` 是可信范围内下调/上调非 blocked 命令风险的机制，但不能下调被判定为 `blocked` 的命令。内置或用户定义的 blocked 规则始终拒绝执行。

#### 敏感命令脱敏

审计记录与 webhook 载荷在落盘或外发之前，会对常见的携带密钥的命令参数脱敏，例如 `--token`、`--password`、`--secret`、`--api-key` 以及 `key=value` 形式。

#### Webhook 外发

webhook 通知是**非阻塞的 fire-and-forget**：失败（网络错误、端点不可达、DNS 解析失败）会记录到 stderr，但绝不会传播给调用方或阻塞主执行流程。

- **超时**：外发 webhook 请求使用 10 秒 HTTP 客户端超时，避免缓慢或无响应的端点阻塞 daemon。该超时硬编码在客户端中，按请求生效。
- **HMAC-SHA256 签名**：在 `~/.agent2ssh/webhook.toml` 中设置 `secret` 字段即启用载荷签名。启用后每个外发请求都会带上 `X-Agent2SSH-Signature` 头，值为 `sha256=<hex>`，由配置的 secret 对原始 JSON body 做 HMAC-SHA256 得到。接收端应校验该签名以确认载荷来源。
- **事件过滤**：只有列在 `webhook.toml` 的 `events` 数组中的事件才会触发外发 HTTP 调用。默认订阅的事件是 `approval_required`。

### 路线图

当前代码库已完成最初的 MVP，并延伸到打包、标签、密钥、剧本、webhook、远端 daemon、daemon API、Web 控制台与审批闸门。接下来的工作聚焦跨平台验证、外部试用、生态可靠性，以及在出现真实多用户需求时的团队场景。详见 [docs/PLAN.md](docs/PLAN.md)。

### 许可

本项目以 [MIT 协议](LICENSE)开源。

Copyright (c) 2026 lengyuqu

---

[↑ Back to top / 回到顶部](#agent2ssh)