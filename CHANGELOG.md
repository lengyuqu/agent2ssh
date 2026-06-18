# Changelog

All notable changes to Agent2SSH are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
- **Control-plane safety layer**: Added daemon-level execution gate controls, execution rate/session limits, unified policy-as-code validation, and audit-window anomaly detection.
- **Execution gate**: Added `agent2ssh pause/resume/status`, daemon 423 rejection for paused non-desktop sources, gate audit entries, and `gate_rejected` / `gate_changed` events.
- **Execution limits**: Added `execution_limits.toml` with per-source, per-host, and per-tag rate/session limits, plus 429 rejection auditing and `limit_rejected` events.
- **Unified policy files**: Added `policy.toml` / `policy.json` support for colocating risk rules and approval policies, with `agent2ssh policy validate` and `agent2ssh policy test`.
- **Anomaly detection**: Added `anomaly.toml`, source burst detection, sensitive command pattern detection, after-hours high-risk detection, `anomaly_detected` events, webhook support, and Live Activity anomaly highlighting.
- **Embedded jump-host and forwarding transport**: Added embedded `direct-tcpip` bastion proxy channels plus local/remote forwarding over the in-process SSH transport, removing the remaining system `ssh`/`scp`/`sshpass` runtime dependency from exec, SFTP, terminal, session, connection, health, and forward paths.
- **Terminal limit coverage**: WebSocket `/terminal` now participates in daemon session concurrency limits and applies execution rate limits to completed terminal input lines.

### Changed
- **Approval scoping**: Multi-host execution and playbook approvals now apply only to the approved host or step. Explicit `force` still applies to the whole requested operation when policy permits.
- **Mutation authorization and audit semantics**: Non-exec mutation paths now use the normal high-risk approval/force semantics instead of implicitly forcing authorization. PTY session writes use line-buffered authorization for completed shell input, and session/forward/connection operations write operation-level audit entries.
- **Approval context risk details**: Daemon approval context now carries the effective risk produced by the authorization path, including trusted host overrides and matched approval policy names when available.
- **Desktop risk previews**: Desktop exec and session input previews now include host-level `risk_override` before prompting.
- **Remote daemon tag scope**: Client-side `remotes.toml` tag checks now read host tags from the remote daemon before forwarding, so tag-based remote scope decisions use the remote daemon as the source of truth.
- **Team config import semantics**: `config-import` now updates changed same-name hosts while preserving local key/password material, matching the existing import preview.
- **Policy compatibility**: Runtime policy loading now prefers unified policy files and falls back to legacy `risk_rules.toml` / `approval_policies.toml` when no unified policy exists.
- **Connection management**: Connection status/connect/disconnect now retain embedded SSH sessions instead of creating ControlMaster sockets; `socket_path` remains `null`.
- **Documentation**: README, architecture, OpenAPI, configuration, daemon quickstart, MCP quickstart, and plan docs now describe the completed G-stage control-plane capabilities and the exact PTY/session audit boundary.

### Fixed
- **Desktop mutation parity**: Desktop-local SFTP, session, forward, and connection operations now use the same high-risk approval/force semantics as daemon, CLI, and MCP paths.
- **Desktop operation audit**: Desktop-local session, forward, and connection mutations now append operation-level audit entries for success and failure.
- **Vite production chunks**: Production frontend builds now split terminal, React, UI, icon, and runtime vendor chunks without the previous large-chunk warning.
- **Terminal session-limit race**: WebSocket `/terminal` and REST session open now reserve session-limit capacity atomically before opening the backend PTY, so concurrent opens cannot bypass `max_sessions`.

## [0.1.1] - 2026-06-16

### Added
- **Live Agent Activity**: Added a desktop activity panel that subscribes to daemon SSE events and polls recent audit entries, giving local visibility into SSH exec/session activity initiated by agents, CLI, daemon API, or the desktop app.
- **Daemon-backed MCP sessions**: MCP PTY sessions now route through the local daemon session registry by default when the daemon is reachable, while retaining process-local fallback when it is not.
- **Desktop session takeover**: The desktop Session panel can list daemon-managed sessions, attach to sessions created by MCP/CLI/daemon clients, read output, write input, close sessions, and fall back to local sessions when daemon access is unavailable.
- **Session takeover safety controls**: Session takeover now supports automatic tailing, read-only attach, active-session read-only mode, and high-risk PTY input confirmation before writing to the session.
- **S6 regression report**: Added `docs/s6-regression-report.md`, covering real-server validation for MCP daemon routing, source attribution, SSE session events, preview redaction, and cleanup proof.
- **Audit context tests (S1)**: 6 new tests covering `exec-multi` and `playbook` audit context propagation — verifying that `reason` and `change_id` survive the full write → JSONL → read round-trip for multi-host and multi-step scenarios.

### Changed
- **Daemon event stream**: Session open/write/read/close and WebSocket exec stream output now publish structured local events with source, host/session identifiers, command metadata, and bounded input/output previews.
- **Source attribution**: CLI, MCP, daemon, and desktop paths now carry standard `source` attribution into daemon events and audit entries, with `AGENT2SSH_SOURCE` available for agent-specific labels such as `codex`, `claude-code`, or `opencode`.
- **Live Activity filtering and alerts**: The activity panel now supports source/type/text filtering, expandable event details, sensitive preview redaction, and visible alerts for high-risk non-desktop activity.
- **Documentation**: Updated architecture and plan docs to reflect the completed S5-S8 activity visibility and desktop takeover work.
- **Test cleanup (S1-3)**: Eliminated all `unused variable` and `dead_code` compiler warnings in `cargo test --no-default-features` and `cargo test --no-default-features --features daemon` builds.
- **Real environment regression (S2)**: Full CLI, daemon HTTP, and MCP regression against a live SSH server — verified host management, exec/exec-multi (with reason/change_id), playbook run, audit (table/jsonl/csv), audit export, health-snapshot, doctor; confirmed MCP tool count at 50. No high-priority issues found. Report: `docs/s2-regression-report.md`.
- **Documentation & contract consistency (S3)**: 9 new tests ensuring README, `docs/skills.md`, `docs/api.yaml`, MCP schema, and daemon handlers stay in sync — MCP tool-name cross-check against `skills.md` (S3-1), request/response schema fixture tests for `/exec`, `/exec-multi`, `/playbooks/run`, `/audit/export` (S3-2), CLI `--help` alignment for `exec`, `exec-multi`, `playbook run` (S3-4). README MCP tools table deduplicated to a summary with link to `docs/skills.md` (S3-3).
- **Release quality gate (S4)**: Established fixed pre-release acceptance commands (`npm run build`, `cargo check` for all binary targets, two `cargo test` configurations). Tauri bundle build verified — `Agent2SSH.app` and `.dmg` generated with correct `agent2ssh-app` main binary. Installation scripts (`verify-install.sh`, `prepare-sidecars.sh`, `generate-checksums.sh`) validated and `verify-install.sh` fixed to avoid hanging on daemon/MCP `--help`. Created `docs/release-checklist.md` as a repeatable pre-release procedure.

### Fixed
- **Audit chain (F4-4)**: `exec-multi` and `playbook run` now correctly propagate `reason` and `change_id` through to every per-host audit entry. Previously, audit entries created via multi-host execution or playbook steps could lose the operation context.
- **MCP tool count**: Corrected documented MCP tool count from 31 to 50, reflecting all tools added in F2–F6 phases (host health, audit export, playbook run, metrics trends, etc.).
- **OpenAPI `/exec-multi` response**: Fixed response schema for the `/exec-multi` daemon endpoint to include `reason` and `change_id` fields in the request body, matching the actual implementation.

### Verified
- `npm run build`
- `git diff --check`
- Real-server S6 regression against `107.174.36.91`
- Browser render checks for the Live Activity and SessionPanel UI changes

## [0.1.0] - 2025-06-12

### Added
- **Interfaces**: Tauri desktop app, CLI, MCP stdio server (50 tools), HTTP/WebSocket daemon, Web Console
- **Host Management**: CRUD, SSH config import, ProxyJump/bastion, tags, per-host risk override, SSH key association
- **Command Execution**: Single-host exec, multi-host exec (by name or tag), ping, ControlMaster connection pooling
- **Safety**: 4-tier risk classification (low/medium/high/blocked), configurable risk rules, approval queue with TTL, audit log, desktop approval dialog
- **File Transfer**: SFTP upload/download/ls/stat/mkdir
- **Sessions & Tunnels**: Interactive PTY sessions, local/remote port forwarding
- **Automation**: Webhook notifications (HMAC-SHA256 signing, Slack Block Kit), Playbooks (command sequences), Remote daemon support
- **SSH Keys**: Ed25519 generation, import, delete, key dropdown in host form
- **Security**: Daemon token 0600 on Unix, SSH key permission enforcement, WebSocket exec stream auth, webhook outbound protection
- **CI/CD**: 4-platform build matrix, Tauri bundle job, Homebrew formula
- **Testing**: 137 unit tests + 56 integration tests + 24 CLI smoke tests

### Security
- Daemon token file restricted to 0600 on Unix systems
- SSH private key permissions enforced to 0600 after generation/import
- WebSocket exec stream requires Bearer token authentication
- Webhook outbound uses non-blocking fire with configurable timeout and HMAC-SHA256 signing
- Approval requests have configurable TTL (default 300s), expired requests auto-marked as timed_out
