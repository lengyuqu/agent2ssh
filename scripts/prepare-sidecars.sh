#!/usr/bin/env bash
# prepare-sidecars.sh
#
# Copies built CLI binaries into src-tauri/binaries/ with the target-triple
# naming convention that Tauri's externalBin sidecar mechanism expects.
#
# Usage:
#   ./scripts/prepare-sidecars.sh [target-triple]
#
# If no target-triple is given, the script auto-detects the current host triple.
#
# Expected output layout:
#   src-tauri/binaries/agent2ssh-<triple>
#   src-tauri/binaries/agent2ssh-daemon-<triple>
#   src-tauri/binaries/agent2ssh-mcp-<triple>
#
# On Windows the .exe extension is appended automatically.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TAURI_DIR="$PROJECT_ROOT/src-tauri"
BINARIES_DIR="$TAURI_DIR/binaries"

# ---------- target triple ----------
if [[ -n "${1:-}" ]]; then
  TARGET="$1"
else
  # Auto-detect host triple via rustc
  TARGET="$(rustc -vV | sed -n 's/^host: //p')"
fi

echo "==> Preparing sidecars for target: $TARGET"

# ---------- file extension ----------
EXT=""
if [[ "$TARGET" == *"windows"* ]]; then
  EXT=".exe"
fi

# ---------- source directory ----------
RELEASE_DIR="$TAURI_DIR/target/$TARGET/release"
if [[ ! -d "$RELEASE_DIR" ]]; then
  echo "ERROR: Release directory not found: $RELEASE_DIR"
  echo "       Build the binaries first with:"
  echo "         cargo build --release --no-default-features --bin agent2ssh --bin agent2ssh-mcp --target $TARGET"
  echo "         cargo build --release --no-default-features --features daemon --bin agent2ssh-daemon --target $TARGET"
  exit 1
fi

# ---------- binaries to copy ----------
BINS=("agent2ssh" "agent2ssh-daemon" "agent2ssh-mcp")

mkdir -p "$BINARIES_DIR"

for bin in "${BINS[@]}"; do
  src="$RELEASE_DIR/${bin}${EXT}"
  dst="$BINARIES_DIR/${bin}-${TARGET}${EXT}"

  if [[ ! -f "$src" ]]; then
    echo "WARN: Source binary not found, skipping: $src"
    continue
  fi

  cp "$src" "$dst"
  chmod +x "$dst"
  echo "    $src -> $dst"
done

echo ""
echo "==> Sidecar binaries prepared in $BINARIES_DIR"
echo "    You can now run: npx @tauri-apps/cli build"
