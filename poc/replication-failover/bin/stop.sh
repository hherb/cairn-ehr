#!/usr/bin/env bash
# Stop both demo nodes (leaves data on disk; use teardown.sh to destroy).
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
"$HERE/node.sh" A stop || true
"$HERE/node.sh" B stop || true
