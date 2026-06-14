#!/usr/bin/env bash
# Two-machine demo: control THIS machine's single node.
#
# For the live demo you usually pull the cable physically. These commands are
# for testing, and for simulating a clean shutdown ("power off this machine").
#
# Usage:  bin/node-ctl.sh start|stop|restart|status
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
POC_ROOT="$(cd "$HERE/.." && pwd)"
source "$HERE/pg-env.sh"

ENV_FILE="${CAIRN_DEMO_ENV:-$POC_ROOT/demo.env}"
[[ -f "$ENV_FILE" ]] && { set -a; source "$ENV_FILE"; set +a; }
SELF_PORT="${CAIRN_SELF_PORT:-55432}"
NODE_DIR="$CAIRN_DEMO_HOME/node"
NODE_LOG="$CAIRN_DEMO_HOME/node.log"

case "${1:-status}" in
  start)
    if "$PG_ISREADY" -h 127.0.0.1 -p "$SELF_PORT" -q; then echo "Node already online (:$SELF_PORT).";
    else echo "🔌 Starting local node…"; "$PG_CTL" -D "$NODE_DIR" -l "$NODE_LOG" -w start >/dev/null; echo "ONLINE."; fi ;;
  stop)
    if "$PG_ISREADY" -h 127.0.0.1 -p "$SELF_PORT" -q; then
      echo "⏻  Stopping local node (simulated power-off)…"
      "$PG_CTL" -D "$NODE_DIR" stop -m fast -w >/dev/null; echo "OFFLINE.";
    else echo "Node already offline."; fi ;;
  restart) "$0" stop || true; "$0" start ;;
  status)
    if "$PG_ISREADY" -h 127.0.0.1 -p "$SELF_PORT" -q; then echo "Local node: ONLINE (:$SELF_PORT)";
    else echo "Local node: OFFLINE (:$SELF_PORT)"; fi ;;
  *) echo "Usage: node-ctl.sh start|stop|restart|status" >&2; exit 1 ;;
esac
