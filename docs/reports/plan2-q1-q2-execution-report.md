# Plan 2 Q1/Q2 Execution Report

Date: 2026-06-26

## Scope

This report records the first execution pass against `plan2.md`, focused on:

- Q1 release confidence and local quality gates.
- Feasible local parts of Q2 credential encryption and WebDAV sync regression.

Q3 external adoption, cross-platform install smoke, real WebDAV server push/pull, and multi-device recovery remain external validation items.

## Q1 Results

### Completed

- Added Rust format, Clippy, and diff-whitespace checks to `scripts/e2e-local.sh`.
- Added the same format/Clippy/diff checks to `docs/release-checklist.md`.
- Fixed current Clippy blockers under `cargo clippy --no-default-features --all-targets -- -D warnings`.
- Verified macOS local Tauri packaging still produces `.app` and `.dmg` bundles.

### Clippy Fixes

The Clippy cleanup was intentionally mechanical:

- Introduced a `ConnectionHandleSnapshot` type alias for retained-connection supervision snapshots.
- Removed redundant branches and guards.
- Replaced unnecessary `sort_by`, `iter().any`, `vec!`, `clone`, `Ok(...?)`, and `return` patterns.
- Moved `keys.rs` tests to the end of the file to satisfy item-order linting.
- Added a local `#[allow(clippy::enum_variant_names)]` only for the MCP tool enum because the `Ssh*` prefix mirrors exported MCP tool names and avoids a large non-behavioral rename.

### Validation Commands

Passed:

```bash
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --no-default-features --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test cli_smoke
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --test daemon_integration
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh --bin agent2ssh-mcp
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon
./scripts/e2e-local.sh
npm run tauri:build
```

`npm run tauri:build` produced:

- `src-tauri/target/release/bundle/macos/Agent2SSH.app`
- `src-tauri/target/release/bundle/dmg/Agent2SSH_0.2.1_aarch64.dmg`

Notarization was skipped because Apple notarization credentials were not configured in the local environment.

### Remaining Q1 Notes

- Frontend has no dedicated ESLint setup today. The current frontend static gate remains `npm run build` (`tsc && vite build`). Adding ESLint should be a separate explicit change because it will introduce new dependencies and rule decisions.
- CI already covers contract consistency, Rust tests, Rust checks, frontend build, release binary builds, and release bundle jobs. Clippy is now enforced by local `e2e-local.sh` and the release checklist; adding it to CI should be considered after confirming cross-platform Clippy output is stable.

## Q2 Results

### Completed Locally

Credential-store CLI smoke with isolated `AGENT2SSH_CONFIG_DIR`:

- `secrets status --json` starts as `{ initialized: false, unlocked: false }`.
- `secrets set-password --password ...` initializes `secrets.enc`.
- A new process without `AGENT2SSH_MASTER_PASSWORD` reports initialized but locked.
- A new process with `AGENT2SSH_MASTER_PASSWORD` reports initialized and unlocked.
- Recursive grep of the isolated config directory did not find the master password in plaintext.

Passed focused regression tests:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features webdav_sync::tests
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features secrets::tests
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features store::tests::migrate_secrets_moves_legacy_plaintext
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features store::tests::passwords_persist_as_marker_not_plaintext
```

These cover:

- encrypted store init/unlock and locked-store behavior,
- plaintext credential migration into secret references,
- password persistence as marker rather than plaintext,
- WebDAV sync file selection excluding local trust/runtime/private-key files,
- backup content selection,
- legacy remote `known_hosts.json` tolerance without local overwrite.

### Remaining Q2 Items

Not completed in this local pass:

- Real WebDAV `push` / `pull` against an actual remote collection.
- Network failure, authentication failure, and remote conflict recovery against a real WebDAV service.
- Cross-device pull/unlock/host-key verification workflow.
- Desktop `SecretsUnlock` manual UI walkthrough.
- MCP/daemon password-host execution using a real password-auth SSH host and `AGENT2SSH_MASTER_PASSWORD`.

These require a real WebDAV endpoint, a second device/profile, a desktop manual run, or a password-auth test host.

## Recommendation

Next work should continue with Q2 real-environment validation before opening Q3 external adoption. The codebase now has a stronger local release gate, so new changes should use `./scripts/e2e-local.sh` as the default preflight.

