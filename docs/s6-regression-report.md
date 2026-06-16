# S6 Regression Report

Date: 2026-06-16

## Scope

S6 validated the S5 Live Agent Activity changes against the real test server with an isolated local config directory. The goal was to prove that MCP-managed PTY sessions now route through the local daemon registry, produce live daemon events, carry source attribution, and redact sensitive data in activity previews.

## Environment

| Field | Value |
|------|-------|
| Server | `107.174.36.91` |
| User | `root` |
| Host alias | `s6-real` |
| Config isolation | `AGENT2SSH_CONFIG_DIR=/tmp/agent2ssh-s6-*/config` |
| SSH auth | Temporary Ed25519 key installed via `.agent2ssh-test.env` password |
| Daemon | Local `127.0.0.1:7722`, isolated token under the temp config dir |
| Binaries | Fresh debug builds of `agent2ssh`, `agent2ssh-mcp`, and `agent2ssh-daemon` |

## Results

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

## Key Observations

- MCP PTY sessions now use the daemon registry by default when the local daemon is reachable and token is available.
- The fallback path remains available but was not used in this regression because the daemon path succeeded.
- Live Activity preview redaction works for event previews. This does not redact the raw output returned to the MCP caller by `ssh_session_read`; callers still receive the actual remote shell output.
- PTY reads still show normal login banners and prompt echo before command output. This is expected and matches the existing session behavior.
- The remote host still emits `LC_ALL: cannot change locale (zh_CN.UTF-8)`. It did not affect the session, event, audit, or cleanup checks.

## Cleanup Proof

- Final daemon `/sessions` response: `[]`
- Remote `authorized_keys` check: `key-removed`
- Remote `/tmp/agent2ssh-s6-*`: no entries returned
- Local daemon process was stopped after the run

## Follow-Up

S7 has since exposed daemon-managed sessions in the desktop `SessionPanel`, so sessions opened through MCP can be listed, attached, read, written to, and closed from the UI.
