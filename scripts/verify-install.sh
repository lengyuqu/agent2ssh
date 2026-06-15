#!/usr/bin/env bash
set -euo pipefail

PASS=0
FAIL=0

check() {
    local desc="$1"
    shift
    if "$@" > /dev/null 2>&1; then
        echo "✅ $desc"
        PASS=$((PASS + 1))
    else
        echo "❌ $desc"
        FAIL=$((FAIL + 1))
    fi
}

echo "=== Agent2SSH Installation Verification ==="
echo ""

check "agent2ssh --version" agent2ssh --version
check "agent2ssh host list --json" agent2ssh host list --json
check "agent2ssh risk ls --json" agent2ssh risk ls --json
check "agent2ssh audit --json" agent2ssh audit --json
check "agent2ssh daemon status" agent2ssh daemon status

if command -v agent2ssh-daemon &>/dev/null; then
    check "agent2ssh-daemon exists" test -x "$(command -v agent2ssh-daemon)"
fi

if command -v agent2ssh-mcp &>/dev/null; then
    check "agent2ssh-mcp exists" test -x "$(command -v agent2ssh-mcp)"
fi

echo ""
echo "Results: $PASS passed, $FAIL failed"

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
