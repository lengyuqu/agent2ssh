# Changelog

All notable changes to Agent2SSH are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
- **Opt-in sshd fixture tests**: `src-tauri/tests/exec_fixture.rs` runs against a real sshd and covers the two runtime behaviours the unit suites structurally cannot reach — draining stdout and stderr off one SSH channel, and bounding a remote command by its timeout. The suite is skipped unless `AGENT2SSH_TEST_SSH_PORT` points at a reachable server; `scripts/sshd-fixture/Dockerfile` builds a throwaway alpine/OpenSSH container to point it at. Both bugs below passed the full unit, CLI and daemon suites before these tests existed.
- **Tauri feature coverage in CI**: A new `tauri-unit-tests` job compiles, lints and tests the default feature set, so `src/tauri_commands.rs` — behind `#[cfg(feature = "tauri")]` — can now fail a pull request. Every other cargo invocation in CI passes `--no-default-features`, which left that module uncompiled, untested and unlinted.
- **Exec fixture suite is no longer local-only**: It now also runs from `scripts/e2e-docker.sh`, and therefore from the `real-ssh-e2e` CI job, against the container that script already starts. It accepts `AGENT2SSH_TEST_SSH_KEY` for that key-only server.
- **Empty-target and zero-budget unit tests**: `exec_multi_core`, `exec_multi_with_strategy` and `preview_exec_multi` have tests pinning the `no hosts matched the requested hosts or tags` error, so the preview cannot drift back to reporting an empty plan as success. `BoundedCapture` gained boundary tests for a zero budget (keeps no bytes) and a one-byte budget (keeps the newest byte, since the odd byte is all tail).

### Changed
- **WebDAV sync set convergence**: The desktop and CLI/daemon portable-config file lists were merged into the single `webdav_sync::SYNCABLE_FILES` constant, so every sync path now carries the same files. `webhook.toml` and `app_preferences.json` are synced from every entry point.
- **Local-only configuration files**: `secrets.enc`, `snippets.json`, and `approval_policies.toml` are no longer part of WebDAV sync and do not cross machines. Machines that relied on cloud sync for shared credentials or snippets must migrate them manually.
- **Byte units in the WebDAV Sync panel**: `formatBytes` now reports `KiB`/`MiB` consistently across the UI, replacing the panel's `KB`/`MB` labels.
- **`ExecRequest::timeout_secs` contract**: The deadline bounds how long the client waits, not the lifetime of the remote command. Documented the measured OpenSSH 9.7 behaviour (below) and directed callers that need a guaranteed stop to wrap the command server-side in `timeout(1)`.
- **`exec_multi` reports an empty target list**: `exec_multi_core`, `exec_multi_with_strategy` and `preview_exec_multi` resolve through one `resolve_exec_targets` helper that returns an error (`no hosts matched the requested hosts or tags`) for an empty resolved list, and propagate host/tag resolution failures instead of collapsing them to an empty vector. A `--tags` typo that selected nothing used to look like a successful run: the CLI printed an empty table and exited 0, MCP returned an empty array, the daemon returned 200, and `exec/preview` reported that a command nobody would receive was safe to send. The CLI, MCP server and Tauri command layer now surface it as a failure, and the daemon answers 400 for `/exec-multi` and `/exec/compare` — matching `/exec` and `/exec/preview`, which already used 400 for the same class of error rather than 500.
- **Connection and prompt budgets follow the caller**: `connect_tcp_address` tries every resolved address within one shared budget capped at 30 s, so a host whose unreachable AAAA record precedes a usable A record connects immediately instead of waiting out the first attempt, and a zero-second budget can no longer be passed to `TcpStream::connect_timeout` or `set_read_timeout` (both reject a zero duration). A failed connection now names every address that was tried instead of only the last one, so a host whose A and AAAA records fail for different reasons does not hide the relevant error, and addresses skipped once the budget ran out are reported as such. The keyboard-interactive external prompt wait now follows the caller's SSH timeout instead of a hard-coded 120 s.
- **Shared health-probe client**: `check_health_blocking` reuses one `reqwest::blocking::Client` instead of constructing one per daemon. Each client owns a dedicated runtime thread, so listing N daemons spawned and joined N of them per call.
- **Stoppable forward writes**: `write_all_tcp` and `write_all_channel` gained `_with_stop` variants, and the port-forward bridge checks the stop flag between reads and inside each write loop. Removing a forward now interrupts an in-flight write instead of leaving the connection thread blocked until the peer drains, and the remote-forward target connection is opened through `connect_tcp_address` with a 5 s budget rather than an unbounded `TcpStream::connect`.

### Fixed
- **stdout/stderr drain deadlock**: `exec_ssh_embedded` drained stdout to EOF before reading stderr. The SSH channel window is shared between the two streams, so a command that filled stderr while still holding stdout open blocked its own writes and the read never returned. Both streams are now read on every pass, EOF is taken from `Channel::eof`, and the deadline is enforced inside the loop. `max_output_bytes` is applied per stream via a head-and-tail accumulator, so a stderr flood can no longer grow the process without bound.
- **Timed-out execs no longer leak a blocking worker**: The timeout path closed the channel while the session had already been switched back to blocking mode, and a blocking `close` waits for a server acknowledgement that only arrives once the remote command exits — so a 2 s deadline on an 8 s command returned after 8 s, and the abandoned `spawn_blocking` task kept the caller's runtime alive. The session now stays non-blocking through the teardown, and the post-drain exchange is bounded by an explicit session timeout instead of blocking indefinitely.
- **Timed-out execs are audited**: A timeout previously returned straight out of the exec and wrote no audit entry, even though the command did run. Timeouts are now recorded with no exit code and a reason of `command timed out after Ns`.
- **Bounded stderr capture**: `max_output_bytes` now caps stderr as well as stdout (each stream keeps its head and tail, and `dropped_bytes` reports the total discarded). Stderr was previously read to EOF with no limit, so a chatty remote command could grow the process without bound.
- **WebDAV failures are surfaced**: The desktop sync actions and the automatic post-change sync now read `lastSuccess` off the returned status. Failures previously showed a success toast, and automatic sync failures produced no toast and no diagnostic at all, because the backend reports sync errors as a status rather than as a rejected call.
- **Unix PATH detection**: `path_contains_dir` splits on the platform separator (`:` on Unix, `;` on Windows) instead of always `;`. Splitting on `;` made the whole Unix PATH a single segment, so the CLI status panel always reported the binaries as absent from `PATH`.
- **Ephemeral-port port forwards**: `bind_port = 0` now binds IPv6 loopback on the port IPv4 actually received. Binding both stacks on the literal `0` gave them two different ephemeral ports while only IPv4's was recorded, so clients resolving `localhost` to `::1` reached the wrong port.
- **Legacy WebDAV markers**: A pull from a marker that still lists a dropped or never-syncable file (`known_hosts.json`, `secrets.enc`, `snippets.json`, `approval_policies.toml`) is accepted and the entry is skipped, rather than failing the whole pull. This restores the 0.3.0 contract that a stale remote manifest cannot overwrite local SSH host-key trust state, credentials, or snippets.
- **Red clippy gate**: `execution_control::authorize_command_without_approval_handler` trips `clippy::too_many_arguments` (11 against a threshold of 7), so both `Rust clippy` steps in `contract-consistency` fail and the whole workflow is blocked — the commit that added it (`f262505`) ran `cargo check` but not clippy. The library also carried five needless borrows and a redundant match guard in the tray setup, invisible because no CI step enabled the tauri feature. All six are resolved.
- **Leaked workers on a silent peer**: `connect_embedded_ssh` set socket-level read/write timeouts but no libssh2 session timeout, and libssh2's blocking-mode waits loop internally without a deadline. A host that accepted the TCP connection and then said nothing — no SSH banner — left `handshake` blocked forever. `ping_hosts_core` and `collect_health_snapshot` wrap the connect in `tokio::time::timeout`, so they returned on schedule, but abandoning the `spawn_blocking` handle leaked the blocked worker and its socket, once per host and per poll (a host list containing one broken entry accumulated threads). The session now gets the same budget the socket has, so blocking calls fail instead of hanging; the same budget also bounds a mid-session stall on any other blocking transport call.
- **IPv6 literals in host:port strings**: `format_host_port` now brackets IPv6 literals, so `::1` formats as `[::1]:22`. The HTTP-proxy path built its authority from the raw host, so `CONNECT ::1:22 HTTP/1.1` was malformed and every IPv6 host routed through an HTTP proxy failed; the fingerprint/trust prompts, connection info and exec audit entries also printed an ambiguous `::1:22`. The known-hosts trust key is unchanged (it is built from the host identity, not from this string), so no re-trusting is required.
- **Daemon health probes ignored the scheme and the hostname**: `check_health_blocking` hand-rolled a plaintext `GET /health` over a raw TCP socket, so an `https://` remote was probed in the clear, and any `host:port` it could not parse silently fell back to the local daemon address — a malformed remote URL therefore reported itself as `connected: true`. The probe now uses the configured scheme and hostname through a real HTTP client under a single 2 s budget and accepts only a 2xx response. `agent2ssh daemon diagnose` likewise resolves DNS names instead of rejecting everything that is not an IP literal.

### Verified
- `src-tauri/tests/connect_deadline.rs` drives `ping` and the health snapshot against a loopback peer that accepts and then goes silent, and asserts the peer observes EOF — the leaked worker used to hold the socket open indefinitely. Runs in CI, no fixture needed.
- `./scripts/e2e-docker.sh` end to end against the containerized OpenSSH server (key auth, non-root user): 8 checks pass, including the exec runtime suite wired in here.
- `cargo clippy` for the lib under both feature sets, and the two `Rust clippy` invocations as CI runs them, now report nothing.
- Containerised OpenSSH 9.7 fixture (`scripts/sshd-fixture`) with the opt-in `exec_fixture` suite: 3 tests covering the stderr flood, the timeout deadline, and the timeout audit entry.
- An 8 MiB stderr flood used to stall past a 30 s deadline; it now drains in under 2 s with both streams captured and truncated to `max_output_bytes`.
- Measured limitation (not a regression): a pty-less exec channel's remote command survives the client closing the channel or dropping the connection (`docker exec ps` shows the process, and its `/tmp` marker appears after the client returned). Only the pty path reaps the process group, so a guaranteed stop requires a remote-side `timeout(1)`.

## [0.3.0] - 2026-08-12

### Added
- **Safe terminal broadcast**: Added token-owned live terminal IDs and preview/run endpoints for explicit multi-terminal command broadcasts. Every target passes scope, gate, rate-limit, effective-risk and approval checks before any input is enqueued, with per-target audit/results and an explicit desktop “Broadcast and run” action.
- **Conflict-aware portable config sync**: Added stable SHA-256 configuration digests, local/remote/diverged status summaries across CLI, daemon, and desktop, plus a reusable sync transport abstraction with a deterministic fake backend for tests.
- **Structured terminal workbench**: Added marker-backed command blocks with color rails, search, navigation, safe plain-text copy, and structured metadata for future audit consumers.
- **Session recordings**: Added opt-in asciicast v2 terminal recording, protected local storage, daemon and desktop management APIs, variable-speed playback, and confirmed audited deletion.
- **CLI completions**: Added Bash, Zsh, Fish, and PowerShell completion generation with read-only dynamic candidates for configured and active resources.
- **Terminal highlight UX**: Wired persisted highlight rules into xterm decorations and added desktop controls for adding, enabling, deleting, and resetting rules.
- **Desktop diagnostics/setup surfaces**: Added in-app structured system reports, CLI PATH install/remove controls, and jump-host selection for port forwards.
- **Secrets vault**: Encrypted backup/sync of `secrets.enc` with Argon2id and AES-256-GCM, lifecycle registry for daemon/managed processes, and structured error codes across the Rust backend.
- **Redaction and sanitization pipeline**: Centralized output redaction (`redaction.rs`, `sanitize.rs`, `copy_redact.rs`) applied consistently across exec, audit, export, and terminal surfaces; secrets never leak into logs or audit exports.
- **Embedded SSH forwarding**: Full local/remote port-forward support over the in-process SSH transport (`forward.rs`), replacing remaining system `ssh` runtime dependencies.
- **Jump-host chain**: Bastion/proxy chain support (`jump_chain.rs`) for multi-hop connections over embedded `direct-tcpip`.
- **Prompt waiter**: Interactive prompt detection and waiting (`prompt_waiter.rs`) for password/OTP/confirmation prompts inside PTY sessions.
- **Container discovery**: Container/platform discovery module (`container_discovery.rs`) for SSH targets inside containers.
- **SSH algorithm management**: Explicit SSH algorithm/curve control (`ssh_algo.rs`), URL safety validation (`url_safety.rs`), path resolution guardrails (`path_resolver.rs`), WebSocket drain handling (`ws_drain.rs`), and OSC/IPC bridging (`osc_ipc.rs`).
- **Snippets**: Reusable command snippet storage with validated CRUD across desktop, CLI, daemon HTTP, and MCP, plus WebDAV synchronization (`snippets.rs`).
- **Encrypted backup crypto**: Key derivation and backup encryption primitives (`backup_crypto.rs`) used by the encrypted sync path.
- **CI lint checks**: Added Biome config (`biome.json`), frontend test infrastructure (Vitest + Testing Library), and CI lint gates.
- **Branch protection**: Documented branch protection policy (`docs/guides/branch-protection.md`) and added `.github/CODEOWNERS`.

### Changed
- **Versioned WebDAV commits**: WebDAV schema v2 uploads immutable per-sync objects and commits markers with ETag compare-and-swap; push/pull reject divergent overwrites unless `--force`, pull validates a complete snapshot before replacement, and schema v1 pulls preserve newer local-only snippets.
- **WebDAV sync trust boundary**: `known_hosts.json` is no longer part of WebDAV sync payloads. Pulls from older remote manifests tolerate and skip legacy `known_hosts.json` entries so local SSH host-key trust state is not overwritten across machines.
- **Desktop internationalization coverage**: Completed Chinese translations for the current desktop surface, including SSH fingerprint confirmation, connection progress, WebDAV Sync, MCP binding removal, the Sync module label, and the React error recovery screen.
- **CI release runners**: Use Intel macOS runners for x86 builds; release workflow portability and macOS runner environment fixes; stop caching Rust toolchain binaries.
- **Windows Tauri command import**: Fixed the Windows Tauri command import in `tauri_commands.rs`.

### Fixed
- **Recent feature completion**: Forward creation now waits for authenticated listener readiness, multi-rule batches roll back on partial failure, SSH `Include` supports multi-directory globs, Fish PATH entries use native syntax, and `v`-prefixed release tags compare correctly.
- **Private-key passphrases**: Store passphrases in the encrypted secrets vault, preserve them during edits, migrate them during host renames, clear cache entries on lock/removal/auth failure, and refuse plaintext persistence while locked.
- **Regression hardening**: Redacted host credentials from every transport response, exposed real tunnel state/traffic in the desktop, aligned highlight validation with JavaScript regex semantics, supported alternate-screen highlighting, rejected malformed remote versions, and removed the browser-opening side effect from URL tests.
- **Tauri bundle build**: Added the missing single-instance plugin dependency for default desktop builds and restored the fingerprint confirmation commands used by the desktop connection flow.
- **Icon packaging**: Regenerated the v3 32px app icons as RGBA PNGs so Tauri bundle generation accepts the icon set.

### Verified
- `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib`
- `npm run build`
- `git diff --check`
- `npm run tauri:build`
- Desktop i18n static audit: 442 checked keys, 0 missing translations, 0 placeholder mismatches

## [0.2.1] - 2026-06-21

### Added
- **App-managed credential encryption**: Added a master-password credential store backed by Argon2id and AES-256-GCM in `~/.agent2ssh/secrets.enc`. `hosts.json` now keeps only `$agent2ssh-secret$` references after migration.
- **Credential unlock surfaces**: Added desktop unlock/change-password commands and CLI `secrets status` / `secrets set-password`, with `AGENT2SSH_MASTER_PASSWORD` support for headless CLI/MCP/daemon use.
- **WebDAV config sync**: Added `agent2ssh webdav push|pull|status` for syncing portable config plus encrypted `secrets.enc`, with local pre-sync backups and a global `sync_version.json` marker on every sync.

### Changed
- **Credential storage model**: SSH host and proxy passwords no longer rely on system key management. Secrets are managed by Agent2SSH encryption and remain unavailable while the store is locked.
- **Build defaults**: Local Tauri bundles no longer create updater signing artifacts by default, so builds do not require `TAURI_SIGNING_PRIVATE_KEY` unless release signing is explicitly enabled.

### Added
- **Control-plane safety layer**: Added daemon-level execution gate controls, execution rate/session limits, unified policy-as-code validation, and audit-window anomaly detection.
- **Execution gate**: Added `agent2ssh pause/resume/status`, daemon 423 rejection for paused non-desktop sources, gate audit entries, and `gate_rejected` / `gate_changed` events.
- **Execution limits**: Added `execution_limits.toml` with per-source, per-host, and per-tag rate/session limits, plus 429 rejection auditing and `limit_rejected` events.
- **Unified policy files**: Added `policy.toml` / `policy.json` support for colocating risk rules and approval policies, with `agent2ssh policy validate` and `agent2ssh policy test`.
- **Anomaly detection**: Added `anomaly.toml`, source burst detection, sensitive command pattern detection, after-hours high-risk detection, `anomaly_detected` events, webhook support, and Live Activity anomaly highlighting.
- **Embedded jump-host and forwarding transport**: Added embedded `direct-tcpip` bastion proxy channels plus local/remote forwarding over the in-process SSH transport, removing the remaining system `ssh`/`scp`/`sshpass` runtime dependency from exec, SFTP, terminal, session, connection, health, and forward paths.
- **Terminal limit coverage**: WebSocket `/terminal` now participates in daemon session concurrency limits and applies execution rate limits to completed terminal input lines.
- **Embedded SSH key generation**: Local Ed25519 key generation now uses the Rust backend and the operating system CSPRNG instead of shelling out to `ssh-keygen`.
- **Portable daemon lifecycle control**: Daemon status/stop/restart and health checks now use Rust process and HTTP APIs instead of shelling out to `kill`, `taskkill`, `tasklist`, or `curl`.

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
