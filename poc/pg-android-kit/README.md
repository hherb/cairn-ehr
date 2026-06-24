# cairn-pg-android-kit

Build-prep kit for **[Spike 0003](../../docs/spikes/0003-postgres-on-android-bionic-node.md)** —
running a Cairn node's substrate (native PostgreSQL 18) on a **stock Android phone**: no Termux
userland, no root, no Linux VM.

**Result on the target handset (RedMagic 11 Pro / Snapdragon 8 Elite Gen 5 / Android 16):
G0–G2 PASS.** PostgreSQL 18.2 runs as a native bionic process, `initdb` builds a cluster, and the
postmaster serves real SQL over TCP. G3 (a pgrx extension) is not yet covered by this kit.

This is a feasibility probe, not a product. See the spike doc for scope and the architectural "why".

---

## What's here

| File | Role |
|---|---|
| `01-stage-prefix.sh` | Download Termux's prebuilt PG-18 `.debs` (+ dependency closure) and flatten them into one relocatable `prefix/` tree. Termux is used only as a *source of cross-compiled aarch64 binaries*; none of its userland lands on the phone. |
| `02-build-shmem.sh` | Fetch upstream `libandroid-shmem` (3-clause BSD), apply our portability patch, and cross-compile a fixed `libandroid-shmem.so` (NDK) that overwrites the broken prebuilt one. |
| `patches/libandroid-shmem-bionic-portability.patch` | Our two-part fix to `libandroid-shmem` (see below). |
| `03-run-on-device.sh` | Push the prefix to a connected device and run the G0/G1/G2 gates. |

## Prerequisites

- macOS or Linux host with the **Android NDK** (set `ANDROID_NDK`; defaults to the macOS SDK path)
  and **platform-tools** (`adb`; set `ADB` if not on `PATH`).
- `curl` and `bsdtar` (libarchive) on the host.
- A phone connected with **USB debugging** enabled. Find its serial with `adb devices`.

## Run it

```bash
WORK=/tmp/pg-android            # any scratch dir
SERIAL=XXXXXXXX                 # from `adb devices`

./01-stage-prefix.sh  "$WORK"
./02-build-shmem.sh   "$WORK"   # MUST run after 01 — it overwrites the broken shmem lib
./03-run-on-device.sh "$WORK" "$SERIAL"
```

`03` prints the version, the `initdb` result, and a `CREATE/INSERT/SELECT` smoke test. **PASS** =
all three succeed. The install is left at `/data/local/tmp/cairnpg` (server stopped); remove it with
`adb shell rm -rf /data/local/tmp/cairnpg`.

---

## The findings that shaped the kit

### §3 procurement fact (confirmed on-device)
`ro.boot.hypervisor.protected_vm.supported = true` while `ro.boot.hypervisor.vm.supported` is empty
(Gunyah hypervisor). So Android's stock **AVF Linux Terminal** path (a non-protected Debian VM where
`apt install postgresql` just works) is **denied on this Qualcomm flagship**. The native bionic build
in this kit is the only route. MediaTek/Exynos handsets expose non-protected VMs and can use the easy
path instead. Nothing in Cairn controls this; it is a hardware-selection input.

### The one real blocker: `libandroid-shmem` (and its two-part fix)
PostgreSQL needs System V shared memory; bionic has none, so Termux's `libandroid-shmem` emulates it.
The prebuilt lib has **two** independent defects on a stock (non-Termux) device, both found by `strace`:

1. **Infinite CPU spin.** It coordinates SysV-shm keys with symlinks under a path baked at compile
   time to the Termux prefix (`/data/data/com.termux/files/usr/tmp`). On a stock device that path is
   unwritable → `EACCES` → it busy-loops forever (observed: millions of `symlinkat`/`readlinkat`
   retries). **Fix:** derive the directory from `$TMPDIR` at runtime, with a writable fallback.
2. **`/dev/ashmem` is gone (Android 11+).** Termux builds at API 24, so the lib backs regions with the
   legacy `/dev/ashmem` device, which returns `EACCES` on Android 16. Building at API ≥ 26 switches to
   `ASharedMemory_create`, but that links `libandroid.so`, which on this handset pulls a vendor lib
   (`libvendorutils.so`) whose `BIO_flush` clashes with PostgreSQL's bundled OpenSSL. **Fix:** back
   regions with a plain `memfd_create()` syscall — no `libandroid`, no vendor chain. The resulting
   `.so` needs only `liblog`/`libdl`/`libc`, which are always present.

### Operational note: no Unix-domain sockets
Creating a Unix-domain socket under `/data/local/tmp` is **SELinux-denied** (a socket node in the
`shell_data_file` context). TCP loopback binds fine, so the kit runs **TCP-only**
(`unix_socket_directories=` empty, `listen_addresses=127.0.0.1`).

---

## Honest limits

- Uses Termux's **prebuilt** PG binaries (the fast route to G1/G2), not a from-source build we drive
  ourselves. Legitimate for a feasibility probe; the one lib that mattered (`libandroid-shmem`) *is*
  built from source here.
- **G3 (pgrx) is not covered.** Cross-compiling a pgrx extension for `aarch64-linux-android` and
  loading it into this bionic Postgres is the remaining unproven step (see the spike doc §5).
- Runs as the `shell` user from `/data/local/tmp`. A real phone node would need the APK/`jniLibs`
  packaging shape; that is out of scope here (and G0 passing means it is not forced).

## Licensing
`libandroid-shmem` is 3-clause BSD (AGPL-compatible). We redistribute only our patch and fetch the
upstream source at build time; the upstream `LICENSE` is downloaded alongside it into the work dir.
