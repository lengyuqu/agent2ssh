#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> Frontend build"
npm run build

echo "==> Rust library tests"
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib

echo "==> CLI and MCP compile"
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --bin agent2ssh --bin agent2ssh-mcp

echo "==> Daemon compile"
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --bin agent2ssh-daemon

echo "==> CLI/MCP smoke tests"
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test cli_smoke

echo "==> Daemon integration tests"
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --features daemon --test daemon_integration

echo "==> Sidecar dry-run build"
TARGET="$(rustc -vV | sed -n 's/^host: //p')"
cargo build --manifest-path src-tauri/Cargo.toml --release --target "$TARGET" --no-default-features --bin agent2ssh --bin agent2ssh-mcp
cargo build --manifest-path src-tauri/Cargo.toml --release --target "$TARGET" --no-default-features --features daemon --bin agent2ssh-daemon
./scripts/prepare-sidecars.sh "$TARGET"

echo "==> Done"
