# Design — Demographics: administrative-sex + gender-identity (slice 4)

**Date:** 2026-06-28 · **Spec home:** demographics §4.2 · **ADRs:** new **0037** (admin-sex provenance-first + per-field winner-policy selector + karyotype = distinct field), [0036](../../spec/decisions/0036-demographic-name-display-recency-first.md) (names recency-first — the recency precedent), [0034](../../spec/decisions/0034-demographic-legibility-twin.md) (twin), [0012](../../spec/decisions/0012-schema-evolution-event-format-and-legibility-across-time.md) (additive evolution / carried-not-projected) · **Substrate:** `cairn-node`

## Purpose

Slice 4 of the demographics subsystem. Slice 2 ([DOB + sex-at-birth](2026-06-27-demographic-dob-sex-at-birth-design.md)) built the **winner-only provenance-precedence** projection on the generic `demographic.field.asserted` event. This slice adds the **other two of the three §4.2 sex/gender fields** through that same spine:

- **`administrative-sex`** — the legal/forms/billing gender marker (M/F/X on documents). The §4.2 table *names* this field but gives **no projection rule**: this slice settles it as **provenance-first** (like DOB) — the administrative marker is document-anchored, an unverified self-claim must not displace a document-verified marker, and recency still wins among *equal* provenance (a new legal document flips it).
- **`gender-identity`** — "patient-stated authoritative, **recency wins**" (§4.2 table, line 17). This is the first **recency-first** projected field, the **inverse** of the slice-2 ordering: the newest assertion wins regardless of provenance, so a patient's current stated identity always displays.

It also resolves the slice-2 **deferred karyotype decision** (spec/ADR only — no karyotype code): sex-at-birth = the sex *assigned at birth*; a karyotype is a distinct future field, never a sex-at-birth value.

### Scope boundaries (what this slice is NOT)

- **No new event type, no new write door.** Both fields are `demographic.field.asserted` with `field="administrative-sex"` / `field="gender-identity"`. The slice-2 generic event + generic floor already accept them. Nothing in db/005/010/012 or `submit_event` is touched.
- **No floor change.** Both values are **open strings** (principle 4: intersex / non-binary / questioning / unknown all recordable; no closed enum). `cairn_check_demographic_field` already enforces `field`/`provenance`/`value` non-empty; neither field adds a structural branch or a vocabulary.
- **No value validation, no matching.** The floor never parses a sex/gender value; "sex-at-birth conflict = strong evidence" matching is the later §5.2 matcher. This slice carries values and projects a current display only.
- **No karyotype field built.** The karyotype = distinct-field decision is recorded (spec §4.2 + ADR-0037); building the field itself (clinical genetics, outside §4.2 demographics) is deferred.
- **No CLI verb.** Authoring is exercised by the test harness (and the future product CLI), as in slices 1–3.

## The headline: the slice-2 spine already accepts both fields

Both events reuse **the generic `demographic.field.asserted` event verbatim**, exactly as names did in slice 3:

- **Floor:** `cairn_check_demographic_field` requires `field`/`provenance`/`value` non-empty and only adds *extra* checks for `field='dob'`. Both new fields pass the generic checks unchanged — **no floor change.**
- **Carried-not-projected today:** the slice-2 projection trigger gates `IF fld NOT IN ('dob','sex-at-birth') RETURN NULL`, so an `administrative-sex` or `gender-identity` event currently lands in `event_log` (stored + legible via its twin) but **never projects**. This slice *opens the gate* for both fields. The ADR-0012 federation-forward design did its job in the interim — a node still on slice 2 stores and legibilises these events; it simply has no winner-policy for them yet.

So the entire slice is **additive**: two Rust builders + two twins, a new `db/013` that supersedes the projection trigger and adds one classifier, tests, an ADR, and a §4.2 wording fill-in.

**Rejected alternative:** dedicated `demographic.sex.asserted` / `demographic.gender.asserted` event types. Cleaner trigger `WHEN` clauses, but each costs a new event-type registration + a floor-dispatch branch — needless surface against the generic-field design built for exactly this.

## The new mechanic: per-field winner policy

Slice 2's projection hard-coded one winner ordering — `(provenance_rank, hlc_wall, hlc_count, origin)` — and one gate — `IN ('dob','sex-at-birth')`. Gender-identity needs the **inverse** ordering. Rather than special-case it, this slice introduces a **single classifier** that is the source of truth for *both* the gate and the ordering.

### (a) `cairn_demographic_field_policy(field) → text` — the policy selector

```sql
CREATE OR REPLACE FUNCTION cairn_demographic_field_policy(p_field text)
RETURNS text LANGUAGE sql IMMUTABLE AS $$
    SELECT CASE p_field
        WHEN 'dob'                THEN 'provenance-first'
        WHEN 'sex-at-birth'       THEN 'provenance-first'
        WHEN 'administrative-sex' THEN 'provenance-first'
        WHEN 'gender-identity'    THEN 'recency-first'
        ELSE NULL                            -- unknown field: carried, legible, not projected
    END;
$$;
```

- **IMMUTABLE** so it is trigger/index-safe and every node computes the identical policy.
- **`NULL` ⇒ not projected** is the one gate: it replaces the slice-2 `IN (...)` list and preserves the ADR-0012 carried-not-projected degrade for any field this node doesn't recognise.
- Names (`field='name'`) are **not** here — they project through their own `db/012` retained-set table, not `patient_demographic`. The classifier governs only the single-valued `patient_demographic` projection.

### (b) `patient_demographic_apply()` — policy-driven winner

The trigger (superseding **db/011's** definition — standard latest-loaded-wins additive migration; db/012/names projects through its own retained-set table, untouched) looks up the policy once, gates on non-NULL, and selects the winner tuple by a CASE:

| Policy | Winner tuple (max wins) | Semantics |
|---|---|---|
| `provenance-first` | `(provenance_rank, hlc_wall, hlc_count, origin)` | verified value locks vs. lower provenance; recency breaks equal-provenance ties — **unchanged slice-2 ordering** (dob, sex-at-birth, administrative-sex) |
| `recency-first` | `(hlc_wall, hlc_count, provenance_rank, origin)` | newest wins regardless of provenance; provenance then origin break equal-HLC ties — **new** (gender-identity) |

Both tuples are built from the four columns `patient_demographic` already stores (`provenance_rank`, `asserted_hlc_wall`, `asserted_hlc_count`, `asserted_origin`) — **no table schema change**. Each is a **total order** (origin is the final deterministic tiebreak), so every node converges to the same winner regardless of apply order — the convergence guarantee slice 2 already relies on, applied to the inverted ordering too.

The ON CONFLICT WHERE clause becomes a CASE over the two tuple comparisons; the table-driven gate (`policy IS NULL ⇒ RETURN NULL`) replaces the hard-coded field list. No other projection logic changes.

## cairn-event builders (pure, `crates/cairn-event/src/demographics.rs`)

Mirror `sex_at_birth_assertion_body` exactly — value-open scalar, no facets:

```rust
pub fn administrative_sex_assertion_body(value: &str, provenance: &str) -> Value {
    demographic_field_body("administrative-sex", value, None, provenance)
}
pub fn gender_identity_assertion_body(value: &str, provenance: &str) -> Value {
    demographic_field_body("gender-identity", value, None, provenance)
}
pub fn render_administrative_sex_twin(value: &str, provenance: &str) -> String {
    format!("Administrative sex ({provenance}): {value}")
}
pub fn render_gender_identity_twin(value: &str, provenance: &str) -> String {
    format!("Gender identity ({provenance}): {value}")
}
```

## Karyotype resolution (spec/ADR only)

sex-at-birth = the sex **assigned/observed at birth** (birth record; realistic provenance ceiling = document-verified). A **karyotype** (chromosomal sex — 46,XY etc.) is a **different fact** and belongs to its own future field (clinical genetics, outside §4.2); it must **never be asserted as a `sex-at-birth` value**. This avoids conflating assigned sex with chromosomal sex — the AIS / Swyer case (sex-at-birth = female, karyotype = 46,XY) is recorded as two facts, never one overwriting the other. The `fact-proven` tier **stays** in the ladder for legitimately same-field lab confirmation. Consequence: the projection's `fact-proven`-displaces-`sex-at-birth` path remains *mechanically present* but is never exercised by well-formed input — a **modeling convention** (UI soft-policy authors the right field), not a floor gate, keeping the floor culture-neutral (principle 12). No code changes for this; it is a spec §4.2 note + ADR-0037.

## Tests (TDD — failing first)

**cairn-event unit (4):** each body carries `field`/`value`/`provenance` and **no `facets`** bag; each twin renders the profile-independent plaintext.

**cairn-node integration (PG18 + cairn_pgx):**
- **admin-sex provenance-locks:** a later `patient-stated` value does **not** displace an earlier `document-verified` one; an equal-provenance later value **does** win (recency-among-equals).
- **gender-identity recency-wins:** a later `clinician-observed` assertion displaces an earlier `document-verified` one (provenance is *subordinate* to recency); among equal HLC, higher provenance wins; among equal HLC+provenance, origin breaks the tie (convergence).
- **gate opens:** both fields now appear in `patient_demographic` (regression vs. slice-2's carried-not-projected).
- **unknown field still carried-not-projected:** a `field='gender-marker-v2'` event lands in `event_log`, passes the floor, but does not project (ADR-0012 degrade intact).
- **slices 1–3 regress green** (identifiers, dob/sex-at-birth, names untouched).

## Docs / artefacts

- `db/013_demographics_sex_gender.sql` — the classifier + the policy-driven `patient_demographic_apply()` (supersedes **db/011's** trigger definition). Registered in the `cairn-node` SCHEMA array (`db.rs` — bump the array size literal `; 11]` → `; 12]` and add the `013_demographics_sex_gender` entry).
- **ADR-0037** — admin-sex = provenance-first (rationale: administrative marker is document-anchored; gender-identity carries the dignity/recency surface, so the two are deliberately split); the per-field winner-policy selector mechanism; karyotype = distinct field / sex-at-birth = assigned.
- **spec §4.2** — fill the administrative-sex projection rule into the table; add the karyotype note alongside the existing names note. Bump `index.md` spec version 0.37 → 0.38 and add the ADR-0037 row.
- **HANDOVER.md / ROADMAP.md** currency (bundled in this PR per working convention).
