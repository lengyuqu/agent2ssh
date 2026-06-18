#!/usr/bin/env bash
# Build release sidecar binaries and copy them into Tauri's externalBin layout.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TAURI_DIR="$PROJECT_ROOT/src-tauri"

if [[ -n "${1:-}" ]]; then
  TARGET="$1"
else
  TARGET="$(rustc -vV | sed -n 's/^host: //p')"
fi

echo "==> Building sidecars for target: $TARGET"
cargo build \
  --manifest-path "$TAURI_DIR/Cargo.toml" \
  --release \
  --target "$TARGET" \
  --no-default-features \
  --bin agent2ssh \
  --bin agent2ssh-mcp

cargo build \
  --manifest-path "$TAURI_DIR/Cargo.toml" \
  --release \
  --target "$TARGET" \
  --no-default-features \
  --features daemon \
  --bin agent2ssh-daemon

"$SCRIPT_DIR/prepare-sidecars.sh" "$TARGET"
