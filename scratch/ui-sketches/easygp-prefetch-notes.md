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

## Order provenance — RESOLVED: it's already in the envelope, not a new field
HH described the easyGP order-capture path: context = current consultation; order triggered by
button **or** by typing `tx!`+tab inline in the progress note (keyboard-only, fingers never
leave home row); the order form is prefilled from the current consultation.

Mapping to Cairn — this is **not** a bespoke "order.consult_id" feature:
- **`encounter` is already a typed envelope scope key** ([data-model §3.1](../../docs/spec/data-model.md),
  alongside facility/department). An order authored in the active consult inherits that key
  ambiently, like every other event born in that context.
- **The "current consultation" = the armed write-context** ([ADR-0008](../../docs/spec/decisions/0008-point-of-care-identity-possession-and-salvage.md),
  [data-model §3.10](../../docs/spec/data-model.md#310-session-identity-event-authorship-and-draft-durability)):
  possession binds `(clinician, patient)`; the draft/context store is keyed `(author, patient)`,
  durable across re-auth. The active encounter rides on top.
- **Reproducing "the ordering consult"** = fold all events sharing that encounter key into the
  progress-note view. No reconstruction, no guessing.
- **Result-returns-later chain (two hops):** `result → references order → order.encounter →
  fold that encounter`. The order is the pivot; it carries the key for free because authored in-context.

**Therefore the day-one requirement is a UX invariant, NOT a schema addition:**
> Orders are always authored *inside* an armed encounter context, never as a free-floating
> action. The `tx!`+tab inline trigger is that invariant made ergonomic — the order is woven
> into note authoring (structured order event + note prose = two events, one encounter scope,
> one possession). Paper-parity: writing "FBC" in the note IS placing the order.

**External-results gap is now structurally explained:** referral-in / post-hospital results were
authored under a foreign node's encounter context (or none) → the `encounter` key is someone
else's or null. Cairn-to-Cairn it can survive federation; from a foreign system it can't.
Honest degradation (principle 4), labeled fallback to most-recent note.

### To verify next week against easyGP
- [ ] Granularity match: did easyGP's "consultation" map 1:1 to what Cairn calls an `encounter`?
- [ ] Did the order row store a direct consult FK, or was context inferred? (informs whether the
      two-hop result→order→encounter fold is enough, or we want the order to *also* denormalize a
      pointer for prefetch speed.)
- [ ] The `tx!`+tab parser: how the inline trigger turned note text into a structured order.

## To look at next week (in the easyGP code)
- [ ] Materialized table structures behind inbox + cockpit (schema, indexes).
- [ ] The daemon: what it predicts, eviction policy, how N was chosen.
- [ ] PL/Python functions doing the prediction / context assembly.
- [ ] Order→consult capture path (how the reference was written at order time).
- [ ] Which optimizations are scale-sensitive (assumed single-node, low concurrency, no partition).
- [ ] Trajectory/series queries (Cairn needs these as folds over append-only, latest-truth-per-timepoint).
