# Local execution and resident sessions

Part of the agent2ssh skill. `SKILL.md` has the overview and the rules that always
apply; this file is the detail for one area.

## Local Execution

Not every command belongs on a remote host. `local_exec` runs one command on
the machine the daemon itself runs on, in a named terminal environment, and
returns the same bounded-capture shape as an SSH exec (`stdout`, `stderr`,
`exit_code`, `truncated`, `dropped_bytes`):

```bash
agent2ssh local exec --host local            "uname -a" --json
agent2ssh local exec --host local:pwsh       "Get-Process | Select -First 5" --json
agent2ssh local exec --host local:vs         "where.exe cl" --json
agent2ssh local exec --host local:wsl:Ubuntu "cd /tmp && pwd" --json
agent2ssh local exec --host local:docker:abc123 "cat /etc/os-release" --json
# MCP: local_exec  host: "local:gitbash"  command: "..."
```

The environment rides in `host` — `local`, `local:<environment>` for a shell
of this machine's OS (Windows: `pwsh`, `cmd`, `gitbash`, `vs`,
`wsl[:<distribution>]`; macOS: `zsh`, `bash`, `sh`, `pwsh`; Linux: `bash`,
`sh`, `dash`, `zsh`, `pwsh`), a user-defined `local:<id>` from
`~/.agent2ssh/environments.toml`, or `local:<runtime>:<container-id>` for
capture exec inside a container. Because it is still a host id, scope,
approval host patterns and the audit log can each name it without learning a
new concept. The examples above that name `pwsh`, `vs` or `wsl` are Windows
shells.

Three things to know:

- **It is authorized like an SSH exec, not like a local shell.** The daemon
  runs the same preflight chain (scope, gate, limits, risk, approval) keyed on
  the environment's host id. Only the daemon can prompt for approval, so this
  route fails closed from a CLI or MCP process that has no daemon.
- **A timed-out command is reaped, not abandoned.** The whole process group is
  killed — unlike the SSH path, where a pty-less exec channel cannot be reaped
  remotely.
- **A request carrying `stdin` is refused for container targets** rather than
  silently dropped, because the capture exec binds no interactive stdin.

---

## Resident Sessions

When the point is that shell state survives between commands — a Visual Studio
developer shell whose bootstrap costs seconds, a `cd` you want to keep, an
exported variable — open a resident session instead of paying that cost per
command:

```bash
agent2ssh resident open --host local:vs --json    # blocks until the shell is warm
agent2ssh resident exec <session-id> "where.exe cl" --json
agent2ssh resident exec <session-id> "cd /src && export A2S=1" --json
agent2ssh resident list --json
agent2ssh resident close <session-id>
# MCP: resident_session_open / resident_session_exec / resident_session_close
```

The daemon keeps one shell alive per session and writes each command to its
stdin, followed by a sentinel line carrying a per-command nonce and the exit
code. What that buys and what it costs:

- **State persists by design** — cwd, exported variables and the warmed
  environment survive across commands, which is the whole reason to use one.
- **A command that outlives its timeout kills the session.** A shell without a
  console cannot be interrupted, so honesty beats a fake resume; long-running
  work belongs in `local_exec`, which reaps its own process tree.
- **A command that exits the shell kills the session too**, and reports the
  shell's exit code — `exit` is an authorized command that does what it says.
- **Open and every command pay the same preflight chain** as a one-shot exec,
  keyed on the session's own `local:<id>` host. Containers are refused at open:
  they are one-shot exec targets.
- **State is daemon-owned.** CLI, MCP and the desktop panel are thin clients
  over `/resident-sessions`; there is deliberately no process-local fallback, so
  a call with no daemon running fails with an error naming what to start.

For a command backgrounded inside the shell (`(sleep 1; ls) &`), read the
session's rolling buffer without consuming it — this consumes nothing, counts
no idle activity, and reports `dead` for a shell that exited on its own, so a
collector can stop polling before the idle reaper runs:

```bash
curl -H "Authorization: Bearer $(cat ~/.agent2ssh/daemon.token)" \
  http://127.0.0.1:7722/resident-sessions/<session-id>
```

Bounds: `AGENT2SSH_MAX_RESIDENT_SESSIONS` (default 8) and
`AGENT2SSH_RESIDENT_IDLE_TTL_SECS` (default 600, reaped by a daemon sweep).
