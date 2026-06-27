# Design — Demographic provenance-precedence fields: DOB + sex-at-birth (slice 2)

**Date:** 2026-06-27 · **Spec home:** demographics §4.1/§4.2/§4.5 · **ADRs:** [0034](../../spec/decisions/0034-demographic-legibility-twin.md) (twin), [0010](../../spec/decisions/0010-additive-vs-suppressing-classification.md) (additive registry), [0012](../../spec/decisions/0012-schema-evolution-event-format-and-legibility-across-time.md) (additive evolution / legibility across time) · **Substrate:** `cairn-node`

## Purpose

Slice 2 of the demographics subsystem. Slice 1 ([identifier assertion](2026-06-27-demographic-identifier-assertion-design.md)) deliberately built the field with the **simplest** projection — pure set-union, no precedence. This slice builds the next-hardest projection mechanic, **provenance-precedence**, and lands the two §4.2 fields that share it:

- **DOB** — single value, *provenance beats recency*, verified value locks; precision-aware `(value, precision, basis)`.
- **sex-at-birth** — single value, *provenance-locked* (the identical rule).

Both are "strong evidence against link" rows in the §4.2 table and the most safety-critical demographic fields after identifiers. Building them together is efficient: **one projection policy serves both**. The deferred §4.2 fields (names multi-valued + display-winner; administrative-sex; gender-identity recency-wins) reuse this slice's spine.

### Scope boundaries (what this slice is NOT)

- **Matching is out of scope.** The §4.2 "strong evidence against link" semantics live in the identity §5.2 matching pipeline — a separate, later subsystem. This slice carries the values and projects a current winner; it does **not** compare DOBs across linked records or compute link evidence.
- **No value validation.** The floor never parses or validates a date, never enforces a sex vocabulary. Those are advisory (above the floor) or simply free-text (principle 4 / principle 12).
- **DOB precision is carried, not interpreted.** `precision`/`basis` are stored and rendered in the twin; the projection's precedence logic does not read them. The matcher's "down-weight default 01-01" behaviour (§4.2 note) is later, advisory work.
- **No CLI verb.** Authoring is exercised by the test harness (and the future product CLI), as in slice 1.
- **Single-valued fields only.** Names (multi-valued retained set + a display-winner pointer) need a different projection shape and are a later slice.

## The new mechanic: provenance-precedence projection

The shared primitive both fields need is a **provenance-rank function** plus a **precedence-aware projection winner**.

### `cairn_provenance_rank(text) → int` (IMMUTABLE)

Encodes the §4.1 ladder as a total order:

```
fact-proven         70
document-verified   60
patient-stated      50
third-party-stated  40
clinician-observed  30
imported / unknown  20
inferred            10
(unrecognized)       0
```

**`fact-proven` (70) — a new top tier above `document-verified`.** Laboratory- or scientifically-established truth (a karyotype, a confirmed assay, a DNA identity test) that can override what an official document merely *attests*. Worked example: sex-at-birth recorded `female` (`document-verified`, observed phenotype) later met by a karyotype `XY` asserted `fact-proven` (CAIS / testicular feminisation). This **extends the canonical §4.1 ladder**, which currently tops out at `document-verified` — the spec §4.1 prose is updated to name `fact-proven` as the top tier as part of this slice (the provenance field is value-open, so this is an additive refinement, not a breaking change).

> ⚠️ **Caveat to confront in the later sex-field work (not resolved here).** The projection picks the highest-provenance value as the *display winner*, so a `fact-proven` karyotype=XY assertion would **displace** a `document-verified` "female" sex-at-birth in the chart. Whether that is clinically correct is genuinely contested — "sex **at birth**" arguably means *sex assigned/observed at birth* (truly `female` in CAIS), whereas karyotype is a *different fact* about the same patient. This is a **field-semantics / §5.2-matching** question, not a provenance-ranking one: the ranking is sound and `event_log` retains *both* assertions as evidence regardless. For this slice (DOB + a single-valued sex-at-birth field with no genotype sub-field) the mechanical winner is acceptable; the names/sex-expansion slice must decide deliberately whether karyotype is the *same field* as assigned-sex or a distinct field, rather than inheriting a silent auto-override.

An **unrecognized** provenance string ranks **0** — below `inferred`. This is the safe default: a string from a newer ladder, or a typo, **can never displace a known-provenance value**. It is also federation-safe (a node that doesn't recognize a peer's newer provenance term degrades to "lowest", never to "highest").

### Winner selection

The projection keeps **one current display winner per `(patient_id, field)`**. Full assertion history stays in `event_log` (append-only — that *is* the matching evidence, so §4.1's "overwriting destroys evidence" is satisfied without a retained-set table; this differs from slice-1's `patient_identifier`, which kept a row per identifier *because* identifiers are inherently multi-valued).

Winner comparison tuple, applied on `ON CONFLICT DO UPDATE`:

```
(new_rank, NEW.hlc_wall, NEW.hlc_counter, NEW.node_origin)
  >  (incumbent_rank, incumbent_hlc_wall, incumbent_hlc_count, incumbent_origin)
```

- **Provenance beats recency** — `rank` is the leading term.
- **Recency breaks ties** among equal provenance (HLC).
- **`node_origin`** is the final deterministic tiebreak, so every node converges to the same winner regardless of apply order (the same discipline `patient_chart` already uses).
- **"Verified value locks vs. lower provenance"** falls out for free: a later `patient-stated` (50) cannot displace an earlier `document-verified` (60).

## The event shape

**Event type:** `demographic.field.asserted` — one generic type with a `field` discriminator, registered **additive** (`targets_other_author = FALSE`) in `event_type_class`. Chosen over per-field event types so every future single-valued demographic field becomes a new `field` value + a one-row projection policy, not a new event type and table; matches §4.1's uniform "field F of patient P has value V" framing.

**Payload** (in `EventBody.payload`):

```jsonc
// DOB
{
  "field":      "dob",                 // discriminator
  "provenance": "document-verified",   // §4.1 ladder; required, value-open
  "value":      "1980-07-15",          // §4.2 core value (the matching-relevant scalar)
  "facets": {                          // field-specific extras — opaque to precedence
    "precision": "day",                // dob: required (principle 4 — never an unqualified date)
    "basis":     "document"            // dob: optional — how the date was derived
  }
}

// sex-at-birth
{ "field": "sex-at-birth", "provenance": "clinician-observed", "value": "female" }
```

- `value` is the core scalar; for sex-at-birth it is an **open string** (intersex / indeterminate / unknown must be recordable — principle 4 + culture-neutrality), never a closed enum. Recommended vocab lives in the UI.
- `facets` is a per-field bag so field-specific keys never collide with the core, and the generic projection stores them as one `jsonb`.

**No `EventBody` change.** Slice 1 already added `plaintext_twin: Option<String>` (the §4.5 authored twin carrier). This slice reuses it unchanged.

## The in-DB floor — `cairn_check_demographic_field(b jsonb)`

A generic core plus a per-field structural dispatch; each violation a distinct legible `RAISE`:

- **Generic, always:** `field` non-empty text · `provenance` non-empty text (§4.1) · `value` non-empty text.
- **`field = 'dob'`:** also `facets.precision` non-empty text (principle 4 — a date must declare its precision; never an unqualified exact date by default). `basis` optional; non-empty text when present. The floor **never parses or validates the date** — a half-recalled "1980, year-only" must record.
- **`field = 'sex-at-birth'`:** no extra (open value).

### The floor stays OPEN; the projection is GATED (the load-bearing decision)

An **unknown `field`** (e.g. a newer node's `gender-identity`) **passes the floor, is stored in `event_log`, and is legible via its twin** — but is **not projected** (no column/policy for it). This is *required* for federation: set-union sync means an older node must accept and store a newer node's demographic assertion it cannot yet project; rejecting it would break convergence ([ADR-0012](../../spec/decisions/0012-schema-evolution-event-format-and-legibility-across-time.md) / principle 11 — "a node generations behind still reads the fact"). So per-field structural checks apply only to fields the node knows; the **projection trigger** carries the per-field policy and silently no-ops on an unknown field. A local-authoring typo (`"dobb"`) stores-but-doesn't-project — a UI concern (same class as any free-text field), not a floor gate. **A test pins this** (test 6).

It **never holds a profile, runs a checksum, validates a value, or rejects on validation** (§4.4/§4.2 / principle 12).

## The twin hook

`db/011` re-declares `cairn_event_twin(p_type, b)` (supersedes `db/010`'s; the latest-loaded definition wins — the standard additive-migration pattern slice 1 used). It dispatches **both** demographic types through their respective floor check, then a **single shared** authored-twin enforcement, falling through to the skeleton for legacy types:

```sql
IF    p_type = 'demographic.identifier.asserted' THEN PERFORM cairn_check_identifier_assertion(b);
ELSIF p_type = 'demographic.field.asserted'      THEN PERFORM cairn_check_demographic_field(b);
ELSE  RETURN cairn_twin_skeleton(p_type, b);
END IF;
-- shared §4.5 authored-twin enforcement (written once, not per branch):
v_twin := b ->> 'plaintext_twin';
IF v_twin IS NULL OR length(trim(v_twin)) = 0 THEN
    RAISE EXCEPTION 'submit_event: demographic assertion requires a non-empty authored twin (§4.5)';
END IF;
RETURN v_twin;
```

`submit_event` (db/005) is reused verbatim — never re-declared — so the validated write door stays single-source (same discipline as slice 1).

## The projection — `patient_demographic` + trigger

```sql
patient_demographic(
  patient_id        UUID NOT NULL,
  field             TEXT NOT NULL,        -- 'dob' | 'sex-at-birth' (known fields only)
  value             TEXT NOT NULL,        -- current display winner's core value
  facets            JSONB,                -- field-specific extras (dob: precision/basis)
  provenance        TEXT NOT NULL,
  provenance_rank   INT  NOT NULL,        -- cached cairn_provenance_rank(provenance) of the winner
  asserted_hlc_wall  BIGINT NOT NULL,
  asserted_hlc_count INTEGER NOT NULL,
  asserted_origin    TEXT NOT NULL,
  updated_at        TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (patient_id, field))
```

`patient_demographic_apply()` — `AFTER INSERT ON event_log WHEN (event_type = 'demographic.field.asserted')`:
- `RETURN NULL` immediately if `NEW.body ->> 'field' NOT IN ('dob','sex-at-birth')` (the gate — unknown fields are stored but not projected).
- compute `v_rank := cairn_provenance_rank(NEW.body ->> 'provenance')`.
- `INSERT … ON CONFLICT (patient_id, field) DO UPDATE SET …` guarded by the winner-comparison tuple above; columns updated only when the new assertion outranks the incumbent.

No overlap with existing triggers: `patient_chart` fires on `patient.created/amended`/`note.added`, `patient_identifier` on `demographic.identifier.asserted`. `GRANT SELECT ON patient_demographic TO cairn_agent`.

## Rust pure functions (`crates/cairn-event/src/demographics.rs`, extend)

The file is 89 lines today; this keeps it well under 500 (house rule 4):

- `demographic_field_body(field, value, facets: Option<Value>, provenance) → Value` — the generic builder; optional `facets` omitted entirely when absent (never serialized as null).
- `dob_assertion_body(value, precision, basis: Option<&str>, provenance) → Value` — delegates, building `facets = { precision, basis? }`.
- `sex_at_birth_assertion_body(value, provenance) → Value` — delegates, no facets.
- `render_dob_twin(value, precision, provenance) → String` — e.g. `"Date of birth (patient-stated): 1980 (year)"`.
- `render_sex_at_birth_twin(value, provenance) → String` — e.g. `"Sex at birth (clinician-observed): female"`.

All pure, explicit inputs/outputs, unit-tested.

## Data flow

1. **Author** (test harness now; product CLI later): `dob_assertion_body(...)` / `sex_at_birth_assertion_body(...)` → payload; `render_*_twin(...)` → twin; assemble `EventBody { event_type: "demographic.field.asserted", payload, plaintext_twin: Some(twin), … }`; `sign()`.
2. **Submit**: `submit_event(signed_bytes)` → verify → parse body → resolve actor → classify (additive) → **demographic branch** in `cairn_event_twin` (floor check + carry authored twin) → append to `event_log` → trigger → provenance-precedence winner into `patient_demographic` (or no-op for an unknown field).
3. **Read**: query `patient_demographic` for the current winner per field; query `event_log` for the full assertion history (the matching evidence).

## File layout & sizes (all < 500 lines)

| File | Change |
|---|---|
| `crates/cairn-event/src/demographics.rs` | +builders +twin renderers +unit tests |
| `db/011_demographics_fields.sql` | **new** — rank fn · floor dispatcher · `patient_demographic` + trigger · `cairn_event_twin` re-decl |
| `crates/cairn-node/src/db.rs` | +1 line in `SCHEMA` array (after `010`) |
| `crates/cairn-node/tests/demographics_fields.rs` | **new** — integration tests |
| `docs/spec/demographics.md` | §4.1 ladder prose gains `fact-proven` as the top tier |

## Acceptance tests (TDD, red-first)

**Unit (`cairn-event`):**
1. `dob_assertion_body` / `sex_at_birth_assertion_body` shape — facets present for dob, `basis` omitted when `None`, absent facets never null; twin renderers' output.

**Integration (`cairn-node`, PG18 + `cairn_pgx`):**
2. **Happy path** — a well-formed DOB and a well-formed sex-at-birth → accepted → exactly one `patient_demographic` row each with the expected `value`/`facets`/`provenance_rank`.
3. **Provenance precedence** — patient-stated DOB, then document-verified DOB → winner = document-verified; then a *newer* patient-stated DOB → winner **still** document-verified (verified value locks vs. lower provenance).
4. **Recency among equals** — two document-verified DOBs → later HLC wins.
5. **Floor rejections** (each isolated, triple-gated: legible error + empty `event_log` + empty projection): value-empty · provenance-missing · field-missing · dob-missing-precision · empty-authored-twin.
6. **Unknown field carried, not projected** — `field = "eye-color"`, otherwise well-formed → accepted, present in `event_log`, **no** `patient_demographic` row (proves the open-floor / gated-projection federation-forward design).
7. **Regression** — slice-1 identifier assertion still projects; legacy `patient.created` still works via the skeleton-twin path.

## Risks / things to verify first

- **Cross-schema-version rank divergence** (noted, out of scope): if two nodes ran *different* `cairn_provenance_rank` tables, they could pick different winners for the same event set. The §4.1 ladder is a **fixed closed set**, so this does not arise in practice; projection-policy versioning is a broader topic deferred with the rest of §4.2.
- **Twin-hook re-declaration** duplicates the dispatch shape from `db/010`. Accepted as the additive-migration pattern; the shared authored-twin enforcement is written once, and each floor check is isolated in its own helper, keeping the re-declared function legible.
- **Trigger gate vs. `WHEN` clause** — the unknown-field gate is inside the function (reading `body ->> 'field'`), not in the trigger `WHEN`, because `WHEN` cannot cheaply branch on a JSON field. Verified to no-op cleanly (test 6).

## Follow-ups (explicitly deferred)

- Names (multi-valued retained set + display-winner pointer) — a later slice; different projection shape.
- Administrative-sex; gender-identity (recency-wins, the inverse precedence toggle) — new `field` values reusing this spine.
- The §5.2 matching pipeline + DOB/sex-at-birth "strong evidence against link" semantics (advisory matcher, Python/fit-for-purpose).
- Down-weighting default 01-01 birthdays (§4.2 note) — matcher-layer, advisory.
- Globalise the authored twin to every event type (principle 11 / §3.13); retire the skeleton-twin fallback.
- Product CLI verb for authoring demographic assertions.
