# Running PostgreSQL 18 (and a Rust pgrx extension) natively on a stock Android phone

*No Termux, no root, no VM — just a relocatable bionic build, one patched shared-memory shim, and a fair amount of strace.*

---

I build an offline-first electronic health record. One of its load-bearing claims is *fractal topology*: the same node software should run at every tier — a workstation, a Raspberry A facility server, and, in principle, a phone in a clinician's pocket. The node's substrate is PostgreSQL. So the claim reduces to an uncomfortable, concrete question:

**Can you run PostgreSQL — and a Rust extension compiled into it — as first-class native code on a stock, non-rooted Android phone, with no Termux userland and no Linux VM?**

I had a high-end handset to try it on (a Snapdragon 8 Elite Gen 5, `SM8850`, 24 GB RAM, Android 16). This is the story of getting from "plug in the phone" to a `SELECT` returning rows from a pgrx extension running inside a native bionic Postgres — including every wall I hit. If you're trying to run native server software on Android, several of these walls are waiting for you too.

## The easy path is closed by the silicon, not the OS

Android 15/16 ships an official **Linux Terminal** — a pKVM-isolated Debian VM where `apt install postgresql` just works. That would have been a five-minute blog post. It doesn't work on this phone, and the reason isn't the OS:

```
ro.boot.hypervisor.protected_vm.supported = true
ro.boot.hypervisor.vm.supported           =        # empty
ro.boot.hypervisor.version                = gunyah
```

Google's Terminal needs *non-protected* VMs (host can see guest memory). Current Qualcomm flagships expose **protected VMs only**. This is platform-wide on Qualcomm and is not a toggle. MediaTek Dimensity and Exynos parts *do* expose non-protected VMs and can run the Terminal — so if you have procurement choice and want the easy path, that's your hardware filter. On Qualcomm, the only native route is a **bionic build**: PostgreSQL compiled against Android's libc.

## You don't have to build bionic Postgres from scratch

Here's the lever that makes this tractable: **Termux already maintains a PostgreSQL 18.2 build for `aarch64`.** Their patch set is the authoritative recipe for the bionic-vs-glibc gap (no System V IPC, no `sem_open`, thin locales, tzdata hardlinks, and so on). I didn't want Termux *on the phone* — that's a third-party userland a product can't depend on — but I happily used it as a *source of cross-compiled binaries*.

The Termux apt repo serves plain `.deb`s. A `.deb` is an `ar` archive containing `data.tar.*`. The plan: download `postgresql` plus its dependency closure, throw away everything except the ELF binaries and shared libraries, and flatten them into one relocatable prefix. PostgreSQL is relocatable — it derives its `share/` paths from `argv[0]` — so a tree run from `/data/local/tmp` with `LD_LIBRARY_PATH` set works fine.

Two host-tooling notes that cost me a few minutes:

- **macOS `ar` chokes on `.deb` member names.** Use `bsdtar` (libarchive), which reads the `ar` container *and* the inner `data.tar.*` directly: `bsdtar -xOf foo.deb data.tar.xz | bsdtar -xf - -C dest`.
- **Resolve the dependency closure with literal string matching, not regex.** I matched package stanzas with a regex and silently lost `libc++` — because `++` are regex quantifiers. ICU then failed to load on device. Match package names with `index()`/literal compare. (`libc++_shared.so` is needed by ICU; don't skip it.)

The closure for `postgresql` is exactly what you'd expect: `libandroid-shmem`, `libicu`, `libuuid`, `libxml2`, `openssl`, `readline`, `zlib`, `libandroid-execinfo`, `libc++`. Flatten them into `prefix/` and you have an ~90 MB self-contained Postgres.

## G0: can you even exec a binary?

Before anything clever, the cheapest high-information test on Android: **will the `shell` user exec a binary you pushed to `/data/local/tmp` under enforcing SELinux?** On locked-down devices this is where people hit a W^X wall.

Cross-compile a trivial PIE with the NDK, push, run:

```
$ adb push g0probe /data/local/tmp/ && adb shell 'cd /data/local/tmp && chmod 755 g0probe && ./g0probe'
G0_OK pid=10113 uid=2000 sys=Linux rel=6.12.23-android16 machine=aarch64
```

Green. The `shell`/adb domain (`shell_data_file` context) can exec from `/data/local/tmp`. If *this* had failed, the whole approach would have needed the APK/`jniLibs` packaging shape (binaries shipped as `lib*.so` in `nativeLibraryDir`). It didn't, so I pressed on.

**Pushing gotcha:** `adb push` of a directory tree with many `.so → .so.NN` symlinks died mid-transfer (`failed to read copy response: EOF`). Tar the prefix (preserving symlinks), push one file, extract on-device with toybox `tar`. Much faster, no symlink drama.

The binaries load and report themselves:

```
$ LD_LIBRARY_PATH=$BASE/lib $BASE/bin/postgres --version
postgres (PostgreSQL) 18.2
```

Every `NEEDED` library resolved. Then I ran `initdb` and it hung.

## Debugging a native hang on a phone you don't own (root-wise)

`initdb` sat on `selecting default "max_connections"` — the step that repeatedly spawns trial backends to probe shared-memory limits. `ps` showed the trial `postgres --check` in state `R`, burning **five minutes of CPU**, `wchan = 0`. Not blocked on a syscall, not an SELinux denial — a **pure userspace spin**.

Now the fun part: figuring out *where* it spins, on a device where every easy introspection tool is locked:

- `debuggerd -b <pid>` → *"root is required."*
- `simpleperf` hardware `cpu-cycles` → *"not supported on the device"* (PMU locked). Software `cpu-clock` → also blocked (`perf_event_open` restricted).
- `/proc/<pid>/stack` → needs root.

What *does* work without root: **strace as the parent of the process** (parent→child ptrace is always allowed; no Yama issue). Termux ships an `aarch64` strace — though getting it to run was its own dependency-peeling exercise (`libdw → libelf, argp, libbz2, liblzma → libzstd`; fetch and flatten each, just like Postgres).

The trace was unambiguous — 2.38 *million* iterations of:

```
symlinkat("...", "/data/data/com.termux/files/usr/tmp/ashv_key_132156") = -1 EACCES
readlinkat(  "/data/data/com.termux/files/usr/tmp/ashv_key_132156", ...) = -1 EACCES
```

That path is the **Termux prefix**. `libandroid-shmem` — the library that emulates System V shared memory on bionic — coordinates its segment "keys" via symlinks under a directory **baked in at compile time** to `/data/data/com.termux/files/usr/tmp`. On a stock device that directory doesn't exist and isn't writable. The lib gets `EACCES`, expects `EEXIST` (the "someone else made it first" race), and retries **forever**.

So the spike's "#1 risk" (the shared-memory shim) *was* the blocker — but for a mundane, fixable reason, not a deep Android-16 incompatibility.

## Fixing libandroid-shmem (twice)

`libandroid-shmem` is ~600 lines of BSD-licensed C. I fetched the upstream source and looked:

```c
#define ASHV_KEY_SYMLINK_PATH _PATH_TMP "ashv_key_%d"
```

Termux's build defines `_PATH_TMP` as its prefix. **Fix #1:** derive the directory from `$TMPDIR` at runtime, with a writable fallback:

```c
const char* ashv_tmpdir = getenv("TMPDIR");
if (!ashv_tmpdir || !*ashv_tmpdir) ashv_tmpdir = "/data/local/tmp";
snprintf(symlink_path, sizeof(symlink_path), "%s/ashv_key_%d", ashv_tmpdir, key);
```

I rebuilt with the NDK, pushed, re-ran. The spin became a *different* failure — progress. The symlink now got created (good), but the trace showed:

```
openat("/dev/ashmem", O_RDWR) = -1 EACCES
```

`/dev/ashmem` is **removed on Android 11+**. The source gates on the API level:

```c
#if __ANDROID_API__ >= 26
    return ASharedMemory_create(name, size);   // memfd-backed, modern
#else
    int fd = open("/dev/ashmem", O_RDWR);       // legacy, gone on Android 11+
#endif
```

Termux builds at API 24, so it took the dead `/dev/ashmem` branch. "Just build at API ≥ 26," you say. I did — and hit a *third* wall: `ASharedMemory_create` lives in `libandroid.so`, which on this handset transitively pulls a vendor library (`libvendorutils.so`) whose `BIO_flush` reference clashes with Postgres's bundled OpenSSL:

```
CANNOT LINK EXECUTABLE ".../postgres": cannot locate symbol "BIO_flush"
  referenced by "/system_ext/lib64/libvendorutils.so"
```

**Fix #2:** sidestep `libandroid` entirely. `ASharedMemory_create` is, on modern Android, a thin wrapper over `memfd_create`. So back the regions with `memfd_create` directly:

```c
int fd = (int) syscall(__NR_memfd_create, name, MFD_CLOEXEC);
if (fd < 0) return fd;
if (ftruncate(fd, (off_t) size) != 0) { /* ... */ }
return fd;
```

A `memfd` is mmap- and fd-passing-equivalent to ashmem, and `memfd_create` is a plain syscall — no `libandroid`, no vendor chain. The rebuilt `.so` now needs only `liblog`/`libdl`/`libc`, all always present. `postgres --check` returned `rc=0`. `initdb` completed. **G1 ✅.**

## G2 and the SELinux socket wall

Starting the postmaster surfaced one more device fact:

```
LOG:  listening on IPv4 address "127.0.0.1", port 5432            # TCP bind OK
LOG:  could not bind Unix address ".../.s.PGSQL.5432": Permission denied
FATAL:  could not create any Unix-domain sockets
```

You **cannot create a Unix-domain socket node under `/data/local/tmp`** — SELinux denies a `sock_file` in the `shell_data_file` context. TCP loopback binds fine. So: `unix_socket_directories=` (empty), `listen_addresses=127.0.0.1`, and connect with `psql -h 127.0.0.1`. A `CREATE TABLE / INSERT / SELECT` round-tripped, checksums on, `shared_memory_type=mmap`. **G2 ✅.**

## G3: cross-compiling a pgrx extension for Android

The real prize. The safety-critical core of my node is Rust compiled into Postgres via [pgrx](https://github.com/pgcentralfoundation/pgrx). Nobody had publicly cross-compiled a pgrx extension for `aarch64-linux-android` and loaded it into a bionic Postgres. This was supposed to be the hard part. It mostly wasn't — but the surprises were instructive.

I wrote a deliberately tiny extension touching the layers most likely to fault under a foreign libc: a plain return, argument passing, varlena/`palloc` (text in/out), and **SPI** (a call back into the executor).

**The pg_config problem.** pgrx's build script runs `pg_config --includedir-server` etc. to find the server headers. Our `pg_config` is an `aarch64` ELF — it can't run on the host. The fix is a **shell shim** named `pg_config` that returns the right strings, with include/lib paths pointing at the *host* copy of the staged prefix (so bindgen reads the headers locally). Point `PGRX_PG_CONFIG_PATH` at it.

**bindgen "just worked."** This was the feared gotcha, and it was a non-event. With the shim plus:

```
BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-linux-android24 --sysroot=<NDK>/sysroot \
  -I<prefix>/include/postgresql/server -I<prefix>/include"
LIBCLANG_PATH=<NDK>/.../lib
```

bindgen parsed the bionic server headers for the device ABI and generated `pg18.rs` on the first try.

**cc-rs needs the NDK's versioned clang.** The only build friction: pgrx's C shim is compiled by `cc-rs`, which looks for `aarch64-linux-android-clang` (unversioned). The NDK ships `aarch64-linux-androidNN-clang`. Set the env explicitly:

```
CC_aarch64_linux_android=<NDK>/.../aarch64-linux-android28-clang
AR_aarch64_linux_android=<NDK>/.../llvm-ar
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=<NDK>/.../aarch64-linux-android28-clang
```

Then `cargo build --release --target aarch64-linux-android` produced a clean `.so` with `Pg_magic_func` and all the `*_wrapper` symbols. (NEEDED: just `libdl`/`libc`; Postgres's own symbols resolve at `LOAD` time inside the postmaster.)

**`cargo pgrx schema` is impossible here** — it generates the install SQL by `dlopen`-ing the built `.so`, which is ARM. But the control file and the install SQL are architecture-independent, so **hand-write them**: bind each function to its pgrx `*_wrapper` symbol with `LANGUAGE c`, and set `module_pathname = '$libdir/cairn_smoke'` in the `.control`.

Push the `.so` into `pkglibdir`, the control+SQL into `sharedir/extension`, start the server, and:

```
CREATE EXTENSION
(1) plain return     cairn_smoke_answer()           = 42
(2) arg passing      cairn_smoke_add(40,2)          = 42
(3) varlena/palloc   cairn_smoke_echo('phone-node') = cairn:phone-node
(4) SPI              cairn_smoke_spi()              = 3
```

**G3 ✅.** A Rust pgrx extension, cross-compiled for Android, executing inside a native bionic Postgres on a stock Snapdragon phone — including a call back into the executor via SPI.

## Takeaways for anyone doing this

- **Check `ro.boot.hypervisor.*` first.** It decides whether you get the easy VM path or the bionic grind. It's a hardware property, not a setting.
- **Reuse Termux as a binary source, not a runtime.** Their `.deb`s flatten into a relocatable prefix; nothing Termux-specific needs to touch the device.
- **G0 (exec from `/data/local/tmp`) is the cheapest test that can kill your whole approach.** Run it before building anything large.
- **Without root, `strace` as a parent is your scalpel.** `debuggerd`, `perf`, and `/proc/pid/stack` all want root; parent→child ptrace doesn't.
- **`libandroid-shmem` from prebuilt Termux will not work unmodified on a stock device.** Two fixes: derive its key path from `$TMPDIR`, and back regions with `memfd_create` (skip both `/dev/ashmem` and `libandroid.so`, the latter of which can drag in vendor libs that clash with bundled OpenSSL).
- **No Unix-domain sockets under `/data/local/tmp`** — run Postgres TCP-only on loopback.
- **pgrx cross-compiles cleanly** with a `pg_config` shim, `BINDGEN_EXTRA_CLANG_ARGS` for the target, and `CC_*`/linker env pointed at the NDK's versioned clang. Hand-write the control+SQL because `cargo pgrx schema` can't `dlopen` an ARM `.so`.

The headline result: *the substrate is portable all the way down to the phone tier, and so is the Rust escape hatch.* If your project leans on "the same code runs everywhere," it's worth proving on the most hostile substrate you can find — the failures are specific, fixable, and far less scary than the silence of an untested claim.

*The full reproducible kit — staging, the shared-memory patch, the pgrx crate, and the on-device run scripts — is open source (AGPL-3.0); the patched `libandroid-shmem` is BSD and pgrx is MIT, both compatible.*
