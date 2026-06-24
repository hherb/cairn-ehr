#!/usr/bin/env bash
#
# Spike 0003 — stage a relocatable, bionic PostgreSQL 18 prefix from prebuilt
# Termux .debs.
#
# WHY this exists, for a junior reader:
#   A Cairn node is "PostgreSQL + a thin daemon" (ADR-0001). To prove a node can
#   run on a stock Android phone we need PostgreSQL built for Android's libc
#   (bionic), which differs from glibc enough that a normal build won't run.
#   Termux already maintains exactly such a build (PG 18.2 for aarch64), so we
#   reuse their cross-compiled ELF binaries — but NOT their userland or package
#   manager. We download the .debs, throw away everything except the binaries +
#   shared libraries, and flatten them into one self-contained directory tree
#   ("prefix") that PostgreSQL can run from anywhere (it is relocatable: it
#   derives its share/ path from argv[0]). Nothing Termux-specific lands on the
#   phone — only standard ELF .so/.bin files.
#
# OUTPUT: <workdir>/prefix  — a tree you push to the device in step 03.
#
# Note: the bundled libandroid-shmem.so from these .debs is BROKEN on a
# non-Termux device (see step 02 and the README) — step 02 builds a fixed one
# that overwrites it before you run on-device.
set -euo pipefail

REPO="https://packages.termux.dev/apt/termux-main"
PKGS_URL="$REPO/dists/stable/main/binary-aarch64/Packages.gz"
WORK="${1:?usage: 01-stage-prefix.sh <workdir>}"
DEBS="$WORK/debs"; PREFIX="$WORK/prefix"; META="$WORK/Packages"
mkdir -p "$DEBS" "$PREFIX"

need() { command -v "$1" >/dev/null || { echo "missing tool: $1" >&2; exit 1; }; }
need curl; need bsdtar   # bsdtar (libarchive) reads BOTH the .deb (ar) and the inner data.tar.*

# 1. Package index: one stanza per package, carrying its Depends + Filename.
[ -f "$META" ] || curl -fsS --max-time 30 "$PKGS_URL" | gunzip > "$META"

# field PKG FIELD -> value of FIELD in PKG's stanza.
# NB: matched with index() (literal), NOT a regex — package names like "libc++"
# contain regex metacharacters that silently break a regex match (this was a
# real bug while developing the kit: libc++ got skipped, ICU then failed to load).
field() {
  awk -v pkg="$1" -v f="$2" '
    BEGIN { RS=""; FS="\n" }
    index($0, "Package: " pkg "\n") {
      for (i=1;i<=NF;i++) if (index($i, f ": ")==1) { sub("^"f": ","",$i); print $i; exit }
    }' "$META"
}

# 2. Resolve the dependency closure starting from postgresql (breadth-first).
declare -A seen; queue=(postgresql); order=()
while [ ${#queue[@]} -gt 0 ]; do
  p="${queue[0]}"; queue=("${queue[@]:1}")
  p="${p%% *}"; p="${p%%:*}"                 # strip version constraints / arch quals
  [ -n "$p" ] && [ -z "${seen[$p]:-}" ] || continue
  seen[$p]=1; order+=("$p")
  deps="$(field "$p" Depends || true)"
  if [ -n "$deps" ]; then
    IFS=',' read -ra parts <<< "$deps"
    for d in "${parts[@]}"; do
      d="$(printf '%s' "$d" | sed -E 's/\(.*\)//; s/^ *//; s/ *$//; s/ .*//')"
      [ -n "$d" ] && queue+=("$d")
    done
  fi
done
echo "closure: ${order[*]}"

# 3. Download each package and flatten its data tree into the single prefix.
for p in "${order[@]}"; do
  fn="$(field "$p" Filename || true)"
  [ -n "$fn" ] || { echo "  - $p: no Filename (system-provided / virtual) — skipping"; continue; }
  deb="$DEBS/$(basename "$fn")"
  [ -f "$deb" ] || curl -fsS --retry 3 --max-time 180 -o "$deb" "$REPO/$fn"
  tmp="$WORK/_unpack"; rm -rf "$tmp"; mkdir -p "$tmp"
  data="$(bsdtar -tf "$deb" | grep '^data\.tar' | head -1)"
  bsdtar -xOf "$deb" "$data" | bsdtar -xf - -C "$tmp"
  # Termux installs under data/data/com.termux/files/usr — strip that to get a clean prefix.
  src="$tmp/data/data/com.termux/files/usr"
  [ -d "$src" ] && bsdtar -cf - -C "$src" . | bsdtar -xf - -C "$PREFIX"
done

echo "=== staged prefix at $PREFIX ==="
echo "postgres : $(ls -la "$PREFIX/bin/postgres" 2>&1 | awk '{print $5, $NF}')"
echo "initdb   : $([ -x "$PREFIX/bin/initdb" ] && echo ok || echo MISSING)"
echo "psql     : $([ -x "$PREFIX/bin/psql" ] && echo ok || echo MISSING)"
echo "libc++   : $([ -e "$PREFIX/lib/libc++_shared.so" ] && echo ok || echo MISSING)"
echo "libs     : $(ls "$PREFIX/lib"/*.so* 2>/dev/null | wc -l | tr -d ' ') shared objects"
