# Agent2SSH MCP Tools Reference

Agent2SSH exposes 76 tools via the Model Context Protocol (MCP) stdio server. By default `tools/list` offers the 26 **core** tools and leaves the rest unlisted — see [Which tools are listed](#which-tools-are-listed).

## Which tools are listed

Every tool in a `tools/list` answer is paid for in the model's context on every turn of every session. All 76 are about 40 KB (~11k tokens); the 26 core tools are about 16 KB (~4.4k). So the server lists the **core** group unless told otherwise, and says so in the `instructions` it returns at `initialize` ("N more tools are not listed …") so the model does not take the short list for everything.

| Group | Tools | What is in it |
|-------|-------|---------------|
| `core` (always listed) | 26 | `ssh_list_hosts`, `ssh_add_host`, `ssh_exec`, `ssh_exec_multi`, `ssh_ping`, `ssh_risk_check`, `ssh_audit`, `ssh_sftp_ls`/`stat`/`mkdir`/`rename`/`rm`/`upload`/`download`, `ssh_session_*` (5), `ssh_connect`, `ssh_disconnect`, `ssh_connection_status`, `local_exec`, `resident_session_*` (3) |
| `ops` | 14 | `ssh_exec_compare`, `ssh_preview_exec`, `ssh_sftp_rmdir`, `ssh_sftp_rm_rf`, `ssh_forward_*`, `ssh_playbook_*`, `ssh_snippet_*`, `containers_start_service` |
| `admin` | 14 | `ssh_list_daemons`, `ssh_import_config`, `ssh_remove_host`, `ssh_gate_status`, `ssh_approval_*`, `ssh_webhook_config`, `ssh_config_*`, `ssh_sync_*` |
| `rules` | 13 | `ssh_redact_rule_*`, `ssh_highlight_rule_*`, `ssh_algo_prefs_*` |
| `diagnostics` | 9 | `ssh_audit_export`, `ssh_doctor`, `ssh_metrics`, `ssh_metrics_trend`, `ssh_health_snapshot`, `ssh_daemon_diagnose`, `ssh_daemon_version_check`, `ssh_daemons_view`, `ssh_events_subscribe` |

Set `AGENT2SSH_MCP_TOOLS` in the MCP server's environment: unset or `core` is the default, `all` lists everything, and a comma-separated list of group names (`rules,diagnostics`) adds those groups to core. An unknown word is ignored and named in the `instructions`.

**This is a listing choice, not an access rule.** Every tool stays callable by name whatever the setting, so a prompt or skill that names `ssh_audit_export` keeps working; only discovery is narrower. Authorization, approval and the gate apply to a hidden tool exactly as to a listed one.

**Annotations.** Each listed tool carries MCP `annotations` where they differ from the protocol's defaults (not read-only, destructive, not idempotent, open-world): `readOnlyHint: true` for tools that change nothing, `destructiveHint: false` for tools that only add, `idempotentHint: true`, `openWorldHint: false` for tools that touch only this machine's own state. A tool that can do anything — `ssh_exec` — carries none. The group and the hints for every tool live in one table, `src-tauri/src/bin/agent2ssh_mcp/catalog.rs`; `the_catalog_covers_every_tool` fails when a tool is missing from it.

## Daemon address

Every tool that reaches the local daemon does so through `local_daemon_url()`:
`http://127.0.0.1:7722` by default, the value of `AGENT2SSH_DAEMON_ADDR` when that
names another `host:port`, and loopback whenever the daemon is bound to a wildcard
(`0.0.0.0` / `::`). No tool hard-codes the address, so a daemon moved off the
default port stays reachable without reconfiguring the client. This applies to
every daemon-backed tool — `ssh_metrics`, `ssh_gate_status`, the approval and
event tools, `local_exec`, `resident_session_*` and `containers_*` alike — not to
`ssh_metrics` alone.

## Tool List

The numbering below matches the order returned by `tools/list` in
`src-tauri/src/bin/agent2ssh_mcp/tools.rs`, which is the contract enforced by
the `mcp_tools_match_skills_md_documentation` integration test.

| # | Tool | Description |
|---|------|-------------|
| 1 | `ssh_list_hosts` | List configured SSH host profiles. |
| 2 | `ssh_list_daemons` | List all configured daemons (localhost + remote daemons from `~/.agent2ssh/remotes.toml`). Returns alias, URL, and connected status. |
| 3 | `ssh_import_config` | Import SSH host profiles from `~/.ssh/config` (or a custom path). Skips aliases that already exist. |
| 4 | `ssh_add_host` | Create or update an SSH host profile. |
| 5 | `ssh_remove_host` | Remove a configured SSH host profile by alias. |
| 6 | `ssh_exec` | Run a non-interactive command over SSH. Returns stdout, stderr, exit code, timing, and risk level. High-risk commands require `force: true`; blocked commands always fail. Optionally forward to a remote daemon via `daemon_alias`. |
| 7 | `ssh_ping` | Check SSH reachability of one or more hosts. Returns reachable status and latency for each. |
| 8 | `ssh_exec_multi` | Run the same command on multiple hosts concurrently. Returns an array of per-host results. Supports optional batch strategy for concurrency limits, failure thresholds, and batched rollout. Fails when the requested hosts or tags match no configured host. |
| 9 | `ssh_exec_compare` | Compare execution results across multiple hosts. Groups by exit code and highlights stdout/stderr differences. Provide either results directly or run a command on multiple hosts. Fails when the multi-host selection matches no configured host. |
| 10 | `ssh_audit` | Return recent SSH execution audit log entries with optional filtering. |
| 11 | `ssh_audit_export` | Export audit log entries as JSONL or CSV with optional filtering. Redaction is applied at write time so exported data preserves redaction. |
| 12 | `ssh_sftp_ls` | List a remote directory via embedded SFTP. |
| 13 | `ssh_sftp_stat` | Stat a remote file or directory via embedded SFTP. |
| 14 | `ssh_sftp_mkdir` | Create a remote directory recursively via embedded SFTP. |
| 15 | `ssh_sftp_rename` | Rename or move a remote file or directory via embedded SFTP. Rated medium risk. |
| 16 | `ssh_sftp_rm` | Delete a single remote file via embedded SFTP. Rated low risk, but host approval policies may still require approval. |
| 17 | `ssh_sftp_rmdir` | Remove an empty remote directory via embedded SFTP. Fails when the directory is not empty; use `ssh_sftp_rm_rf` for a recursive delete. Rated low risk. |
| 18 | `ssh_sftp_rm_rf` | Recursively delete a remote directory tree (the remote `rm -rf`). Rated high risk, and blocked outright when the target is `/`, `/*` or `/.`. This cannot be undone. |
| 19 | `ssh_sftp_upload` | Upload a local file to a remote host via embedded SFTP. |
| 20 | `ssh_sftp_download` | Download a file from a remote host via embedded SFTP. |
| 21 | `ssh_session_open` | Open a persistent interactive PTY session. Returns a `session_id` for subsequent write/read/close calls. |
| 22 | `ssh_session_write` | Send input to an open PTY session (e.g. a command followed by `\n`). |
| 23 | `ssh_session_read` | Read buffered output from a PTY session. Returns whatever arrived since the last read. |
| 24 | `ssh_session_close` | Close and terminate a PTY session. |
| 25 | `ssh_session_list` | List open PTY sessions, merging the local daemon registry with MCP process-local fallback sessions. Every row carries `backend`: `daemon` for a session the daemon owns (it outlives this MCP process), `process` for one living in this process and dying with it — the same distinction `durable` draws for forwards, and the only signal that tells the two kinds apart. |
| 26 | `ssh_forward_add` | Start an SSH port forward tunnel (`-L` local or `-R` remote). Returns a `forward_id`. |
| 27 | `ssh_forward_list` | List active SSH port forward tunnels. Each entry carries `durable`: `true` when the daemon owns it (it outlives this client), `false` when it lives in the MCP server's own process and ends with it. |
| 28 | `ssh_forward_remove` | Stop and remove an SSH port forward by ID. |
| 29 | `ssh_risk_check` | Check the risk level of a command using built-in rules plus user rules. Those user rules come from `policy.toml` (or `policy.json`) when either exists, and only otherwise from the legacy `risk_rules.toml`: a policy file present at all means `risk_rules.toml` is never read, so that is the file to edit when you have one. |
| 30 | `ssh_gate_status` | Read the local daemon execution gate status. When paused, non-desktop daemon execution is rejected until resumed from CLI or desktop. |
| 31 | `ssh_approval_list` | List all pending and recent approval requests (for high-risk command authorization). |
| 32 | `ssh_approval_respond` | Approve or reject a pending approval request by ID. |
| 33 | `ssh_playbook_list` | List all configured playbooks from `~/.agent2ssh/playbooks.toml`. |
| 34 | `ssh_playbook_run` | Run a named playbook (sequence of SSH commands) against a target host. Steps execute sequentially; halts on first failure. Supports template parameters via the `params` object. |
| 35 | `ssh_playbook_dry_run` | Preview a playbook without executing. Resolves template parameters and returns the commands that would be run. |
| 36 | `ssh_snippet_list` | List reusable command snippets from `~/.agent2ssh/snippets.json`. |
| 37 | `ssh_snippet_save` | Create or replace a reusable command snippet without executing it. |
| 38 | `ssh_snippet_delete` | Delete a reusable command snippet by name without executing it. |
| 39 | `ssh_connection_status` | List all configured hosts and their current embedded SSH connection status. |
| 40 | `ssh_connect` | Manually establish and retain an embedded SSH connection to a specific host. |
| 41 | `ssh_disconnect` | Manually close a retained embedded SSH connection to a specific host. |
| 42 | `ssh_webhook_config` | Get or set webhook notification configuration. Use `action: "get"` to retrieve current config, or `action: "set"` with `url`/`events`/`secret` to update. |
| 43 | `ssh_config_export` | Export team configuration (hosts without private key paths, risk rules, and playbooks). Returns a JSON object suitable for sharing within a team. |
| 44 | `ssh_config_import` | Import team configuration from a JSON object. Merges hosts (skips duplicates by name), and overwrites risk rules and playbooks if provided. |
| 45 | `ssh_config_import_preview` | Preview what a team config import will change without actually importing. Shows hosts to add, skip, update, and risk rules/playbook changes. |
| 46 | `ssh_doctor` | Run diagnostic checks on the agent2ssh environment: embedded SSH/keygen capability, config directory, `hosts.json`, daemon token permissions, daemon health, optional config files, and audit log size. |
| 47 | `ssh_metrics` | Retrieve basic metrics from the local agent2ssh daemon (requests, execs, blocked commands, durations, approvals). Reads from `GET /metrics` on the local daemon — see [Daemon address](#daemon-address). |
| 48 | `ssh_preview_exec` | Preview what an execution will do before running it. Returns target hosts, commands, risk levels, warnings, and whether approval is required. Supports single-host and multi-host preview. Fails when a multi-host selection matches no configured host, matching `ssh_exec_multi`. The `requires_approval` field answers the risk question alone — `true` only for a high-risk command — and never consults approval policies, so `false` does not mean `/exec` will run without an approval. |
| 49 | `ssh_approval_policies_list` | List all configured approval policies. Each policy specifies when approval is required based on host, tags, risk level, and command pattern. |
| 50 | `ssh_approval_check` | Check whether running a command on a specific host requires approval based on configured policies. Returns the matching policy name and whether approval is needed. `false` means no policy demands an approval, not that `/exec` will run unapproved: a high-risk command still needs `force` or an approval, and this check takes no `force` to account for it. |
| 51 | `ssh_health_snapshot` | Collect a health snapshot (uptime, disk, memory, load, SSH latency) for configured hosts. Returns per-host data collected concurrently via SSH. |
| 52 | `ssh_daemon_diagnose` | Run connection diagnostics on a remote daemon: TCP connectivity, TLS handshake, token configuration, authentication, version compatibility, latency. Returns a detailed report. |
| 53 | `ssh_daemon_version_check` | Check version compatibility between this build and a remote daemon. Returns local version, remote version, compatibility status, and a human-readable message. |
| 54 | `ssh_daemons_view` | Get a unified view of all daemons (localhost + remotes) with their health, metrics, and host counts. |
| 55 | `ssh_metrics_trend` | Show execution metrics trends: volume, failure rate, risk distribution, top hosts, hourly breakdown. Supports `24h` / `7d` / `30d` / `all` period selection. |
| 56 | `ssh_events_subscribe` | Subscribe to the event bus. Returns an `events` array that is empty in practice: the call creates a fresh `broadcast` receiver and drains it a few statements later in the same synchronous call, so the only events it can see are those published inside that window — a new receiver gets no backlog. Use the daemon's SSE endpoint `GET /events/stream` for actual events. |
| 57 | `ssh_sync_diff` | Compare Agent2SSH hosts with `~/.ssh/config`. Shows hosts only on one side and conflicts. |
| 58 | `ssh_sync_export` | Export Agent2SSH hosts to SSH config format file (default `~/.ssh/config.d/agent2ssh.conf`). MCP explicit paths must stay directly inside `~/.ssh/config.d/`. |
| 59 | `ssh_redact_rule_list` | List the sensitive-data redaction rules this instance applies before it writes an audit record, webhook notification, playbook result or diagnostic export. Each rule is marked when it is one of the built-in defaults. Read-only. |
| 60 | `ssh_redact_rule_add` | Add a redaction rule. A rule's pattern is its identity, so adding a pattern that is already present fails instead of overwriting it, and an invalid regex is rejected before anything is written. The replacement is literal. |
| 61 | `ssh_redact_rule_update` | Replace the redaction rule identified by `pattern`. Omit `new_pattern` to keep the current pattern, which is the usual case of changing only the replacement. |
| 62 | `ssh_redact_rule_remove` | Remove a redaction rule. Requires `force: true`, because it stops that class of secret being redacted everywhere this instance writes; a call without it is rejected and changes nothing. `ssh_redact_rule_reset` restores the built-in set. |
| 63 | `ssh_redact_rule_reset` | Restore the built-in redaction rule set, discarding every edit. This is the way back from a removal, because it only ever adds rules back. |
| 64 | `ssh_highlight_rule_list` | List the terminal highlight rules the UI paints matches with. Read-only. |
| 65 | `ssh_highlight_rule_add` | Add a terminal highlight rule. A rule's keyword is its identity, so a duplicate fails instead of overwriting. Rules are stored as regular expressions: pass `literal: true` to escape the keyword first so it matches as plain text. |
| 66 | `ssh_highlight_rule_update` | Replace the terminal highlight rule identified by `keyword`. Every field you omit keeps its current value. `literal` escapes `new_keyword` and is meaningful only together with it. Changes only what the terminal paints, never what the app writes. |
| 67 | `ssh_highlight_rule_remove` | Remove a terminal highlight rule. Requires `force: true` so a mistyped keyword cannot quietly drop a rule you meant to keep. `ssh_highlight_rule_reset` restores the built-in set. |
| 68 | `ssh_highlight_rule_reset` | Restore the built-in terminal highlight rules, discarding every edit. |
| 69 | `ssh_algo_prefs_get` | Show the SSH algorithm preferences in effect for the next handshake, whether they are an override or the built-in defaults, and the defaults they would fall back to. Read-only. |
| 70 | `ssh_algo_prefs_set` | Replace one or more SSH algorithm lists; the ones you leave out keep their current value. Each list is checked before it is stored, so a list that could not complete a handshake is rejected rather than written. Takes effect on the next handshake. |
| 71 | `ssh_algo_prefs_clear` | Drop the SSH algorithm overrides and go back to the built-in safe defaults. |
| 72 | `local_exec` | Run one command on the machine the daemon runs on, in a one-shot terminal environment: `local`, `local:<environment>` for a named shell of this machine's OS (Windows: `local:pwsh`, `local:cmd`, `local:gitbash`, `local:vs`, `local:wsl[:<distribution>]`; macOS: `local:zsh`, `local:bash`, `local:sh`, `local:pwsh`; Linux: `local:bash`, `local:sh`, `local:dash`, `local:zsh`, `local:pwsh`; or an id you defined in `environments.toml`) or `local:<runtime>:<container-id>` (`local:docker:abc123`). Forwards to the daemon, so the same gate chain as SSH exec applies (scope, risk, approval). For a shell whose state persists across commands use `resident_session_open`. |
| 73 | `resident_session_open` | Open a resident session: one shell for a `local:<id>` environment (a built-in id, or one you defined in `environments.toml`) that stays alive across commands, so cwd and exported variables persist. Blocks until the shell is warm. Forwards to the daemon; the open is authorized like an exec keyed on the environment's host id. Containers are refused (one-shot targets). |
| 74 | `resident_session_exec` | Run one command inside a resident session opened by `resident_session_open`. Authorized against the session's own host id, so it pays the same gate chain as a one-shot exec. Output is bounded by the session's rolling window. A command that outlives its timeout kills the session — long-running work belongs in `local_exec`. |
| 75 | `resident_session_close` | Close a resident session and kill its shell. Authorized as `resident-session close` against the session's own host through scope, execution gate, limits, risk and approval. Closing an already-dead session still reclaims it. To read a session's buffered output without running a command, use `GET /resident-sessions/{id}` on the daemon, authorized as `resident-session read`. |
| 76 | `containers_start_service` | Start the local container runtime's background service — the step a stopped runtime needs before discovery or exec can work. Apple container is the runtime with one; Docker and wslc have none and are refused rather than silently accepted. Reports `started`: `false` means the service was already up, so nothing was spawned. Local to the caller, like the CLI command: the service it starts is on this machine, so it does not need a daemon. |

The local-execution tools above wait for the daemon's execution, shell-warmup and approval deadlines, rather than imposing a separate 30-second HTTP timeout. Connecting to the daemon is limited to 10 seconds in every case; command `timeout_secs` still applies. The session, forward and gate tools (`ssh_session_*`, `ssh_forward_*`, `ssh_gate_status`) use a second client that also caps the whole request at 30 seconds, so a daemon which accepts the connection and then stalls fails them at that cap rather than at any command deadline.

## Result contract

How every tool answers, and what a client can rely on.

**Success.** `content` holds one text block and `structuredContent` holds the same data as JSON. For the three tools that run one command — `ssh_exec`, `local_exec`, `resident_session_exec` — the text is a header line followed by the output as the command printed it:

```text
exit_code: 2 | duration_ms: 41 | risk_level: low | host: web1

[stdout]
...
[stderr]
...
```

A non-zero `exit_code` is a normal result: the tool worked, the command reported failure. Every other tool answers with compact JSON text.

**`structuredContent` is always an object**, as the protocol defines it — a strict client validates it as one and rejects the whole result otherwise. A tool whose natural answer is a list (`ssh_list_hosts`, `ssh_audit`, `ssh_ping`, `ssh_session_list`, `ssh_doctor`, …) or a bare value comes back as `{"result": [...]}`; the `content` text stays the list itself.

**Protocol revision.** `initialize` answers with the revision the client asked for when the server speaks it (`2025-06-18`, `2025-03-26`, `2024-11-05`) and `2025-06-18` otherwise, so the client can decide. What the server sends beyond the oldest — `structuredContent`, `isError`, tool `annotations`, `instructions` — is additive and ignored by a client that does not know it.

**Output size.** Each `stdout` / `stderr` — in a single result, in every per-host result of `ssh_exec_multi`, in every playbook step — is bounded to `max_output_bytes` (default 32768) before it is returned. A longer stream keeps its head and tail around a `[... N bytes omitted; full stdout (M bytes) saved to <path> ...]` marker, and the result gains `<stream>_total_bytes` and `<stream>_file`. The file is on the machine the MCP server runs on, under `~/.agent2ssh/outputs/` (override with `AGENT2SSH_MCP_OUTPUT_DIR`), owner-readable only, and removed after seven days. `AGENT2SSH_MCP_MAX_OUTPUT_BYTES` changes the default bound for a whole client. The capture behind it is still bounded at 4 MiB per stream; `truncated` / `dropped_bytes` report that outer bound, as before.

**Failure.** A tool that fails answers with a result whose `isError` is `true` — not a JSON-RPC error — so the model reads it and can act. The text starts `Error [<code>]: <message>`, then `Hint: <next step>` when there is one. `structuredContent.error` carries `code`, `message` and `hint`.

| Code | Meaning | What to do |
|------|---------|------------|
| `invalid_arguments` | A required argument is missing or has the wrong shape | Fix the call |
| `host_not_found` | No host profile has that alias | The hint lists the configured aliases |
| `force_required` | High-risk command without `force` | Confirm with the user, then retry with `force: true` |
| `command_blocked` | Blocked by risk policy | Do not retry |
| `approval_required` | An approval policy applies and this path cannot prompt | Ask the user |
| `approval_rejected` / `approval_timed_out` | A person said no, or nobody answered | Ask the user before retrying |
| `policy_denied` | Outside the caller's scope | Do not retry |
| `gate_paused` | A person paused execution globally | Do not retry; ask the user to resume it |
| `rate_limited` | An execution rate or session limit is in force | Wait, or run fewer commands at once |
| `exec_timeout` | The command outlived `timeout_secs` | See below |
| `ssh_auth_failed` / `ssh_connect_failed` / `ssh_host_key_mismatch` | The connection could not be made | Check the profile; `ssh_ping` tests reachability |
| `daemon_unavailable` | A daemon-only tool with no daemon running | `agent2ssh daemon start` |
| `daemon_error` | The connection to the daemon dropped after the request went out | The command may have run: check `ssh_audit` before retrying |
| `binding_invalid` | The MCP binding did not verify; no tool runs | `agent2ssh integrate add <client>`, restart the server |
| `timeout` / `tool_error` | Any other timeout / any other failure | Read the message |

**Timeouts keep the output.** An `exec_timeout` result carries what the command printed before the deadline — `[stdout before the timeout]` / `[stderr before the timeout]` in the text, `stdout` / `stderr` / `timed_out` / `process_stopped` in `structuredContent`. `process_stopped` is `false` for an SSH host: closing the channel does not stop the remote process, so it may still be running. It is `true` for a local target, whose process tree is killed at the deadline.

**Protocol errors.** Only a request the server cannot dispatch is a JSON-RPC error: an unknown method (`-32601`), an unknown tool or missing `params` (`-32602`), a line that is not JSON (`-32700`, after which the server keeps serving).

**Concurrency and cancellation.** `tools/call` requests run concurrently, so a slow command does not hold up other calls; responses arrive as each finishes, matched by `id`. `notifications/cancelled` with a `requestId` drops that call's response. Work already running on an SSH host is not interrupted by a cancellation — it ends at its own `timeout_secs` and is audited as usual.

## Risk Levels

Commands are classified into four risk levels:

- **low** — safe read-only commands (ls, cat, whoami, etc.)
- **medium** — commands that modify state (apt install, git push, sed -i, etc.)
- **high** — potentially destructive commands (sudo, rm -rf, chmod 777, etc.)
- **blocked** — unconditionally dangerous commands (mkfs, rm -rf /, shutdown, fork bomb, etc.)

High-risk commands require daemon approval or `force: true` when policy allows. `ssh_exec` and `ssh_exec_multi` hand the work to the **local daemon** whenever one is running, so an approval policy raises a card a person can answer in the desktop app, and the execution gate and rate limits apply; `daemon_alias` names a *remote* daemon instead. Only when the local daemon cannot be reached at all (no token file, or the connection is refused) does the exec run inside the MCP process, which has no approval handler: a policy match then fails closed with `approval_required`. A connection that drops *after* the request reached the daemon is never retried in process — the command may already have run — and answers `daemon_error` with a pointer to `ssh_audit`. `ssh_connect`, `ssh_disconnect` and `ssh_connection_status` follow the same rule, so a connection retained for later `ssh_exec` calls lives in the process those calls run in. Blocked commands are always rejected.

## Execution Authorization

MCP exec, exec-multi, playbook, SFTP, session open/write, forward add, connect, and disconnect operations use the shared Agent2SSH authorization path where applicable:

- daemon or remote-token scope is checked before approval
- effective risk combines built-in rules and user policy rules
- user policy rules can only escalate built-in risk
- host/playbook `risk_override` can lower or raise non-blocked risk
- rejected attempts are written to the audit log with the MCP source

## Approval Flow

When the daemon is running, high-risk commands can be routed through an approval gate:

1. MCP `ssh_exec` or another daemon-routed operation with high-risk work creates an approval request. The MCP call stays open while the card is pending and returns when it is approved (the result), rejected (`approval_rejected`) or times out (`approval_timed_out`)
2. Query pending approvals with `ssh_approval_list`
3. Approve or reject with `ssh_approval_respond`

## Policy-as-Code (Recommended)

Define unified security policies in `~/.agent2ssh/policy.toml`. This file supports risk rules and approval policies in one versionable place. Execution gate, execution limits, and anomaly detection use their own files: `execution_gate.json`, `execution_limits.toml`, and `anomaly.toml`.

```toml
[risk.blocked]
patterns = ["kubectl delete namespace*", "terraform destroy*"]

[risk.high]
patterns = ["docker system prune*", "git push *force*"]

[risk.medium]
patterns = ["apt install*", "yum install*"]

[[approval.policies]]
name = "production high risk"
tags = ["production"]
min_risk = "high"
requires_approval = true
ttl_secs = 300

[[approval.policies]]
name = "sandbox auto approve"
hosts = ["sandbox"]
requires_approval = false
```

All risk rules support glob patterns with `*`. User rules are merged with built-in classification and can only raise risk; they cannot downgrade an internally classified high or blocked command.

> Legacy `~/.agent2ssh/risk_rules.toml` is still supported for backward compatibility, but `policy.toml` is the recommended approach for new deployments.

## Per-Host Risk Override

Set `risk_override` on a host profile to override the risk level for non-blocked commands on that host. For example, setting `risk_override: "low"` on a sandbox host lowers non-blocked commands before approval checks. Commands classified as `blocked` by built-in or user-defined rules are never downgraded by overrides.

## SSH Connection Retention

Agent2SSH can retain embedded SSH sessions for faster connection checks and preconnect workflows:

- Use `ssh_connection_status` to see which hosts have active connections
- Use `ssh_connect` to pre-establish a connection, `ssh_disconnect` to tear one down
- Jump-host connections use the same embedded direct-tcpip bastion channel as exec, SFTP, terminal, sessions, and forwards

**`ssh_exec` reuses a retained connection.** After `ssh_connect` to a host, every `ssh_exec` to it in the same process opens a channel on that connection instead of a new one — no TCP connect, key exchange or authentication, and for a host behind a jump host none of the chain either. Measured against a local sshd, a call went from about 240 ms to about 30 ms; the saving grows with network latency and with every jump host. The rules:

- **Opt-in.** Only a host that was explicitly retained is ever reused; a host that was never `ssh_connect`ed behaves exactly as before. Retained connections are process-local, so this applies to the process that ran `ssh_connect` (the MCP server, the daemon, the desktop app) — separate CLI invocations never share one.
- **Concurrent.** Calls on one retained connection run in parallel as separate channels. A call that times out closes only its own channel and never takes the connection down for the others.
- **Never worse than before.** If the retained connection is unhealthy, reconnecting, made for a host profile that has since changed (address, port, user, key, jump host, proxy), cannot open a channel within 5 seconds, or is already at the server's channel limit (`MaxSessions`, 10 on OpenSSH by default), that call quietly connects for itself. This is decided before the command is sent, so a command is never run twice.
- **A timed-out command still occupies a server-side session slot** until it exits, because closing a channel does not stop a pty-less remote process (see `timeout_secs`). Many timed-out long-running commands on one retained connection therefore exhaust its slots; later calls fall back to their own connections until the commands end. Wrap long work in `timeout <secs>` on the remote side.

SFTP, PTY sessions, port forwards and the other tools still open connections of their own.

## Webhook Notifications

Configure `~/.agent2ssh/webhook.toml` to receive event notifications:

```toml
url = "https://example.com/agent2ssh-webhook"
events = ["approval_required", "exec_blocked", "exec_completed"]
secret = ""  # Optional HMAC-SHA256 signing key
```

When the URL contains `hooks.slack.com`, messages are automatically formatted as Slack Block Kit messages. Use `ssh_webhook_config` to get or set configuration programmatically.

## Playbooks

Define reusable command sequences in `~/.agent2ssh/playbooks.toml`:

```toml
[[playbooks]]
name = "deploy-web"
description = "Pull latest code and restart web service"
steps = [
  "cd /opt/app && git pull",
  "systemctl restart nginx",
]
tags = ["production", "web"]
risk_override = "high"
```

Use `ssh_playbook_list` to list all playbooks, `ssh_playbook_run` to execute one on a target host. Steps run sequentially and halt on first failure. Provide `reason` and `change_id` when the resulting audit entries need operational context.

## Remote Daemon

Connect to agent2ssh-daemon instances running on other machines via `~/.agent2ssh/remotes.toml`:

```toml
[[remotes]]
alias = "ci-server"
url = "http://192.168.1.100:7722"
token_env = "AGENT2SSH_PROD_TOKEN"

[remotes.scope]
allowed_tags = ["staging"]
allowed_commands = ["uptime", "df *", "git *"]
denied_commands = ["rm *"]
```

Use `ssh_list_daemons` to see configured daemons with connectivity status. Pass `daemon_alias` to `ssh_exec` to route commands through a remote daemon. The CLI supports `--daemon <alias>` as a global flag. `remotes.toml` scope is enforced client-side before forwarding; the remote daemon can also enforce server-side scoped tokens via `daemon_tokens.toml`.
