#!/usr/bin/env bash
# Shared environment for the Cairn replication/failover PoC.
#
# This file is *sourced* by the other scripts. It locates a PostgreSQL >= 18
# binary set and defines the two throwaway "nodes" used in the demo.
#
# SAFETY: this PoC never touches your production clusters. It creates brand-new
# clusters in their own data directories on non-standard ports. Your running
# servers (PG16 on :5432 and PG18 on :5532) are never opened, modified, or
# stopped by anything in this folder.

set -euo pipefail

# --- Where the throwaway clusters live (NOT inside the git worktree, so a
#     worktree cleanup can never wipe a live demo) ----------------------------
CAIRN_DEMO_HOME="${CAIRN_DEMO_HOME:-$HOME/.cairn-replication-demo}"

# --- Node definitions --------------------------------------------------------
# Node A and Node B are two independent PostgreSQL clusters = two "machines".
NODE_A_PORT="${NODE_A_PORT:-55432}"
NODE_B_PORT="${NODE_B_PORT:-55433}"
NODE_A_DIR="$CAIRN_DEMO_HOME/nodeA"
NODE_B_DIR="$CAIRN_DEMO_HOME/nodeB"
NODE_A_LOG="$CAIRN_DEMO_HOME/nodeA.log"
NODE_B_LOG="$CAIRN_DEMO_HOME/nodeB.log"
DB_NAME="${CAIRN_DB_NAME:-cairn}"
DB_USER="${CAIRN_DB_USER:-$(id -un)}"

# --- Locate a PostgreSQL >= 18 toolchain -------------------------------------
# We prefer an explicit override, then Postgres.app 18, then anything on PATH
# that reports a major version >= 18.
find_pg_bin() {
  if [[ -n "${PG_BIN:-}" && -x "$PG_BIN/initdb" ]]; then
    echo "$PG_BIN"; return 0
  fi
  local candidates=(
    "/Applications/Postgres 2.app/Contents/Versions/18/bin"
    "/Applications/Postgres.app/Contents/Versions/18/bin"
    "/Applications/Postgres.app/Contents/Versions/latest/bin"
    "/opt/homebrew/opt/postgresql@18/bin"
    "/usr/local/opt/postgresql@18/bin"
    "/usr/lib/postgresql/18/bin"
  )
  local c
  for c in "${candidates[@]}"; do
    if [[ -x "$c/initdb" ]]; then
      local major
      major="$("$c/initdb" --version | grep -oE '[0-9]+' | head -1)"
      if [[ "$major" -ge 18 ]]; then echo "$c"; return 0; fi
    fi
  done
  # Last resort: initdb on PATH, if it is >= 18.
  if command -v initdb >/dev/null 2>&1; then
    local major
    major="$(initdb --version | grep -oE '[0-9]+' | head -1)"
    if [[ "$major" -ge 18 ]]; then dirname "$(command -v initdb)"; return 0; fi
  fi
  echo "ERROR: could not find a PostgreSQL >= 18 toolchain. Set PG_BIN=/path/to/bin" >&2
  return 1
}

PG_BIN="$(find_pg_bin)"
INITDB="$PG_BIN/initdb"
PG_CTL="$PG_BIN/pg_ctl"
PSQL="$PG_BIN/psql"
CREATEDB="$PG_BIN/createdb"
PG_ISREADY="$PG_BIN/pg_isready"

# psql/connection helpers ------------------------------------------------------
# Connect to a node's "postgres" maintenance db, or to the demo db.
psql_node() {           # usage: psql_node <port> [extra psql args...]
  local port="$1"; shift
  "$PSQL" -h 127.0.0.1 -p "$port" -U "$DB_USER" -d "$DB_NAME" -v ON_ERROR_STOP=1 "$@"
}

node_dir()  { case "$1" in A) echo "$NODE_A_DIR";; B) echo "$NODE_B_DIR";; esac; }
node_port() { case "$1" in A) echo "$NODE_A_PORT";; B) echo "$NODE_B_PORT";; esac; }
node_log()  { case "$1" in A) echo "$NODE_A_LOG";; B) echo "$NODE_B_LOG";; esac; }

node_is_up() {          # usage: node_is_up A|B  -> exit 0 if accepting connections
  local port; port="$(node_port "$1")"
  "$PG_ISREADY" -h 127.0.0.1 -p "$port" -q
}
