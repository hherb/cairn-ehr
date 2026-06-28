# Design — §5.2/§5.13 advisory matcher: the pure scoring core (piece B1)

**Date:** 2026-06-29 · **Spec home:** [identity §5.2](../../spec/identity.md#52-matching-pipeline-safety-asymmetric-false-merge-worse-than-false-split),
[identity §5.13](../../spec/identity.md#513-locale-pluggable-comparators-the-matcher-extension-point),
[demographics §4.2](../../spec/demographics.md#42-per-field-projection-policy) ·
**Implements:** [ADR-0014](../../spec/decisions/0014-locale-pluggable-matcher-comparators.md) (the comparator
API contract) · **No new ADR** (implements settled spec). **No spec-version bump.**

## 1. Purpose & the safety boundary

The §5.2 matching pipeline decomposes (see HANDOVER) into three pieces with a hard dependency order:

| Piece | Layer | Status |
|---|---|---|
| A. Hard-veto + coherence check — same-system identifier mismatch · verified-DOB clash · verified-sex-at-birth clash | **In-DB, safety-critical (§9)** | **done** (`db/016`, SCHEMA 14→15) |
| **B. Advisory probabilistic matcher** (comparators + Fellegi–Sunter + blocking + locale packs) | **Python, fit-for-purpose (advisory)** | **in progress — this slice is B1** |
| C. Proposal → `link` apply seam | In-DB, safety-critical | deferred — needs §5.7 identity algebra (unbuilt) |

Piece B is itself a subsystem, so it sub-decomposes:

| Sub-slice | What | This slice? |
|---|---|---|
| **B1. Pure scoring core** | comparator API contract + culture-neutral starter comparators + Fellegi–Sunter combiner → a `MatchScore` with per-field evidence | **yes** |
| B2. Data adapter + blocking + veto-gated pipeline | read the `patient_*` projections, generate candidate pairs by blocking, run B1, call the `db/016` veto gate, classify into bands, emit an advisory proposal worklist | deferred |
| B3. Locale packs + ops | content-addressed comparator-profile loading; phonetic/nickname/transliteration comparators; weight-learning; evaluation harness; hub duplicate-sweep | deferred |

**This slice builds B1 only:** pure functions that turn *two already-projected candidate records* into a
*match score with per-field evidence*. **No Postgres, no I/O, no network.** Everything that reads the DB, blocks,
gates on the veto floor, or applies a decision threshold is B2/in-DB and out of scope here.

### Why this is the right first slice

- It is the **reusable pure-function heart** (house rule #1) and **fully unit-testable with zero infrastructure**
  (TDD, house rule #2) — no DB, no fixtures, no network.
- It establishes the **comparator extension contract** ([§5.13](../../spec/identity.md#513-locale-pluggable-comparators-the-matcher-extension-point))
  that B3's locale packs plug into, without yet committing to any one culture's model.
- It has **no dependency on the unbuilt piece C** (§5.7 link algebra): its output is an advisory score, not an
  authoritative event.

### The safety boundary it respects

The matcher is **advisory** — it only *proposes* ([§5.2 NOTE](../../spec/identity.md#52-matching-pipeline-safety-asymmetric-false-merge-worse-than-false-split)).
Per the [§9 blast-radius](../../spec/language-substrate.md) rule, B1 is **fit-for-purpose** (Python): a defect
yields a bad *proposal* a human reviews, never a silent record corruption. Three things stay firmly on the
**safety-critical** (in-DB) side of the database seam and are therefore **not** B1's to own:

1. The **hard-veto set** — already built (`db/016`); B2 calls it, B1 never re-implements it.
2. The **conservative auto-link threshold** and band classification — B2/in-DB.
3. The **proposal → identity-algebra apply seam** — piece C.

B1 produces a *score with evidence* and stops. It owns no thresholds and makes no link/no-link decision.

## 2. Project layout

A new top-level **`matcher/`** uv project (per the standing rule: **uv, never venv/pip**), packaged as
`cairn-matcher`, import package `cairn_matcher`, **AGPL-3.0**. A clean sibling to `crates/` and `db/`.

```
matcher/
  pyproject.toml          # uv project, AGPL-3.0, pytest dev-dep
  README.md               # what B1 is / is not; how to run tests
  src/cairn_matcher/
    __init__.py
    agreement.py          # AgreementLevel + the Comparator contract type
    comparators.py        # the 4 culture-neutral comparators + Jaro–Winkler
    records.py            # CandidateRecord / DateValue / Name + field_comparisons()
    scoring.py            # Weights + Fellegi–Sunter combiner + MatchScore/FieldEvidence
  tests/
    test_comparators.py
    test_scoring.py
    test_records.py
    test_properties.py    # symmetry, no-data-never-lowers-score
```

Every dependency must be **AGPL-3.0-compatible**, checked before adding (house rule #1 / supply-chain mission).
B1 targets **zero runtime dependencies** (see §5).

## 3. The comparator contract (`agreement.py`)

A comparator is a **pure, field-typed function** returning a **graded agreement level**, never a boolean
([§5.13](../../spec/identity.md#513-locale-pluggable-comparators-the-matcher-extension-point), ADR-0014 §Decision 2):

```python
Comparator = Callable[[ValueA, ValueB, Context], AgreementLevel]
```

`AgreementLevel` is an **ordinal** enum (the ADR-0014 ladder, ordered so higher = stronger agreement):

| Member | Meaning |
|---|---|
| `INSUFFICIENT_DATA` | one or both sides absent/unknown → **zero evidence** (not on the m/u ladder) |
| `DISAGREE` | both present, no agreement at any level |
| `PARTIAL` | precision-coarsened / weak agreement (e.g. year-only DOB vs full DOB) |
| `EDIT_DISTANCE` | agree within an edit-distance band |
| `PHONETIC` | reserved — **no core comparator emits this** (B3 locale packs) |
| `NICKNAME` | reserved — **no core comparator emits this** (B3 locale packs) |
| `EXACT` | exact agreement |

`PHONETIC`/`NICKNAME` are present in the vocabulary as the **reserved plug points** that prove the extension
point, but shipping a phonetic encoder (Soundex is anglo) or a nickname lexicon (cultural) in the core would be
the **cultural capture** ADR-0014 forbids — they belong in pluggable locale packs.

`Context` is a small frozen dataclass carrying per-field facets a comparator needs (e.g. DOB precision); it never
carries I/O handles.

## 4. The four culture-neutral comparators (`comparators.py`)

All pure, all return `INSUFFICIENT_DATA` when either side is absent/unknown (never a penalty — §3.7).

- **`compare_exact(a, b, ctx)`** — `EXACT` if equal after a minimal, culture-neutral normalization (strip
  surrounding whitespace only — no casefolding/transliteration, which are culture-touching); else `DISAGREE`.
- **`compare_edit_distance(a, b, ctx)`** — **Jaro–Winkler implemented in-house** (≈40 lines, pure,
  dependency-free, fully testable — chosen over a string-distance dependency for reviewer-legibility and
  supply-chain hygiene): `EXACT` if identical, `EDIT_DISTANCE` if similarity ≥ a configurable band, else
  `DISAGREE`.
- **`compare_dob(a, b, ctx)`** — **precision-aware, parses no date strings.** Operates on a canonical
  `DateValue(year, month?, day?)` (precision is implied by which parts are present). Parsing a locale date
  *string* into canonical parts is a B2/locale-pack concern, kept out of the culture-neutral core. Comparison:
  compare only the parts **both** sides carry — all shared parts equal **and** same depth → `EXACT`; all shared
  parts equal but **different** depth (year-only vs full) → `PARTIAL` (consistent coarsening — principle 4);
  any shared part differs → `DISAGREE`; either side absent → `INSUFFICIENT_DATA`.
- **`compare_name_set(a_names, b_names, ctx)`** — the rich one. Inputs are two **name history sets**
  ([§4.2](../../spec/demographics.md#42-per-field-projection-policy): every asserted name retained), each name a
  bag of **role-tagged tokens** (given / family / …). **Order-tolerant and role-tolerant** — compares bags of
  role-tagged tokens, not positionally, so given-name order, given/family swaps, and hyphenated-surname order all
  match. Matches if **any** historical name pair agrees (maiden/married switching, aliases). Token agreement uses
  `compare_exact` then `compare_edit_distance`. Returns the **best** `AgreementLevel` found across the
  cross-product of the two sets; either set empty → `INSUFFICIENT_DATA`.

## 5. Input value types (`records.py`)

Frozen dataclasses — the value types B1 scores over. B2's adapter will populate them from the `patient_*`
projections; B1 builds them by hand in tests.

```python
@dataclass(frozen=True)
class DateValue:          # canonical, parsed; precision = which parts are present
    year: int | None; month: int | None; day: int | None

@dataclass(frozen=True)
class Name:               # one asserted name as role-tagged token bags
    tokens: Mapping[str, tuple[str, ...]]   # role -> tokens, e.g. {"given": (...), "family": (...)}

@dataclass(frozen=True)
class FieldValue:         # a single demographic field's value + its provenance rank
    value: object                            # str | DateValue | frozenset[Name] | ...
    provenance_rank: int = 0                 # from patient_demographic.provenance_rank

@dataclass(frozen=True)
class CandidateRecord:    # everything one patient contributes to a comparison
    dob: FieldValue | None
    sex_at_birth: FieldValue | None
    names: FieldValue | None                 # value is a frozenset[Name] (history set)
    identifiers: Mapping[str, frozenset[str]] # system -> values (advisory signal only)
    # ... additive: more fields land in later slices
```

A pure orchestrator runs the configured comparator per field:

```python
def field_comparisons(a: CandidateRecord, b: CandidateRecord,
                       config: ComparatorConfig) -> list[FieldComparison]
```

`FieldComparison{ field: str, level: AgreementLevel, provenance_rank: int }`. `ComparatorConfig` maps each field
to its comparator — the **registry** B3's locale packs will extend (the extension seam, demonstrated here with
the culture-neutral defaults).

> **Note on identifiers in B1:** the per-system identifier *veto* is the in-DB floor's job (`db/016`). In the
> scoring core an exact identifier match is a strong *positive* agreement signal; B1 treats identifiers via the
> same comparator machinery (exact agreement per system) and contributes positive evidence only. It never
> emits a veto — disagreement routing is B2 + `db/016`.

## 6. The Fellegi–Sunter combiner (`scoring.py`)

```python
@dataclass(frozen=True)
class FieldWeights:       # per agreement level, for one field
    # log2(m/u) for each agreement level; the DISAGREE / no-agreement weight is log2((1-m)/(1-u))
    ...

@dataclass(frozen=True)
class Weights:            # the deployment's locale tuning (learning is B3)
    per_field: Mapping[str, FieldWeights]
    # ships sensible defaults; callers override

def score(comparisons: list[FieldComparison], weights: Weights) -> MatchScore
```

For each `FieldComparison`:

- `INSUFFICIENT_DATA` → contributes **0** (no-data is never disagreement — §3.7; the principle made mechanical).
- otherwise → the `(field, level)` log-weight from `Weights` (positive when the level's m > u; the
  no-agreement/`DISAGREE` weight is negative).
- the weight is **scaled by `provenance_factor(provenance_rank)`** (a monotonic factor, e.g. mapping the
  `cairn_provenance_rank` ladder to a multiplier) so a *verified*-DOB agreement or clash weighs more than an
  *imported*/unknown one (the §4.2 provenance-aware property — ADR-0014 §Decision 2).

Result:

```python
@dataclass(frozen=True)
class FieldEvidence:
    field: str; level: AgreementLevel; provenance_rank: int; weight_contribution: float

@dataclass(frozen=True)
class MatchScore:
    total: float                       # summed log-likelihood ratio
    fields: tuple[FieldEvidence, ...]  # per-field breakdown; sum of contributions == total
```

`MatchScore` is **fully explainable** — every point of the total is attributable to a named field at a named
agreement level with a named provenance. B2/in-DB will band `total` against the conservative threshold; B1 owns
no threshold.

## 7. Data flow

```
CandidateRecord × CandidateRecord
        └─ field_comparisons(config) ─▶ list[FieldComparison]
                                              └─ score(weights) ─▶ MatchScore{ total, per-field evidence }
```

One direction, no I/O. `score(a,b)` is **symmetric**: `score(a,b).total == score(b,a).total`.

## 8. Error handling

- Absent / `None` / unknown field → `INSUFFICIENT_DATA` (zero evidence), **never an error** — clinical absence
  is normal and must stay on the safe side.
- A **structurally** malformed value (wrong type — an adapter bug, e.g. a `str` where a `DateValue` is required)
  → raise a typed `MatcherTypeError`. The distinction keeps clinical absence safe while surfacing real defects
  loudly (house rule #5 — no silent failure).

## 9. Testing (TDD — pure unit tests, pytest via uv)

Red-first, no DB. Matrix:

- **Each comparator:** agree / partial / disagree / insufficient cases; `compare_edit_distance` band boundaries;
  `compare_dob` precision coarsening (`EXACT` / `PARTIAL` / `DISAGREE` / `INSUFFICIENT_DATA`); `compare_name_set`
  order-tolerance, role-swap tolerance, history-set "any historical name agrees", and empty-set →
  `INSUFFICIENT_DATA`.
- **Jaro–Winkler:** known reference pairs (published expected similarities) pin the in-house implementation.
- **Combiner:** known weights → known total; `INSUFFICIENT_DATA` contributes exactly 0; `provenance_factor`
  scaling (verified clash outweighs imported clash); `DISAGREE` contributes a negative weight; per-field
  contributions **sum to** `total` (explainability invariant).
- **Property tests:** `score(a,b).total == score(b,a).total` (**symmetry**); **adding a missing/`None` field
  never lowers a score** (the §3.7 invariant, directly asserted).

All tests green before any commit (house rule #6).

## 10. Explicitly deferred (recorded, not lost)

- **B2:** the PG adapter that populates `CandidateRecord` from `patient_identifier` / `patient_demographic` /
  `patient_name` / `patient_address`; blocking / candidate-pair generation; the `db/016` veto-gate call; band
  classification and the conservative threshold; the advisory proposal worklist (its output table/shape).
- **B3:** phonetic, nickname, and transliteration comparators (the `PHONETIC`/`NICKNAME` plug points);
  content-addressed locale-profile loading (the comparator-profile tag that travels with the data); weight
  **learning** from local adjudication outcomes; the evaluation harness; the hub duplicate-sweep background job.
- **Piece C:** the proposal → `link` apply seam (needs the §5.7 identity event algebra, unbuilt).
- **Address comparator:** `compare_address` is a natural fifth culture-neutral comparator but is left to a
  follow-on (the §4.3 three-facet value is richer); B1's registry is additive, so it slots in without rework.
