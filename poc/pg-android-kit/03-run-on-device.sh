#!/usr/bin/env bash
#
# Spike 0003 — push the staged prefix to a connected Android device and run the
# G0/G1/G2 gates: exec a bionic binary, initdb a cluster, start the postmaster
# and run real SQL over TCP. No Termux, no root, no VM.
#
# WHY this exists, for a junior reader:
#   This is the actual on-device experiment. It assumes:
#     - steps 01 and 02 have produced <workdir>/prefix (with the FIXED shmem lib),
#     - the phone is connected with USB debugging on (adb sees it).
#   Two device facts drive the odd-looking flags below, both established in the
#   spike (see README):
#     - Unix-domain sockets cannot be created under /data/local/tmp (SELinux
#       denies a socket node in the shell_data_file context) -> we run TCP-only.
#     - libandroid-shmem coordinates via $TMPDIR -> we point it at a writable dir.
set -euo pipefail

WORK="${1:?usage: 03-run-on-device.sh <workdir> [adb-serial]}"
SERIAL="${2:-}"
ADB="${ADB:-$HOME/Library/Android/sdk/platform-tools/adb}"
PREFIX="$WORK/prefix"
DEV=/data/local/tmp/cairnpg          # on-device install root
PORT="${PG_PORT:-5432}"
adb() { if [ -n "$SERIAL" ]; then "$ADB" -s "$SERIAL" "$@"; else "$ADB" "$@"; fi; }

[ -x "$PREFIX/bin/postgres" ] || { echo "no staged prefix — run steps 01+02 first" >&2; exit 1; }

echo "=== push prefix (as a tarball; adb push mishandles the .so symlink farm) ==="
tar -C "$WORK" -czf "$WORK/prefix.tgz" prefix
adb shell "rm -rf $DEV && mkdir -p $DEV"
adb push "$WORK/prefix.tgz" /data/local/tmp/cairnpg.tgz >/dev/null
adb shell "cd $DEV && tar -xzf /data/local/tmp/cairnpg.tgz --strip-components=1 && rm /data/local/tmp/cairnpg.tgz"

# A single remote env preamble reused by every step.
ENV="export LD_LIBRARY_PATH=$DEV/lib PATH=$DEV/bin:\$PATH HOME=/data/local/tmp TZ=UTC TMPDIR=$DEV/tmp PGDATA=$DEV/data"

echo "=== G0: exec a bionic binary from /data/local/tmp ==="
adb shell "$ENV; mkdir -p $DEV/tmp; postgres --version" | tr -d '\r'

echo "=== G1: initdb ==="
adb shell "$ENV; rm -rf $DEV/data; rm -f $DEV/tmp/ashv_key_*;
  initdb -D $DEV/data -U postgres --encoding=UTF8 --locale-provider=icu --icu-locale=en-US --no-sync 2>&1 | tail -4" | tr -d '\r'

echo "=== G2: start postmaster (TCP-only) + smoke SQL ==="
adb shell "$ENV; rm -f $DEV/tmp/ashv_key_*;
  postgres -D $DEV/data -c unix_socket_directories= -c listen_addresses=127.0.0.1 -c port=$PORT >$DEV/server.log 2>&1 &
  # Wait until the postmaster actually accepts connections rather than a fixed
  # sleep — a cold or busy device can take well over 4s, and a fixed wait then
  # connects too early and reports a spurious failure. pg_isready returns 0 only
  # once the server is accepting (~30s ceiling so we still fail, not hang).
  i=0; until pg_isready -h 127.0.0.1 -p $PORT -q 2>/dev/null || [ \$i -ge 30 ]; do i=\$((i+1)); sleep 1; done
  PSQL=\"psql -h 127.0.0.1 -p $PORT -U postgres -d postgres -tA\"
  \$PSQL -c 'select version();'
  \$PSQL -c 'create table t(id serial primary key, note text, ts timestamptz default now());'
  \$PSQL -c \"insert into t(note) values ('cairn phone node'),('append-only') returning id, note;\"
  \$PSQL -c 'select count(*) as rows from t;'
  \$PSQL -c \"select current_setting('server_version') sv, current_setting('data_checksums') ck, current_setting('shared_memory_type') shm;\"
  pg_ctl -D $DEV/data stop -m fast" | tr -d '\r'

echo
echo "PASS if: postgres --version printed, initdb said Success, and the SELECTs returned rows."
echo "Device install left at $DEV (server stopped). Remove with: adb shell rm -rf $DEV"
