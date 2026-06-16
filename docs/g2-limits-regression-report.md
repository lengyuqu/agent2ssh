# G2 Limits Regression Report

Date: 2026-06-16

## Scope

G2 adds daemon-enforced execution limits:

- Per-source execution rate limit
- Per-host execution rate limit
- Per-tag execution rate limit
- Per-source max concurrent sessions
- Per-host max concurrent sessions
- Per-tag max concurrent sessions

Configuration is loaded from:

```text
~/.agent2ssh/execution_limits.toml
```

If the file is absent, daemon uses conservative defaults. `per_minute = 0` and `max_sessions = 0` disable that specific dimension.

## Enforced Paths

- `POST /exec`
- `POST /exec-multi`
- `POST /playbooks/run`
- `POST /sessions`
- `POST /sessions/:id/write`
- `GET /exec/stream`
- `POST /daemons/localhost/exec`

## Expected Behavior

- Execution rate limits use an in-memory sliding window.
- Session concurrency limits use daemon session registration.
- Over-limit HTTP requests return 429.
- Over-limit requests append a `blocked` audit entry with the source.
- Over-limit requests publish `limit_rejected` to the daemon event bus.
- Closing a daemon-managed session releases its session limit slot.

## Verification

Local verification performed:

```bash
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh --bin agent2ssh-mcp
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features limits
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon limit
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon -- --test-threads=1
git diff --check
```

Regression coverage:

- Source rate limit rejects the second operation in the same window.
- Host-specific rule overrides the default rate limit.
- Host session limit rejects new sessions when the max is reached.
- Daemon rejection path returns 429 and writes blocked audit.
