# S9 Release Preflight Report

Date: 2026-06-16

> **Archive note**: This report is the preflight for the `v0.1.1` cut. The
> current shipped preflight is `v0.2.1` and is documented in
> [`plan2-q1-q2-execution-report.md`](plan2-q1-q2-execution-report.md), which
> covers the Q1 quality-gate closure and the Q2 WebDAV / master-password
> regression pass against the `v0.2.1` bundle
> (`Agent2SSH_0.2.1_aarch64.dmg`). Treat this S9 file as a historical
> artefact only.

## Scope

S9 completed the local pre-release closure for `v0.1.1` after the S5-S8 Live Activity and desktop session takeover work. This preflight did not create or push the `v0.1.1` tag; it verified that the main branch is ready for that release action.

## Version State

| File | Version |
|------|---------|
| `src-tauri/Cargo.toml` | `0.1.1` |
| `package.json` | `0.1.1` |
| `package-lock.json` | `0.1.1` |
| `src-tauri/tauri.conf.json` | `0.1.1` |
| `docs/api.yaml` | `0.1.1` |
| `scripts/agent2ssh.rb` | `0.1.1` |

Local tag check: `v0.1.1` does not exist yet.

## Release Notes

`CHANGELOG.md` now has a single `0.1.1` release section dated 2026-06-16. It includes:

- S1-S4 audit, documentation, contract, and release-gate work.
- S5 Live Agent Activity and daemon event stream work.
- S6 real-server regression evidence.
- S7 desktop takeover of daemon-managed sessions.
- S8 session takeover safety and usability controls.

## Local Quality Gate

| Check | Result |
|------|--------|
| `npm run build` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh --bin agent2ssh-mcp` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon` | Passed |
| `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features` | Passed: 137 unit, 24 CLI smoke, 56 daemon integration |
| `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon` | Passed: 142 unit, 24 CLI smoke, 56 daemon integration |
| `git diff --check` | Passed |

## Remaining Release Actions

1. Commit and push this S9 preflight update.
2. Create annotated tag `v0.1.1`.
3. Push `main` and tags to `github` and `git233`.
4. Wait for CI release assets.
5. Verify release checksums.
6. Replace `SHA256_PLACEHOLDER` values in `scripts/agent2ssh.rb` after assets are available.
