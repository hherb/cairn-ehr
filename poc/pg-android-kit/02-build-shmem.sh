#!/usr/bin/env bash
#
# Spike 0003 — build a bionic-portable libandroid-shmem.so for a STOCK Android device.
#
# WHY this exists, for a junior reader:
#   PostgreSQL needs System V shared memory (shmget/shmat/...). bionic has no
#   SysV IPC, so Termux ships libandroid-shmem.so which emulates it. The prebuilt
#   Termux copy works inside Termux but DEADLOCKS / SPINS on a stock device for
#   two reasons (both found by strace during spike 0003):
#     1. It coordinates segment "keys" via symlinks under a path that is baked in
#        at compile time to the Termux prefix (/data/data/com.termux/files/usr/tmp).
#        On a stock device that path is unwritable -> EACCES -> it busy-loops on
#        the CPU forever.
#     2. It is built at Android API 24, so it backs regions with the legacy
#        /dev/ashmem device, which is REMOVED on Android 11+. (Building at API>=26
#        switches to ASharedMemory_create, but that drags in libandroid.so, which
#        on some handsets pulls a vendor lib whose symbols clash with PostgreSQL's
#        bundled OpenSSL.)
#   Our patch (patches/libandroid-shmem-bionic-portability.patch) fixes both:
#     1. derive the symlink dir from $TMPDIR at runtime (writable fallback), and
#     2. back regions with a plain memfd_create() syscall — no libandroid, no
#        vendor chain (the .so then needs only liblog/libdl/libc, always present).
#   libandroid-shmem is 3-clause BSD (AGPL-compatible); we redistribute only our
#   patch and fetch the upstream source at build time.
#
# OUTPUT: <workdir>/prefix/lib/libandroid-shmem.so  (overwrites the broken one)
set -euo pipefail

WORK="${1:?usage: 02-build-shmem.sh <workdir>  (same workdir as step 01)}"
NDK="${ANDROID_NDK:-$HOME/Library/Android/sdk/ndk/29.0.14206865}"
API="${ANDROID_API:-28}"   # >=26 not required (we use memfd), but a modern API is fine
HOST="darwin-x86_64"; [ "$(uname)" = Linux ] && HOST="linux-x86_64"
CC="$NDK/toolchains/llvm/prebuilt/$HOST/bin/aarch64-linux-android${API}-clang"
KIT="$(cd "$(dirname "$0")" && pwd)"
PREFIX="$WORK/prefix"; SRC="$WORK/shmem-src"
[ -x "$CC" ] || { echo "NDK clang not found: $CC (set ANDROID_NDK)" >&2; exit 1; }
[ -d "$PREFIX/lib" ] || { echo "run 01-stage-prefix.sh first (no $PREFIX/lib)" >&2; exit 1; }

# 1. Fetch pristine upstream source + license at a PINNED commit. The patch in
#    patches/ carries fixed hunk offsets and is generated against exactly this
#    revision; tracking a moving `master` would let `patch` reject silently (or
#    apply with fuzz) the day upstream drifts, breaking a kit we claim is
#    re-verifiable from the committed scripts. Bump deliberately + regenerate the
#    patch (see patches/…-portability.patch header) when you want a newer base.
SHMEM_COMMIT="${SHMEM_COMMIT:-7f0bd7e25dbdd146265aff7c6a890029e374622d}"
mkdir -p "$SRC"
base="https://raw.githubusercontent.com/termux/libandroid-shmem/$SHMEM_COMMIT"
for f in shmem.c shm.h LICENSE; do
  curl -fsSL --max-time 30 -o "$SRC/$f" "$base/$f"
done

# 2. Apply our portability patch.
( cd "$SRC" && patch -p1 < "$KIT/patches/libandroid-shmem-bionic-portability.patch" )

# 3. Cross-compile. memfd_create is reached via syscall() so no API gating is
#    needed. The patch is self-contained (it carries its own <sys/stat.h> /
#    <sys/syscall.h> includes), so no -include flags are required here.
"$CC" -shared -fPIC -O2 -Wall -Wno-unused \
  -I"$SRC" "$SRC/shmem.c" -llog -o "$PREFIX/lib/libandroid-shmem.so"

echo "=== built $PREFIX/lib/libandroid-shmem.so ==="
READELF="$NDK/toolchains/llvm/prebuilt/$HOST/bin/llvm-readelf"
echo "NEEDED:"; "$READELF" -d "$PREFIX/lib/libandroid-shmem.so" 2>/dev/null | awk '/NEEDED/{print "  "$NF}'
echo "exports shm*: $("$READELF" --dyn-syms "$PREFIX/lib/libandroid-shmem.so" 2>/dev/null | grep -cE ' shmget| shmat| shmctl| shmdt')"
echo "(expect NEEDED = liblog/libdl/libc only; NO libandroid.so)"
