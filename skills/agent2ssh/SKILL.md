---
name: agent2ssh
description: Operate remote machines over SSH through Agent2SSH — host management, command execution with risk classification and approvals, SFTP transfers, persistent PTY sessions, port forwards, playbooks, audit, the local redaction / highlight / SSH-algorithm rule sets, and local execution on the daemon's own machine (one-shot `local_exec` environments and long-lived resident shell sessions). Use when asked to run commands on remote servers, transfer files, manage SSH hosts or tunnels, run a command in a local terminal environment, keep shell state across commands, tune what gets redacted or highlighted, or automate multi-host operations via the agent2ssh CLI or its MCP tools.
version: 0.4.10
---

# Agent2SSH Skill

Use this skill to operate remote machines over SSH through Agent2SSH — the
`agent2ssh` CLI or its MCP tools — and to run commands on the local machine in a
named terminal environment.

This file holds what always applies. The detail for each area is in
`references/`: read the one file the task needs, not all of them.

## Rules that always apply

1. **Hosts are addressed by alias.** Start with `ssh_list_hosts` (CLI:
   `agent2ssh host list --json`) and use an alias from it.
2. **Confirm with the user before `force: true`.** A `high`-risk command (see
   Risk Levels below) needs their yes first; say exactly what it will do.
3. **Read results as they come.** `ssh_exec` (and `local_exec`,
   `resident_session_exec`) answer with a header line — `exit_code`, duration,
   risk — then the output as printed. A non-zero `exit_code` is a normal result,
   not a tool failure.
4. **A failed tool says why.** It answers `Error [<code>]: …` plus a `Hint:` —
   follow the hint. Never retry `command_blocked`, `approval_rejected`,
   `gate_paused` or `policy_denied`. After `daemon_error` the command may have
   run: check `ssh_audit` before retrying.
5. **Output is bounded.** Each stream is returned up to 32 KiB (`max_output_bytes`),
   head and tail kept; a longer one is saved to a local file named in the result.
   Read or grep that file instead of re-running the command.
6. **A timeout does not stop the remote process.** `timeout_secs` is how long the
   call waits. Prefix the command with `timeout <secs>` when it must stop.
7. **Several commands on one host: `ssh_connect` first.** Later `ssh_exec` calls
   reuse the connection and are several times faster; `ssh_disconnect` when done.
8. **With the daemon running, an approval policy asks a person.** `ssh_exec` then
   waits for them to approve in the desktop app instead of failing.
9. **In scripts, pass `--json`.** Without it the CLI's exit status is the remote
   command's own, so 1 or 2 could be either side's. With `--json` a failure is
   `{"ok": false, "error": {"code", "message"}}`.
10. **Not every MCP tool is listed.** `tools/list` offers the core set; the rest
    still work when called by name, and `initialize` says how many are unlisted.
    A user lists them all with `AGENT2SSH_MCP_TOOLS=all` for the MCP server.

## What to read

| Task | Capability | Read |
|------|-----------|------|
| Run a command / script on a host | `ssh_exec` / `agent2ssh exec` | `references/exec.md` |
| Same command across many hosts | `ssh_exec_multi` / `agent2ssh exec-multi` | `references/exec.md` |
| Pipe input, bound output, read errors and exit status | `--stdin-file`, `--max-output-bytes`, `--json` | `references/exec.md` |
| Add / remove / import hosts, check reachability | `ssh_add_host`, `ssh_ping`, `host import-config` | `references/hosts-and-credentials.md` |
| Generate a key that will actually authenticate | `agent2ssh keys generate` | `references/hosts-and-credentials.md` |
| Password hosts fail on a new machine | master password, `secrets.enc` | `references/hosts-and-credentials.md` |
| Copy a file up/down, browse a remote path | `ssh_sftp_*` | `references/files-sessions-forwards.md` |
| Interactive REPL / long-running shell | `ssh_session_*` | `references/files-sessions-forwards.md` |
| Expose a remote/local port | `ssh_forward_*` | `references/files-sessions-forwards.md` |
| Run a command on this machine, not over SSH | `local_exec` / `agent2ssh local exec` | `references/local-and-resident.md` |
| Keep shell state (cwd, exports) across commands | `resident_session_*` | `references/local-and-resident.md` |
| Review past executions | `ssh_audit` | `references/config-and-audit.md` |
| Tune redaction / highlight / SSH algorithms | `ssh_redact_rule_*`, `ssh_highlight_rule_*`, `ssh_algo_prefs_*` | `references/config-and-audit.md` |
| Typical multi-step jobs (diagnose, deploy, migrate) | — | `references/workflows.md` |

## Discovery Checklist

Before using Agent2SSH from an agent, find the entry point:

```bash
command -v agent2ssh
command -v agent2ssh-mcp
```

For MCP clients, configure `agent2ssh-mcp` as a stdio server (by absolute path if
it is not on `PATH`), then call `tools/list` and look for `ssh_list_hosts`,
`ssh_exec`, `ssh_sftp_ls` and `ssh_risk_check`. `agent2ssh integrate list` shows
which clients are registered.

Inside an Agent2SSH source checkout, after a build, the binaries are also at
`src-tauri/target/debug/` and `src-tauri/target/release/` — a development
convenience, not an install location.

## Risk Levels

Every exec result and audit entry carries a `risk_level`:

| Level | Examples | Behaviour |
|-------|----------|-----------|
| `low` | `ls`, `cat`, `ps`, `df`, `grep` | Executes freely |
| `medium` | `apt install`, `sed -i`, `git push`, `chmod` | Executes; shown in UI |
| `high` | `sudo`, `rm -rf`, `kill -9`, `iptables` | **Requires `force: true`** |
| `blocked` | `shutdown`, `mkfs`, `rm -rf /`, fork-bomb | **Always rejected** |

Ask the user for confirmation before passing `force: true` on a `high`-risk command.
Explain the exact command and its effect first.

## Quick Reference

| Need | Command / MCP tool |
|------|--------------------|
| List hosts | `agent2ssh host list --json` / `ssh_list_hosts` |
| Add host | `agent2ssh host add` / `ssh_add_host` |
| Check reachability | `agent2ssh ping web1 web2` / `ssh_ping` |
| Run command | `agent2ssh exec <host> "<cmd>" --json` / `ssh_exec` |
| Run on many hosts | `agent2ssh exec-multi h1 h2 --command "..."` / `ssh_exec_multi` |
| Upload file | `agent2ssh sftp put <host> <local> <remote>` / `ssh_sftp_upload` |
| Download file | `agent2ssh sftp get <host> <remote> <local>` / `ssh_sftp_download` |
| List remote dir | `agent2ssh sftp ls <host> <path>` / `ssh_sftp_ls` |
| Stat remote path | `agent2ssh sftp stat <host> <path>` / `ssh_sftp_stat` |
| Make remote dir | `agent2ssh sftp mkdir <host> <path>` / `ssh_sftp_mkdir` |
| Rename / move remote path | `agent2ssh sftp rename <host> <old> <new>` / `ssh_sftp_rename` |
| Delete remote file | `agent2ssh sftp rm <host> <path>` / `ssh_sftp_rm` |
| Remove empty remote dir | `agent2ssh sftp rmdir <host> <path>` / `ssh_sftp_rmdir` |
| Recursive remote delete | `agent2ssh sftp rm-rf <host> <path>` / `ssh_sftp_rm_rf` |
| Persistent session | `agent2ssh session open/write/read/close` / `ssh_session_*` |
| Port forward | `agent2ssh forward add/list/rm` / `ssh_forward_*` |
| View audit log | `agent2ssh audit --json` / `ssh_audit` |
| Generate a usable key | `agent2ssh keys generate <name>` / `agent2ssh keys algorithm` |
| Run locally, one shot | `agent2ssh local exec --host local:vs "..."` / `local_exec` |
| Run locally, shell kept warm | `agent2ssh resident open/exec/close` / `resident_session_*` |
