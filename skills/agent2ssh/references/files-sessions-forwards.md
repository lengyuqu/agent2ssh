# Files, interactive sessions and port forwards

Part of the agent2ssh skill. `SKILL.md` has the overview and the rules that always
apply; this file is the detail for one area.

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

> **Note:** When the Agent2SSH daemon is running, sessions are held by the
> daemon — `ssh_session_list` shows them and the desktop app can see them. With
> no daemon, an MCP session falls back to the MCP server process and ends with
> it. Either way, close sessions when finished.

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
