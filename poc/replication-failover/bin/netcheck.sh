#!/usr/bin/env bash
# Connectivity pre-flight for the two-machine demo.
#   * prints this machine's network addresses (to fill in the peer's demo.env)
#   * tests whether the configured peer is reachable
#
# Usage:  bin/netcheck.sh
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
POC_ROOT="$(cd "$HERE/.." && pwd)"
source "$HERE/pg-env.sh"

ENV_FILE="${CAIRN_DEMO_ENV:-$POC_ROOT/demo.env}"
[[ -f "$ENV_FILE" ]] && { set -a; source "$ENV_FILE"; set +a; }

echo "=== This machine's network addresses ==="
if command -v ipconfig >/dev/null 2>&1; then        # macOS
  for i in en0 en1 en2 en3 en4 en5; do
    ip="$(ipconfig getifaddr "$i" 2>/dev/null)"; [[ -n "$ip" ]] && echo "  $i: $ip"
  done
fi
# Portable fallback (Linux / anything with `ip` or `ifconfig`)
if command -v ip >/dev/null 2>&1; then
  ip -4 -o addr show scope global 2>/dev/null | awk '{print "  "$2": "$4}'
elif command -v ifconfig >/dev/null 2>&1; then
  ifconfig 2>/dev/null | awk '/inet /{print "  "$2}' | grep -v 127.0.0.1
fi
echo "  -> put the address the OTHER machine should dial into its CAIRN_PEER_HOST"

SELF_PORT="${CAIRN_SELF_PORT:-55432}"
echo
echo "=== Local node ==="
if "$PG_ISREADY" -h 127.0.0.1 -p "$SELF_PORT" -q; then
  echo "  ONLINE on :$SELF_PORT"
else
  echo "  OFFLINE on :$SELF_PORT — run bin/setup-node.sh"
fi

PEER_HOST="${CAIRN_PEER_HOST:-}"
PEER_PORT="${CAIRN_PEER_PORT:-55432}"
echo
echo "=== Peer reachability ==="
if [[ -z "$PEER_HOST" ]]; then
  echo "  (CAIRN_PEER_HOST not set in demo.env — skipping)"
else
  echo "  peer = $PEER_HOST:$PEER_PORT"
  if "$PG_ISREADY" -h "$PEER_HOST" -p "$PEER_PORT" -q; then
    echo "  ✅ peer PostgreSQL is reachable"
  else
    echo "  ❌ cannot reach peer. Checklist:"
    echo "     - both machines on the same network / cable plugged in?"
    echo "     - peer ran bin/setup-node.sh and its node is ONLINE?"
    echo "     - peer's CAIRN_LISTEN allows the network (default '*')?"
    echo "     - any host firewall allowing inbound TCP $PEER_PORT on the peer?"
  fi
  # Also verify an authenticated application connection works end to end.
  if [[ -n "${CAIRN_DB_PASSWORD:-}" ]]; then
    if PGPASSWORD="$CAIRN_DB_PASSWORD" "$PSQL" -h "$PEER_HOST" -p "$PEER_PORT" \
         -U "${CAIRN_DB_USER:-cairn_demo}" -d "${CAIRN_DB_NAME:-cairn}" \
         -tAc "SELECT 'auth-ok'" 2>/dev/null | grep -q auth-ok; then
      echo "  ✅ authenticated connection to peer works (role + password OK)"
    else
      echo "  ⚠  reachable but auth/database check failed — confirm CAIRN_DB_PASSWORD"
      echo "     matches on both machines and the peer loaded the schema."
    fi
  fi
fi
