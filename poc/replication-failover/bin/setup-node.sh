#!/usr/bin/env bash
# Two-machine demo: create + start THIS machine's single node, configured to be
# reachable by the peer across the network. Run this on BOTH machines (each with
# its own demo.env).
#
# Reads configuration from demo.env (see demo.env.example).
#
# Usage:  bin/setup-node.sh [--force]
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
POC_ROOT="$(cd "$HERE/.." && pwd)"
source "$HERE/pg-env.sh"

ENV_FILE="${CAIRN_DEMO_ENV:-$POC_ROOT/demo.env}"
if [[ ! -f "$ENV_FILE" ]]; then
  echo "ERROR: $ENV_FILE not found. Copy demo.env.example to demo.env and edit it." >&2
  exit 1
fi
# shellcheck disable=SC1090
set -a; source "$ENV_FILE"; set +a

SELF_NAME="${CAIRN_SELF_NAME:?set CAIRN_SELF_NAME in demo.env}"
SELF_PORT="${CAIRN_SELF_PORT:-55432}"
DB_USER="${CAIRN_DB_USER:-cairn_demo}"
DB_PASS="${CAIRN_DB_PASSWORD:?set CAIRN_DB_PASSWORD in demo.env}"
DB_NAME="${CAIRN_DB_NAME:-cairn}"
LISTEN="${CAIRN_LISTEN:-*}"
ALLOW="${CAIRN_ALLOW_CIDR:-samenet}"

NODE_DIR="$CAIRN_DEMO_HOME/node"
NODE_LOG="$CAIRN_DEMO_HOME/node.log"
# The local OS superuser created by initdb (used to bootstrap the demo role).
SUPER="$(id -un)"

FORCE=0; [[ "${1:-}" == "--force" ]] && FORCE=1

echo "PostgreSQL toolchain: $PG_BIN"; "$INITDB" --version
echo "This node: $SELF_NAME on port $SELF_PORT  (data: $NODE_DIR)"
mkdir -p "$CAIRN_DEMO_HOME"

if [[ -d "$NODE_DIR/base" ]]; then
  if [[ "$FORCE" -eq 1 ]]; then
    echo "--force: destroying existing node at $NODE_DIR"
    "$PG_CTL" -D "$NODE_DIR" stop -m immediate >/dev/null 2>&1 || true
    rm -rf "$NODE_DIR"
  else
    echo "Node already exists at $NODE_DIR (use --force to recreate). Skipping initdb."
  fi
fi

if [[ ! -d "$NODE_DIR/base" ]]; then
  echo "initdb…"
  "$INITDB" -D "$NODE_DIR" -U "$SUPER" --auth-local=trust --auth-host=trust \
    --encoding=UTF8 >/dev/null
  {
    echo "port = $SELF_PORT"
    echo "listen_addresses = '$LISTEN'"
    echo "unix_socket_directories = '$NODE_DIR'"
    echo "logging_collector = off"
    echo "password_encryption = 'scram-sha-256'"
  } >> "$NODE_DIR/postgresql.conf"
  # Allow the peer to connect over TCP with a password. 127.0.0.1 stays trust
  # so the local CLI never needs the password.
  {
    echo "# --- cairn demo: allow peer machine over the network ---"
    echo "host  all  $DB_USER  $ALLOW  scram-sha-256"
  } >> "$NODE_DIR/pg_hba.conf"
fi

if ! "$PG_ISREADY" -h 127.0.0.1 -p "$SELF_PORT" -q; then
  echo "starting node (log: $NODE_LOG)…"
  "$PG_CTL" -D "$NODE_DIR" -l "$NODE_LOG" -w start >/dev/null
fi

# Bootstrap the shared demo role + database (idempotent), as the local superuser.
echo "ensuring role '$DB_USER' and database '$DB_NAME'…"
"$PSQL" -h 127.0.0.1 -p "$SELF_PORT" -U "$SUPER" -d postgres -v ON_ERROR_STOP=1 -q <<SQL
DO \$\$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '$DB_USER') THEN
    CREATE ROLE $DB_USER LOGIN PASSWORD '$DB_PASS';
  ELSE
    ALTER ROLE $DB_USER WITH LOGIN PASSWORD '$DB_PASS';
  END IF;
END
\$\$;
SQL
if ! "$PSQL" -h 127.0.0.1 -p "$SELF_PORT" -U "$SUPER" -d postgres -tAc \
      "SELECT 1 FROM pg_database WHERE datname='$DB_NAME'" | grep -q 1; then
  "$CREATEDB" -h 127.0.0.1 -p "$SELF_PORT" -U "$SUPER" -O "$DB_USER" "$DB_NAME"
fi

echo "loading schema…"
"$PSQL" -h 127.0.0.1 -p "$SELF_PORT" -U "$DB_USER" -d "$DB_NAME" \
  -v ON_ERROR_STOP=1 -q -f "$POC_ROOT/schema.sql"

echo
echo "✅ Node $SELF_NAME is up on port $SELF_PORT, reachable on this machine's"
echo "   network address. Run  bin/netcheck.sh  to confirm the peer can reach it."
echo "   Then on each machine:  uv run cairn-demo status"
