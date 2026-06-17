# easyGP → Cairn: prefetch & materialization scavenge notes

> Scratch / conversation aid — NOT canonical. Pre-read for the next-week session
> when HH has access to the full easyGP Postgres schema + PL/pgSQL + PL/Python.

## Context
UI requirements discussion (clinical dashboard + results/reports inbox) surfaced
the performance machinery easyGP used. Goal: scavenge what scales to Cairn's
larger/federated/offline-first deployments; discard what only worked at
single-practice scale.

## What easyGP did (from HH, production experience)
- **Daemon continuously filled materialized tables from algorithmically predicted needs.**
- Inbox/cockpit context: **background prefetch of the first N items, top-to-bottom**
  (assumes clinicians work down the urgency-sorted list). Pragmatic compromise — no
  crystal ball, can't prefetch too many.
- **Order provenance:** tests ordered *within* easyGP automatically captured a direct
  reference to the ordering consult → vast majority of results had it. Results from
  *outside* (referrals in, post-hospital) lacked it → fell back to **most recent consult**,
  no smart guessing.
- Hit/miss of prediction was **never measured** — "worked as intended" at a handful of
  clinicians, small practice scale.

## Architectural reads (Cairn)
1. **Validates [ADR-0001](../../docs/spec/decisions/0001-fat-postgres-thin-daemon.md)** —
   fat Postgres + thin daemon filling materialized tables, arrived at independently in
   production.
2. **Scale worry likely inverts under fractal topology.** Small-predictable-working-set
   is a property of *node role*, not of *being small*: a workstation node = one clinician =
   the easyGP regime, guaranteed at every tier. Cairn never asks one node to predict for
   everyone (the non-scaling case).
3. **Mechanism/policy split** = the de-risking lever:
   - *Mechanism* (maintained projections + warming daemon) → safety-neutral, scales, ADR-0001.
     **This is what we scavenge from easyGP.**
   - *Prediction policy* (what to warm; easyGP's top-N heuristic) → advisory/fit-for-purpose,
     swappable with zero blast radius. Start with the exact easyGP heuristic.
4. **Instrument hit/miss from day one** (prefetch-used vs. evicted-unused). Nearly free;
   essential at scale; doubles as drift/tuning signal — same pattern as the matcher in
   [ADR-0014](../../docs/spec/decisions/0014-locale-pluggable-matcher-comparators.md).
5. **Principle-4 on the missing-provenance fallback:** label the fallback note
   ("most recent · ordering consult unknown"), don't silently present it as the ordering
   consult. Federation narrows the gap (Cairn-to-Cairn results may carry their own
   order→encounter link). Later AI cross-referencing only *proposes* a link as a new event,
   never asserts one (overlay discipline).

## Concrete data-model requirement (already noted in the inbox sketch)
- **Order event must carry a provenance link to its originating encounter/consult**, recorded
  at order time. Cannot be reconstructed by any MV after the fact. The result event already
  references its order; the order just also references its encounter.

## To look at next week (in the easyGP code)
- [ ] Materialized table structures behind inbox + cockpit (schema, indexes).
- [ ] The daemon: what it predicts, eviction policy, how N was chosen.
- [ ] PL/Python functions doing the prediction / context assembly.
- [ ] Order→consult capture path (how the reference was written at order time).
- [ ] Which optimizations are scale-sensitive (assumed single-node, low concurrency, no partition).
- [ ] Trajectory/series queries (Cairn needs these as folds over append-only, latest-truth-per-timepoint).
