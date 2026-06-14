#!/usr/bin/env bash
# Reset both nodes to a pristine, empty, in-sync state WITHOUT recreating the
# clusters — truncates the event log and resets the HLC. Run this right before
# the live demo so you start from a clean slate.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$HERE/pg-env.sh"

"$HERE/start.sh" >/dev/null

reset_node() {
  local name="$1" port
  port="$(node_port "$name")"
  echo "[$name] truncating event_log and resetting HLC"
  # event_log is append-only via trigger; TRUNCATE bypasses row triggers, which
  # is exactly why a privileged reset uses it rather than DELETE.
  "$PSQL" -h 127.0.0.1 -p "$port" -U "$DB_USER" -d "$DB_NAME" -v ON_ERROR_STOP=1 -q -c \
    "TRUNCATE event_log; UPDATE hlc_state SET hlc_wall=0, hlc_counter=0 WHERE id IS TRUE;"
}

for n in A B; do reset_node "$n"; done
echo "✅ Both nodes reset to empty and in sync."
