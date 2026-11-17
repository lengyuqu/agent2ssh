# Running commands

Part of the agent2ssh skill. `SKILL.md` has the overview and the rules that always
apply; this file is the detail for one area.

## Command Execution

### Basic exec

```bash
agent2ssh exec prod "uname -a" --json
```

### With timeout (default 60 s)

```bash
agent2ssh exec prod "npm run build" --timeout-secs 120 --json
```

The timeout is how long the call **waits**. On an SSH host the remote process
is not killed when it expires and may keep running — prefix the command with
`timeout <secs>` when it must stop, or start long work in the background
(`nohup ... > /tmp/job.log 2>&1 &`) and poll the log. Through MCP a timeout
comes back as an `exec_timeout` error result that still carries everything the
command printed before the deadline.

### Reading MCP results

`ssh_exec`, `local_exec` and `resident_session_exec` answer with a header line
and then the output as printed:

```text
exit_code: 0 | duration_ms: 212 | risk_level: low | host: prod

[stdout]
Linux prod 6.8.0 ...
```

- A non-zero `exit_code` is a normal result, not a tool error.
- Each stream is returned up to `max_output_bytes` (default 32768). Longer
  output keeps its head and tail around a
  `[... N bytes omitted; full stdout (M bytes) saved to <path> ...]` marker —
  read or grep that local file instead of re-running the command, or re-run
  with a narrower command (`tail -n 200`, `grep`).
- A failed tool answers `Error [<code>]: <message>` plus a `Hint:` line. Act on
  the code: `host_not_found` (the hint lists the aliases), `force_required`
  (confirm with the user first), `command_blocked` / `approval_rejected` (do not
  retry), `gate_paused` (a person stopped execution — do not retry),
  `rate_limited` (wait), `exec_timeout`, `ssh_auth_failed`,
  `ssh_connect_failed`, `daemon_unavailable`, `binding_invalid`, and
  `daemon_error` (the daemon dropped the connection after receiving the
  command — it may have run; check `ssh_audit` before retrying).
- When the Agent2SSH daemon is running, `ssh_exec` goes through it: a command an
  approval policy covers waits for a person to approve it in the desktop app
  (the call stays open, up to the approval timeout) instead of failing.
- Tool calls run concurrently, so independent commands can be issued in
  parallel.
- The MCP server lists only its core tools by default. The redaction /
  highlight / algorithm rule tools, approvals, playbooks, port forwards, the
  diagnostics tools and the like are not in `tools/list` but **still work when
  called by name**; `initialize` says how many are unlisted. A user who wants
  them listed sets `AGENT2SSH_MCP_TOOLS=all` for the MCP server.

### Several commands on one host

Call `ssh_connect` once first. While the connection is retained, every
`ssh_exec` to that host opens a channel on it instead of a new connection —
about 240 ms → 30 ms per call on a local sshd, much more through a jump host —
and calls on it run in parallel. If the retained connection is unhealthy the
call connects for itself, so it is never slower or less reliable than not
retaining. `ssh_disconnect` when the series is done. This only applies to
`ssh_exec` in the process that made the connection (the MCP server or the
daemon); the CLI is one process per call and does not share it.

### With stdin

Pipe data into the remote command — the pipe is closed after writing so the
remote process sees EOF:

```bash
agent2ssh exec prod "cat > /tmp/config.json" --stdin '{"key":"value"}' --json
agent2ssh exec prod "wc -l" --stdin "$(cat local.txt)" --json
```

For anything large or newline-sensitive use `--stdin-file` — a path, or `-` for
the process's own stdin. `--stdin "$(cat f)"` drops trailing newlines and hits
the command-line length limit; `--stdin-file` does neither (UTF-8 text only):

```bash
cat big.sql | agent2ssh exec prod "psql mydb" --stdin-file - --json
```

### Exit status and failures (CLI)

Always pass `--json` when a program reads the result:

| Situation | Exit | stdout |
|-----------|------|--------|
| command ran, `--json` | 0 | the result; the remote status is `exit_code` |
| command ran, no `--json` | the remote command's own status | its stdout |
| Agent2SSH itself failed | 1 | `{"ok": false, "error": {"code", "message"}}` under `--json`, else empty |
| usage error | 2 | empty |

Without `--json` an exit status of 1 or 2 could be the remote command's or
Agent2SSH's own, so `--json` is how to tell: a failure is an object with
`"ok": false`, a result has no `ok`. `error.code` is the same set the MCP tools
report (`host_not_found`, `command_blocked`, `approval_required`, `timeout`,
`ssh_connect_failed`, …). `--max-output-bytes N` bounds what a command prints
back (head and tail kept) — worth setting before running anything chatty.

### Multi-host concurrent

Run the same command across a fleet simultaneously:

```bash
agent2ssh exec-multi web1 web2 web3 --command "systemctl status nginx" --json
# MCP: ssh_exec_multi  hosts: ["web1","web2","web3"]  command: "..."
```

Returns a per-host array with `result` or `error`.
