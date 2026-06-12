#!/usr/bin/env bash
# generate-checksums.sh
#
# Generate SHA256 checksums for release assets.
#
# Usage:
#   ./scripts/generate-checksums.sh [dist-directory]
#
# If no directory is given, defaults to "dist".
# The script produces a CHECKSUMS-SHA256.txt file in the target directory.

set -euo pipefail

DIST_DIR="${1:-dist}"

if [[ ! -d "$DIST_DIR" ]]; then
  echo "ERROR: Directory not found: $DIST_DIR"
  exit 1
fi

echo "Generating SHA256 checksums for $DIST_DIR..."

cd "$DIST_DIR"

# macOS uses `shasum -a 256`, Linux uses `sha256sum`
if command -v shasum &>/dev/null; then
  shasum -a 256 * > CHECKSUMS-SHA256.txt 2>/dev/null || true
elif command -v sha256sum &>/dev/null; then
  sha256sum * > CHECKSUMS-SHA256.txt 2>/dev/null || true
else
  echo "ERROR: Neither shasum nor sha256sum found in PATH."
  exit 1
fi

# Remove the checksum file's own entry if it was included
if [[ -f CHECKSUMS-SHA256.txt ]]; then
  grep -v 'CHECKSUMS-SHA256.txt' CHECKSUMS-SHA256.txt > CHECKSUMS-SHA256.tmp 2>/dev/null || true
  mv CHECKSUMS-SHA256.tmp CHECKSUMS-SHA256.txt
fi

echo ""
echo "Checksums written to $DIST_DIR/CHECKSUMS-SHA256.txt"
echo "---"
cat CHECKSUMS-SHA256.txt
echo "---"
