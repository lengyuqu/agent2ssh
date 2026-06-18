# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Agent2SSH is a local SSH capability layer for general-purpose agents. One Rust core is exposed through four binaries plus a desktop UI:

- `agent2ssh-app` — Tauri desktop app (React/TS frontend in `src/`, Rust commands in `src-tauri/src/tauri_commands.rs`)
- `agent2ssh` — CLI (`src-tauri/src/bin/agent2ssh.rs`)
- `agent2ssh-mcp` — stdio MCP server, 51 tools (`src-tauri/src/bin/agent2ssh-mcp.rs`)
- `agent2ssh-daemon` — local HTTP/WebSocket/SSE server on `127.0.0.1:7722` (`src-tauri/src/bin/agent2ssh-daemon.rs`)

## Build / check / test

The frontend must be built before any Tauri build (`tauri.conf.json` expects `dist/`).

```bash
npm install
npm run build            # tsc + vite build → dist/

cd src-tauri
# Each binary compiles under a different Cargo feature set — check them separately:
cargo check --no-default-features --bin agent2ssh --bin agent2ssh-mcp   # CLI + MCP: no features
cargo check --no-default-features --features daemon --bin agent2ssh-daemon
cargo check                                                              # default = "tauri" feature → agent2ssh-app

cargo test --no-default-features                       # run the Rust unit tests (tests live in #[cfg(test)] modules)
cargo test --no-default-features classify_risk         # run a single test / filter by name
```

Run a surface during development:

```bash
npm run tauri:dev                                                       # desktop app
cd src-tauri && cargo run --no-default-features --bin agent2ssh -- host list --json
cd src-tauri && cargo run --no-default-features --bin agent2ssh-mcp     # speaks MCP over stdio
cd src-tauri && cargo run --no-default-features --features daemon --bin agent2ssh-daemon
```

End-to-end / smoke scripts live in `scripts/` (`e2e-local.sh`, `e1-mcp-client-smoke.py`, `e2-scale-plan-smoke.py`, `verify-install.sh`).

## Frontend (design system)

The React/TS UI in `src/` uses **Tailwind v4** (`@tailwindcss/vite`) with a **shadcn-style token + primitive** system — there is **no global stylesheet** (the old hand-written `styles.css` was removed; all styling is token-driven so light/dark "just works").

- `src/index.css` — design-system entry. Defines semantic CSS variables (`--background`, `--card`, `--primary`, `--muted`, `--border`, `--sidebar`, `--success`/`--warning`/`--destructive`, …) in **two themes: light by default, dark via `@media (prefers-color-scheme: dark)`** (follows the OS). `@theme inline` maps them onto Tailwind utilities (`bg-card`, `text-muted-foreground`, `rounded-lg`, …). Only Tailwind's `theme` + `utilities` layers are imported, plus a small hand-written `base` layer — **preflight is intentionally not enabled** (so the `ui/` primitives explicitly reset native control styles with `appearance-none`).
- `src/components/ui/` — shadcn-style primitives: `button`, `icon-button`, `card`, `input`, `textarea`, `select`, `badge`, `dialog`. Built with `cva` variants + the `cn()` helper in `src/lib/utils.ts` (clsx + tailwind-merge).
- Every panel in `src/components/` is composed from those primitives + Tailwind utilities that consume the tokens.

Conventions when adding/editing UI:
- Use tokens, never hardcoded hex colors, so dark mode keeps working (`bg-card` / `text-foreground` / `border-border` / `text-muted-foreground`, …).
- Reuse the `ui/` primitives instead of restyling raw `<button>`/`<input>`. Form field labels use `className="grid gap-1.5 text-sm font-medium text-foreground/90"`.
- Terminal / console output blocks keep a fixed dark palette (`bg-[#0e1620] text-[#e6edf3]`) regardless of theme.
- The sidebar is always dark; `PingPanel` (rendered inside it) uses fixed light state colors (e.g. `text-emerald-300`) rather than theme tokens.

## Cargo feature gating (important)

`src-tauri/Cargo.toml` defines mutually-relevant feature sets. Getting these wrong is the most common build failure:

- `default = ["tauri"]` — only `agent2ssh-app` needs this; pulls in `tauri`, `tauri-plugin-shell`.
- `daemon` — only `agent2ssh-daemon` needs this; pulls in `axum`, `tower`, `tracing`.
- The CLI and MCP binaries require **no features** — always build/check them with `--no-default-features`.
- Tauri-only code is gated with `#[cfg(feature = "tauri")]` (e.g. `tauri_commands` module in `lib.rs`). Daemon-only code is gated with `#[cfg(feature = "daemon")]`.

## Architecture

**The whole point of the codebase is that all four surfaces share one core.** Business logic lives in `src-tauri/src/` as a library (`lib.rs` re-exports everything); each binary is a thin adapter that parses its own input format (clap args / MCP JSON / HTTP requests / Tauri IPC) and calls the same core functions.

Key conventions:

- Core functions are suffixed `_core` (e.g. `exec_ssh_core`, `list_hosts_core`, `run_playbook_core`). When adding a capability, implement it once in a core module and wire all four binaries to it — do not reimplement logic inside a binary.
- Functions suffixed `_with_source` (e.g. `sftp_upload_core_with_source`) carry the originating client label (`AGENT2SSH_SOURCE` env, or set per surface) so audit entries and the Live Activity panel can attribute who ran what.

**The unified authorization layer is mandatory.** Every mutating entry point (exec, exec-multi, playbooks, SFTP, PTY session open/write/close, port forwards, connection ops) across CLI/MCP/Tauri/daemon must route through `execution_control.rs` (`authorize_command_with_approval`, `effective_command_risk`, `expand_exec_authorization_targets`). This is what enforces risk classification → approval → gate → limits → audit. Bypassing it in a new code path is a security regression.

Safety pipeline (the modules that authorization composes):

- `core.rs::classify_risk` + `risk_config.rs` — built-in risk classification (`low/medium/high/blocked`); user `policy.toml` rules can only **escalate** risk, never downgrade.
- `policy.rs` — unified policy-as-code (`~/.agent2ssh/policy.toml`/`.json`); legacy `risk_rules.toml` / `approval_policies.toml` still read for compat.
- `approval.rs` — approval queue + policies; high-risk commands need approval or `--force`/`force:true`. Multi-host and per-step approvals are scoped — one approval does not cascade to other hosts/steps.
- `gate.rs` — daemon-level execution gate (active/paused) that can block a source entirely.
- `limits.rs` — execution rate / session limits.
- `anomaly.rs` — audit-window anomaly detection → alerts.
- Host/playbook `risk_override` is the only trusted **downgrade** mechanism, and it still cannot unblock `blocked`.

Other core modules: `store.rs` (config + JSONL audit persistence under `~/.agent2ssh/`, write-locked), `embedded_ssh.rs` (in-process SSH transport, exec/SFTP, jump-host direct-tcpip proxying, PTY terminal worker, host-key fingerprints), `connection.rs` (retained embedded SSH connections), `session.rs` (process-local PTY session registry backed by embedded SSH), `forward.rs` (process-local embedded SSH direct-tcpip port forwards), `keys.rs` (SSH key mgmt, `0600`), `notify.rs` (fire-and-forget HMAC-signed webhooks), `remote.rs` (remote daemon registry + version compat, `PROTOCOL_VERSION`), `events.rs` (in-process event bus feeding daemon SSE/WS and the desktop activity panel), `health.rs`, `daemon_control.rs` (start/stop/restart the bundled daemon sidecar).

Note: `session.rs` and `forward.rs` state is **process-local**. MCP routes sessions through the local daemon when it is reachable, so those sessions are visible in the daemon registry and desktop; if MCP falls back to local mode, its sessions remain local to that MCP process. Forwards are still process-local.

SSH transport is embedded-first. Exec, SFTP, WebSocket `/terminal`, persistent PTY sessions, jump-host ProxyJump-style hops, retained connections, and local/remote port forwards use in-process libssh2 via the `ssh2` crate (`connect_embedded_ssh`, `exec_ssh_embedded`, `spawn_terminal`). Jump hosts are implemented with embedded `direct-tcpip` proxy channels. Keep the transport matrix in `docs/architecture.md` in sync when changing this boundary.

## Data & config layout

All runtime state is under `~/.agent2ssh/`: `hosts.json`, `audit.jsonl`, `policy.toml`/`.json`, `playbooks.toml`, `remotes.toml`, `webhook.toml`, `daemon.token` (admin bearer, auto-`0600`), `daemon_tokens.toml` (scoped tokens), `keys/` (`0600`). See `docs/guides/configuration-guide.md` for the full format.

## Docs worth reading before changing related code

- `docs/architecture.md` — system design
- `docs/api.yaml` — daemon REST/WS contract (keep in sync when changing daemon routes)
- `docs/skills.md` — full reference for the 51 MCP tools (keep in sync when adding/changing MCP tools)
- `docs/plan.md` — roadmap / milestones
