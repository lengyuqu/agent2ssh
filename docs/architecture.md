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
        | Embedded SSH for exec, SFTP, terminal, sessions,
        | jump-host proxying, connection retention, and forwarding
        v
Remote hosts
```

## Components

| File | Role |
|------|------|
| `src-tauri/src/core.rs` | SSH exec, ping, exec-multi, SFTP wrappers, risk scoring |
| `src-tauri/src/embedded_ssh.rs` | In-process SSH transport, authentication, jump-host direct-tcpip proxying, host-key fingerprint capture, PTY shell, resize, and SFTP/exec helpers |
| `src-tauri/src/execution_control.rs` | Shared execution authorization for scope, effective risk, approval, and rejected audit entries |
| `src-tauri/src/store.rs` | Host profile persistence and audit log storage under `~/.agent2ssh` |
| `src-tauri/src/session.rs` | Persistent PTY sessions |
| `src-tauri/src/forward.rs` | SSH port forward tunnel management |
| `src-tauri/src/connection.rs` | Retained embedded SSH connection management with keepalive/health/auto-reconnect supervisor (K5) and `~/.ssh/config` parser |
| `src-tauri/src/secrets.rs` | App-managed encrypted credential store (K1): Argon2id master-password KDF + AES-256-GCM in `secrets.enc`; disk holds only a reference marker, no OS keychain |
| `src-tauri/src/sftp_transfer.rs` | SFTP transfer cancellation registry + cancellable copy + resume-offset logic (K6) |
| `src-tauri/src/telemetry.rs` | Opt-in, local-only crash/usage telemetry, off by default (K10) |
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

MCP PTY sessions route to the local daemon registry by default when the daemon is reachable and the local token is available. If the daemon is unavailable, MCP falls back to the process-local session store so basic PTY usage still works. Both daemon-managed and process-local sessions use the embedded SSH terminal worker, so password, key-file, ssh-agent, and jump-host authentication do not require system `ssh` or `sshpass`. The desktop Session panel also connects to the daemon session registry, so daemon-managed MCP sessions can be listed, attached, tailed, read, written to, and closed from the UI. Read-only attach and high-risk input confirmation provide a conservative default for observing externally created PTY sessions.

The daemon also exposes `/terminal` as an authenticated WebSocket endpoint for an interactive terminal. It streams terminal bytes directly, accepts resize control messages, and emits a connection metadata frame containing the host-key SHA256 fingerprint, host-key algorithm, address, username, and server banner before shell output. Completed input lines are checked through the same authorization path as REST session writes before the bytes are forwarded to the remote PTY.

### Diagnostics And Exception Logging

`diagnostics.rs` is the shared structured-log core. `append_diagnostic_log(level, component, message, fields)` writes one redacted JSONL record per line to `~/.agent2ssh/app.log`, rotating inline at 5 MB (3 generations). All four surfaces feed it:

- **Frontend** routes uncaught errors, unhandled promise rejections, and a top-level React `ErrorBoundary` (plus per-panel `catch` blocks via the `reportError` helper in `src/api.ts`) into the backend `write_diagnostic_log` command.
- **MCP** logs every failed tool dispatch (method + tool name) before returning the JSON-RPC error.
- **Daemon** composes a `tracing` layer that forwards `WARN`/`ERROR` events whose target starts with `agent2ssh` into `app.log`, so the daemon's structured logs are observable even when its stdout/stderr are not captured. Setting `AGENT2SSH_BRIDGE_DEPS` additionally bridges dependency-layer (`hyper`/`reqwest`/`ssh2`/…) warnings/errors for debugging transport issues (`1`/`true`/`all` = built-in prefix set, or a comma-separated custom list); those dependency entries are written via `append_diagnostic_log_no_sink` so they never re-enter the error-alert path (the webhook itself uses `reqwest`, which would otherwise loop). Its redirected stdout/stderr still land in `daemon.log`, which `daemon_control.rs` rotates at (re)start time.
- **All binaries** install a process-wide panic hook (`install_panic_hook`) that records panics to `app.log` before the default stderr behavior.

Error-level entries also fan out to an optional sink: the daemon registers one via `set_error_sink` that (a) fires the `diagnostic_error` notify webhook (opt-in through the webhook `events` list) and (b) feeds `anomaly::record_diagnostic_error`, a sliding-window detector that raises a single aggregate `anomaly_detected` alert when the error rate spikes (`diagnostic_error_threshold` within `window_secs`, gated by `diagnostic_cooldown_secs`) instead of one webhook per error. The desktop Settings → Diagnostics panel lists, exports (bundle), and clears these logs.

**Correlation IDs.** A `trace_id` ties one logical operation across surfaces. Synchronous surfaces (CLI/MCP/Tauri) use a thread-local set via `set_trace_id`/`seed_trace_id_from_env` (`AGENT2SSH_TRACE_ID`), auto-stamped onto each diagnostic entry. The daemon binds a per-request id through `trace_id_middleware` (reusing an inbound `X-Agent2SSH-Trace-Id` header or minting one), carries it in a task-local so the tracing bridge tags daemon log lines, and echoes it on the response. The desktop frontend generates a per-session id, stamps it on every frontend diagnostic, and sends it as `X-Agent2SSH-Trace-Id` on direct daemon calls; the MCP server forwards its id the same way when proxying to the local daemon.

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

PTY session and WebSocket terminal writes are authorized at completed-line boundaries by combining any buffered pending input with the new write. Daemon-managed sessions and desktop-local sessions both use this line buffer and append operation-level audit entries for completed input. This blocks normal fragmented shell commands such as splitting `rm -rf /` across multiple writes, but it is not a full shell parser for arbitrary interactive terminal applications.

For multi-host execution and playbooks, high-risk approvals are applied only to the target host or playbook step that received approval. Explicit `force` still applies to the whole requested operation when the caller chooses it and policy permits it.

## Execution Control Flow

`execution_control.rs` centralizes the common authorization path used by CLI, MCP, Tauri, and daemon surfaces:

1. Resolve the host target, tags, and optional host/playbook `risk_override`.
2. Enforce daemon or remote-token scope restrictions before approval is requested.
3. Calculate effective risk from built-in rules, user rules, and trusted overrides.
4. Reject `blocked` work immediately and write a rejected audit entry.
5. Enforce approval policy or high-risk approval/force requirements.
6. Execute the operation with approved-host or approved-step force only where applicable, then append the final audit entry for exec/mutation paths.

The daemon approval handler creates an approval request and waits for approval, rejection, or timeout. Local CLI/MCP and desktop-local paths without an approval handler fail closed and instruct the caller to use the daemon approval flow or `--force` when policy permits. WebSocket exec streaming uses the same embedded SSH transport as non-streaming exec, so password, key, jump-host, and fingerprint behavior stay aligned.

## SSH Transport Status

Command execution, SFTP, ping/health probes, WebSocket exec streaming, the WebSocket terminal, persistent PTY sessions, HTTP/SOCKS5 proxy dialing, jump-host proxying, retained connections, and port forwards use the in-process `ssh2` transport in `embedded_ssh.rs`. The embedded transport records connection diagnostics including authentication method, server banner, host-key algorithm, SHA256 host-key fingerprint, and jump-host alias when present. SSH host fingerprints are trusted automatically on first use in `~/.agent2ssh/known_hosts.json`; later algorithm or fingerprint changes for the same `host:port` identity are rejected. The terminal/session path requests a remote PTY and forwards resize changes through libssh2 rather than relying on a local system PTY process. HTTP proxies use CONNECT, SOCKS5 proxies support no-auth and username/password authentication, and jump hosts are implemented by opening an embedded `direct-tcpip` channel through the bastion and using that channel as the transport for the target SSH session.

Runtime SSH transport and local Ed25519 key generation do not depend on system `ssh`, `scp`, `sshpass`, or `ssh-keygen`. Key generation is implemented in Rust and reads entropy from the operating system CSPRNG. Daemon lifecycle status/stop checks use Rust process APIs and HTTP clients instead of shelling out to `kill`, `taskkill`, `tasklist`, or `curl`. SSH config import/export reads and writes local config text, but connection execution remains embedded.

| Path | Current backend |
|------|-----------------|
| Exec, exec-multi, ping, and health snapshots | Embedded `ssh2` |
| SFTP list/stat/mkdir/upload/download | Embedded `ssh2` SFTP |
| WebSocket `/exec/stream` | Embedded `ssh2` exec channel |
| WebSocket `/terminal` and REST/MCP/Tauri PTY sessions | Embedded `ssh2` terminal worker |
| HTTP CONNECT / SOCKS5 proxy connections | Embedded TCP proxy handshake before SSH session handshake |
| Jump-host / ProxyJump-style connections | Embedded `direct-tcpip` bastion channel |
| Connection status/connect/disconnect | Retained embedded `ssh2` sessions |
| Local and remote port forwards | Embedded `direct-tcpip` forwarding |
| Local Ed25519 SSH key generation | Embedded Rust key generator + OS CSPRNG |
| Daemon process status/stop and health check | Rust process APIs + Rust HTTP client |

## Control Plane

Authentication is enforced centrally by an axum middleware (`auth_middleware`) that runs ahead of every handler: requests to non-public routes must carry a valid admin or scoped token via `Authorization: Bearer` (or a `?token=` query parameter for browser WebSocket/SSE handshakes) and are rejected with 401 otherwise. Only `/`, `/console`, `/health`, and `/metrics` are exempt. Because the gate is a layer rather than a per-handler call, a newly added route is authenticated by default — a forgotten check can no longer silently expose an endpoint. Handlers still resolve their `AuthContext` scope for per-target authorization; the middleware is the gate, not the authorizer.

The daemon's listen address is resolved by the shared core helper `local_daemon_addr` (honoring `AGENT2SSH_DAEMON_ADDR`, default `127.0.0.1:7722`), and every client surface dials the same address through `local_daemon_url`/`local_daemon_connect_addr` so a non-default port stays reachable end to end; a wildcard bind (`0.0.0.0`/`::`) is mapped back to loopback for clients, and binding off-loopback emits a `warn` diagnostic. The daemon serves under `with_graceful_shutdown`, draining in-flight requests on Ctrl-C / SIGTERM and removing `daemon.pid` on exit so a signalled shutdown does not leave a stale PID file.

The current control-plane layer is enforced at the daemon/audit boundary rather than only in the desktop UI:

| Capability | Config / entry point | Behaviour |
|------------|----------------------|-----------|
| Execution gate | `agent2ssh pause/resume/status`, `execution_gate.toml` | Pauses non-desktop daemon mutation/execution paths and returns HTTP 423 for blocked sources |
| Execution limits | `execution_limits.toml` | Enforces per-source, per-host, and per-tag execution rate plus session concurrency limits; returns HTTP 429 on limit rejection |
| Policy dry-run | `policy.toml` / `policy.json`, `agent2ssh policy validate/test` | Validates policy-as-code and predicts `allow` / `approve` / `block` decisions |
| Anomaly detection | `anomaly.toml` | Detects source bursts, sensitive command patterns, and after-hours high-risk activity from audit windows |

The desktop Settings menu is the operator surface for local recovery and control. It exposes local daemon health from `/health` with version, PID, and last check time; can start, stop, and restart the bundled local daemon sidecar; shows execution gate status as active, paused, or unavailable; supports manual refresh; and links to the daemon Web Console URL. The first-run setup wizard uses the same desktop daemon start command so new users do not need to drop to a terminal before opening the console. The desktop control-plane research record is in `docs/reports/r5-desktop-control-plane-research-report.md`.

The daemon event stream exposes `gate_rejected`, `limit_rejected`, and `anomaly_detected`, allowing Live Agent Activity and webhook consumers to react while the activity is still local and recent.

For remote daemon routing, client-side `remotes.toml` scope is checked before forwarding. Host allowlists and command patterns are local checks; tag-based scope fetches the target host metadata from the remote daemon so multi-node tag decisions use the remote daemon as the source of truth.

## Persistence And Locking

Host configuration, audit, and diagnostic-log writes use cross-process lock files under `~/.agent2ssh/` plus atomic temp-file writes and rename. This keeps concurrent CLI, MCP, daemon, and desktop access from corrupting `hosts.json`, interleaving audit writes, or racing an `app.log` append/rotation. The internal lock files are `.hosts.lock`, `.audit.lock`, and `.app_log.lock`; each writer holds a process-local mutex plus the exclusive file lock (`store::lock_config_file`) for the duration of the write/rotation.

Read-heavy config files (`anomaly.toml`, `execution_limits.toml`, `daemon_tokens.toml`, `webhook.toml`) are read on hot paths — per exec, per authenticated request, per fired event/error. Each is wrapped in a `config_cache::ConfigCache`, a single-slot cache keyed by the file's `(mtime, len)` that memoizes the parsed value and only re-reads when the file changes (or after an in-process `save_*` calls `invalidate`). This removes repeated TOML parsing from those paths while still picking up external edits promptly.

## Current Direction

The original daemon, approval, risk configuration, web console, Live Activity, session takeover, and G-stage control-plane milestones are implemented. R5 confirmed the desktop Settings menu as the local operator surface for daemon health, daemon lifecycle control, gate recovery, and console handoff. The next phase is release/adoption closure, cross-platform validation, and broader end-to-end testing.

```text
Desktop App (Windows / Linux / macOS)  →  local HTTP/WebSocket daemon
Web Console                            →  local or configured remote daemon
MCP / CLI                              →  local or configured remote daemon
```
