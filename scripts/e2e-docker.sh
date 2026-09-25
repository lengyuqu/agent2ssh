#!/usr/bin/env bash
#
# Compatibility shim. This harness was renamed to scripts/e2e-container.sh when
# it stopped assuming Docker: it now resolves Docker, the WSL container CLI
# (wslc) or Apple's container runtime through `agent2ssh containers runtime`, so
# the old name described one runtime out of three.
#
# The shim exists only so the name that older reports and downstream scripts
# reference keeps working. Prefer scripts/e2e-container.sh.
set -euo pipefail
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/e2e-container.sh" "$@"
