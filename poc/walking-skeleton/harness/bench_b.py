#!/usr/bin/env python3
"""Bet B benchmark harness — Spike 0001 §6 (the Pi compute-cost bet).

Drives the `cairn-sync` binary to produce the §6 pass/fail table. Stdlib only, so
it runs on a Pi with no pip install.

IMPORTANT: use a **release** binary — `cargo build --release` — or the crypto and
projection numbers are meaningless (debug Ed25519/BLAKE3 are an order of magnitude
slow). The DB-backed rows (B1/B2) must run **on the Pi itself** against its local
PostgreSQL; the crypto rows (B3/B4) are pure CPU.

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
from statistics import median


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


def render_table(rows):
    w = max(len(r[1]) for r in rows)
    print(f"\n{'':4}{'check':<{w}}  {'result':<6}  detail")
    print("-" * (12 + w + 48))
    ok = True
    for code, name, passed, detail in rows:
        ok = ok and passed
        print(f"{code:<4}{name:<{w}}  {'PASS' if passed else 'FAIL':<6}  {detail}")
    print("-" * (12 + w + 48))
    print(f"\nBet B: {'PASS — go on this hardware' if ok else 'FAIL — see the mitigation ladder below'}\n")
    return ok


def cmd_bench(args):
    print(json.dumps(run_json(args.bin, "bench",
                              "--hash-mb", str(args.hash_mb),
                              "--sig-iters", str(args.sig_iters),
                              "--dek-iters", str(args.dek_iters)), indent=2))


def cmd_selftest(args):
    conn = args.conn
    # Fresh DB.
    psql(conn, "drop table if exists event_log,hlc_state,sync_state,patient_chart,blob_store cascade;")
    init_db(args.bin, conn)

    sizes = sorted(args.sizes)
    b1 = {}  # log_size -> p95 maintained-write latency
    for target in sizes:
        have = int(psql(conn, "select count(*) from event_log") or 0)
        if target > have:
            # gen creates `patients` demographic events + `count` notes.
            subprocess.run([args.bin, "gen", "--conn", conn, "--node", "pi",
                            "--key", "/tmp/cairn_bench.key", "--patients", "20",
                            "--count", str(target - have)],
                           capture_output=True, text=True, check=True)
        m = run_json(args.bin, "bench-insert", "--conn", conn, "--node", "pi",
                     "--key", "/tmp/cairn_bench.key", "--count", str(args.insert_count))
        b1[m["log_size"]] = m["p95_ms"]
        print(f"  B1 @ {m['log_size']:>8} events: p50 {m['p50_ms']:.2f}ms  p95 {m['p95_ms']:.2f}ms")

    # B2: time the fattest patient's full chart, a few times.
    fattest = psql(conn, "select patient_id from patient_chart order by note_count desc limit 1")
    chart = [run_json(args.bin, "chart", "--conn", conn, "--patient", fattest)["elapsed_ms"]
             for _ in range(args.chart_reads)]
    notes = run_json(args.bin, "chart", "--conn", conn, "--patient", fattest)["notes"]

    # B3/B4: pure-CPU crypto.
    c = run_json(args.bin, "bench", "--hash-mb", str(args.hash_mb),
                 "--sig-iters", str(args.sig_iters), "--dek-iters", str(args.dek_iters))

    small, large = min(b1), max(b1)
    growth = b1[large] / b1[small] if b1[small] > 0 else float("inf")
    b1_flat = growth <= args.growth_factor
    b1_fast = b1[large] <= args.insert_budget_ms
    b2_p95 = p95(chart)

    rows = [
        ("B1", "projection maintenance (single-op)",
         b1_fast and b1_flat,
         f"p95 {b1[large]:.2f}ms @ {large} events; growth x{growth:.2f} vs {small} events "
         f"(budget {args.insert_budget_ms}ms, flat<=x{args.growth_factor})"),
        ("B2", "chart read beats paper",
         b2_p95 <= args.chart_budget_ms,
         f"p50 {median(chart):.1f}ms  p95 {b2_p95:.1f}ms over {notes} notes (budget {args.chart_budget_ms}ms)"),
        ("B3", "keystore cost (crypto-shred)",
         c["body_seal_mbps"] > 0,
         f"DEK-wrap {c['dek_wrap_per_s']:,.0f}/s, body-seal {c['body_seal_mbps']:.0f} MB/s "
         f"(per-episode unwrap is 1 op; per-event is N)"),
        ("B4", "crypto on ARM (verify + hash)",
         c["ed25519_verify_per_s"] >= args.verify_floor,
         f"Ed25519 {c['ed25519_verify_per_s']:,.0f} verify/s; "
         f"BLAKE3 {c['blake3_mbps']:.0f} vs SHA-256 {c['sha256_mbps']:.0f} MB/s "
         f"({'BLAKE3 faster — ADR-0015 blob default holds' if c['blake3_faster_than_sha256'] else 'SHA-256 faster — revisit ADR-0015 blob default'})"),
    ]

    print(f"\nNOTE: run this ON THE TARGET (a Pi) for real numbers — these reflect whatever ran it.")
    ok = render_table(rows)
    if not ok:
        print("Mitigation ladder (ADR-0002) if B1/B2 miss: PL/pgSQL -> pgrx (in-DB Rust)\n"
              "for the hot projection -> external Rust. A miss tells you WHICH rung, not\n"
              "whether the design works.\n")
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
    st.add_argument("--sizes", type=int, nargs="+", default=[2000, 20000],
                    help="log sizes (events) to sample B1 at — use big ones on the Pi")
    st.add_argument("--insert-count", type=int, default=200, help="B1 maintained-writes per sample")
    st.add_argument("--chart-reads", type=int, default=20)
    st.add_argument("--insert-budget-ms", type=float, default=50.0)
    st.add_argument("--growth-factor", type=float, default=3.0)
    st.add_argument("--chart-budget-ms", type=float, default=1000.0)
    st.add_argument("--verify-floor", type=float, default=2000.0)
    st.add_argument("--hash-mb", type=int, default=256)
    st.add_argument("--sig-iters", type=int, default=20000)
    st.add_argument("--dek-iters", type=int, default=100000)
    st.set_defaults(func=cmd_selftest)

    bn = sub.add_parser("bench", help="just the pure-CPU crypto numbers (B3/B4)")
    bn.add_argument("--hash-mb", type=int, default=256)
    bn.add_argument("--sig-iters", type=int, default=20000)
    bn.add_argument("--dek-iters", type=int, default=100000)
    bn.set_defaults(func=cmd_bench)

    args = ap.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
