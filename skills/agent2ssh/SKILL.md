---
name: agent2ssh
description: Operate remote machines over SSH through Agent2SSH — host management, command execution with risk classification and approvals, SFTP transfers, persistent PTY sessions, port forwards, playbooks, audit, the local redaction / highlight / SSH-algorithm rule sets, and local execution on the daemon's own machine (one-shot `local_exec` environments and long-lived resident shell sessions). Use when asked to run commands on remote servers, transfer files, manage SSH hosts or tunnels, run a command in a local terminal environment, keep shell state across commands, tune what gets redacted or highlighted, or automate multi-host operations via the agent2ssh CLI or its MCP tools.
version: 0.4.4
---

# Agent2SSH Skill

Use this skill when you need to operate remote machines through Agent2SSH.

## Skill Catalog

Match the task to a capability, then drill into only that section (load-on-demand,
don't read the whole file for a single task):

| Task scenario | Capability | Section |
|---------------|-----------|---------|
| Run a command / script on a host | `ssh_exec` / `agent2ssh exec` | Command Execution |
| Same command across many hosts | `ssh_exec_multi` / `agent2ssh exec-multi` | Command Execution |
| Copy a file up/down | `ssh_sftp_upload` / `ssh_sftp_download` | SFTP File Operations |
| Browse / stat a remote path | `ssh_sftp_ls` / `ssh_sftp_stat` | SFTP File Operations |
| Interactive REPL / long-running shell | `ssh_session_*` | Persistent PTY Sessions |
| Expose a remote/local port | `ssh_forward_*` | Port Forwarding |
| Check reachability before batch ops | `ssh_ping` | Connectivity Check |
| Add / remove / import hosts | `ssh_add_host` / `host import-config` | Host Management |
| Generate a key that will actually authenticate | `agent2ssh keys generate` | SSH Keys |
| Review past executions | `ssh_audit` | Audit Log |
| Tune redaction / highlight / SSH algorithms | `ssh_redact_rule_*` / `ssh_highlight_rule_*` / `ssh_algo_prefs_*` | Local Configuration |
| Run a command on this machine, not over SSH | `local_exec` / `agent2ssh local exec` | Local Execution |
| Keep shell state (cwd, exports) across commands | `resident_session_open` / `_exec` / `_close` | Resident Sessions |
| High-risk command (sudo/rm/kill) | `force: true` + user confirmation | Risk Levels |

## Discovery Checklist

Before using Agent2SSH from an agent, discover the available local entry point:

```bash
command -v agent2ssh
command -v agent2ssh-mcp
```

If the binaries are not in `PATH` but the agent is running inside an Agent2SSH
source checkout, prefer the local debug binaries after a build:

```bash
test -x src-tauri/target/debug/agent2ssh && src-tauri/target/debug/agent2ssh --version
test -x src-tauri/target/debug/agent2ssh-mcp && src-tauri/target/debug/agent2ssh-mcp --version
```

For MCP clients, configure `agent2ssh-mcp` as a stdio server, then call
`tools/list` and look for `ssh_list_hosts`, `ssh_exec`, `ssh_sftp_ls`, and
`ssh_risk_check`. If `agent2ssh-mcp` is not in `PATH`, use its absolute path in
the client config.

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
| Run locally, one shot | `agent2ssh local exec --host local:vs --command "..."` / `local_exec` |
| Run locally, shell kept warm | `agent2ssh resident open/exec/close` / `resident_session_*` |

---

## Host Management

Before executing commands, confirm the host exists:

```bash
agent2ssh host list --json
```

Add a new host:

```bash
agent2ssh host add prod \
  --host 10.0.0.12 --user ubuntu --port 22 --key ~/.ssh/id_ed25519 --json
```

Import all hosts from `~/.ssh/config` at once:

```bash
agent2ssh host import-config --json
# MCP: ssh_import_config
```

Remove a host:

```bash
agent2ssh host rm prod --json
```

---

## SSH Keys

Key pairs live in `~/.agent2ssh/keys/` and are referenced by `key_path` on a host.

```bash
agent2ssh keys algorithm            # ed25519 or rsa — what this build can use
agent2ssh keys algorithm --name     # the same, as a bare word for scripts
agent2ssh keys algorithm --json     # adds bits + reads_openssh_private_keys
agent2ssh keys generate id-work     # prints the private key path, nothing else
agent2ssh keys generate id-work --type rsa --json
agent2ssh host add web1 --host 10.0.0.5 --key "$(agent2ssh keys generate web1)"
```

Three things matter before generating:

- **Ask, do not assume.** `keys algorithm` is the only correct source for the
  type, because it depends on what the SSH library can read rather than on the
  platform. Generating Ed25519 on a build that cannot read the OpenSSH container
  would produce a key that is stored, listed, and then rejected at
  authentication time. `keys generate` with no `--type` follows that
  recommendation on its own, which is what makes the one-liner above safe.
- **RSA here means traditional PEM.** 3072-bit, `-----BEGIN RSA PRIVATE KEY-----`
  (the shape `rsa` itself emits, which `russh` reads like any other format). The
  public half is still the standard single-line `ssh-rsa …` that
  `authorized_keys` expects. Sizes below 2048 are refused.
- **Replacing an existing name fails** rather than overwriting it, and the
  generated private key is restricted to its owner (0600). Deleting a key does
  not update hosts that reference it.

---

## Connectivity Check

Before batch operations, verify which hosts are reachable:

```bash
agent2ssh ping web1 web2 db1 --json
# → [{host, reachable, latency_ms, error}]
```

MCP: `ssh_ping` with `hosts` array.

---

## Command Execution

### Basic exec

```bash
agent2ssh exec prod "uname -a" --json
```

### With timeout (default 60 s)

```bash
agent2ssh exec prod "npm run build" --timeout-secs 120 --json
```

### With stdin

Pipe data into the remote command — the pipe is closed after writing so the
remote process sees EOF:

```bash
agent2ssh exec prod "cat > /tmp/config.json" --stdin '{"key":"value"}' --json
agent2ssh exec prod "wc -l" --stdin "$(cat local.txt)" --json
```

### Multi-host concurrent

Run the same command across a fleet simultaneously:

```bash
agent2ssh exec-multi web1 web2 web3 --command "systemctl status nginx" --json
# MCP: ssh_exec_multi  hosts: ["web1","web2","web3"]  command: "..."
```

Returns a per-host array with `result` or `error`.

---

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

---

## SFTP File Operations

### Transfer files

```bash
agent2ssh sftp put  prod ./build.tar.gz /opt/app/build.tar.gz --json
agent2ssh sftp get  prod /var/log/app.log ./app.log --json
```

### Browse the remote filesystem

```bash
agent2ssh sftp ls   prod /var/log           # list directory
agent2ssh sftp stat prod /etc/nginx.conf    # size, mode, mtime
agent2ssh sftp mkdir prod /opt/app/releases # mkdir -p
```

Use `--json` to get structured `ExecResult` output (stdout contains the command output).

---

## Persistent PTY Sessions

For interactive programs (Python REPL, mysql CLI, long-running shells):

```bash
# Open a session — returns a session_id UUID
ID=$(agent2ssh session open prod --json | jq -r .session_id)

# Send a command (include \n to submit)
agent2ssh session write "$ID" "python3\n"

# Read buffered output (waits up to timeout_ms for quiet)
agent2ssh session read "$ID" --timeout-ms 2000

# Send more input
agent2ssh session write "$ID" "print('hello')\n"
agent2ssh session read "$ID"

# Close when done
agent2ssh session close "$ID"
```

MCP tools: `ssh_session_open`, `ssh_session_write`, `ssh_session_read`,
`ssh_session_close`, `ssh_session_list`.

> **Note:** Sessions are process-local — they live only as long as the MCP
> server process. Always close sessions when finished.

---

## Port Forwarding

### Local forward (access a remote service locally)

```bash
# Forward localhost:5432 → prod:5432 (reach remote Postgres locally)
agent2ssh forward add prod --direction local \
  --bind-port 5432 --target-host localhost --target-port 5432 --json
```

### Remote forward (expose a local port on the remote)

```bash
agent2ssh forward add prod --direction remote \
  --bind-port 8080 --target-host localhost --target-port 3000 --json
```

```bash
agent2ssh forward list --json
agent2ssh forward rm <forward-id> --json
```

MCP tools: `ssh_forward_add`, `ssh_forward_list`, `ssh_forward_remove`.

---

## Audit Log

Review recent executions:

```bash
agent2ssh audit --limit 50 --json
agent2ssh audit --host prod --risk high --json
agent2ssh audit --exit-code 1 --json
```

MCP: `ssh_audit` with optional `host`, `risk_level`, `exit_code`, `since`, `until` filters.

---

## Local Configuration

Three independent rule sets live under `~/.agent2ssh/` and are reachable from
every surface — desktop Settings, `agent2ssh <group>`, and the `ssh_*` MCP tools:

| Rule set | Group | What it changes |
|----------|-------|-----------------|
| Sensitive-data redaction | `redact` | What gets scrubbed from audit records, webhook notifications, playbook results and diagnostic exports |
| Terminal highlight | `highlight` | What the terminal paints — never what the app writes |
| SSH algorithm preferences | `algo` | The algorithms offered in the next handshake |

```bash
agent2ssh redact list --json
agent2ssh redact add 'MY_SECRET_[0-9]+' '<scrubbed>' --json
agent2ssh highlight add 'ERROR' --name Errors --color '#FF6B6B' --json
agent2ssh highlight add 'a.b' --name Literal --color '#FFD060' --literal --json
agent2ssh algo set --kex curve25519-sha256 --json
```

Seven things to know before calling these:

- **Removal needs `--force` / `force: true`.** `redact remove` and
  `highlight remove` refuse an unconfirmed call — exit **2** on the CLI, HTTP
  **400** from the daemon and MCP — and change nothing. `... reset` restores the
  built-in set, which is the way back from a removal.
- **A rule's pattern or keyword is its identity.** Adding a duplicate fails
  rather than overwriting it — including when it duplicates a built-in.
- **Only `redact` validates the pattern.** `redact add` rejects a pattern that
  does not compile or that matches the empty string, before anything is written.
  `highlight` does not: its rule is compiled by the terminal renderer's
  JavaScript engine, so a Rust-side check would accept a different grammar.
- **`--literal` escapes at the call site**, so the text matches as plain text
  rather than as a regex. Every rule is stored as a regex, so `highlight list`
  does not distinguish the two, and renaming a literal rule needs its new
  keyword passed with `--literal` as well or it would be escaped twice.
- **`algo set` touches only the lists you name**; `algo clear` drops the
  overrides and falls back to the built-in safe defaults. A handshake already
  open keeps the algorithms it negotiated.
- **An algorithm this build cannot negotiate is accepted at save time and then
  fails every connection.** `algo set` checks the shape of a name, not whether
  the linked transport can do it, and the preferences are applied fail-closed
  immediately before each handshake. `russh` has no SHA-1 key exchange at all, so
  `--kex diffie-hellman-group14-sha1` saves fine and then every connection reports
  it as unsupported. The built-in defaults are narrowed to what the build can
  actually negotiate. `algo clear` is the way back. Ask `algo get` rather than
  assuming a list is portable.
- **Every key format the app can generate, it can also use.** The SSH transport
  is `russh`, whose parser reads the OpenSSH container, PKCS#1 and PKCS#8 alike,
  so a key written by `ssh-keygen` works without conversion — the old advice to
  run `ssh-keygen -p -m PEM` described a libssh2 limit that no longer exists.
  `keys algorithm` reports the type this build generates by default (Ed25519) and
  `keys generate` produces it; RSA is still available as a choice.
- **No OpenSSL, Perl or NASM on any platform.** The Windows build used to compile
  OpenSSL from source to escape WinCNG's MODP-only key exchange, which cost a C
  toolchain, a full Perl and an `OPENSSL_SRC_PERL` indirection. The transport now
  uses `ring`, so nothing in the default build links OpenSSL and there is no
  `libssl*.dll`/`libcrypto*.dll` to ship.

---

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

The environment rides in `host` — `local`, `local:<environment>` (`pwsh`,
`cmd`, `gitbash`, `vs`, `wsl[:<distribution>]`), a user-defined `local:<id>`
from `~/.agent2ssh/environments.toml`, or `local:<runtime>:<container-id>` for
capture exec inside a container. Because it is still a host id, scope,
approval host patterns and the audit log can each name it without learning a
new concept.

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

---

## Workflow Patterns

### Diagnose a server

```
1. ssh_ping → confirm reachable
2. ssh_exec "df -h && free -m && ps aux --sort=-%cpu | head" → overview
3. ssh_exec "journalctl -u myapp --since '5 min ago'" → recent logs
4. ssh_sftp_ls host /var/log/myapp → check log files
```

### Deploy an artifact

```
1. ssh_sftp_mkdir host /opt/releases/<version>
2. ssh_sftp_upload host ./dist.tar.gz /opt/releases/<version>/dist.tar.gz
3. ssh_exec host "tar -xzf /opt/releases/<version>/dist.tar.gz -C /opt/app" (medium risk)
4. ssh_exec host "systemctl restart myapp" --force true (high risk — confirm first)
```

### Run a migration on multiple DB replicas

```
1. ssh_ping → verify all reachable
2. ssh_exec_multi hosts command:"pg_dump -Fc mydb > /tmp/backup.dump" timeout_secs:300
3. Confirm backups exist with ssh_exec_multi "ls -lh /tmp/backup.dump"
4. ssh_exec_multi command:"psql mydb < migration.sql" force:true
```
