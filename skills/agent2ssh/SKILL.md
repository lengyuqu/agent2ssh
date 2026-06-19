# Agent2SSH Skill

Use this skill when you need to operate remote machines through Agent2SSH.

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
| Persistent session | `agent2ssh session open/write/read/close` / `ssh_session_*` |
| Port forward | `agent2ssh forward add/list/rm` / `ssh_forward_*` |
| View audit log | `agent2ssh audit --json` / `ssh_audit` |

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
