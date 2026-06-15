## S2 Real Environment Regression Report

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
| `tools/list` | Pass | **50 tools** confirmed (matching `docs/skills.md`) |
| `ssh_doctor` | Pass | 11 checks: ssh, ssh-keygen, config, hosts.json, daemon.token, playbooks, audit = pass |
| `ssh_exec_multi` | Pass | 2 hosts, both exit 0, reason="S2 MCP regression", change_id="CHG-S2-M01" |
| `ssh_playbook_run` | Pass | 3/3 steps completed, reason="S2 MCP playbook", change_id="CHG-S2-M02" |
| `ssh_audit_export` (csv) | Pass | Valid CSV with reason/change_id columns |

**Tool count verification:** The `tools/list` response contains exactly 50 tools, matching the documented count in `docs/skills.md` and the integration test `mcp_tool_list_contains_exactly_50_tools`.

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
