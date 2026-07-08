# Agent2SSH 回归与研究报告合集（Regression & Research Log）

> 本文件合并了原 `docs/reports/` 下的 11 份独立报告（S2、S6、G1、G2、S9、R5、E1、E2、E3、I5、Plan 2 Q1/Q2 执行报告），作为单一历史证据源。各报告按时间顺序归档，保留原始结论与可复现验证命令。

## 报告索引

| 编号 | 标题 | 日期 | 主题 |
|------|------|------|------|
| S2 | [真实环境回归](#s2-真实环境回归) | 2026-06-15 | CLI/daemon/MCP 真实服务器回归 |
| S6 | [真实会话回归](#s6-真实会话回归) | 2026-06-16 | S5 Live Activity 端到端验证 |
| G1 | [Gate 回归](#g1-gate-回归) | 2026-06-16 | 急停 gate 验证 |
| G2 | [Limits 回归](#g2-limits-回归) | 2026-06-16 | 执行限额验证 |
| S9 | [Release Preflight (v0.1.1)](#s9-release-preflight-v011) | 2026-06-16 | 0.1.1 发布前收口 |
| R5 | [Desktop Control Plane 调研](#r5-desktop-control-plane-调研) | 2026-06-18/22 | 桌面控制面调研 |
| Q1Q2 | [Plan 2 Q1/Q2 执行报告](#plan-2-q1q2-执行报告) | 2026-06-26 | 0.2.1 质量收口与 Q2 回归 |
| E1 | [多 agent 接入验证](#e1-多-agent-接入验证报告) | — | 多 agent MCP 协议 smoke |
| E2 | [可靠性与规模](#e2-可靠性与规模报告) | — | 100 host / 1000 event 规模基线 |
| E3 | [契约一致性 CI](#e3-契约一致性-ci-报告) | — | 契约检查入 CI |
| I5 | [配置热加载一致性审计](#i5-配置热加载一致性审计) | — | 配置缓存审计 |

---

## S2 真实环境回归

Date: 2026-06-15
Target server: 107.174.36.91 (Debian, hostname `racknerd-ef7655c`, kernel 6.1.0-48-amd64)
SSH user: root, port 22
Agent2SSH version: 0.1.0

### Configuration Isolation

All tests used an isolated config directory via `AGENT2SSH_CONFIG_DIR=$(mktemp -d)/config` to avoid writing to the real `~/.agent2ssh`. A temporary Ed25519 SSH key was generated, installed on the server via `sshpass`, and removed after testing. The server's `/tmp/agent2ssh-*` was cleaned up and the `authorized_keys` entry deleted.

### S2-1: CLI Regression

Two hosts were added to the isolated config: `test-s2` and `test-s2-b` (both pointing to the same server with different aliases for multi-host testing). Both hosts carried `env=testing`, `role=test`, `tags=[regression, s2]`.

| Command | Result | Notes |
|---------|--------|-------|
| `host add` | Pass | Positional `<NAME>` + `--host` + `--key` + `--tags` + `--env` + `--role` |
| `host list --json` | Pass | Returns JSON array with full metadata (env, role, owner, tags) |
| `exec test-s2 "echo S2-REGRESSION-OK"` | Pass | stdout: `S2-REGRESSION-OK` |
| `exec test-s2 "hostname" --reason "S2 regression test" --change-id "CHG-S2-001"` | Pass | Audit entry contains reason and change_id |
| `ping test-s2` | Pass | Reachable, ~3100ms latency |
| `risk "echo hello"` | Pass | Risk: low |
| `exec-multi --command "echo multi-ok && hostname" test-s2 test-s2-b --reason "S2 multi-host regression" --change-id "CHG-S2-002" --json` | Pass | Both hosts exit 0, audit entries carry shared reason/change_id |
| `exec-multi --command "uname -r" test-s2 test-s2-b --compare --json` | Pass | Comparison: stdout identical, exit code groups: all 0 |
| `playbook list` | Pass | Lists `s2-health` with 3 steps |
| `playbook dry-run s2-health` | Pass | Shows resolved commands without executing |
| `playbook run s2-health --host test-s2 --reason "S2 playbook regression" --change-id "CHG-S2-003" --json` | Pass | 3/3 steps completed, all audit entries share reason/change_id |
| `audit` | Pass | 9 entries shown (2 exec + 2 exec-multi + 2 compare + 3 playbook) |
| `audit --format jsonl` | Pass | Valid JSONL, reason/change_id fields present where expected |
| `audit --format csv` | Pass | CSV header includes `reason,change_id`; data rows match |
| `doctor` | Pass | 11 checks, 0 fail, 5 warn (expected: daemon not running, no risk_rules, no remotes, no webhook) |

**Audit chain verification:** The JSONL audit log confirmed that `reason` and `change_id` are correctly propagated through all three entry points: single exec, exec-multi (per host), and playbook run (per step). Entries without `--reason`/`--change-id` correctly have `null` values.

### S2-2: Daemon HTTP Regression

The daemon was started with the isolated config dir on 127.0.0.1:7722. Authentication used the auto-generated Bearer token from `daemon.token`.

| Endpoint | Method | Result | Notes |
|----------|--------|--------|-------|
| `/health` | GET | Pass | `{"ok": true, "version": "0.1.0"}` |
| `/hosts` | GET | Pass | Returns 2 hosts with full metadata |
| `/exec` | POST | Pass | `{"exit_code": 0, "stdout": "daemon-exec-ok\n", "reason": "S2 daemon regression", "change_id": "CHG-S2-D01"}` |
| `/exec-multi` | POST | Pass | Both hosts successful, reason/change_id present in request body |
| `/playbooks` | GET | Pass | Lists `s2-health` playbook |
| `/playbooks/run` | POST | Pass | 3/3 steps completed, all audit entries share `CHG-S2-D03` |
| `/audit` | GET | Pass | 15 entries (9 CLI + 6 daemon), reason/change_id preserved |
| `/audit/export?format=jsonl` | GET | Pass | Valid JSONL output |
| `/audit/export?format=csv` | GET | Pass | CSV with correct `reason,change_id` headers |
| `/health-snapshot` | POST | Pass | Host reachable, uptime/disk/memory/load collected |

**Response structure:** All endpoint responses match the structure documented in `docs/api.yaml`. The `/exec-multi` response includes `total_hosts`, `successful`, `failed`, `skipped`, `stopped_early`, `batches_executed`, and `total_duration_ms` fields.

### S2-3: MCP Regression

The MCP server was tested via stdio with JSON-RPC 2.0 protocol.

| Tool | Result | Notes |
|------|--------|-------|
| `tools/list` | Pass | **50 tools** confirmed at S2 time; current G1 baseline is 51 tools (matching `docs/skills.md`) |
| `ssh_doctor` | Pass | 11 checks: ssh, ssh-keygen, config, hosts.json, daemon.token, playbooks, audit = pass |
| `ssh_exec_multi` | Pass | 2 hosts, both exit 0, reason="S2 MCP regression", change_id="CHG-S2-M01" |
| `ssh_playbook_run` | Pass | 3/3 steps completed, reason="S2 MCP playbook", change_id="CHG-S2-M02" |
| `ssh_audit_export` (csv) | Pass | Valid CSV with reason/change_id columns |

**Tool count verification:** At S2 time, the `tools/list` response contained exactly 50 tools. Current G1 baseline contains 51 tools, matching the documented count in `docs/skills.md` and the integration test `mcp_tool_list_contains_exactly_51_tools`.

### Issues Found

No high-priority issues discovered during this regression. All CLI, daemon, MCP, and audit functionality works correctly against a real SSH server.

**Minor observations:**

- The remote Debian server emits `bash: warning: setlocale: LC_ALL: cannot change locale (zh_CN.UTF-8)` on every command via stderr. This is a server-side locale configuration issue, not an Agent2SSH bug.
- SSH connection latency varies between 600ms and 6500ms depending on ControlMaster connection reuse. First connections take ~3-6s, reused connections are ~600-800ms.

### Cleanup Proof

- Remote: `sed -i "/agent2ssh-s2-regression/d" ~/.ssh/authorized_keys` executed successfully
- Remote: `rm -rf /tmp/agent2ssh-*` executed successfully
- Local: Temporary config directory and SSH key removed (`rm -rf $TMPDIR`)
- Server response: `CLEANUP_DONE` confirmed both operations

---

## S6 真实会话回归

Date: 2026-06-16

### Scope

S6 validated the S5 Live Agent Activity changes against the real test server with an isolated local config directory. The goal was to prove that MCP-managed PTY sessions now route through the local daemon registry, produce live daemon events, carry source attribution, and redact sensitive data in activity previews.

### Environment

| Field | Value |
|------|-------|
| Server | `107.174.36.91` |
| User | `root` |
| Host alias | `s6-real` |
| Config isolation | `AGENT2SSH_CONFIG_DIR=/tmp/agent2ssh-s6-*/config` |
| SSH auth | Temporary Ed25519 key installed via `.agent2ssh-test.env` password |
| Daemon | Local `127.0.0.1:7722`, isolated token under the temp config dir |
| Binaries | Fresh debug builds of `agent2ssh`, `agent2ssh-mcp`, and `agent2ssh-daemon` |

### Results

| Check | Result | Evidence |
|------|--------|----------|
| Real SSH reachability | Passed | CLI `exec s6-real 'printf s6-cli-ok'` returned exit code `0` |
| MCP session daemon routing | Passed | `ssh_session_open` returned `backend: "daemon"` and `source: "codex"` |
| Daemon registry visibility | Passed | `ssh_session_list` returned the MCP-opened session; `/sessions` returned `[]` after close |
| Session lifecycle events | Passed | SSE captured `session_opened`, `session_input`, `session_output`, and `session_closed` |
| Source attribution in events | Passed | SSE session events carried `source: "claude-code"` when `AGENT2SSH_SOURCE=claude-code` |
| Source attribution in audit | Passed | CLI exec with `AGENT2SSH_SOURCE=opencode` wrote audit entry with `source: "opencode"` |
| Audit CSV source column | Passed | CSV header includes `source`; exported row ended with `opencode` |
| Preview redaction | Passed | SSE preview redacted `Authorization: Bearer ...` and did not contain the test secret |
| Cleanup | Passed | Remote temporary key removed; remote `/tmp/agent2ssh-s6-*` removed; daemon stopped |

### Key Observations

- MCP PTY sessions now use the daemon registry by default when the local daemon is reachable and token is available.
- The fallback path remains available but was not used in this regression because the daemon path succeeded.
- Live Activity preview redaction works for event previews. This does not redact the raw output returned to the MCP caller by `ssh_session_read`; callers still receive the actual remote shell output.
- PTY reads still show normal login banners and prompt echo before command output. This is expected and matches the existing session behavior.
- The remote host still emits `LC_ALL: cannot change locale (zh_CN.UTF-8)`. It did not affect the session, event, audit, or cleanup checks.

### Cleanup Proof

- Final daemon `/sessions` response: `[]`
- Remote `authorized_keys` check: `key-removed`
- Remote `/tmp/agent2ssh-s6-*`: no entries returned
- Local daemon process was stopped after the run

### Follow-Up

S7 has since exposed daemon-managed sessions in the desktop `SessionPanel`, so sessions opened through MCP can be listed, attached, read, written to, and closed from the UI.

---

## G1 Gate 回归

Date: 2026-06-16

### Scope

G1 adds a daemon-level execution gate that can pause non-desktop execution while keeping the desktop recovery path available.

Covered entry points:

- `POST /exec`
- `POST /exec-multi`
- `POST /playbooks/run`
- `POST /sessions/:id/write`
- `GET /exec/stream`
- CLI `agent2ssh pause`, `agent2ssh resume`, `agent2ssh status`
- MCP read-only `ssh_gate_status`
- Desktop gate status indicator and pause/resume action

### Expected Behavior

- Default gate state is `active` when `execution_gate.json` is absent.
- `paused` state is persisted under the Agent2SSH config directory.
- In `paused` state, non-`desktop` daemon sources are rejected before SSH execution starts.
- HTTP execution entry points return 423 when rejected by the gate.
- Gate rejections append a `blocked` audit entry with the original source.
- Gate state changes publish `gate_changed`; rejected attempts publish `gate_rejected`.
- Desktop-sourced requests can still pause/resume and write sessions so the UI can recover from an emergency stop.

### Verification

Local verification performed:

```bash
npm run build
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh --bin agent2ssh-mcp
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features gate
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon gate
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test daemon_integration mcp_tool
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test daemon_integration mcp_call_tool_handler_covers_all_tools
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test cli_smoke mcp_stdio_end_to_end_initialize_tools_and_risk
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon -- --test-threads=1
git diff --check
```

Regression coverage:

- Core gate state persists and defaults to `active`.
- Daemon paused gate rejects non-`desktop` source with 423 and writes audit.
- Daemon paused gate allows `desktop` source for recovery.
- MCP `tools/list` and `docs/skills.md` both include 51 tools.
- MCP call handler covers `ssh_gate_status`.
- Browser UI check on `http://127.0.0.1:1420/` confirmed the topbar renders `Gate active` and `Pause` without overlap. Plain Vite browser mode cannot call Tauri `invoke`, so the button is disabled there as expected.

### Notes

MCP tool count is now 51 because `ssh_gate_status` was added. Current MCP contract documentation is `docs/skills.md`.

---

## G2 Limits 回归

Date: 2026-06-16

### Scope

G2 adds daemon-enforced execution limits:

- Per-source execution rate limit
- Per-host execution rate limit
- Per-tag execution rate limit
- Per-source max concurrent sessions
- Per-host max concurrent sessions
- Per-tag max concurrent sessions

Configuration is loaded from:

```text
~/.agent2ssh/execution_limits.toml
```

If the file is absent, daemon uses conservative defaults. `per_minute = 0` and `max_sessions = 0` disable that specific dimension.

### Enforced Paths

- `POST /exec`
- `POST /exec-multi`
- `POST /playbooks/run`
- `POST /sessions`
- `POST /sessions/:id/write`
- `GET /exec/stream`
- `POST /daemons/localhost/exec`

### Expected Behavior

- Execution rate limits use an in-memory sliding window.
- Session concurrency limits use daemon session registration.
- Over-limit HTTP requests return 429.
- Over-limit requests append a `blocked` audit entry with the source.
- Over-limit requests publish `limit_rejected` to the daemon event bus.
- Closing a daemon-managed session releases its session limit slot.

### Verification

Local verification performed:

```bash
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh --bin agent2ssh-mcp
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features limits
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon limit
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon -- --test-threads=1
git diff --check
```

Regression coverage:

- Source rate limit rejects the second operation in the same window.
- Host-specific rule overrides the default rate limit.
- Host session limit rejects new sessions when the max is reached.
- Daemon rejection path returns 429 and writes blocked audit.

---

## S9 Release Preflight (v0.1.1)

Date: 2026-06-16

> **Archive note**: This report is the preflight for the `v0.1.1` cut. The current shipped preflight is `v0.2.1` and is documented in the [Plan 2 Q1/Q2 执行报告](#plan-2-q1q2-执行报告) section of this log, which covers the Q1 quality-gate closure and the Q2 WebDAV / master-password regression pass against the `v0.2.1` bundle (`Agent2SSH_0.2.1_aarch64.dmg`). Treat this S9 file as a historical artefact only.

### Scope

S9 completed the local pre-release closure for `v0.1.1` after the S5-S8 Live Activity and desktop session takeover work. This preflight did not create or push the `v0.1.1` tag; it verified that the main branch is ready for that release action.

### Version State

| File | Version |
|------|---------|
| `src-tauri/Cargo.toml` | `0.1.1` |
| `package.json` | `0.1.1` |
| `package-lock.json` | `0.1.1` |
| `src-tauri/tauri.conf.json` | `0.1.1` |
| `docs/api.yaml` | `0.1.1` |
| `scripts/agent2ssh.rb` | `0.1.1` |

Local tag check: `v0.1.1` does not exist yet.

### Release Notes

`CHANGELOG.md` now has a single `0.1.1` release section dated 2026-06-16. It includes:

- S1-S4 audit, documentation, contract, and release-gate work.
- S5 Live Agent Activity and daemon event stream work.
- S6 real-server regression evidence.
- S7 desktop takeover of daemon-managed sessions.
- S8 session takeover safety and usability controls.

### Local Quality Gate

| Check | Result |
|------|--------|
| `npm run build` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh --bin agent2ssh-mcp` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon` | Passed |
| `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features` | Passed: 137 unit, 24 CLI smoke, 56 daemon integration |
| `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon` | Passed: 142 unit, 24 CLI smoke, 56 daemon integration |
| `git diff --check` | Passed |

### Remaining Release Actions

1. Commit and push this S9 preflight update.
2. Create annotated tag `v0.1.1`.
3. Push `main` and tags to `github` and `git233`.
4. Wait for CI release assets.
5. Verify release checksums.
6. Replace `SHA256_PLACEHOLDER` values in `scripts/agent2ssh.rb` after assets are available.

---

## R5 Desktop Control Plane 调研

### Scope

This research reviewed whether the desktop Settings surface should continue expanding as a local operator control plane, and which daemon capabilities are safe to expose there without adding new backend protocol surface.

The reviewed paths were:

- Desktop app Settings menu and topbar state.
- Local daemon `/health`, `/gate`, and `/console` surfaces.
- Existing CLI/MCP daemon health, doctor, metrics, and gate capabilities.
- Current roadmap guidance in `docs/PLAN.md` and control-plane architecture in `docs/architecture.md`.

### Findings

The daemon already exposes enough local read-only health state for a useful desktop operator surface:

- `/health` is unauthenticated and returns `ok`, `version`, `uptime_secs`, `config_dir_available`, `ssh_available`, and `pid`.
- `/gate` is authenticated and already powers pause/resume recovery from desktop.
- `/console` is the existing browser console entry point; the desktop should link to it rather than duplicating every console feature.

The desktop Settings menu is the right place for these controls because it is always visible, already houses language/setup/import controls, and is less disruptive than adding another full page module.

### Implemented Outcome

The desktop Settings menu now provides:

- Local daemon health status with version, PID, and last check time.
- Manual daemon health refresh using `/health`.
- Local daemon lifecycle controls for start, stop, and restart using the bundled sidecar.
- First-run setup wizard daemon start using the same desktop sidecar command.
- Execution gate status with active, paused, and unavailable states.
- Manual execution gate refresh.
- Web Console URL display, open action, and copy action.

Documentation was synchronized in:

- `README.md`
- `docs/architecture.md`
- `docs/guides/web-console-guide.md`
- `docs/api.yaml`

### Deferred Items

The following were intentionally not implemented during this pass:

- Remote daemon switching from the desktop Settings menu. Remote daemon operation already exists in CLI/API surfaces; adding it to Settings should wait for real multi-node dogfood.
- Full metrics and doctor reports in Settings. The menu should remain an operator summary. Detailed diagnostics belong in CLI/Web Console unless repeated user feedback shows otherwise.

Daemon lifecycle controls have been implemented for the bundled local sidecar. R1 package validation has since been updated in `docs/PLAN.md`; Windows packaged behavior was confirmed by user testing on 2026-06-22. New platform differences should now be tracked as explicit bug reports rather than broad validation debt.

### Validation

The implementation was verified with:

```bash
npm run build
(cd src-tauri && cargo test)
npm run tauri:build
```

The post-implementation regression was re-run on 2026-06-18. No test bugs were found. `npm run build` passed, `cargo test` passed with 161 unit tests, 27 CLI smoke tests, and 56 daemon integration tests, and `npm run tauri:build` produced the macOS `.app` and `.dmg` bundles.

Follow-up validation on 2026-06-22 confirmed the current closure baseline: Windows runtime smoke was user-confirmed, frontend/backend performance work completed, desktop i18n static audit reported 442 checked keys with 0 missing translations and 0 placeholder mismatches, and `npm run tauri:build` regenerated the macOS `.app` and `.dmg` bundles.

### Next Recommendation

No broad R5 follow-up remains. Cross-platform, performance, and desktop-control-plane follow-up should be limited to concrete bugs or regressions found during normal use.

---

## Plan 2 Q1/Q2 执行报告

Date: 2026-06-26

### Scope

This report records the first execution pass against Plan 2 (`docs/PLAN.md`), focused on:

- Q1 release confidence and local quality gates.
- Feasible local parts of Q2 credential encryption and WebDAV sync regression.

Q3 external adoption, cross-platform install smoke, real WebDAV server push/pull, and multi-device recovery remain external validation items.

### Q1 Results

#### Completed

- Added Rust format, Clippy, and diff-whitespace checks to `scripts/e2e-local.sh`.
- Added the same format/Clippy/diff checks to the release checklist (`docs/RELEASE.md`).
- Fixed current Clippy blockers under `cargo clippy --no-default-features --all-targets -- -D warnings`.
- Verified macOS local Tauri packaging still produces `.app` and `.dmg` bundles.

#### Clippy Fixes

The Clippy cleanup was intentionally mechanical:

- Introduced a `ConnectionHandleSnapshot` type alias for retained-connection supervision snapshots.
- Removed redundant branches and guards.
- Replaced unnecessary `sort_by`, `iter().any`, `vec!`, `clone`, `Ok(...?)`, and `return` patterns.
- Moved `keys.rs` tests to the end of the file to satisfy item-order linting.
- Added a local `#[allow(clippy::enum_variant_names)]` only for the MCP tool enum because the `Ssh*` prefix mirrors exported MCP tool names and avoids a large non-behavioral rename.

#### Validation Commands

Passed:

```bash
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --no-default-features --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test cli_smoke
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --test daemon_integration
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh --bin agent2ssh-mcp
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon
./scripts/e2e-local.sh
npm run tauri:build
```

`npm run tauri:build` produced:

- `src-tauri/target/release/bundle/macos/Agent2SSH.app`
- `src-tauri/target/release/bundle/dmg/Agent2SSH_0.2.1_aarch64.dmg`

Notarization was skipped because Apple notarization credentials were not configured in the local environment.

#### Remaining Q1 Notes

- Frontend has no dedicated ESLint setup today. The current frontend static gate remains `npm run build` (`tsc && vite build`). Adding ESLint should be a separate explicit change because it will introduce new dependencies and rule decisions.
- CI already covers contract consistency, Rust tests, Rust checks, frontend build, release binary builds, and release bundle jobs. Clippy is now enforced by local `e2e-local.sh` and the release checklist; adding it to CI should be considered after confirming cross-platform Clippy output is stable.

### Q2 Results

#### Completed Locally

Credential-store CLI smoke with isolated `AGENT2SSH_CONFIG_DIR`:

- `secrets status --json` starts as `{ initialized: false, unlocked: false }`.
- `secrets set-password --password ...` initializes `secrets.enc`.
- A new process without `AGENT2SSH_MASTER_PASSWORD` reports initialized but locked.
- A new process with `AGENT2SSH_MASTER_PASSWORD` reports initialized and unlocked.
- Recursive grep of the isolated config directory did not find the master password in plaintext.

Passed focused regression tests:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features webdav_sync::tests
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features secrets::tests
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features store::tests::migrate_secrets_moves_legacy_plaintext
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features store::tests::passwords_persist_as_marker_not_plaintext
```

These cover:

- encrypted store init/unlock and locked-store behavior,
- plaintext credential migration into secret references,
- password persistence as marker rather than plaintext,
- WebDAV sync file selection excluding local trust/runtime/private-key files,
- backup content selection,
- legacy remote `known_hosts.json` tolerance without local overwrite.

#### Remaining Q2 Items

Not completed in this local pass:

- Real WebDAV `push` / `pull` against an actual remote collection.
- Network failure, authentication failure, and remote conflict recovery against a real WebDAV service.
- Cross-device pull/unlock/host-key verification workflow.
- Desktop `SecretsUnlock` manual UI walkthrough.
- MCP/daemon password-host execution using a real password-auth SSH host and `AGENT2SSH_MASTER_PASSWORD`.

These require a real WebDAV endpoint, a second device/profile, a desktop manual run, or a password-auth test host.

### Recommendation

Next work should continue with Q2 real-environment validation before opening Q3 external adoption. The codebase now has a stronger local release gate, so new changes should use `./scripts/e2e-local.sh` as the default preflight.

---

## E1 多 agent 接入验证报告

### 目标

验证 Agent2SSH MCP stdio server 对不同 agent 客户端的基础接入路径保持一致：初始化、工具枚举和安全工具调用都能工作。

### 本次验证范围

本次完成的是协议级 smoke：

- `codex`
- `opencode`
- `cursor`
- `claude-code`

验证方式是用同一个 `agent2ssh-mcp` 二进制，分别设置 `AGENT2SSH_SOURCE`，通过 JSON-RPC stdio 执行：

1. `initialize`
2. `tools/list`
3. `tools/call` -> `ssh_risk_check`，命令为 `rm -rf /`

这能覆盖 MCP server 对不同来源标识的基础兼容性，但不自动打开各客户端 UI。

### 可复现命令

```bash
python3 scripts/e1-mcp-client-smoke.py
```

可用环境变量覆盖 MCP 二进制：

```bash
AGENT2SSH_MCP_BIN=/path/to/agent2ssh-mcp python3 scripts/e1-mcp-client-smoke.py
```

### 验收结果

- 4 个 source label 均能完成 MCP initialize。
- `tools/list` 均返回 51 个工具。
- `ssh_risk_check` 均将 `rm -rf /` 判定为 `blocked`。

### 边界

真实客户端 UI 的菜单、配置文件路径、重启行为和权限提示仍属于外部 dogfood 范围。E1 当前关闭的是 MCP 协议和 source 标识兼容性，不替代 R4 的真人接入反馈。

---

## E2 可靠性与规模报告

### 目标

建立不依赖 100 台真实 SSH 主机的本机规模基线，覆盖：

- 100+ host 配置读取与批量计划构建。
- 批量执行计划在不打开 SSH 连接时的低成本回归。
- event bus 对 1000 条事件突发的接收稳定性。

### 本次验证范围

#### 100 host 批量计划 smoke

新增脚本：

```bash
python3 scripts/e2-scale-plan-smoke.py
```

脚本行为：

1. 创建临时 `AGENT2SSH_CONFIG_DIR`。
2. 写入 100 个 synthetic host profile。
3. 用 `host list --tag scale --json` 验证配置读取。
4. 用 `exec-multi ... --plan --json` 生成 100 target 执行计划。
5. 验证计划为 `low` risk 且不需要 approval。

可用环境变量调整规模：

```bash
AGENT2SSH_SCALE_HOSTS=150 python3 scripts/e2-scale-plan-smoke.py
```

#### Rust 回归

新增测试：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features test_preview_exec_multi_scales_to_100_hosts
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features test_event_bus_handles_1000_event_burst
```

覆盖：

- `build_plan_from_profile` 对 100 hosts 的风险聚合和 target 生成。
- `events` broadcast bus 在 1024 buffer 内容量内接收 1000 条突发事件。

### 验收结果

- 100 host 本机配置与 `exec-multi --plan` smoke 通过。
- 100 host Rust plan 回归通过。
- 1000 event burst 回归通过。

### 边界

本报告不声称已经验证：

- 100 台真实 SSH 主机并发执行。
- 多台真实 daemon 的跨进程/跨机器聚合吞吐。
- 浏览器 EventSource 长连接在长时间网络抖动下的恢复。

这些需要真实环境或专门压测环境，建议在 R4 外部 dogfood 或后续专项压测中继续记录。

---

## E3 契约一致性 CI 报告

### 目标

把 S3 阶段已有的文档、OpenAPI、MCP 和 CLI help 一致性检查提升为显式 CI 门槛，避免契约漂移只在人工发布检查或大测试日志中被动发现。

### CI 入口

`.github/workflows/ci.yml` 新增 `contract-consistency` job，触发范围与主 CI 一致：

- push 到 `main`
- pull request 到 `main`
- GitHub release published

`build` matrix 和 release-only `tauri-bundle` job 都依赖该 job。契约检查失败时，跨平台编译和安装包打包不会继续消耗 CI 时间。

### 覆盖范围

#### MCP 文档一致性

```bash
cargo test --no-default-features --test daemon_integration mcp_tools_match_skills_md_documentation
```

校验 `docs/skills.md` 表格中的 51 个 MCP 工具名与实现侧预期列表一致。

#### OpenAPI / daemon schema fixture

```bash
cargo test --no-default-features --test daemon_integration exec_request_schema_includes_reason_and_change_id
cargo test --no-default-features --test daemon_integration exec_multi_body_schema_matches_contract
cargo test --no-default-features --test daemon_integration exec_multi_batch_result_schema_matches_contract
cargo test --no-default-features --test daemon_integration playbook_run_body_schema_matches_contract
cargo test --no-default-features --test daemon_integration audit_export_response_contract
```

覆盖 `/exec`、`/exec-multi`、`/playbooks/run` 和 `/audit/export` 的高频请求/响应契约。

#### CLI help 与文档参数

```bash
cargo test --no-default-features --test cli_smoke cli_exec_help_shows_reason_and_change_id
cargo test --no-default-features --test cli_smoke cli_exec_multi_help_shows_reason_and_change_id
cargo test --no-default-features --test cli_smoke cli_playbook_run_help_shows_reason_and_change_id
```

覆盖 `exec`、`exec-multi`、`playbook run` 的关键参数帮助输出，尤其是 `--reason` 和 `--change-id`。

### 验收结果

- CI workflow 中已有独立 `Contract consistency` job。
- `build` 和 `tauri-bundle` job 已通过 `needs: contract-consistency` 依赖该门槛。
- 本地已运行 `contract-consistency` job 中列出的 9 个目标测试，全部通过。
- `git diff --check` 通过，workflow YAML 基本语法解析通过。

---

## I5 配置热加载一致性审计

目标：盘点 `~/.agent2ssh/` 下各配置文件的读取频率与失效语义，给出「哪些值得纳入统一 `ConfigCache`、哪些应保持每次读盘」的结论，并落地至少一处确认收益项。

### 背景

`config_cache::ConfigCache<T>` 是单槽、`(mtime, len)` 签名失效的解析缓存：

- 命中时返回克隆值，不读盘、不解析；
- 文件 `mtime`/`len` 变化时自动 reload，因此**跨进程**外部编辑（CLI/桌面改了文件）会被另一个进程（daemon）自动感知；
- 同进程写入后调用 `invalidate()` 立即失效，规避文件系统 mtime 粒度问题。

适用判据：**读多写少 + 容忍亚秒级跨进程延迟**。不适用：需要严格实时一致的状态。

### 现状盘点

| 配置文件 | 加载函数 | 读取热度 | 当前状态 | 写入方 |
|----------|----------|----------|----------|--------|
| `anomaly.toml` | `load_anomaly_config` | 每次诊断 error 聚合 | ✅ 已缓存（O2-3） | 用户编辑 |
| `execution_limits.toml` | `load_execution_limits` | 每次 exec/session | ✅ 已缓存（O2-3） | 用户编辑 |
| `daemon_tokens.toml` | `load_scoped_daemon_tokens` | 每个鉴权请求 | ✅ 已缓存（O2-3） | 用户编辑 |
| `webhook.toml` | `load_webhook_config` | 每个事件/告警 | ✅ 已缓存（O2-3，`save_webhook_config` invalidate） | `save_webhook_config` |
| `hosts.json` | `load_config` | **每次 host 查找**（exec/list/SFTP/session） | ✅ 已缓存（**I5 本次**，`save_config_unlocked` invalidate） | `save_config_unlocked`（全部写入唯一漏斗） |
| `execution_gate.toml` | `load_execution_gate` | **每次 mutating op** | ⏳ 未缓存（建议保持，见下） | `pause`/`resume` |
| `policy.toml` | `load_policy_file` | **每次风险分类/授权** | ⏳ 未缓存（**建议纳入**） | `save_policy_approval_policies` |
| `risk_rules.toml` | `load_risk_rules`（async） | 每次分类（legacy 兼容） | ⏳ 未缓存（低优先，见下） | 用户编辑 |
| `playbooks.toml` | `load_playbooks` | playbook 运行/列举（非 per-exec） | ⏳ 未缓存（暂保持） | 用户编辑 |
| `approval_policies.toml` | `load_approval_policies` | 审批决策（warm） | ⏳ 未缓存（暂保持） | 用户编辑 |
| `remotes.toml` | `load_remotes` | 列举/路由/scope 检查 | ⏳ 未缓存（暂保持） | 用户编辑 |

### 结论

#### 本次落地

- **`hosts.json` 纳入 `ConfigCache`。** 它是最热的读路径（几乎每个操作都要解析 host），但仅在显式增删改 host 时变化。所有写入都经由唯一漏斗 `save_config_unlocked`，在其成功分支统一 `invalidate()`，覆盖全部 15 处 `save_config` 调用点，无遗漏风险。新增 `load_config_reflects_saved_hosts_via_cache` 单测验证写后不返回陈旧值。

#### 建议后续纳入（高收益、低风险）

- **`policy.toml`：** 每次风险分类/授权都读盘解析，写入极少（仅 `save_policy_approval_policies`）。缓存安全（policy 只能升级风险，旧值不会放松安全），收益高。接入时在 `save_policy_approval_policies` 成功后 `invalidate()`。
- **`execution_gate.toml`：** 读取最热（每个 mutating op）。**但建议谨慎**——execution gate 是「急停」语义，pause 必须尽快被各进程观察到。`(mtime,len)` 跨进程失效存在亚秒级窗口；对安全急停，新鲜度优先于这点微优化。若要缓存，应让 `pause`/`resume` 写后 `invalidate()`，并接受跨进程亚秒延迟。**当前结论：保持每次读盘，优先正确性。**

#### 保持每次读盘

- **`risk_rules.toml`：** legacy 兼容路径，且 `load_risk_rules` 是 async（`ConfigCache::load_with` 为同步 API），接入需额外适配；收益被 `policy.toml` 覆盖，低优先。
- **`playbooks.toml` / `approval_policies.toml` / `remotes.toml`：** 均非 per-exec 热路径（playbook 运行、审批决策、daemon 列举/路由频率远低于 exec），且无内置写入方，缓存收益有限，保持每次读盘以最大化新鲜度、降低复杂度。

### 验收

- 审计结论落档（本文件）。
- `hosts.json` 接入 `ConfigCache` + 写后 `invalidate`，含 `load_config_reflects_saved_hosts_via_cache` 单测。
- 两套 `cargo check`、两套 `cargo test` 全绿。
