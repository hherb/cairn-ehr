#!/usr/bin/env python3
"""Bet B benchmark harness — Spike 0001 §6 (the Pi compute-cost bet).

Drives the `cairn-sync` binary to produce the §6 pass/fail table. No Python
dependencies (stdlib only, no pip install); the DB-backed `selftest` does shell out
to the `psql` client, which is present on any node running PostgreSQL.

IMPORTANT: use a **release** binary — `cargo build --release` — or the crypto and
projection numbers are meaningless (debug Ed25519/BLAKE3 are an order of magnitude
slow). The harness warns if the `--bin` path is not a release build. The DB-backed
rows (B1/B2) must run **on the Pi itself** against its local PostgreSQL; the crypto
rows (B3/B4) are pure CPU.

WHY THE ENVIRONMENT BLOCK MATTERS. Bet B is a *hardware-class* bet — its whole point
is "does this hold on weak hardware." A §6 number with no record of the board, the
PostgreSQL version, whether PGDATA sat on the SSD or the SD card, the CPU governor,
and whether the Pi thermally throttled mid-run is not reproducible and not
trustworthy. (Concretely: the same release binary measured SHA-256 at ~1500 MB/s on
a host with SHA-NI and ~200 MB/s on one without — the number means nothing without
the host.) So every run prints, and `--json-out` records, a self-describing
environment header. Pi-only probes (`vcgencmd`, the cpufreq governor) degrade to
"n/a" off-Pi so the same harness still runs in CI / on the x86 dev box.

FINDING THE FLOOR. The user's question is "is the Pi 5 / 16 GB the realistic floor?"
The gates (B1 flat + fast, B2 sub-second, B4 verify keeps up) answer go/no-go on one
board; the **headroom** appended to each gated row (how many × under budget / over
floor the measurement landed) is what predicts whether a *smaller* board would still
pass — so a single Pi 5 run already informs the floor question, before the second
board is even plugged in. Run each board with a distinct `--label` and keep the two
`--json-out` records side by side to compare.

WARNING: `selftest` **drops and recreates** the Cairn tables on the target DB before
it runs. It is destructive by design (a benchmark needs a known-empty log). It refuses
to run without `--force` so a mistyped `--conn` cannot silently wipe a real database.

§6 rows:
  B1 projection maintenance : single-op maintained-write latency, and that it does
                              NOT grow with log size (the ADR-0001 load-bearing bet)
  B2 chart read             : full chart assembly beats "grab the paper chart" (sub-second)
  B3 keystore cost          : DEK-wrap / body-seal throughput → per-event vs per-episode
                              crypto-shred key granularity (ADR-0005)
  B4 crypto on ARM          : Ed25519 verify/s (the safety gate keeps up) and
                              SHA-256-vs-BLAKE3 (the input to ADR-0015's provisional
                              blob-digest default)
"""

import argparse
import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from statistics import median

# Bumped when the emitted JSON record shape or the §6 method changes, so a recorded
# result is self-identifying.
HARNESS_VERSION = "0001-betB/2"


def p95(xs):
    if not xs:
        return 0.0
    s = sorted(xs)
    return s[min(len(s) - 1, int(round(0.95 * (len(s) - 1))))]


def run_json(bin_path, *args):
    cmd = [bin_path, *args]
    out = subprocess.run(cmd, capture_output=True, text=True)
    if out.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)}\n{out.stderr.strip()}")
    line = [l for l in out.stdout.splitlines() if l.strip().startswith("{")][-1]
    return json.loads(line)


def psql(conn, sql):
    out = subprocess.run(["psql", conn, "-tAc", sql], capture_output=True, text=True, check=True)
    return out.stdout.strip()


# ---------------------------------------------------------------------------
# Environment capture — all probes degrade gracefully to None off their target,
# so the same harness runs on a Pi, on the x86 dev box, and in CI.
# ---------------------------------------------------------------------------

def _read_file(path):
    try:
        with open(path) as f:
            return f.read().strip()
    except OSError:
        return None


def _sh(cmd):
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=10)
        return out.stdout.strip() if out.returncode == 0 else None
    except (OSError, subprocess.SubprocessError):
        return None


def _cpuinfo():
    """(board, cpu): the Pi puts its board string in a 'Model' line; x86 has
    'model name'; some ARM kernels only expose 'Hardware'."""
    txt = _read_file("/proc/cpuinfo") or ""
    board = cpu = None
    for line in txt.splitlines():
        if ":" not in line:
            continue
        k, v = (s.strip() for s in line.split(":", 1))
        kl = k.lower()
        # The Pi exposes a descriptive board string ("Raspberry Pi 5 Model B Rev 1.0")
        # under 'Model'; x86 exposes a *numeric* 'model' (e.g. "85") that is not a board.
        # Require an alphabetic character so we only capture the real board string.
        if kl == "model" and any(ch.isalpha() for ch in v) and not board:
            board = v
        elif kl in ("model name", "hardware") and v and not cpu:
            cpu = v
    return board, cpu


def _mem_total_gb():
    for line in (_read_file("/proc/meminfo") or "").splitlines():
        if line.startswith("MemTotal:"):
            return round(int(line.split()[1]) / (1024 * 1024), 1)
    return None


def _throttled():
    """Pi-only. vcgencmd get_throttled -> 'throttled=0x0'. Low bits = throttling
    happening now; bits 16-19 = it happened at some point this boot. Either makes a
    crypto/CPU number suspect."""
    raw = _sh(["vcgencmd", "get_throttled"])
    if not raw or "=" not in raw:
        return None
    val = raw.split("=", 1)[1].strip()
    try:
        n = int(val, 16)
    except ValueError:
        return {"raw": raw}
    return {"raw": val, "clean": n == 0,
            "throttled_now": bool(n & 0x7), "throttled_this_boot": bool(n & 0x70000)}


def _pg(conn, sql):
    if not conn:
        return None
    try:
        return psql(conn, sql)
    except Exception:
        return None


def _pg_storage(conn, data_dir):
    """Map PGDATA to its backing block device and flag the SD-card case — the single
    biggest way to render B1/B2 meaningless on a Pi."""
    if not conn or not data_dir:
        return None, None
    out = _sh(["df", "--output=source", data_dir])
    device = None
    if out:
        lines = [l for l in out.splitlines() if l.strip()]
        if len(lines) >= 2:
            device = lines[-1].strip()
    on_sd = bool(device and "mmcblk" in device)  # Pi SD cards are /dev/mmcblk*
    return device, on_sd


def capture_env(bin_path, conn=None, label=None):
    u = os.uname()
    board, cpu = _cpuinfo()
    data_dir = _pg(conn, "show data_directory")
    device, on_sd = _pg_storage(conn, data_dir)
    settings = {s: _pg(conn, f"show {s}") for s in (
        "shared_buffers", "effective_cache_size", "max_wal_size", "work_mem",
        "fsync", "synchronous_commit", "wal_compression", "max_worker_processes",
    )}
    bin_real = os.path.realpath(bin_path)
    profile = "release" if f"{os.sep}release{os.sep}" in bin_real + os.sep \
        else ("debug" if f"{os.sep}debug{os.sep}" in bin_real + os.sep else "unknown")
    return {
        "harness_version": HARNESS_VERSION,
        "label": label,
        "captured_at_utc": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "host": u.nodename,
        "arch": u.machine,
        "kernel": f"{u.sysname} {u.release}",
        "board": board,
        "cpu": cpu,
        "cores": os.cpu_count(),
        "mem_total_gb": _mem_total_gb(),
        "cpu_governor": _read_file("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"),
        "throttle": _throttled(),
        "bin": bin_real,
        "bin_profile": profile,
        "pg_version": _pg(conn, "select version()"),
        "pg_server_version": _pg(conn, "show server_version"),
        "pg_data_directory": data_dir,
        "pg_storage_device": device,
        "pg_on_sd_card": on_sd,
        "pg_settings": settings,
    }


def print_env(env):
    print("\nEnvironment  (the §6 numbers below are only meaningful read WITH this header)")
    print("-" * 78)

    def row(k, v):
        print(f"  {k:<22}{v}")

    if env.get("label"):
        row("label", env["label"])
    row("host / arch", f"{env['host']} / {env['arch']}")
    row("board", env.get("board") or "(no /proc/cpuinfo Model line — not a Pi / unknown)")
    if env.get("cpu"):
        row("cpu", env["cpu"])
    row("cores / RAM", f"{env.get('cores')} cores / {env.get('mem_total_gb')} GB")
    row("kernel", env["kernel"])

    gov = env.get("cpu_governor")
    row("cpu governor", gov if gov is not None else "n/a")

    thr = env.get("throttle")
    if thr is None:
        row("throttle", "n/a (no vcgencmd — not a Pi)")
    elif isinstance(thr, dict) and "clean" in thr:
        if thr["clean"]:
            row("throttle", "clean (0x0)")
        else:
            flags = []
            if thr.get("throttled_now"):
                flags.append("THROTTLING NOW")
            if thr.get("throttled_this_boot"):
                flags.append("throttled earlier this boot")
            row("throttle", f"{thr.get('raw')}  <-- {', '.join(flags) or 'non-zero'}")
    else:
        row("throttle", str(thr))

    row("postgres", env.get("pg_server_version") or env.get("pg_version") or "unknown")
    dd = env.get("pg_data_directory")
    if dd:
        dev = env.get("pg_storage_device") or "?"
        row("pg data dir", f"{dd}  [{dev}]")
    st = env.get("pg_settings") or {}
    sb = " · ".join(f"{k}={st[k]}" for k in ("shared_buffers", "synchronous_commit", "fsync") if st.get(k))
    if sb:
        row("pg tuning", sb)
    prof = env.get("bin_profile")
    row("binary", f"{env['bin']}  [{prof}]")
    print("-" * 78)


def env_warnings(env):
    """Return a list of human-readable warnings about a measurement context that
    would make the numbers misleading."""
    w = []
    gov = env.get("cpu_governor")
    if gov not in (None, "performance"):
        w.append(f"CPU governor is '{gov}', not 'performance' — expect slower, noisier numbers; "
                 "set it with `sudo cpupower frequency-set -g performance` before the real run.")
    thr = env.get("throttle")
    if isinstance(thr, dict) and thr.get("clean") is False:
        w.append(f"Pi reports throttling (get_throttled={thr.get('raw')}) — add cooling and re-run; "
                 "crypto/CPU numbers taken while throttled are not the hardware's real ceiling.")
    if env.get("pg_on_sd_card"):
        w.append("PGDATA is on an SD card (/dev/mmcblk*) — B1/B2 measure the SD card, not the design. "
                 "Move the data directory to the SSD (see PI-RUNBOOK.md) and re-run.")
    if env.get("bin_profile") != "release":
        w.append(f"Binary is a '{env.get('bin_profile')}' build, not release — crypto and projection "
                 "numbers are an order of magnitude off. Rebuild with `cargo build --release`.")
    pgv = (env.get("pg_server_version") or "")
    if pgv and pgv.split(".")[0].isdigit() and int(pgv.split(".")[0]) < 18:
        w.append(f"PostgreSQL {pgv} is below the project floor (≥18). The skeleton runs on it, but record "
                 "the real run on PG 18 so the number matches the deployment floor.")
    return w


def render_table(rows):
    # `passed` is True/False for a real gate, or None for an informational row that
    # is measured-and-reported but never fails the run (e.g. B3 keystore cost).
    w = max(len(r[1]) for r in rows)
    print(f"\n{'':4}{'check':<{w}}  {'result':<6}  detail")
    print("-" * (12 + w + 56))
    ok = True
    for code, name, passed, detail in rows:
        if passed is None:
            result = "INFO"
        else:
            ok = ok and passed
            result = "PASS" if passed else "FAIL"
        print(f"{code:<4}{name:<{w}}  {result:<6}  {detail}")
    print("-" * (12 + w + 56))
    print(f"\nBet B: {'PASS — go on this hardware' if ok else 'FAIL — see the mitigation ladder below'}\n")
    return ok


def headroom_lower(measured, budget):
    """For 'lower is better' gates (latency): how many × under budget we landed."""
    return float("inf") if measured <= 0 else budget / measured


def headroom_higher(measured, floor):
    """For 'higher is better' gates (verify/s): how many × over the floor."""
    return float("inf") if floor <= 0 else measured / floor


def write_json_out(path, env, thresholds, rows, ok):
    record = {
        "spike": "0001",
        "bet": "B",
        "harness_version": HARNESS_VERSION,
        "verdict": "PASS" if ok else "FAIL",
        "environment": env,
        "thresholds": thresholds,
        "rows": [{"code": c, "check": n,
                  "result": ("INFO" if p is None else ("PASS" if p else "FAIL")),
                  "detail": d} for (c, n, p, d) in rows],
    }
    with open(path, "w") as f:
        json.dump(record, f, indent=2)
        f.write("\n")
    print(f"Recorded a self-describing result to {path}")


def cmd_bench(args):
    env = capture_env(args.bin, conn=None, label=args.label)
    print_env(env)
    for warning in env_warnings(env):
        print(f"  ! {warning}")
    c = run_json(args.bin, "bench",
                 "--hash-mb", str(args.hash_mb),
                 "--sig-iters", str(args.sig_iters),
                 "--dek-iters", str(args.dek_iters))
    rows = [
        ("B3", "keystore cost (crypto-shred)", None,
         f"DEK-wrap {c['dek_wrap_per_s']:,.0f}/s, body-seal {c['body_seal_mbps']:.0f} MB/s "
         f"(per-episode unwrap is 1 op; per-event is N)"),
        ("B4", "crypto on ARM (verify + hash)",
         c["ed25519_verify_per_s"] >= args.verify_floor,
         f"Ed25519 {c['ed25519_verify_per_s']:,.0f} verify/s "
         f"({headroom_higher(c['ed25519_verify_per_s'], args.verify_floor):.1f}x over floor {args.verify_floor:,.0f}); "
         f"BLAKE3 {c['blake3_mbps']:.0f} vs SHA-256 {c['sha256_mbps']:.0f} MB/s "
         f"({'BLAKE3 faster — ADR-0015 blob default holds' if c['blake3_faster_than_sha256'] else 'SHA-256 faster — revisit ADR-0015 blob default'})"),
    ]
    ok = render_table(rows)
    thresholds = {"verify_floor_per_s": args.verify_floor}
    if args.json_out:
        write_json_out(args.json_out, env, thresholds, rows, ok)
    # `bench` alone never fails the process on B4 unless explicitly gating; keep exit 0
    # for the quick crypto-only look unless the verify floor was missed.
    sys.exit(0 if ok else 1)


def cmd_selftest(args):
    conn = args.conn
    if not args.force:
        sys.exit(
            f"refusing to run: selftest DROPs and recreates the Cairn tables on:\n  {conn}\n"
            "This is destructive. Re-run with --force once you have confirmed the target DB."
        )

    env = capture_env(args.bin, conn=conn, label=args.label)
    print_env(env)
    warnings = env_warnings(env)
    for warning in warnings:
        print(f"  ! {warning}")

    # Fresh DB.
    psql(conn, "drop table if exists event_log,hlc_state,sync_state,patient_chart,blob_store,blob_chunk cascade;")
    init_db(args.bin, conn)

    sizes = sorted(args.sizes)
    b1 = []  # (log_size, p95 maintained-write latency), in sample order
    for target in sizes:
        have = int(psql(conn, "select count(*) from event_log") or 0)
        if target > have:
            # gen creates `--patients` demographic events + `count` notes, round-robin
            # across the panel, so the fattest patient's chart (B2) is ~count/patients
            # notes — keep the panel large enough that this stays a *realistic* chart.
            subprocess.run([args.bin, "gen", "--conn", conn, "--node", "pi",
                            "--key", "/tmp/cairn_bench.key", "--patients", str(args.patients),
                            "--count", str(target - have)],
                           capture_output=True, text=True, check=True)
        m = run_json(args.bin, "bench-insert", "--conn", conn, "--node", "pi",
                     "--key", "/tmp/cairn_bench.key", "--count", str(args.insert_count))
        b1.append((m["log_size"], m["p95_ms"]))
        print(f"  B1 @ {m['log_size']:>9} events: p50 {m['p50_ms']:.2f}ms  p95 {m['p95_ms']:.2f}ms")

    # B2: time the fattest patient's full chart, a few times. The chart op also
    # reports the note count, so capture it from the reads rather than re-querying.
    fattest = psql(conn, "select patient_id from patient_chart order by note_count desc limit 1")
    reads = [run_json(args.bin, "chart", "--conn", conn, "--patient", fattest)
             for _ in range(args.chart_reads)]
    chart = [r["elapsed_ms"] for r in reads]
    notes = reads[-1]["notes"]

    # B3/B4: pure-CPU crypto.
    c = run_json(args.bin, "bench", "--hash-mb", str(args.hash_mb),
                 "--sig-iters", str(args.sig_iters), "--dek-iters", str(args.dek_iters))

    (small, small_p95), (large, large_p95) = b1[0], b1[-1]
    growth = large_p95 / small_p95 if small_p95 > 0 else float("inf")
    b1_flat = growth <= args.growth_factor
    b1_fast = large_p95 <= args.insert_budget_ms
    b2_p95 = p95(chart)

    rows = [
        ("B1", "projection maintenance (single-op)",
         b1_fast and b1_flat,
         f"p95 {large_p95:.2f}ms @ {large:,} events ({headroom_lower(large_p95, args.insert_budget_ms):.0f}x under "
         f"budget {args.insert_budget_ms}ms); growth x{growth:.2f} vs {small:,} events (flat<=x{args.growth_factor})"),
        ("B2", "chart read beats paper",
         b2_p95 <= args.chart_budget_ms,
         f"p50 {median(chart):.1f}ms  p95 {b2_p95:.1f}ms over {notes:,} notes "
         f"({headroom_lower(b2_p95, args.chart_budget_ms):.0f}x under budget {args.chart_budget_ms}ms)"),
        # B3 is informational (INFO, never FAIL): the keystore cost is reported to
        # inform per-event vs per-episode DEK granularity, not gated against a budget.
        ("B3", "keystore cost (crypto-shred)",
         None,
         f"DEK-wrap {c['dek_wrap_per_s']:,.0f}/s, body-seal {c['body_seal_mbps']:.0f} MB/s "
         f"(per-episode unwrap is 1 op; per-event is N)"),
        ("B4", "crypto on ARM (verify + hash)",
         c["ed25519_verify_per_s"] >= args.verify_floor,
         f"Ed25519 {c['ed25519_verify_per_s']:,.0f} verify/s "
         f"({headroom_higher(c['ed25519_verify_per_s'], args.verify_floor):.1f}x over floor {args.verify_floor:,.0f}); "
         f"BLAKE3 {c['blake3_mbps']:.0f} vs SHA-256 {c['sha256_mbps']:.0f} MB/s "
         f"({'BLAKE3 faster — ADR-0015 blob default holds' if c['blake3_faster_than_sha256'] else 'SHA-256 faster — revisit ADR-0015 blob default'})"),
    ]

    print(f"\nNOTE: run this ON THE TARGET (a Pi) for real numbers — these reflect whatever ran it.")
    ok = render_table(rows)
    thresholds = {
        "insert_budget_ms": args.insert_budget_ms,
        "growth_factor": args.growth_factor,
        "chart_budget_ms": args.chart_budget_ms,
        "verify_floor_per_s": args.verify_floor,
        "sizes": sizes,
        "patients": args.patients,
    }
    if args.json_out:
        write_json_out(args.json_out, env, thresholds, rows, ok)
    if not ok:
        print("On a miss, the remedy depends on WHICH gate failed:\n"
              "  B1/B2 (projection/chart cost) -> mitigation ladder (ADR-0002): PL/pgSQL\n"
              "         -> pgrx (in-DB Rust) for the hot projection -> external Rust. A miss\n"
              "         tells you which rung, not whether the design works.\n"
              "  B4 (crypto throughput) -> a hardware-class signal: faster node, or accept\n"
              "         SHA-256 for blobs (ADR-0015's provisional line) — not a projection fix.\n")
    sys.exit(0 if ok else 1)


def init_db(bin_path, conn):
    subprocess.run([bin_path, "init", "--conn", conn], capture_output=True, text=True, check=True)


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    default_bin = os.path.join(here, "..", "target", "release", "cairn-sync")

    ap = argparse.ArgumentParser(description="Bet B benchmark harness (Spike 0001 §6)")
    ap.add_argument("--bin", default=default_bin, help="path to a RELEASE cairn-sync")
    sub = ap.add_subparsers(dest="cmd", required=True)

    st = sub.add_parser("selftest", help="run the whole §6 table against one local DB")
    st.add_argument("--conn", required=True)
    st.add_argument("--force", action="store_true",
                    help="confirm the target DB may be DROPped and recreated (required)")
    st.add_argument("--sizes", type=int, nargs="+", default=[2000, 20000],
                    help="log sizes (events) to sample B1 at — use big ones on the Pi "
                         "(see PI-RUNBOOK.md for the prescribed ladder)")
    st.add_argument("--patients", type=int, default=200,
                    help="demographic panel size; the fattest patient's chart (B2) is "
                         "~count/patients notes — scale up with --sizes to keep B2 realistic")
    st.add_argument("--insert-count", type=int, default=200, help="B1 maintained-writes per sample")
    st.add_argument("--chart-reads", type=int, default=20)
    st.add_argument("--insert-budget-ms", type=float, default=50.0)
    st.add_argument("--growth-factor", type=float, default=3.0)
    st.add_argument("--chart-budget-ms", type=float, default=1000.0)
    st.add_argument("--verify-floor", type=float, default=2000.0)
    st.add_argument("--hash-mb", type=int, default=256)
    st.add_argument("--sig-iters", type=int, default=20000)
    st.add_argument("--dek-iters", type=int, default=100000)
    st.add_argument("--label", default=None,
                    help="free-text tag for the board (e.g. 'pi5-16gb-nvme') — recorded in the env header")
    st.add_argument("--json-out", default=None,
                    help="write a self-describing JSON result (env + thresholds + rows) to this path")
    st.set_defaults(func=cmd_selftest)

    bn = sub.add_parser("bench", help="just the pure-CPU crypto numbers (B3/B4)")
    bn.add_argument("--hash-mb", type=int, default=256)
    bn.add_argument("--sig-iters", type=int, default=20000)
    bn.add_argument("--dek-iters", type=int, default=100000)
    bn.add_argument("--verify-floor", type=float, default=2000.0)
    bn.add_argument("--label", default=None,
                    help="free-text tag for the board (e.g. 'pi5-16gb-nvme') — recorded in the env header")
    bn.add_argument("--json-out", default=None,
                    help="write a self-describing JSON result (env + B3/B4 rows) to this path")
    bn.set_defaults(func=cmd_bench)

    args = ap.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
