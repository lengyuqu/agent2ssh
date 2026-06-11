# Agent2SSH Skill

Use this skill when you need to operate a remote machine through Agent2SSH.

## Prefer CLI JSON

Use the CLI with `--json` whenever possible so outputs are structured and auditable.

```bash
agent2ssh host list --json
agent2ssh exec <host> "<command>" --json
```

## Host Discovery

Before executing a command, list configured hosts:

```bash
agent2ssh host list --json
```

If the requested host is missing, ask the user for host, user, port, and key path, or add it if those values are already available:

```bash
agent2ssh host add prod --host 10.0.0.12 --user ubuntu --port 22 --key ~/.ssh/id_ed25519 --json
```

To remove a host that is no longer needed:

```bash
agent2ssh host rm prod --json
```

## Command Execution

Use one non-interactive command for simple tasks:

```bash
agent2ssh exec prod "uname -a" --json
```

Keep commands explicit and bounded. Prefer:

```bash
agent2ssh exec prod "cd /opt/app && git status --short" --json
```

Avoid interactive commands until persistent PTY sessions are available.

## Risk Handling

Pause and request approval before running commands involving:

- `sudo`
- `rm`, `mv` over system or production paths
- `chmod` or `chown` over broad paths
- service restarts in production
- database writes or migrations
- firewall, disk, boot, shutdown, or reboot operations

Explain the intended effect and the exact command before asking for approval.

## Audit

Review recent executions with:

```bash
agent2ssh audit --limit 20 --json
```
