#!/usr/bin/env python3
"""WAN byte-tier spike driver (Spike 0001 §8.2) — real Cape York <-> Dorrigo run.

Runs on the MacBook (FETCHER, Cape York, WireGuard 10.0.0.2). Drives the DGX
(SOURCE, Dorrigo, 10.0.0.3) over ssh for *setup only*; every fetch/pull timing
measurement runs locally and crosses the real ~710 ms WireGuard satellite link.

Confirms the three §8.2 claims the local selftest cannot exercise on a real link:
  T1 throughput + round-trip reduction  (windowed vs sequential, vs the 64 KiB stub)
  T2 resume across a real drop           (kill mid-fetch, resume from persisted chunks)
  T5 availability floor                  (clinical pull p95 unaffected during a fetch)

Swarm (T3) and lying-peer heal (T4) are content-addressing properties already proven
by the local selftest; with only two nodes they are not separately WAN-tested here.
"""
import argparse
import json
import math
import subprocess
import sys
import time

DGX_SSH = "dgx"
DGX_BIN = "/home/hherb/cairn-skeleton/target/release/cairn-sync"
DGX_PSQL = "/usr/lib/postgresql/18/bin/psql"
DGX_CONN = "postgresql://postgres@localhost:5444/skeleton"
DGX_LISTEN = "10.0.0.3:7700"

LOCAL_BIN = "target/release/cairn-sync"
LOCAL_CONN = "postgresql://hherb@localhost:5432/cairn_a"
PEER = "10.0.0.3:7700"

SLICE = 262144      # MUST match SLICE_BYTES in cairn-sync
OLD_CHUNK = 65536   # the pre-§8.2 stub: one synchronous RTT per 64 KiB chunk
MEDIA = "application/dicom"
DROP = ("drop table if exists event_log,hlc_state,sync_state,patient_chart,"
        "blob_store,blob_chunk cascade;")


def p95(xs):
    if not xs:
        return 0.0
    s = sorted(xs)
    return s[min(len(s) - 1, int(round(0.95 * (len(s) - 1))))]


# --- local (MacBook) ---------------------------------------------------------
def local(*args, background=False):
    cmd = [LOCAL_BIN, args[0], "--conn", LOCAL_CONN, *args[1:]]
    if background:
        return subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    out = subprocess.run(cmd, capture_output=True, text=True)
    if out.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)}\n{out.stderr.strip()}")
    return out.stdout


def local_json(*args):
    lines = [l for l in local(*args).splitlines() if l.strip().startswith("{")]
    return json.loads(lines[-1])


def lpsql(sql, tuples=False):
    flag = "-tAc" if tuples else "-qc"
    out = subprocess.run(["psql", LOCAL_CONN, flag, sql],
                         capture_output=True, text=True, check=True)
    return out.stdout.strip()


def local_reset():
    lpsql(DROP)
    local("init")


def reference(addr, length):
    lpsql(f"select blob_note_reference(decode('{addr}','hex'),'{MEDIA}',{length});")


def present(addr):
    out = lpsql(f"select present, coalesce(octet_length(content),0) "
                f"from blob_store where blob_address=decode('{addr}','hex');", tuples=True)
    if not out:
        return (False, 0)
    pr, length = out.split("|")
    return (pr == "t", int(length))


def chunk_count(addr):
    return int(lpsql(f"select count(*) from blob_chunk where "
                     f"blob_address=decode('{addr}','hex');", tuples=True) or 0)


# --- DGX (source) over ssh ---------------------------------------------------
def dgx(shell_cmd):
    out = subprocess.run(["ssh", DGX_SSH, shell_cmd], capture_output=True, text=True)
    if out.returncode != 0:
        raise RuntimeError(f"ssh dgx: {shell_cmd}\n{out.stderr.strip()}")
    return out.stdout


def dgx_sync_json(*args):
    out = dgx(f"{DGX_BIN} {args[0]} --conn '{DGX_CONN}' {' '.join(args[1:])}")
    lines = [l for l in out.splitlines() if l.strip().startswith("{")]
    return json.loads(lines[-1])


def dgx_setup(size_mb, n_events):
    dgx(f"pkill -f '[c]airn-sync serve' 2>/dev/null; true")
    dgx(f"{DGX_PSQL} '{DGX_CONN}' -qc \"{DROP}\"")
    dgx(f"{DGX_BIN} init --conn '{DGX_CONN}'")
    blob = dgx_sync_json("gen-blob", "--size-mb", str(size_mb), "--media", MEDIA)
    dgx(f"{DGX_BIN} gen --conn '{DGX_CONN}' --node dorrigo --key /tmp/dorrigo.key "
        f"--patients 5 --count {n_events}")
    return blob["addr"], int(blob["bytes"])


def dgx_serve_start():
    # Keep serve alive for the whole run by owning the ssh process from here; the
    # remote serve dies when we close this channel (plus a pkill belt at teardown).
    return subprocess.Popen(
        ["ssh", DGX_SSH, f"{DGX_BIN} serve --conn '{DGX_CONN}' --listen {DGX_LISTEN}"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def dgx_gen(count):
    dgx(f"{DGX_BIN} gen --conn '{DGX_CONN}' --node dorrigo --key /tmp/dorrigo.key "
        f"--patients 5 --count {count}")


# --- the run -----------------------------------------------------------------
def fetch_timed(addr, nbytes, window, budget_ms, max_passes=400):
    t0 = time.time()
    passes, fetched, rejected = 0, 0, 0
    while not present(addr)[0] and passes < max_passes:
        m = local_json("blobd", "--blob-peer", PEER, "--window", str(window),
                       "--budget-ms", str(budget_ms), "--metrics")
        fetched += int(m.get("slices_fetched", 0))
        rejected += int(m.get("slices_rejected", 0))
        passes += 1
    elapsed = time.time() - t0
    pr, length = present(addr)
    mbps = (nbytes / (1 << 20)) / elapsed if elapsed > 0 else 0.0
    return dict(ok=(pr and length == nbytes), elapsed=elapsed, mbps=mbps,
                passes=passes, fetched=fetched, rejected=rejected, length=length)


def main():
    ap = argparse.ArgumentParser(description="Cairn WAN byte-tier spike (§8.2)")
    ap.add_argument("--size-mb", type=int, default=16)
    ap.add_argument("--window", type=int, default=8)
    ap.add_argument("--budget-ms", type=int, default=20)
    ap.add_argument("--base-rounds", type=int, default=5)
    ap.add_argument("--during-rounds", type=int, default=15)
    ap.add_argument("--kill-after", type=float, default=6.0)
    ap.add_argument("--skip-seq", action="store_true", help="skip the window=1 baseline")
    ap.add_argument("--skip-t2", action="store_true")
    ap.add_argument("--skip-t5", action="store_true")
    ap.add_argument("--log", default="wan_spike.jsonl")
    args = ap.parse_args()

    nbytes = args.size_mb * 1024 * 1024
    n_slices = math.ceil(nbytes / SLICE)
    seq_rtts = math.ceil(nbytes / OLD_CHUNK)
    waves = math.ceil(n_slices / args.window)
    rows = []
    log = open(args.log, "w")

    def record(tag, obj):
        obj = {"t": tag, **obj}
        log.write(json.dumps(obj) + "\n"); log.flush()

    print(f"# WAN spike: {args.size_mb} MB blob, {n_slices} slices @ {SLICE//1024} KiB, "
          f"window {args.window}\n# link:", end=" ", flush=True)
    ping = subprocess.run(["ping", "-c", "3", "-t", "8", "10.0.0.3"],
                          capture_output=True, text=True).stdout
    rtt = next((l for l in ping.splitlines() if "min/avg/max" in l), "rtt unknown")
    print(rtt.strip())

    print("\n[setup] DGX: reset, init, gen-blob, gen clinical events …", flush=True)
    addr, dgx_bytes = dgx_setup(args.size_mb, 200)
    assert dgx_bytes == nbytes, f"DGX minted {dgx_bytes} != {nbytes}"
    serve_proc = dgx_serve_start()
    # Confirm the source is reachable over the link before timing anything; serve
    # startup over the satellite link (ssh connect + bind) is variable, so retry.
    probe = None
    for _ in range(20):
        time.sleep(2)
        try:
            probe = local_json("pull", "--peer", PEER, "--peer-name", "dorrigo", "--metrics")
            break
        except RuntimeError:
            continue
    if probe is None:
        raise SystemExit("source never came up on the link (serve not reachable)")
    print(f"[setup] blob {addr[:16]}… ({dgx_bytes} bytes) served on {DGX_LISTEN}; "
          f"link probe: pulled {probe['shipped']} events")

    # ---- T1 throughput: sequential baseline (window=1) vs windowed -----------
    seq = None
    if not args.skip_seq:
        print(f"\n[T1] sequential baseline (window=1) over the link … (≈{seq_rtts//1}×RTT-class, be patient)", flush=True)
        local_reset(); reference(addr, nbytes)
        seq = fetch_timed(addr, nbytes, window=1, budget_ms=args.budget_ms)
        record("T1.seq", seq)
        print(f"      window=1: {seq['elapsed']:.1f}s ({seq['mbps']:.2f} MB/s), "
              f"{seq['fetched']} slices, {seq['passes']} pass(es)")

    print(f"[T1] windowed (window={args.window}) …", flush=True)
    local_reset(); reference(addr, nbytes)
    win = fetch_timed(addr, nbytes, window=args.window, budget_ms=args.budget_ms)
    record("T1.win", win)
    print(f"      window={args.window}: {win['elapsed']:.1f}s ({win['mbps']:.2f} MB/s), "
          f"{win['fetched']} slices, {win['passes']} pass(es)")
    if seq:
        speedup = seq["elapsed"] / win["elapsed"] if win["elapsed"] > 0 else 0.0
        rows.append(("T1", "windowed throughput + RTT reduction",
                     win["ok"] and seq["ok"],
                     f"{args.size_mb}MB @ {win['mbps']:.2f} MB/s window {args.window} ({win['elapsed']:.1f}s) "
                     f"vs {seq['mbps']:.2f} MB/s sequential ({seq['elapsed']:.1f}s) = {speedup:.1f}x; "
                     f"~{seq_rtts} seq RTTs (64KiB stub) -> ~{waves} windowed waves"))
    else:
        rows.append(("T1", "windowed throughput", win["ok"],
                     f"{args.size_mb}MB @ {win['mbps']:.2f} MB/s window {args.window} ({win['elapsed']:.1f}s), "
                     f"{win['passes']} pass(es); {n_slices} slices @ {SLICE//1024} KiB -> ~{waves} waves"))

    # ---- T2 resume across a real drop --------------------------------------
    if not args.skip_t2:
        print(f"\n[T2] resume: start fetch, kill after {args.kill_after}s, resume …", flush=True)
        local_reset(); reference(addr, nbytes)
        bd = local("blobd", "--blob-peer", PEER, "--window", "4",
                   "--budget-ms", "50", "--metrics", background=True)
        time.sleep(args.kill_after)
        bd.terminate(); bd.wait()
        partial = chunk_count(addr)
        res = fetch_timed(addr, nbytes, window=args.window, budget_ms=args.budget_ms)
        resumed = res["ok"] and 0 < partial < n_slices
        record("T2", dict(partial=partial, n_slices=n_slices, **res))
        print(f"      {partial}/{n_slices} chunks persisted at kill, then resumed to "
              f"{'complete' if res['ok'] else 'INCOMPLETE'}")
        rows.append(("T2", "resume across a real drop", resumed,
                     f"{partial}/{n_slices} chunks survived the interrupt, resumed to complete"))

    # ---- T5 availability floor ---------------------------------------------
    if not args.skip_t5:
        print(f"\n[T5] availability floor: clinical pull p95 base vs during a windowed fetch …", flush=True)
        local_reset(); reference(addr, nbytes)

        def drain():
            for _ in range(400):
                if local_json("pull", "--peer", PEER, "--peer-name", "dorrigo",
                              "--metrics")["applied_new"] == 0:
                    return

        def sample():
            dgx_gen(20)
            return local_json("pull", "--peer", PEER, "--peer-name", "dorrigo",
                              "--metrics")["elapsed_ms"]

        drain()
        base = [sample() for _ in range(args.base_rounds)]
        print(f"      baseline pull p95 {p95(base):.0f}ms ({len(base)} rounds); now during fetch …", flush=True)
        bd = local("blobd", "--blob-peer", PEER, "--window", str(args.window),
                   "--budget-ms", str(args.budget_ms), "--metrics", background=True)
        during = []
        while bd.poll() is None and len(during) < args.during_rounds:
            during.append(sample())
        bd.wait()
        bp, dp = p95(base), p95(during)
        floor_ok = dp <= bp * 1.30 + 50.0  # 30% + 50ms slack (link jitter dominates on satellite)
        record("T5", dict(base_ms=base, during_ms=during, base_p95=bp, during_p95=dp))
        print(f"      during pull p95 {dp:.0f}ms ({len(during)} rounds)")
        rows.append(("T5", "availability floor (clinical p95)", floor_ok,
                     f"clinical pull p95 base {bp:.0f}ms -> during {dp:.0f}ms (budget-ms {args.budget_ms})"))

    # ---- teardown + render -------------------------------------------------
    serve_proc.terminate()
    try:
        serve_proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        serve_proc.kill()
    dgx("pkill -f '[c]airn-sync serve' 2>/dev/null; echo stopped")
    log.close()

    w = max(len(r[1]) for r in rows)
    print(f"\n{'':4}{'check':<{w}}  result  detail")
    print("-" * (12 + w + 60))
    ok = True
    for code, name, passed, detail in rows:
        ok = ok and passed
        print(f"{code:<4}{name:<{w}}  {'PASS' if passed else 'FAIL':<6}  {detail}")
    print("-" * (12 + w + 60))
    print(f"\nWAN byte tier (§8.2): {'PASS' if ok else 'FAIL'}   (raw -> {args.log})\n")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
