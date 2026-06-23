# Design — cairn-node incremental sync watermark + genesis HLC

**Date:** 2026-06-23 · **Scope:** [issue #38](https://github.com/cairn-ehr/cairn-ehr/issues/38)
(the declared follow-up to [PR #39](https://github.com/cairn-ehr/cairn-ehr/pull/39)) ·
**Surface:** `cairn-node` federation sync (safety-critical, §9) · **No spec/ADR change.**

## Problem

`cairn-node` pulls the **full `node_event` set every cycle**. `sync.rs::stream_node_events`
orders by `(recorded_at, node_event_id)` and uses an **exclude-one placeholder**
(`node_event_id <> $1`); the puller always sends `after_id: None`. It is *correct* (idempotent
`apply_remote_node_event` with `ON CONFLICT DO NOTHING`) but wasteful, and the placeholder must be
**replaced**, not extended.

The obvious fix — a `(recorded_at, id) > watermark` predicate — is a **convergence hole**, not a perf
tweak:

1. **`recorded_at` is node-LOCAL insertion time.** When B pulls from A, B inserts each event with B's own
   `clock_timestamp()`, so B's `recorded_at` is not A's order. The puller only ever receives raw
   `signed_bytes` and never sees the server's `recorded_at`, so it cannot form a watermark in the
   server's order without a wire change.
2. **`clock_timestamp()` is not monotonic.** A backward clock adjustment between two inserts can place a
   *new* row *before* an already-advanced watermark → that row is **silently skipped forever**.

### Prior art we must NOT copy

`crates/cairn-sync` (the walking-skeleton PoC) already ships the naive design this issue warns against:
a **global HLC watermark** — `WHERE (hlc_wall, hlc_counter) >= ($1,$2) ORDER BY hlc_wall, hlc_counter,
node_origin`, with a per-peer `sync_state` cursor advanced to the max HLC seen. The silent-skip hole:
a late-arriving event whose HLC is *below* the cursor (a partitioned origin, multi-path propagation) is
never returned. The `node_origin` is only a sort tiebreaker, not in the predicate, so different origins
collapse onto one global scalar — once the watermark passes a low-HLC origin, its events are excluded
permanently. **cairn-node must not inherit this.**

### A correction to the issue's own reasoning

Issue #38 suggests *"lean on the spec's already-mandated background full-sweep as the miss safety-net."*
There is **no such mandate for `node_event`.** The only background sweep in the spec is the **matcher's
advisory duplicate sweep** ([ADR-0014](../../spec/decisions/0014-locale-pluggable-matcher-comparators.md)),
a different layer (identity matching, not event reconciliation). This design therefore **introduces** an
explicit `node_event` full-sweep as the correctness floor; it is not reusing an existing one.

## Approach (chosen: local-insertion `seq` cursor + full-sweep floor)

Key the watermark on the **serving node's own monotonic insertion sequence**, not on HLC or wall-clock.

> **Why this is structurally skip-proof.** When a node *newly learns* an event, `apply_remote_node_event`
> inserts it with a **fresh high `seq`** → it sorts *above* any puller's cursor → it is returned on the
> next pull. HLC cannot promise this: an old event carries a low HLC forever, so an HLC watermark that has
> advanced past it never re-selects it. Insertion-order is the only ordering where "new knowledge" always
> lands above the cursor.

Two alternatives were considered and rejected:

- **Per-origin HLC watermark** (issue #38 option b): requires the genesis-HLC fix as a hard prerequisite,
  carries a cursor vector that grows with origin count, and — because events from one origin can arrive at
  a server out of that origin's HLC order via multi-path propagation — *still* needs the full-sweep to be
  safe. More complex, same safety net, no gain.
- **Anti-entropy set reconciliation** (puller ships its held content-addresses / a Bloom filter, server
  returns the complement): no ordering hazard at all, but ships the puller's whole id-set each cycle and
  does not scale. Overkill for the low-volume `node_event` table; the right tool for a future large
  clinical-event plane, not this.

### Residual hazards — all healed by the full-sweep floor, none can admit an unauthorized event

The in-DB admission gate (`apply_remote_node_event`) is **unchanged**; every hazard below can only *delay a
legitimate event* until the next sweep, never *admit an illegitimate one*.

1. **BIGSERIAL commit-visibility race.** `seq` is assigned at INSERT, visible at COMMIT; under write
   concurrency a transaction with a lower `seq` can commit *after* the puller advanced past it. → full-sweep.
2. **Rejected-then-later-trusted backlog.** An event from a not-yet-trusted author is received (and the
   cursor advances past it) but rejected; after that author is peered, incremental won't re-offer it. →
   the `run` loop forces a full-sweep on any trust-set change, and the periodic sweep is the backstop.
3. **Address→different-node remap.** The cursor is keyed by peer address (see below); if an address is
   reused by a different node with a different `seq` space, incremental can skip until the cursor is
   exceeded. → full-sweep.

## Components

### 1. Schema (`db/007_node_federation.sql`, additive — [ADR-0012](../../spec/decisions/0012-schema-evolution-event-format-and-legibility-across-time.md))

- **`node_event.seq`** — `BIGINT GENERATED ALWAYS AS IDENTITY`, plus `node_event_seq_idx` btree.
  The sync ordering key: local insertion order on the serving node. Never signed, never on the wire core.
  Auto-populates on INSERT in both doors, so the existing INSERT column lists are unchanged. (The
  append-only trigger fires only on UPDATE/DELETE, so `ALTER TABLE … ADD COLUMN` is unaffected.) On an
  already-populated DB the column-add assigns `seq` to existing rows in unspecified physical order —
  irrelevant for fresh PoC databases; noted for honesty.
- **`sync_cursor(peer_addr TEXT PRIMARY KEY, last_seq BIGINT NOT NULL DEFAULT 0, updated_at TIMESTAMPTZ
  NOT NULL DEFAULT clock_timestamp())`** — per-peer pull checkpoint. **Mutable node-local operational
  state**, not signed content: it sits *outside* the append-only trigger, and the runtime `cairn_node`
  role is granted `SELECT/INSERT/UPDATE` on it. This does not weaken the in-DB floor — the floor protects
  the signed append-only log; a corrupted cursor can only cause a re-pull or a transient skip (healed by
  the sweep), never the admission of an unauthorized event. (Mirrors cairn-sync's writable `sync_state`.)

### 2. Genesis HLC — real local clock (separable second phase, same PR)

Today every node event is authored with `hlc { wall: 0, counter: 0 }` (genesis *and* peer/revoke), so
`trust_peer` status resolution (`ORDER BY hlc_wall DESC, hlc_counter DESC, recorded_at DESC`) falls back
entirely to node-local `recorded_at`. Fix by mirroring the **proven cairn-sync pattern**:

- **`hlc_state`** singleton table (`hlc_wall BIGINT`, `hlc_counter INTEGER`, seeded one row).
- **`node_hlc_tick()`** (`SECURITY DEFINER`, granted to `cairn_node`): advance
  `wall = GREATEST(wall, now_ms)`, `counter = (wall advanced ? 0 : counter + 1)`; returns `(wall, counter)`.
- **Merge-forward** in `apply_remote_node_event`: after inserting a remote event, the local clock never
  falls behind it (`UPDATE hlc_state SET hlc_wall = GREATEST(...), hlc_counter = CASE …`).
- **Rust authoring** (`identity.rs::provision` / `author_peer` / `author_unpeer`) calls `node_hlc_tick()`
  to obtain a real `(wall, counter)` *before* building and signing the body, replacing the hardcoded `0,0`.

**Independence:** because the watermark is keyed on `seq`, the genesis-HLC fix is **not** a prerequisite
for sync safety — it dissolves the coupling issue #38 assumed. It is implemented as a distinct phase /
commit so it stays separable and could be split to its own PR without affecting the sync work.

### 3. Wire protocol (`sync.rs`, additive, principle-12 reviewed)

- **Request:** replace the non-functional placeholder `NodeEventsAfter { after_id: Option<Uuid> }` with
  **`NodeEventsAfterSeq { after_seq: i64 }`** (`after_seq = 0` ⇒ full sweep). The placeholder never worked
  (exclude-one), so this replaces dead code, not a live contract; the request enum is documented as
  versioned, with future changes additive-only (principle 12).
- **Response framing:** each event frame becomes **`[8-byte big-endian seq][signed_bytes]`**. The signed
  event core is byte-identical to today; `seq` is pure transport metadata for cursoring (the puller assigns
  its *own* `seq` when it inserts, so the server's `seq` is never authoritative downstream). A test asserts
  the signed bytes are identical regardless of `seq` — the principle-12 guard.

### 4. Puller (`sync.rs::pull_into` / `run`)

- `pull_into` gains `full_sweep: bool`. It reads `sync_cursor` by **peer address** (known *before*
  connecting, so the existing request-first protocol shape is preserved — no greeting/reorder needed),
  sends `after_seq = full_sweep ? 0 : last_seq`, applies each event via the unchanged gate, tracks the max
  `seq` received, and **only at a clean EOF** upserts `last_seq = GREATEST(last_seq, max_seq)` (advance-only;
  a mid-stream failure never advances the cursor past un-applied events).
  - The cursor advances over *received* frames (the stream is `seq`-ordered), including rejected ones;
    rejections are re-evaluated on the next full-sweep (hazard 2 above).
- `run` chooses `full_sweep = (cycle % FULL_SWEEP_EVERY == 0) || trust_set_changed_this_cycle`. The
  trust-change trigger pulls a newly-peered node's backlog immediately rather than waiting for the cadence.
  `FULL_SWEEP_EVERY` is a named constant with a comment (tunable; the table is low-volume so a frequent
  sweep is cheap).

### 5. Serve (`sync.rs::stream_node_events`)

- Takes `after_seq: i64`; query `SELECT seq, signed_bytes FROM node_event WHERE seq > $1 ORDER BY seq`,
  writing `[seq][bytes]` frames. `after_seq = 0` returns the full set (the sweep path).

## Data flow (one incremental cycle)

```
run: cycle++ ; full_sweep = (cycle % N == 0) || trust_changed
  pull_into(peer, full_sweep)
    last_seq := SELECT last_seq FROM sync_cursor WHERE peer_addr = <addr>   (0 if absent)
    after_seq := full_sweep ? 0 : last_seq
    → send NodeEventsAfterSeq { after_seq }
    server: SELECT seq, signed_bytes FROM node_event WHERE seq > after_seq ORDER BY seq
            → stream [seq][bytes] frames, EOF
    for each frame: apply_remote_node_event(bytes) ; max_seq := max(max_seq, seq)
    on clean EOF: UPSERT sync_cursor SET last_seq = GREATEST(last_seq, max_seq)
```

## Error handling

- Per-event rejection stays **non-fatal** (unchanged): the gate's deny-all for an untrusted author is the
  normal case; logged with the legible reason, the pull continues, the cursor still advances over it.
- A mid-stream transport failure aborts the pull **without** checkpointing → the next cycle re-pulls from
  the last committed cursor. No event is lost; at worst a frame is re-applied (idempotent).
- A malformed `seq` prefix / oversized frame is rejected by the existing `MAX_FRAME_BYTES` framing guard.

## Testing (TDD — safety-critical surface, tests first)

1. **`out_of_order_skip_is_reconciled_by_full_sweep`** — the acceptance test from issue #38: artificially
   advance `sync_cursor.last_seq` past an un-applied event, prove an incremental pull misses it, then prove
   a full-sweep (`after_seq = 0`) delivers it. This proves the floor without needing to race real commits.
2. **`incremental_pull_returns_only_new_events`** — cursor advances after a pull; a second pull with no new
   events ships nothing; appending one event ships exactly that one.
3. **`genesis_hlc_is_nonzero`** + **`hlc_advances_across_events`** — provision yields `wall > 0`; a
   subsequent `peer.added` carries a strictly higher HLC; `trust_peer` orders by real HLC.
4. **`wire_seq_not_in_signed_core`** — the streamed `signed_bytes` are byte-identical to the stored event
   regardless of the `seq` prefix (principle-12 guard).
5. **two-node E2E still converges** under incremental + sweep (extend the existing convergence test).

DB-gated tests follow the existing harness (`CAIRN_TEST_PG`, `db::test_serial_guard` advisory lock,
PG16 + `cairn_pgx`). Pure units (frame seq-prefix split; the tick arithmetic if extractable) run under
plain `cargo test`.

## Out of scope (declared)

Key rotation / `supersede`, DR/recovery escrow (ADR-0026), and any clinical-event sync plane. This design
covers only the `node_event` federation plane named in issue #38.
