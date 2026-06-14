#!/usr/bin/env bash
# Start both demo nodes (no-op for any already running).
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
"$HERE/node.sh" A start
"$HERE/node.sh" B start
