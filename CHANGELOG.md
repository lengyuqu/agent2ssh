# Changelog

All notable changes to Agent2SSH are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [0.1.0] - 2025-06-12

### Added
- **Interfaces**: Tauri desktop app, CLI, MCP stdio server (31 tools), HTTP/WebSocket daemon, Web Console
- **Host Management**: CRUD, SSH config import, ProxyJump/bastion, tags, per-host risk override, SSH key association
- **Command Execution**: Single-host exec, multi-host exec (by name or tag), ping, ControlMaster connection pooling
- **Safety**: 4-tier risk classification (low/medium/high/blocked), configurable risk rules, approval queue with TTL, audit log, desktop approval dialog
- **File Transfer**: SFTP upload/download/ls/stat/mkdir
- **Sessions & Tunnels**: Interactive PTY sessions, local/remote port forwarding
- **Automation**: Webhook notifications (HMAC-SHA256 signing, Slack Block Kit), Playbooks (command sequences), Remote daemon support
- **SSH Keys**: Ed25519 generation, import, delete, key dropdown in host form
- **Security**: Daemon token 0600 on Unix, SSH key permission enforcement, WebSocket exec stream auth, webhook outbound protection
- **CI/CD**: 4-platform build matrix, Tauri bundle job, Homebrew formula
- **Testing**: 31 unit tests + 24+ integration tests + CLI smoke tests

### Security
- Daemon token file restricted to 0600 on Unix systems
- SSH private key permissions enforced to 0600 after generation/import
- WebSocket exec stream requires Bearer token authentication
- Webhook outbound uses non-blocking fire with configurable timeout and HMAC-SHA256 signing
- Approval requests have configurable TTL (default 300s), expired requests auto-marked as timed_out
