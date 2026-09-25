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
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: run Rust library tests for the default (tauri) feature set. This is the only command that compiles and runs `src/tauri_commands.rs` tests, so run it alongside the `--no-default-features` one. It needs the frontend `dist/` to exist, so run `npm run build` first. It also compiles `build.rs` with the tauri feature, where `tauri_build::build()` requires every `bundle.externalBin` sidecar to already exist as `src-tauri/binaries/<name>-<host-triple>` — build them and run `./scripts/prepare-sidecars.sh`, or blank the check out with `TAURI_CONFIG='{"bundle":{"externalBin":[]}}'` when you only want to lint or unit-test the library. Without either, the build script aborts with `resource path binaries/agent2ssh-cli-<triple> doesn't exist`.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test cli_smoke`: run CLI/MCP smoke tests.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test connect_deadline`: run the connect-deadline regressions (a peer that accepts TCP and then goes silent must not leak a blocked worker). Binds its own loopback listener, so it needs no fixture.
- `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh --bin agent2ssh-mcp`: CLI/MCP compile check (no features).
- `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon`: daemon compile check.
- `cargo check --manifest-path src-tauri/Cargo.toml`: Tauri app compile check (default feature). Same sidecar prerequisite as `cargo test --lib` above.
- **Windows builds compile OpenSSL from source, so they need two extra tools.** `src-tauri/Cargo.toml` enables `ssh2`'s `openssl-on-win32` together with `vendored-openssl`, which makes the Windows binary self-contained (nothing to ship alongside it) and gives it curve25519, ed25519/ECDSA host keys, AES-GCM and the OpenSSH private-key container parser — a WinCNG build has none of those, which is why it could not handshake with OpenSSH 10.0+. `openssl-src` drives that build with `nmake` (from the MSVC toolchain) and a **full** Perl for OpenSSL's `Configure`. `src-tauri/.cargo/config.toml` pins `OPENSSL_SRC_PERL` to `scripts/win-perl.cmd`, which resolves one and explains itself when it cannot, because the Perl bundled with Git for Windows is trimmed and fails with `Can't locate Locale/Maketext/Simple.pm in @INC` — an error that names no fix. Install it with `scoop install perl`. NASM is optional and only enables OpenSSL's assembly routines (`scoop install nasm`; `openssl-src` silently falls back to `no-asm` without it). The first build after either changes takes roughly 12 minutes; later ones reuse it.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --test daemon_integration`: run daemon integration tests.
- `AGENT2SSH_TEST_SSH_PORT=2222 cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test exec_fixture -- --test-threads=1`: run the sshd fixture tests (skipped without the env var). Also run by `./scripts/e2e-container.sh` and by the CI `real-ssh-e2e` job, which supply `AGENT2SSH_TEST_SSH_KEY`. Build a local server with `docker build -t a2s-sshd:local scripts/sshd-fixture && docker run -d --name a2s-sshd -p 2222:22 a2s-sshd:local` (swap `docker` for the runtime you use — `wslc`, or Apple's `container`).
- `./scripts/e2e-container.sh`: run the containerized real-SSH E2E (real exec, SFTP round-trips, resume, exec-fixture suite). The runtime is resolved by `agent2ssh containers runtime` — Docker, wslc or Apple container — so it is not Docker-only. `scripts/e2e-docker.sh` remains a compatibility shim for the old name. The client key is generated by `agent2ssh keys generate`, so the run also proves that a key this app produces is a key it can authenticate with; `AGENT2SSH_E2E_IMAGE` selects the sshd image; the default is a current release (OpenSSH 10.3 at the time of writing) and works on every platform this project builds, because the Windows build links OpenSSL. Pin `lscr.io/linuxserver/openssh-server:9.9_p2-r0-ls198` only to test a WinCNG build, which shares no key-exchange algorithm with OpenSSH 10.0+.
- `agent2ssh containers list [--all] [--runtime <id>] [--json]`: list container exec targets across Docker, wslc, Apple container and Kubernetes. `agent2ssh containers runtime [--json|--binary]`: show (or print just the path of) the local runtime the app drives.
- `agent2ssh keys algorithm [--name|--json]`: print the key algorithm this build can authenticate with (a property of the linked libssh2, not of the host OS; every platform this project builds now links OpenSSL and therefore answers `ed25519`). `agent2ssh keys generate <name> [--type ed25519|rsa] [--json]`: generate a pair into `~/.agent2ssh/keys/`, printing the private key path alone so it can be fed to `--key`.
- `./scripts/e2e-local.sh`: run local preflight builds, tests, smoke checks, and sidecar preparation.

## Coding Style & Naming Conventions

Use TypeScript with React function components and PascalCase component filenames, such as `HostList.tsx`. Keep helpers and modules in lower camel case. Rust uses the 2021 edition, snake_case modules, and focused files by capability, such as `policy.rs`, `session.rs`, and `daemon_control.rs`. Run `cargo fmt --manifest-path src-tauri/Cargo.toml` before Rust changes. Keep UI copy aligned with `src/i18n.tsx`. Exec, SFTP, terminal, persistent sessions, jump-host proxying, connection retention, and port forwarding use the embedded SSH transport, so document backend changes carefully.

## Testing Guidelines

Place Rust integration tests in `src-tauri/tests/` and name them by behavior or surface, for example `cli_smoke.rs` or `daemon_integration.rs`. Prefer targeted tests for policy, risk, approval, daemon, and command execution changes. Tests behind `#[cfg(feature = "tauri")]` — such as the ones in `src/tauri_commands.rs` — only compile under the default feature set, so Rust changes need both `cargo test --no-default-features --lib` and `cargo test --lib`. Any test that needs a real SSH server should follow `tests/exec_fixture.rs`: skip unless `AGENT2SSH_TEST_SSH_PORT` is set, and wire it into `scripts/e2e-container.sh` so CI runs it. Frontend behavior tests live next to components as `src/**/*.test.tsx` (for example `src/components/HostList.test.tsx`) and run with `npm test` (Vitest + jsdom + Testing Library, configured in `vite.config.ts` with setup in `src/test-setup.ts`); mock `src/api.ts` in tests so they never touch the Tauri bridge. For frontend changes, run `npm test` and `npm run build`; add manual desktop checks with `npm run tauri:dev` when UI behavior changes.

## Commit & Pull Request Guidelines

Recent commits use short, imperative, sentence-case messages, for example `Add desktop daemon lifecycle controls` and `Document R5 regression retest`. Keep commits scoped to one behavior or documentation update. Pull requests should explain the change, list validation commands, link related issues or reports, and include screenshots for desktop UI changes.

## Security & Configuration Tips

Local runtime data is stored under `~/.agent2ssh/`, including daemon tokens and policy files. Do not commit secrets, host keys, private SSH config, or generated local tokens. For policy and storage details, update `docs/guides/configuration-guide.md` alongside behavior changes.
