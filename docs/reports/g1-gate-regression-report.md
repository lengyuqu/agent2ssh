# G1 Gate Regression Report

Date: 2026-06-16

## Scope

G1 adds a daemon-level execution gate that can pause non-desktop execution while keeping the desktop recovery path available.

Covered entry points:

- `POST /exec`
- `POST /exec-multi`
- `POST /playbooks/run`
- `POST /sessions/:id/write`
- `GET /exec/stream`
- CLI `agent2ssh pause`, `agent2ssh resume`, `agent2ssh status`
- MCP read-only `ssh_gate_status`
- Desktop gate status indicator and pause/resume action

## Expected Behavior

- Default gate state is `active` when `execution_gate.json` is absent.
- `paused` state is persisted under the Agent2SSH config directory.
- In `paused` state, non-`desktop` daemon sources are rejected before SSH execution starts.
- HTTP execution entry points return 423 when rejected by the gate.
- Gate rejections append a `blocked` audit entry with the original source.
- Gate state changes publish `gate_changed`; rejected attempts publish `gate_rejected`.
- Desktop-sourced requests can still pause/resume and write sessions so the UI can recover from an emergency stop.

## Verification

Local verification performed:

```bash
npm run build
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh --bin agent2ssh-mcp
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features gate
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon gate
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test daemon_integration mcp_tool
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test daemon_integration mcp_call_tool_handler_covers_all_tools
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test cli_smoke mcp_stdio_end_to_end_initialize_tools_and_risk
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon -- --test-threads=1
git diff --check
```

Regression coverage:

- Core gate state persists and defaults to `active`.
- Daemon paused gate rejects non-`desktop` source with 423 and writes audit.
- Daemon paused gate allows `desktop` source for recovery.
- MCP `tools/list` and `docs/skills.md` both include 51 tools.
- MCP call handler covers `ssh_gate_status`.
- Browser UI check on `http://127.0.0.1:1420/` confirmed the topbar renders `Gate active` and `Pause` without overlap. Plain Vite browser mode cannot call Tauri `invoke`, so the button is disabled there as expected.

## Notes

MCP tool count is now 51 because `ssh_gate_status` was added. Current MCP contract documentation is `docs/skills.md`.
