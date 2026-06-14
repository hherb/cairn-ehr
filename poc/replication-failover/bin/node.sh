#!/usr/bin/env bash
# Control a single demo node — this is how you "pull the plug" and "plug back
# in" during the live demo.
#
# Usage:
#   bin/node.sh A stop      # pull the plug on node A (stop its PostgreSQL)
#   bin/node.sh A start     # plug it back in
#   bin/node.sh A restart
#   bin/node.sh A status
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$HERE/pg-env.sh"

NAME="${1:-}"; ACTION="${2:-status}"
case "$NAME" in A|B) ;; *) echo "Usage: node.sh A|B start|stop|restart|status" >&2; exit 1;; esac

DIR="$(node_dir "$NAME")"; LOG="$(node_log "$NAME")"; PORT="$(node_port "$NAME")"

case "$ACTION" in
  stop)
    if node_is_up "$NAME"; then
      echo "⏻  Pulling the plug on node $NAME (port $PORT)…"
      # -m fast = abrupt but clean shutdown, like power loss to that machine.
      "$PG_CTL" -D "$DIR" stop -m fast -w >/dev/null
      echo "   Node $NAME is now OFFLINE."
    else
      echo "Node $NAME is already offline."
    fi
    ;;
  start)
    if node_is_up "$NAME"; then
      echo "Node $NAME is already online (port $PORT)."
    else
      echo "🔌 Plugging node $NAME back in…"
      "$PG_CTL" -D "$DIR" -l "$LOG" -w start >/dev/null
      echo "   Node $NAME is ONLINE (still stale until you sync)."
    fi
    ;;
  restart)
    "$0" "$NAME" stop || true
    "$0" "$NAME" start
    ;;
  status)
    if node_is_up "$NAME"; then echo "Node $NAME: ONLINE  (port $PORT)";
    else echo "Node $NAME: OFFLINE (port $PORT)"; fi
    ;;
  *)
    echo "Unknown action: $ACTION (use start|stop|restart|status)" >&2; exit 1;;
esac
