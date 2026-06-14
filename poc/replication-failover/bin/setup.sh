#!/usr/bin/env bash
# Create the two throwaway demo clusters from scratch, start them, and load the
# schema into each. Idempotent-ish: refuses to clobber an existing cluster
# unless you pass --force (which destroys and recreates).
#
# Usage:  bin/setup.sh [--force]
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$HERE/pg-env.sh"
POC_ROOT="$(cd "$HERE/.." && pwd)"

FORCE=0
[[ "${1:-}" == "--force" ]] && FORCE=1

echo "PostgreSQL toolchain: $PG_BIN"
"$INITDB" --version
echo "Demo home: $CAIRN_DEMO_HOME"
mkdir -p "$CAIRN_DEMO_HOME"

init_node() {
  local name="$1" dir port
  dir="$(node_dir "$name")"; port="$(node_port "$name")"

  if [[ -d "$dir/base" ]]; then
    if [[ "$FORCE" -eq 1 ]]; then
      echo "[$name] --force: stopping & destroying existing cluster at $dir"
      "$PG_CTL" -D "$dir" stop -m immediate >/dev/null 2>&1 || true
      rm -rf "$dir"
    else
      echo "[$name] cluster already exists at $dir (use --force to recreate). Skipping initdb."
    fi
  fi

  if [[ ! -d "$dir/base" ]]; then
    echo "[$name] initdb -> $dir (port $port)"
    # trust auth: localhost-only throwaway clusters. NOT production posture.
    "$INITDB" -D "$dir" -U "$DB_USER" --auth-local=trust --auth-host=trust \
      --encoding=UTF8 >/dev/null
    # Pin the port in the cluster config so pg_ctl always uses it.
    {
      echo "port = $port"
      echo "listen_addresses = '127.0.0.1'"
      echo "unix_socket_directories = '$dir'"
      echo "logging_collector = off"
    } >> "$dir/postgresql.conf"
  fi
}

start_node() {
  local name="$1" dir log port
  dir="$(node_dir "$name")"; log="$(node_log "$name")"; port="$(node_port "$name")"
  if node_is_up "$name"; then
    echo "[$name] already running on port $port"
  else
    echo "[$name] starting (log: $log)"
    "$PG_CTL" -D "$dir" -l "$log" -w start >/dev/null
  fi
}

create_db_and_schema() {
  local name="$1" port
  port="$(node_port "$name")"
  if ! "$PSQL" -h 127.0.0.1 -p "$port" -U "$DB_USER" -d postgres -tAc \
        "SELECT 1 FROM pg_database WHERE datname='$DB_NAME'" | grep -q 1; then
    echo "[$name] creating database '$DB_NAME'"
    "$CREATEDB" -h 127.0.0.1 -p "$port" -U "$DB_USER" "$DB_NAME"
  fi
  echo "[$name] loading schema"
  "$PSQL" -h 127.0.0.1 -p "$port" -U "$DB_USER" -d "$DB_NAME" \
    -v ON_ERROR_STOP=1 -q -f "$POC_ROOT/schema.sql"
}

for n in A B; do init_node "$n"; done
for n in A B; do start_node "$n"; done
for n in A B; do create_db_and_schema "$n"; done

echo
echo "✅ Setup complete. Node A on :$NODE_A_PORT, Node B on :$NODE_B_PORT."
echo "   Next:  cd $POC_ROOT && uv run cairn-demo status"
