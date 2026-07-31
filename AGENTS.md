# Repository Guidelines

## Project Structure & Module Organization

Agent2SSH combines a React/Vite desktop frontend with a Rust/Tauri backend. Frontend code lives in `src/`, with reusable UI in `src/components/`, shared types in `src/types.ts`, API helpers in `src/api.ts`, and styling/tokens in `src/index.css` plus `src/components/ui/`. Rust code is under `src-tauri/src/`; binaries are in `src-tauri/src/bin/` for the CLI, daemon, and MCP server. Rust integration tests live in `src-tauri/tests/`. Documentation is in `docs/`, `README.md`, and `CHANGELOG.md`. Helper scripts are in `scripts/`; bundled skill instructions are in `skills/agent2ssh/SKILL.md`.

## Build, Test, and Development Commands

- `npm install`: install frontend and Tauri dependencies.
- `npm run dev`: run the Vite frontend.
- `npm run build`: type-check and build the frontend.
- `npm test`: run frontend behavior tests with Vitest (jsdom + Testing Library).
- `npm run test:watch`: run frontend tests in watch mode.
- `npm run tauri:dev`: launch the desktop app in development mode.
- `npm run tauri:build`: build sidecar binaries, frontend, and the packaged Tauri app.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib`: run Rust library tests.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test cli_smoke`: run CLI/MCP smoke tests.
- `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh --bin agent2ssh-mcp`: CLI/MCP compile check (no features).
- `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon`: daemon compile check.
- `cargo check --manifest-path src-tauri/Cargo.toml`: Tauri app compile check (default feature).
- `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --test daemon_integration`: run daemon integration tests.
- `./scripts/e2e-local.sh`: run local preflight builds, tests, smoke checks, and sidecar preparation.

## Coding Style & Naming Conventions

Use TypeScript with React function components and PascalCase component filenames, such as `HostList.tsx`. Keep helpers and modules in lower camel case. Rust uses the 2021 edition, snake_case modules, and focused files by capability, such as `policy.rs`, `session.rs`, and `daemon_control.rs`. Run `cargo fmt --manifest-path src-tauri/Cargo.toml` before Rust changes. Keep UI copy aligned with `src/i18n.tsx`. Exec, SFTP, terminal, persistent sessions, jump-host proxying, connection retention, and port forwarding use the embedded SSH transport, so document backend changes carefully.

## Testing Guidelines

Place Rust integration tests in `src-tauri/tests/` and name them by behavior or surface, for example `cli_smoke.rs` or `daemon_integration.rs`. Prefer targeted tests for policy, risk, approval, daemon, and command execution changes. Frontend behavior tests live next to components as `src/**/*.test.tsx` (for example `src/components/HostList.test.tsx`) and run with `npm test` (Vitest + jsdom + Testing Library, configured in `vite.config.ts` with setup in `src/test-setup.ts`); mock `src/api.ts` in tests so they never touch the Tauri bridge. For frontend changes, run `npm test` and `npm run build`; add manual desktop checks with `npm run tauri:dev` when UI behavior changes.

## Commit & Pull Request Guidelines

Recent commits use short, imperative, sentence-case messages, for example `Add desktop daemon lifecycle controls` and `Document R5 regression retest`. Keep commits scoped to one behavior or documentation update. Pull requests should explain the change, list validation commands, link related issues or reports, and include screenshots for desktop UI changes.

## Security & Configuration Tips

Local runtime data is stored under `~/.agent2ssh/`, including daemon tokens and policy files. Do not commit secrets, host keys, private SSH config, or generated local tokens. For policy and storage details, update `docs/guides/configuration-guide.md` alongside behavior changes.
