#!/usr/bin/env bash
# Destroy the demo entirely: stop both nodes and delete their data directories.
# Your production clusters are never touched. Requires --yes to proceed.
#
# Usage:  bin/teardown.sh --yes
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$HERE/pg-env.sh"

if [[ "${1:-}" != "--yes" ]]; then
  echo "This will stop the demo nodes and DELETE $CAIRN_DEMO_HOME"
  echo "Re-run with --yes to confirm."
  exit 1
fi

"$PG_CTL" -D "$NODE_A_DIR" stop -m immediate >/dev/null 2>&1 || true
"$PG_CTL" -D "$NODE_B_DIR" stop -m immediate >/dev/null 2>&1 || true
rm -rf "$CAIRN_DEMO_HOME"
echo "✅ Demo removed. (Production clusters on :5432 and :5532 untouched.)"
