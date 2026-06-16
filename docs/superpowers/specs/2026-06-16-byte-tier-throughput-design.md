# Design — Production-shaped byte tier (Spike 0001 §8.2)

**Date:** 2026-06-16
**Status:** Approved design, pre-implementation
**Area:** `poc/walking-skeleton/` (Rust + SQL)
**Closes:** the two deferred byte-tier throughput deficiencies named in
[Spike 0001 §8.2](../../spikes/0001-walking-skeleton-wan-sync-and-pi-cost.md#82-carried-into-the-real-byte-tier-build-not-blocking-deferred)
— *synchronous one-RTT-per-chunk* and *not resumable across passes* — and realizes the
[§4.4](../../spikes/0001-walking-skeleton-wan-sync-and-pi-cost.md#44-blake3-for-blobs-the-attachment-digest)
claim that **BLAKE3's internal tree lets chunks be verified independently**, the property that makes
windowing and swarm fetch safe.

---

## 1. Problem

The Bet A run (2026-06-16) shipped the **availability** fix (PR #9): the lazy byte tier runs on its own
thread so a blob fetch can never head-of-line-block clinical sync. It left two **throughput** deficiencies,
both already mandated by [ADR-0013](../../spec/decisions/0013-attachments-content-addressed-lazy-blob-tier.md):

1. **`do_blobd` is synchronous, one round-trip per 64 KiB chunk.** Latency, not bandwidth, binds: a 64 MB
   blob is ~1024 sequential RTTs (~12 min on the ~710 ms link) regardless of throughput.
2. **The fetch is not resumable across passes** — it accumulates into an in-memory `Vec` discarded on any
   mid-fetch drop, restarting from offset 0. On a flaky high-latency link a large blob may never complete.

The fix is to make the byte tier **resumable, windowed/pipelined, multi-source (swarm), and per-chunk
verified** — *without regressing the availability floor*.

## 2. Goal & non-goals

**Goal.** Replace the stub fetcher with a production-shaped one: chunks in flight concurrently, pulled from
multiple sources, each chunk self-verifying against the content address, partial progress persisted so a
fetch resumes across drops and restarts.

**Non-goals.**
- Not the production object store — bytes still live inline in `BYTEA` (the chunk-store shape is the
  point, not the storage medium).
- Not a transport change — still framed JSON over WireGuard, `NoTls` (the link is the transport).
- Not key trust / registry work — orthogonal (ADR-0011), out of scope.
- True N-way swarm across many *real* nodes — the rig is 2 nodes, so swarm is exercised locally; the real
  link validates windowing + resume + per-chunk verify + throughput against the single peer.

## 3. The invariant that must not regress

**Byte transfer must never reduce clinical-data availability** (ADR-0013, the #9 fix). Windowing multiplies
bandwidth, so:
- worker count **W is small and bounded** (default 4),
- every worker keeps the **preemptible inter-request budget sleep**, so aggregate byte-tier bandwidth stays
  bounded and yields to the clinical plane,
- the tier still runs on **its own thread** (the #9 fix, unchanged).

The validation must confirm clinical-plane p95 is unaffected during a windowed fetch (the A4 discipline).

## 4. Component design

### 4.1 `cairn-event` (safety-critical core — stays small and reviewable, §9)

Add the BLAKE3 verified-streaming seam using the [`bao`](https://crates.io/crates/bao) crate (reference
implementation by the BLAKE3 author; MIT/Apache-2.0 → AGPL-compatible):

- `blob_outboard(bytes: &[u8]) -> Vec<u8>` — compute the bao **outboard** tree for a blob. Its root hash
  equals `blake3::hash(bytes)` (= `blob_address[2..]`), so it binds to the existing content address with no
  new addressing scheme.
- `extract_slice(content: &[u8], outboard: &[u8], offset: u64, len: u64) -> Result<Vec<u8>>` — server side:
  produce a verified bao slice covering `[offset, offset+len)`.
- `verify_slice(slice: &[u8], root: &[u8;32], offset: u64, len: u64) -> Result<Vec<u8>>` — **the safety
  seam**: client side, decode + verify a slice against the known root, returning the verified content bytes
  or an error. A tampered slice, a wrong-offset slice, or a lying source can never pass.

`blob_address` is unchanged (BLAKE3 root, multihash-wrapped `0x1e 0x20`). Maturity caveat: `bao` is pre-1.0,
but — exactly like the CDE-draft note in the spike (structural move 1) — correctness rides on **our** encoder
producing a slice the decoder verifies against a hash we already trust; we do not bet on an external
canonicalization standard.

### 4.2 Schema (`db/003_blobs.sql`)

- `blob_store.outboard BYTEA` (nullable) — the precomputed bao tree. Present on nodes that hold the bytes
  (set at `put-blob` and on fetch-completion); only needed to *serve* slices, re-derivable from `content`.
- New persistent partial-fetch table:

  ```sql
  CREATE TABLE IF NOT EXISTS blob_chunk (
      blob_address BYTEA       NOT NULL,
      chunk_index  INT         NOT NULL,   -- offset / SLICE_BYTES
      content      BYTEA       NOT NULL,   -- the VERIFIED bytes for this slice
      received_at  TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
      PRIMARY KEY (blob_address, chunk_index)
  );
  ```

  Verified slices land here as they arrive (out-of-order, any source), `ON CONFLICT DO NOTHING` — set-union
  for chunks, idempotent. When the full index set for a blob is present: assemble in index order → final
  whole-blob BLAKE3 check (belt-and-suspenders) → write `blob_store.content` + `outboard`, flip
  `present := TRUE`, delete the blob's `blob_chunk` rows. Resumability falls out for free: a restart fetches
  only the indexes not already in `blob_chunk`.

- A `SLICE_BYTES` constant (the window/chunk granularity, e.g. 256 KiB — larger than the old 64 KiB to
  amortize the per-slice tree overhead; tuned, not load-bearing).

### 4.3 Wire protocol (`cairn-sync` `Request`/response)

Replace `BlobChunk { addr_hex, offset, len }` (raw substring) with:

```
BlobSlice { addr_hex, offset, len }  ->  BlobSliceResponse { found, total_len, slice_hex }
```

The server extracts the slice from stored `content` + `outboard`. `found:false` tells a swarm worker "this
peer is not a source for this blob" — it drops that peer for this blob and continues with the others.

### 4.4 Fetch engine (`do_blobd` rewrite)

For each blob with `present = FALSE`:
1. Compute the chunk-index set from the known `byte_len`; subtract indexes already in `blob_chunk` → the
   work queue (a shared `VecDeque<usize>` behind a `Mutex`, or an atomic cursor + a requeue set).
2. Spawn **W bounded worker threads** (each its own PG client). Each worker loops:
   - pop a chunk index (stop when the queue is empty),
   - pick a source peer **round-robin across the swarm peer list**,
   - request the slice → `verify_slice` against the root,
   - on success: write to `blob_chunk` (`ON CONFLICT DO NOTHING`),
   - on failure (verify fail / peer lacks blob / link drop): **re-queue the index** for another
     source/attempt (bounded retries),
   - sleep the **preemptible budget** before the next request.
3. When all indexes are present: assemble → final verify → flip `present` → delete chunk rows.

Returns metrics (chunks fetched, retries, sources used, elapsed, throughput) for the harness.

### 4.5 CLI / swarm surface

- `blobd` and `run` gain repeatable **`--blob-peer HOST:PORT`** (swarm sources; falls back to `--peer` if
  none given) and **`--window N`** (worker count, default 4). `--budget-ms` retained.
- `blobd --metrics` emits the JSON metrics line.
- New **`gen-blob --size-mb N --media MEDIA`** mints a large random blob locally (so a real multi-MB fetch
  can be driven on the link without shipping a file).

### 4.6 Measurement harness (`harness/bench_blob.py`)

Stdlib-only, `bet_a.py` style. Drives a fetch and reports:
- **throughput** (bytes / elapsed) and **round-trip count** (windowed vs. the sequential-equivalent the stub
  would have paid),
- **resume-across-kill** (start a fetch, kill mid-transfer, restart, confirm completion from persisted
  chunks),
- **availability check** — clinical-plane pull p95 during a concurrent windowed fetch is unaffected (A4).

## 5. Testing strategy

- **`cairn-event` unit tests (pure, no PG — TDD these first):** `blob_outboard` root equals `blob_address`;
  `verify_slice` accepts a good slice and rejects (a) tampered slice bytes, (b) a slice claimed at the wrong
  offset, (c) verification against the wrong root.
- **Integration (real PG; `cargo test` + `clippy` green is the project bar):**
  - windowed fetch converges and every chunk verifies;
  - **resume** — pre-seed a subset of `blob_chunk`, fetch only the missing indexes;
  - **swarm** — two serve endpoints both holding the blob; chunks distribute; completes;
  - **lying peer** — a source serving wrong bytes has its chunks rejected by `verify_slice` and re-fetched
    from a good source (the per-chunk-verify payoff).
- **Real-link run (user-driven):** `bench_blob.py` over Cape York ↔ Dorrigo — throughput + round-trip
  reduction vs. the sequential stub, resume across a real drop, clinical p95 unaffected.

## 6. Files touched

- `crates/cairn-event/Cargo.toml` — add `bao`.
- `crates/cairn-event/src/lib.rs` — `blob_outboard` / `extract_slice` / `verify_slice` + unit tests.
- `db/003_blobs.sql` — `outboard` column, `blob_chunk` table.
- `crates/cairn-sync/src/main.rs` — `BlobSlice` protocol, `do_blobd` rewrite, worker pool, swarm peer list,
  `gen-blob`, `--blob-peer`/`--window`/`--metrics`, serve-side slice extraction, outboard at `put-blob`.
- `harness/bench_blob.py` — new measurement harness.
- `poc/walking-skeleton/README.md` — document the byte tier (windowed/resumable/swarm/per-chunk-verify) and
  the new commands/flags.
- `docs/spikes/0001-...md` §8.2 — mark the throughput fix delivered (after the run concludes).

## 7. Risks / open points

- **`bao` API shape** (pre-1.0) — confirm `SliceExtractor` / `SliceDecoder` (outboard variant) signatures
  at implementation time; the seam isolates any churn to three `cairn-event` functions.
- **Budget vs. window interaction** — W and `budget_ms` jointly bound byte-tier bandwidth; the A4 check is
  the guard that they were chosen conservatively. If clinical p95 degrades, lower W / raise budget.
- **Slice size tuning** — `SLICE_BYTES` trades per-slice tree overhead against window granularity; a tuned
  constant, not load-bearing, recorded in the run notes.
