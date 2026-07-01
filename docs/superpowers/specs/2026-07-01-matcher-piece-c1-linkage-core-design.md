# C1 — Identity linkage core (`link`/`unlink` + golden-identity projection)

**Date:** 2026-07-01 · **Spec home:** §5.1 / §5.7 (identity event algebra) · **Principle:** 2 (never
merge, always link; never erase, always overlay) · **Blast radius:** safety-critical (in-DB /
Rust) — a defect mis-binds *which person* a record belongs to.

## Why this slice

The HANDOVER's "matcher piece C — the §5.7 link-apply seam" is really the whole closed identity-event
algebra (`assert`, `link`/`unlink`, `identify`, `repudiate`, `reattribute`, `dispute`) — far too large
for one slice-by-slice build. It decomposes:

- **C1 (this doc)** — the **linkage core** (§5.1): `link`/`unlink` event types + the authoritative
  golden-identity (connected-component) projection with clean unmerge. Matcher-independent; the
  destination everything else plugs into.
- **C2** — the proposal→apply seam: a human-reviewed `match_proposal` (db/017) becomes an authoritative
  `link` event; the auto-link-band vs human-confirm paths; matcher-authored recall-traceability.
- **C3+** — the rest of the algebra: `identify`, `repudiate`, `dispute`, `reattribute` (the last a whole
  tiered subsystem, §5.5). Each its own later slice.

C1 is built first because without an authoritative linkage core a match proposal has nowhere to land,
and C1 carries the load-bearing §5.1 invariant: *"unmerge is always possible and clean — split the
component; nothing was rewritten."*

**Deliverable boundary:** the authoritative `patient_id → person_id` component projection is built in
full; the "unified chart unions the event streams of all member UUIDs" read (§5.1) is delivered only as
a *thin demonstrated VIEW*. The real unified-chart read surface belongs to the API/UI tier above the
foundation line.

## Architecture & components

Two new event types flow through the **existing** `submit_event` door (db/005) — unchanged. New types
register in `event_type_class` and add a branch to the `cairn_event_twin` hook, exactly as the
demographics slices did (db/010). `cairn-sync`/federation get link/unlink for free: they are ordinary
signed events that sync set-union.

| Layer | File | What |
|---|---|---|
| Rust builders (pure) | `crates/cairn-event/src/identity.rs` | `link_assertion_body` / `unlink_assertion_body` + twin renderers. Pure functions, no I/O — mirrors `demographics.rs`. |
| In-DB floor + projection | `db/018_identity_linkage.sql` | event-type registration, structural floor, `patient_link` edge overlay, `person_member` component projection + maintenance trigger, `cairn_event_twin` branch, the demonstrated unified-read VIEW. |
| Tests | `db/tests/018_*.sql` + Rust unit tests | TDD, red-first. |

No `submit_event` re-declaration; additive DDL only. The safety-critical write door stays single-source.

## Event model & structural floor

**`identity.link.asserted`** payload (`identity.unlink.asserted` is identical — an unlink of the same
unordered pair):

```json
{
  "subject_a": "<uuid>",
  "subject_b": "<uuid>",
  "provenance": "<ladder value>",
  "confidence": "<optional number|string>"
}
```

- `provenance` — required-present, value-open (§4.1 ladder), e.g. `"matcher:cfg@hash"`,
  `"clinician-asserted"`.
- `confidence` — optional (acknowledged uncertainty, principle 4); **omitted entirely when absent,
  never serialized as null** (the omit-when-absent discipline the demographics builders follow).

Both register as **`additive`, `targets_other_author = FALSE`**.

**Why additive / no mandatory attestation.** The existing `submit_event` gate already handles both
authoring paths with no new logic:

- The **matcher** authors an additive link with no responsibility-bearing contributor → no attestation
  required, consistent with §5.2 *"link/unlink: auto above threshold, else human"* and the
  advisory-actor contract (ADR-0030).
- A **clinician who vouches** includes a responsibility-bearing contributor → the gate *already* forces
  a valid human attestation on it (db/005 step 4).

Same event type, both paths. Safety comes from the advisory-side conservative threshold + the db/016
hard-veto floor + **clean unmerge** (a wrong link is fully reversible, §5.1), not from a write-time
block on the link itself.

**Structural floor** (`cairn_check_link_assertion`, culture-neutral, in-DB — the
`cairn_check_identifier_assertion` pattern). Each violation is a distinct legible exception:

- `subject_a`, `subject_b` present, valid UUIDs, and **distinct** (a self-link is meaningless — a UUID
  is trivially itself — and would corrupt the component walk).
- `provenance` present, non-empty string.
- `confidence`, when present, must be non-null.
- **No** cross-patient existence check: an identity assertion may legitimately arrive before or
  independently of any demographic assertion for a UUID (offline-first, set-union). The edge is honest
  data either way; the component projection simply carries UUIDs that have edges.

The authored §4.5-style legibility twin is **required non-empty** (same rule as demographics), rendered
e.g. `link: <a> ↔ <b> (matcher:cfg@hash)`.

## Projections & maintenance algorithm (the heart of C1)

### `patient_link` — standing-edge overlay

Same shape as `patient_identifier`:

```
patient_link(
    low        UUID   NOT NULL,
    high       UUID   NOT NULL,
    state      TEXT   NOT NULL,          -- 'link' | 'unlink'
    hlc_wall   BIGINT NOT NULL,
    hlc_counter INT   NOT NULL,
    origin     TEXT   NOT NULL,
    provenance TEXT   NOT NULL,
    confidence TEXT,
    PRIMARY KEY (low, high),
    CHECK (low < high)
)
```

On each link/unlink event the trigger upserts the canonical `(low, high)` pair and **overlays by HLC** —
the latest `(hlc_wall, hlc_counter, origin)` wins the `state`, identical tiebreak to db/002. So `link`
then a later `unlink` ⇒ `state='unlink'` (edge gone); `unlink` then later `link` ⇒ `state='link'`.
*Never merge, always overlay.*

### `person_member` — component projection

```
person_member(
    patient_id UUID PRIMARY KEY,
    person_id  UUID NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
)
```

`person_id` = the **minimum UUID in the connected component** — a derived canonical representative. The
"person" is a projection, never a stored immortal id (principle 2). A UUID that *had* an edge and is now
isolated (e.g. by an `unlink`) gets a row mapping to itself. A UUID **never touched by any linkage
event** has *no* `person_member` row at all (the trigger only fires on link/unlink); the `person_chart`
read VIEW therefore `COALESCE`s a missing membership to the patient's own id — "unknown to the link
graph" means "its own person," which is the correct default.

### Maintenance (the same AFTER-INSERT trigger, after the `patient_link` upsert)

Factored into a well-commented helper `cairn_recompute_component(seed uuid[])` (reads `patient_link`,
writes `person_member`; independently testable):

1. Collect the two endpoints of the changed pair as seeds.
2. **Bounded BFS** over *standing `link` edges only* (`state = 'link'`) from each seed, discovering the
   full component each endpoint now belongs to. After an `unlink` the two endpoints may fall into two
   separate components — walking each seed independently discovers that naturally.
3. For every UUID discovered, recompute `person_id = min(uuid over its component)` and upsert
   `person_member`. A UUID that ends up isolated maps to itself.
4. **Guard:** the walk is capped at a configurable `max_component_size` (default generous, e.g.
   10 000). Exceeding it **raises** — a component that large is a matcher pathology (mass false-merge),
   and failing loudly beats silently corrupting membership (mirrors db/017b's oversized-block guard:
   never a silent cap).

Cost is bounded by *touched component size*, not table size — preserving the ADR-0001/Bet-B
incremental-projection promise (chart reads stay O(1); we do **not** recompute on read, which is exactly
what db/002 replaced poc/replication-failover's VIEWs to avoid).

### Demonstrated unified-read VIEW

`person_chart` joins `person_member → patient_chart` (and/or `event_log`) so selecting one member
returns all member UUIDs' rows under the shared `person_id`. Thin; proves the union works; the real read
surface is above the foundation line.

## Error handling & convergence edge cases

Correctness under out-of-order sync (offline-first) is a requirement, pinned by tests:

- **Out-of-order link/unlink converge.** Highest HLC wins regardless of arrival order. A later-HLC
  `unlink` arriving before the `link` it supersedes leaves the edge `unlink`; the subsequent lower-HLC
  `link` loses the overlay. Convergent by construction.
- **Unlink that does *not* disconnect** (diamond A–B, B–C, A–C; unlink A–B): component stays `{A,B,C}`
  because A–C–B still connects them. This is why we recompute the whole touched component via BFS rather
  than naively splitting the pair — a naive split would be a silent false unmerge.
- **Unlink that *does* disconnect** (chain A–B–C, unlink A–B): splits into `{A}` and `{B,C}`;
  BFS-per-endpoint discovers both.
- **Idempotent re-assert:** `submit_event` dedups by content-address; the trigger upserts are
  idempotent — re-applying a link is a no-op.
- **Floor rejections**, each a distinct legible exception: self-link, missing/empty subject or
  provenance, empty authored twin, oversize component.

## Testing (TDD, red-first)

- **DB integration** (`db/tests/018_*.sql`): link → shared min-UUID `person_id`; transitive `{A,B,C}` →
  one person; diamond-unlink stays merged; chain-unlink splits; out-of-order convergence; idempotent
  re-link no-op; self-link rejected; oversize guard raises; empty-twin rejected; unified-read VIEW
  unions member streams.
- **Rust unit** (`cairn-event`): builder payload shape; `confidence` omit-when-absent (never null);
  twin rendering; canonical pair ordering.

Every test drives a specific behaviour before the code exists.

## Out of scope for C1 (deferred, recorded)

- The proposal→apply seam (C2) — no reading of `match_proposal` here.
- `identify` / `repudiate` / `dispute` / `reattribute` (C3+).
- The real unified-chart read surface (API/UI tier).
- Chart *trust states* (confirmed / unconfirmed / under-review, §5.7 projection-side contract) — a
  later read-side concern.
- Coherence-check re-trigger on new demographic assertions (§5.2 feedback loop).
