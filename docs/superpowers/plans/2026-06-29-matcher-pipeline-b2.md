# Matcher pipeline B2 (pairwise, veto-gated, proposal-persisting) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Score a given patient pair via the B1 core, gate it on the in-DB `cairn_match_veto`, and persist an explainable advisory proposal to a new `match_proposal` table.

**Architecture:** A new IO-bearing sub-package `cairn_matcher/pipeline/` beside B1's untouched pure core. `adapter.py` + `banding.py` are pure (projection rows → `CandidateRecord`; `MatchScore` + veto findings → band + proposal payload). `db.py`/`runner.py` are the only IO (psycopg, an *optional* extra). The proposal worklist is a SCHEMA-tracked table (`db/017`).

**Tech Stack:** Python 3.12+ / uv / pytest (pure suite) · psycopg 3 (optional extra, integration only) · PostgreSQL ≥ 18 + `cairn_pgx` (integration) · SQL/PL-pgSQL (`db/017`) · Rust (one-line SCHEMA-array registration in `cairn-node`).

## Global Constraints

- **AGPL-3.0**; every dependency AGPL-3.0-compatible — checked *before* adding. (psycopg 3 is LGPL-3.0-or-later → compatible.)
- **TDD** — failing test first, then minimal code. No production code without a test that drove it.
- **Inline docs for a junior developer** — every non-trivial function explains *why* and *how it fits*, not just *what*.
- **Pure, reusable functions** over clever complexity; keep files focused and < ~500 lines.
- **B1's pure core is untouched** — `agreement.py`, `comparators.py`, `records.py`, `orchestrator.py`, `scoring.py` are not modified. `pipeline/adapter.py` and `pipeline/banding.py` import `psycopg` **never**.
- **The matcher is advisory** — it never auto-links and never auto-rejects. The only safety-critical part (the hard-veto floor, `db/016`) is *called*, never re-implemented.
- All Python work uses **uv**, never venv/pip. Run the pure suite with `cd matcher && uv run pytest`.
- Integration tests are **gated on `CAIRN_TEST_PG`** and skip cleanly when it is unset (same discipline as the Rust DB-gated tests). Example: `CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test"`.

---

### Task 1: Adapter — DOB precision-gated ISO parse

**Files:**
- Create: `matcher/src/cairn_matcher/pipeline/__init__.py`
- Create: `matcher/src/cairn_matcher/pipeline/adapter.py`
- Test: `matcher/tests/test_adapter_dob.py`

**Interfaces:**
- Consumes: `cairn_matcher.records.DateValue`.
- Produces: `parse_dob(value: str | None, precision: str | None) -> DateValue | None` — the precision-driven ISO field extractor. Never raises on malformed input; returns `None` (→ later graded `INSUFFICIENT_DATA`).

- [ ] **Step 1: Write the failing test**

```python
# matcher/tests/test_adapter_dob.py
"""parse_dob extracts ISO date fields at the precision the projection declares.

The patient_demographic dob `value` is ISO-8601 by the cairn-event write convention
(`1980-07-15` day, `1980-07` month, `1980` year) and `facets.precision` declares how
precise it is. parse_dob is NOT a locale date parser — it reads the fields ISO already
exposes, gated by the declared precision. Anything it cannot read at that precision
degrades to None (absence is safe; principle 4), never a wrong DateValue.
"""

from cairn_matcher.pipeline.adapter import parse_dob
from cairn_matcher.records import DateValue


def test_day_precision_parses_full_date():
    assert parse_dob("1980-07-15", "day") == DateValue(year=1980, month=7, day=15)


def test_month_precision_parses_year_and_month():
    assert parse_dob("1980-07", "month") == DateValue(year=1980, month=7, day=None)


def test_year_precision_parses_year_only():
    assert parse_dob("1980", "year") == DateValue(year=1980, month=None, day=None)


def test_absent_value_or_precision_is_none():
    assert parse_dob(None, "day") is None
    assert parse_dob("1980-07-15", None) is None


def test_non_iso_value_degrades_to_none():
    # A non-conformant peer wrote a locale string; we never guess. Safe degrade.
    assert parse_dob("15/07/1980", "day") is None
    assert parse_dob("not-a-date", "year") is None


def test_value_too_coarse_for_declared_precision_degrades():
    # precision says day but only a year is present -> cannot honour the claim -> None.
    assert parse_dob("1980", "day") is None


def test_unknown_precision_token_degrades_to_none():
    assert parse_dob("1980-07-15", "hour") is None
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_adapter_dob.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'cairn_matcher.pipeline'`.

- [ ] **Step 3: Write minimal implementation**

```python
# matcher/src/cairn_matcher/pipeline/__init__.py
"""IO-bearing matcher pipeline (piece B2).

This sub-package is the advisory pipeline that connects B1's pure scoring core to a
node's projections and persists a proposal. It is deliberately SEPARATE from the pure
core: `adapter` and `banding` are pure (no psycopg), while `db` and `runner` are the
only modules that touch Postgres. Importing `db`/`runner` requires the optional
`pipeline` extra (psycopg); `adapter`/`banding` never do.
"""
```

```python
# matcher/src/cairn_matcher/pipeline/adapter.py
"""Pure mappers from a node's patient_* projection rows into B1 CandidateRecords.

No I/O, no psycopg. Callers (pipeline.db) hand these functions plain dict rows; these
functions shape them into the value types B1 scores over. Every field degrades safely
on absence or malformed input (principle 4: absence is never disagreement); a
structurally wrong row raises MatcherTypeError elsewhere in this module (house rule #5).
"""

from cairn_matcher.records import DateValue

# The ISO field counts we can extract per declared precision. precision -> how many of
# (year, month, day) the value must supply. We never parse a locale date string; we only
# read the dash-separated ISO fields the cairn-event writer already emits.
_PRECISION_PARTS = {"year": 1, "month": 2, "day": 3}


def parse_dob(value: str | None, precision: str | None) -> DateValue | None:
    """Extract a DateValue from an ISO dob value at the projection's declared precision.

    Returns None (a safe, gradeable absence) when the value is missing, the precision is
    missing or unknown, or the value is not ISO-shaped to at least the declared precision.
    We never coerce a locale string or guess month/day order — that is a B3/locale-pack
    concern; here, an unreadable value simply has no DOB to compare.
    """
    if not value or precision not in _PRECISION_PARTS:
        return None
    parts = value.split("-")
    needed = _PRECISION_PARTS[precision]
    if len(parts) < needed:
        return None  # value is coarser than the precision it claims
    try:
        nums = [int(p) for p in parts[:needed]]
    except ValueError:
        return None  # non-numeric field -> not ISO -> safe degrade
    year = nums[0]
    month = nums[1] if needed >= 2 else None
    day = nums[2] if needed >= 3 else None
    return DateValue(year=year, month=month, day=day)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_adapter_dob.py -v`
Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/pipeline/__init__.py matcher/src/cairn_matcher/pipeline/adapter.py matcher/tests/test_adapter_dob.py
git commit -m "feat(matcher): B2 adapter — precision-gated ISO dob parse"
```

---

### Task 2: Adapter — names, identifiers, sex, and record assembly

**Files:**
- Modify: `matcher/src/cairn_matcher/pipeline/adapter.py`
- Test: `matcher/tests/test_adapter_record.py`

**Interfaces:**
- Consumes: `cairn_matcher.records.{CandidateRecord, FieldValue, Name, MatcherTypeError}`, and `parse_dob` from Task 1.
- Produces:
  - `build_names(rows: Sequence[Mapping]) -> FieldValue | None` — `value` is `frozenset[Name]` (each name an untagged `{"unspecified": tuple(value.split())}` bag), `provenance_rank` is the max over rows.
  - `build_identifiers(rows: Sequence[Mapping]) -> dict[str, frozenset[str]]` — `{system: {match_key, ...}}`, skipping `system == "unknown"`.
  - `single_field(row: Mapping | None) -> FieldValue | None` — `FieldValue(value=row["value"], provenance_rank=row["provenance_rank"])`.
  - `candidate_from_rows(*, dob_row, sex_row, name_rows, identifier_rows) -> CandidateRecord`.
  - Each row is a `Mapping` with the projection's column names (e.g. `value`, `facets`, `provenance_rank`, `system`, `match_key`).

- [ ] **Step 1: Write the failing test**

```python
# matcher/tests/test_adapter_record.py
"""build_* and candidate_from_rows shape projection rows into a CandidateRecord.

These are pure: they take plain dict rows (as pipeline.db will hand them) and return
B1 value types. They never read the event body and never touch a database.
"""

import pytest

from cairn_matcher.pipeline.adapter import (
    build_identifiers,
    build_names,
    candidate_from_rows,
    single_field,
)
from cairn_matcher.records import CandidateRecord, DateValue, FieldValue, MatcherTypeError, Name


def test_build_names_makes_untagged_bag_and_takes_max_provenance():
    rows = [
        {"value": "Alex Smith", "provenance_rank": 20},
        {"value": "Smith Alex", "provenance_rank": 60},  # order-tolerant -> same bag
    ]
    fv = build_names(rows)
    assert fv is not None
    assert fv.provenance_rank == 60  # weaker-side logic is the orchestrator's; here: max evidence
    assert fv.value == frozenset({Name(tokens={"unspecified": ("alex", "smith")})})


def test_build_names_lowercases_and_splits_on_whitespace():
    fv = build_names([{"value": "  John   Doe ", "provenance_rank": 10}])
    assert fv.value == frozenset({Name(tokens={"unspecified": ("john", "doe")})})


def test_build_names_empty_is_none():
    assert build_names([]) is None


def test_build_identifiers_groups_by_system_and_skips_unknown():
    rows = [
        {"system": "mrn:hospital-a", "match_key": "12345"},
        {"system": "mrn:hospital-a", "match_key": "67890"},
        {"system": "unknown", "match_key": "ignore-me"},
    ]
    assert build_identifiers(rows) == {"mrn:hospital-a": frozenset({"12345", "67890"})}


def test_build_identifiers_empty_is_empty_mapping():
    assert build_identifiers([]) == {}


def test_single_field_maps_value_and_rank():
    assert single_field({"value": "female", "provenance_rank": 60}) == FieldValue("female", 60)


def test_single_field_none_row_is_none():
    assert single_field(None) is None


def test_candidate_from_rows_assembles_all_fields():
    rec = candidate_from_rows(
        dob_row={"value": "1980-07-15", "facets": {"precision": "day"}, "provenance_rank": 60},
        sex_row={"value": "female", "provenance_rank": 60},
        name_rows=[{"value": "Alex Smith", "provenance_rank": 20}],
        identifier_rows=[{"system": "mrn:a", "match_key": "1"}],
    )
    assert rec.dob == FieldValue(DateValue(1980, 7, 15), 60)
    assert rec.sex_at_birth == FieldValue("female", 60)
    assert rec.names == build_names([{"value": "Alex Smith", "provenance_rank": 20}])
    assert rec.identifiers == {"mrn:a": frozenset({"1"})}


def test_candidate_from_rows_all_absent_is_empty_record():
    rec = candidate_from_rows(dob_row=None, sex_row=None, name_rows=[], identifier_rows=[])
    assert rec == CandidateRecord()


def test_candidate_from_rows_unparseable_dob_drops_to_none():
    rec = candidate_from_rows(
        dob_row={"value": "15/07/1980", "facets": {"precision": "day"}, "provenance_rank": 60},
        sex_row=None, name_rows=[], identifier_rows=[],
    )
    assert rec.dob is None  # safe degrade, not a wrong DateValue


def test_candidate_from_rows_wrong_type_raises():
    # A name row whose value is not a string is an adapter/upstream bug -> raise loudly.
    with pytest.raises(MatcherTypeError):
        build_names([{"value": 12345, "provenance_rank": 10}])
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_adapter_record.py -v`
Expected: FAIL — `ImportError: cannot import name 'build_names'`.

- [ ] **Step 3: Write minimal implementation**

Append to `matcher/src/cairn_matcher/pipeline/adapter.py`:

```python
from collections.abc import Mapping, Sequence

from cairn_matcher.records import CandidateRecord, FieldValue, MatcherTypeError, Name


def _name_bag(display: object) -> Name:
    """Turn one opaque display string into an untagged token-bag Name.

    patient_name projects only the authored display string — no given/family roles — so
    we put all whitespace-split, lower-cased tokens under a single 'unspecified' role.
    compare_name_set compares bags per role, so a shared single role reduces to a
    whole-string token-bag comparison (culture-neutral; no schema change). A non-string
    value is a structural bug, not mere absence -> raise (house rule #5).
    """
    if not isinstance(display, str):
        raise MatcherTypeError(f"name value must be str, got {type(display).__name__}")
    return Name(tokens={"unspecified": tuple(display.lower().split())})


def build_names(rows: Sequence[Mapping]) -> FieldValue | None:
    """Collect every asserted name into a frozenset[Name]; provenance = max over rows.

    The name FIELD's provenance is the strongest evidence behind any of the patient's
    retained names; the orchestrator separately reduces cross-record comparisons to the
    weaker side. Empty set -> None (absence -> INSUFFICIENT_DATA downstream).
    """
    if not rows:
        return None
    names = frozenset(_name_bag(r["value"]) for r in rows)
    rank = max(int(r["provenance_rank"]) for r in rows)
    return FieldValue(value=names, provenance_rank=rank)


def build_identifiers(rows: Sequence[Mapping]) -> dict[str, frozenset[str]]:
    """Group identifier match_keys by system, skipping the 'unknown' sentinel.

    match_key == coalesce(normalized, value) — the same key the db/016 veto floor uses,
    so the advisory positive-evidence comparison and the hard veto align on identity.
    """
    out: dict[str, set[str]] = {}
    for r in rows:
        system = r["system"]
        if system == "unknown":
            continue
        out.setdefault(system, set()).add(r["match_key"])
    return {system: frozenset(keys) for system, keys in out.items()}


def single_field(row: Mapping | None) -> FieldValue | None:
    """Map one patient_demographic winner row to a FieldValue, or None when absent."""
    if row is None:
        return None
    return FieldValue(value=row["value"], provenance_rank=int(row["provenance_rank"]))


def candidate_from_rows(
    *,
    dob_row: Mapping | None,
    sex_row: Mapping | None,
    name_rows: Sequence[Mapping],
    identifier_rows: Sequence[Mapping],
) -> CandidateRecord:
    """Assemble a CandidateRecord from one patient's projection rows.

    dob is special: its value is parsed via parse_dob at the row's declared precision; an
    unparseable value drops the whole dob field to None (safe degrade), never a guess.
    """
    dob = None
    if dob_row is not None:
        precision = (dob_row.get("facets") or {}).get("precision")
        parsed = parse_dob(dob_row["value"], precision)
        if parsed is not None:
            dob = FieldValue(value=parsed, provenance_rank=int(dob_row["provenance_rank"]))
    return CandidateRecord(
        dob=dob,
        sex_at_birth=single_field(sex_row),
        names=build_names(name_rows),
        identifiers=build_identifiers(identifier_rows),
    )
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_adapter_record.py -v`
Expected: PASS (11 tests).

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/pipeline/adapter.py matcher/tests/test_adapter_record.py
git commit -m "feat(matcher): B2 adapter — names/identifiers/sex + record assembly"
```

---

### Task 3: Banding — band classification + proposal payload + matcher version

**Files:**
- Create: `matcher/src/cairn_matcher/pipeline/banding.py`
- Test: `matcher/tests/test_banding.py`

**Interfaces:**
- Consumes: `cairn_matcher.scoring.{MatchScore, FieldEvidence, DEFAULT_WEIGHTS, Weights}`, `cairn_matcher.agreement.AgreementLevel`, `cairn_matcher.__version__`.
- Produces:
  - `Band` (`enum.Enum`): `AUTO_CANDIDATE = "auto_candidate"`, `REVIEW = "review"`.
  - `VetoFinding` (frozen dataclass): `veto_kind: str, severity: str, subject: str, detail: str`.
  - `Thresholds` (frozen dataclass): `review: float, auto: float`; `DEFAULT_THRESHOLDS = Thresholds(review=3.0, auto=8.0)`.
  - `band(score: MatchScore, vetoes: Sequence[VetoFinding], thresholds: Thresholds = DEFAULT_THRESHOLDS) -> Band | None`.
  - `matcher_version(weights: Weights = DEFAULT_WEIGHTS) -> str`.
  - `ProposalPayload` (frozen dataclass): `score_total: float, band: Band, veto_findings: tuple[dict, ...], evidence: tuple[dict, ...], matcher_version: str`.
  - `build_payload(score: MatchScore, vetoes: Sequence[VetoFinding], band_value: Band, weights: Weights = DEFAULT_WEIGHTS) -> ProposalPayload`.

- [ ] **Step 1: Write the failing test**

```python
# matcher/tests/test_banding.py
"""Banding turns a score + veto findings into an advisory band (or None), and shapes the
persisted proposal payload. Pure — no database.

The band honours db/016 exactly: ANY veto finding (hard_veto OR degrade_hold) caps the
band at REVIEW — a veto never auto-links and never auto-rejects. Below the review
threshold nothing is proposed (the noise floor; the B3 hub sweep is the declared
backstop for missed signal).
"""

from cairn_matcher.agreement import AgreementLevel
from cairn_matcher.pipeline.banding import (
    DEFAULT_THRESHOLDS,
    Band,
    ProposalPayload,
    Thresholds,
    VetoFinding,
    band,
    build_payload,
    matcher_version,
)
from cairn_matcher.scoring import FieldEvidence, MatchScore


def _score(total: float) -> MatchScore:
    return MatchScore(total=total, fields=(
        FieldEvidence("name", AgreementLevel.EXACT, 60, total),
    ))


def test_high_score_no_veto_is_auto_candidate():
    assert band(_score(9.0), []) is Band.AUTO_CANDIDATE


def test_mid_score_no_veto_is_review():
    assert band(_score(4.0), []) is Band.REVIEW


def test_below_review_threshold_is_none():
    assert band(_score(2.9), []) is None


def test_hard_veto_caps_high_score_at_review():
    v = [VetoFinding("dob", "hard_veto", "dob", "verified dob clash")]
    assert band(_score(9.0), v) is Band.REVIEW


def test_degrade_hold_also_caps_high_score_at_review():
    v = [VetoFinding("identifier", "degrade_hold", "mrn:a", "profile absent")]
    assert band(_score(9.0), v) is Band.REVIEW


def test_veto_does_not_resurrect_a_sub_threshold_pair():
    # No positive signal + a veto -> still nothing to propose.
    assert band(_score(1.0), [VetoFinding("dob", "hard_veto", "dob", "x")]) is None


def test_review_threshold_is_inclusive():
    assert band(_score(3.0), []) is Band.REVIEW


def test_auto_threshold_is_inclusive():
    assert band(_score(8.0), []) is Band.AUTO_CANDIDATE


def test_custom_thresholds_apply():
    assert band(_score(5.0), [], Thresholds(review=1.0, auto=4.0)) is Band.AUTO_CANDIDATE


def test_matcher_version_is_deterministic_and_carries_package_version():
    from cairn_matcher import __version__
    v1 = matcher_version()
    v2 = matcher_version()
    assert v1 == v2
    assert v1.startswith(f"{__version__}+")


def test_build_payload_serializes_evidence_and_vetoes():
    score = _score(9.0)
    vetoes = [VetoFinding("dob", "hard_veto", "dob", "verified dob clash")]
    payload = build_payload(score, vetoes, Band.REVIEW)
    assert isinstance(payload, ProposalPayload)
    assert payload.score_total == 9.0
    assert payload.band is Band.REVIEW
    assert payload.evidence[0]["field"] == "name"
    assert payload.evidence[0]["level"] == "EXACT"
    assert payload.veto_findings[0]["severity"] == "hard_veto"
    assert payload.matcher_version == matcher_version()
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_banding.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'cairn_matcher.pipeline.banding'`.

- [ ] **Step 3: Write minimal implementation**

```python
# matcher/src/cairn_matcher/pipeline/banding.py
"""Band a match score (gated by the db/016 veto findings) and shape the proposal payload.

This module owns the conservative auto-link threshold B1 deliberately did NOT (B1 returns
a raw score; the decision to act lives here, on the advisory side). It is pure: no DB.

Banding rule (priority order), honouring db/016's "never auto-link, never auto-reject":
  * total >= auto AND no veto findings (any severity)        -> AUTO_CANDIDATE
  * total >= review (incl. a high score capped by any veto)  -> REVIEW
  * total <  review                                          -> None  (persist nothing)

The thresholds here are SHIPPED DEFAULTS — illustrative magnitudes. Learning real ones
from local adjudication data is B3. Note the provenance_factor 0.5 floor (scoring.py)
halves every field at unknown provenance, so defaults are chosen with that in mind.
"""

import hashlib
from collections.abc import Sequence
from dataclasses import dataclass
from enum import Enum

from cairn_matcher import __version__
from cairn_matcher.scoring import DEFAULT_WEIGHTS, MatchScore, Weights


class Band(Enum):
    """The advisory disposition of a scored pair. Persisted as the string value."""

    AUTO_CANDIDATE = "auto_candidate"
    REVIEW = "review"


@dataclass(frozen=True)
class VetoFinding:
    """One row returned by the in-DB cairn_match_veto floor (carried verbatim)."""

    veto_kind: str
    severity: str
    subject: str
    detail: str


@dataclass(frozen=True)
class Thresholds:
    """The two conservative score cut-offs. review < auto. Defaults below; B3 learns."""

    review: float
    auto: float


DEFAULT_THRESHOLDS = Thresholds(review=3.0, auto=8.0)


def band(
    score: MatchScore,
    vetoes: Sequence[VetoFinding],
    thresholds: Thresholds = DEFAULT_THRESHOLDS,
) -> Band | None:
    """Classify a scored pair into AUTO_CANDIDATE / REVIEW / None (no proposal).

    ANY veto finding (hard_veto or degrade_hold) forbids AUTO_CANDIDATE and caps the band
    at REVIEW — never an auto-link, never an auto-reject. A pair below the review
    threshold yields None regardless of vetoes (no positive signal to act on).
    """
    if score.total < thresholds.review:
        return None
    if score.total >= thresholds.auto and not vetoes:
        return Band.AUTO_CANDIDATE
    return Band.REVIEW


def matcher_version(weights: Weights = DEFAULT_WEIGHTS) -> str:
    """A version-pin string for a proposal: package version + a digest of the weights.

    ADR-0014 makes the matcher a config-version-pinned actor. This is the lightweight
    slice of that: a proposal records WHICH matcher config produced it, so a re-run with
    different weights is distinguishable. Full §7.5 actor registration/signing is B3.
    """
    items = sorted(
        (field, level.name, w)
        for field, fw in weights.per_field.items()
        for level, w in fw.weights.items()
    )
    digest = hashlib.sha256(repr(items).encode()).hexdigest()[:12]
    return f"{__version__}+{digest}"


@dataclass(frozen=True)
class ProposalPayload:
    """Everything db.upsert_proposal needs, already JSON-serializable for the JSONB cols."""

    score_total: float
    band: Band
    veto_findings: tuple[dict, ...]
    evidence: tuple[dict, ...]
    matcher_version: str


def build_payload(
    score: MatchScore,
    vetoes: Sequence[VetoFinding],
    band_value: Band,
    weights: Weights = DEFAULT_WEIGHTS,
) -> ProposalPayload:
    """Shape a self-explaining proposal payload: the band, the score, and WHY (evidence
    breakdown + veto findings), plus the matcher version that produced it."""
    evidence = tuple(
        {
            "field": e.field,
            "level": e.level.name,
            "provenance_rank": e.provenance_rank,
            "weight_contribution": e.weight_contribution,
        }
        for e in score.fields
    )
    findings = tuple(
        {"veto_kind": v.veto_kind, "severity": v.severity, "subject": v.subject, "detail": v.detail}
        for v in vetoes
    )
    return ProposalPayload(
        score_total=score.total,
        band=band_value,
        veto_findings=findings,
        evidence=evidence,
        matcher_version=matcher_version(weights),
    )
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_banding.py -v`
Expected: PASS (11 tests).

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/pipeline/banding.py matcher/tests/test_banding.py
git commit -m "feat(matcher): B2 banding — veto-gated bands + proposal payload"
```

---

### Task 4: The `match_proposal` worklist table (`db/017`) + SCHEMA registration

**Files:**
- Create: `db/017_match_proposal.sql`
- Modify: `crates/cairn-node/src/db.rs:3-23` (the `SCHEMA` array)

**Interfaces:**
- Produces: table `match_proposal(patient_low, patient_high, score_total, band, veto_findings, evidence, matcher_version, status, created_at, updated_at)`, PK `(patient_low, patient_high)`, `CHECK (patient_low < patient_high)`; grants `SELECT, INSERT, UPDATE` to `cairn_agent`. (Read grants on `patient_*` already exist in db/010/011/012.)

- [ ] **Step 1: Write the failing test**

There is no Python test here; the deliverable is verified by the `cairn-node` schema-load path (the loader applies every SCHEMA entry on connect, so a malformed `017` breaks the existing DB-gated suite). Confirm the array currently has 15 entries and the suite is green first:

Run: `cargo test -p cairn-node --quiet 2>&1 | tail -5` (or, if `CAIRN_TEST_PG` is unset, just `cargo build -p cairn-node`).
Expected: builds; array length is `15`.

- [ ] **Step 2: Create the migration**

```sql
-- db/017_match_proposal.sql
-- §5.2 advisory match-proposal worklist (matcher piece B2 output).
--
-- WHAT: the durable, advisory output of the probabilistic matcher — one row per scored
-- patient pair the matcher thinks MIGHT be the same person. A review UI reads it; the
-- (future, §5.7) link-apply seam (piece C) consumes it.
--
-- ADVISORY, NOT A SAFETY GATE. There is no validated submit door here and no
-- submit_event involvement: a bad row is a bad PROPOSAL a human reviews, never record
-- corruption. The safety-critical floor is db/016 (cairn_match_veto), which the matcher
-- CALLS before writing; this table only records the advisory verdict.
--
-- Additive: no event-format change, no submit_event change. Reads nothing; only the
-- Python pipeline writes here (as a role granted cairn_agent).

CREATE TABLE IF NOT EXISTS match_proposal (
    -- The pair is stored in canonical (least, greatest) order so it is a natural unique
    -- key and the whole table is symmetric: propose(a,b) and propose(b,a) touch one row,
    -- mirroring cairn_match_veto's symmetry. The CHECK enforces the ordering invariant.
    patient_low        UUID    NOT NULL,
    patient_high       UUID    NOT NULL,
    score_total        DOUBLE PRECISION NOT NULL,
    band               TEXT    NOT NULL,   -- 'auto_candidate' | 'review'
    veto_findings      JSONB   NOT NULL,   -- cairn_match_veto rows, verbatim (explainability)
    evidence           JSONB   NOT NULL,   -- per-field MatchScore breakdown (explainability)
    matcher_version    TEXT    NOT NULL,   -- cairn_matcher version + config digest (ADR-0014)
    status             TEXT    NOT NULL DEFAULT 'pending',  -- human disposition
    created_at         TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (patient_low, patient_high),
    CHECK (patient_low < patient_high)
);

-- Advisory writer. cairn_agent is the NOLOGIN role (db/004) the matcher's login role is
-- granted into. No DELETE: retraction is a B3/sweep concern, deliberately not enabled here.
GRANT SELECT, INSERT, UPDATE ON match_proposal TO cairn_agent;
```

- [ ] **Step 3: Register the migration in the node SCHEMA array**

In `crates/cairn-node/src/db.rs`, change the array length and append the entry:

```rust
const SCHEMA: [(&str, &str); 16] = [
```

and after the `016_match_veto` line (before the closing `];`):

```rust
    ("016_match_veto",    include_str!("../../../db/016_match_veto.sql")),
    ("017_match_proposal", include_str!("../../../db/017_match_proposal.sql")),
];
```

- [ ] **Step 4: Verify it builds and (if PG available) loads**

Run: `cargo build -p cairn-node`
Expected: compiles (array length now `16`, file embedded).

If `CAIRN_TEST_PG` is set: `cargo test -p cairn-node --quiet 2>&1 | tail -5`
Expected: the schema-load path applies `017` cleanly; suite still green.

- [ ] **Step 5: Commit**

```bash
git add db/017_match_proposal.sql crates/cairn-node/src/db.rs
git commit -m "feat(db): 017 advisory match_proposal worklist + SCHEMA registration"
```

---

### Task 5: Integration scaffolding — `pipeline` extra + gated conftest

**Files:**
- Modify: `matcher/pyproject.toml`
- Create: `matcher/tests/conftest.py`
- Test: `matcher/tests/test_pipeline_smoke.py`

**Interfaces:**
- Produces pytest fixtures:
  - `pg_conn` — a `psycopg.Connection` to `CAIRN_TEST_PG`, with the full node schema applied (idempotent), or `pytest.skip` when `CAIRN_TEST_PG` is unset. Truncates `match_proposal` and the `patient_*` projections before each test.
  - `seed_patient(conn, patient_id, *, dob=None, sex=None, names=(), identifiers=())` helper — inserts rows directly into the projections. **Design note:** B2 (and db/016) read ONLY the projections, so seeding the projection rows directly is a faithful, focused integration test; it deliberately avoids pulling Ed25519 signing + the `submit_event`/`cairn_pgx` event path into the matcher's Python suite.

- [ ] **Step 1: Add the optional dependency extra**

In `matcher/pyproject.toml`, add (create the table if absent):

```toml
[project.optional-dependencies]
pipeline = ["psycopg[binary]>=3.1"]
```

- [ ] **Step 2: Write the conftest (the scaffolding under test)**

```python
# matcher/tests/conftest.py
"""Shared fixtures for the gated integration tests.

These tests need a real PostgreSQL >= 18 with the cairn_pgx extension installed (the same
substrate the Rust DB-gated tests use). They are SKIPPED cleanly when CAIRN_TEST_PG is
unset, so `uv run pytest` stays green on a machine with no database.

The conftest applies the node schema itself (the same db/*.sql files, in the same order,
the cairn-node loader applies on connect — all idempotent) so the Python suite is
self-sufficient given a PG+cairn_pgx cluster.
"""

import os
from pathlib import Path

import pytest

CAIRN_TEST_PG = os.environ.get("CAIRN_TEST_PG")

# Mirror crates/cairn-node/src/db.rs SCHEMA order. 008 is intentionally skipped (spike-only).
_SCHEMA_FILES = [
    "001_envelope", "002_projection", "003_blobs", "004_actors", "005_submit",
    "006_recall", "007_node_federation", "009_node_supersede_and_restore",
    "010_demographics", "011_demographics_fields", "012_demographics_names",
    "013_demographics_sex_gender", "014_demographics_address", "015_globalise_twin",
    "016_match_veto", "017_match_proposal",
]

_DB_DIR = Path(__file__).resolve().parents[2] / "db"

# Projection tables a test seeds / the fixture truncates between tests.
_PROJECTION_TABLES = ["match_proposal", "patient_identifier", "patient_demographic", "patient_name"]


def _apply_schema(conn) -> None:
    """Apply every SCHEMA file in order (idempotent; CREATE IF NOT EXISTS / OR REPLACE)."""
    with conn.cursor() as cur:
        for name in _SCHEMA_FILES:
            cur.execute((_DB_DIR / f"{name}.sql").read_text())
    conn.commit()


@pytest.fixture
def pg_conn():
    """A connection with schema applied and projection tables truncated; skip if no DB."""
    if not CAIRN_TEST_PG:
        pytest.skip("CAIRN_TEST_PG not set — skipping DB-gated integration test")
    import psycopg

    conn = psycopg.connect(CAIRN_TEST_PG, autocommit=False)
    try:
        _apply_schema(conn)
        with conn.cursor() as cur:
            cur.execute(f"TRUNCATE {', '.join(_PROJECTION_TABLES)}")
        conn.commit()
        yield conn
    finally:
        conn.rollback()
        conn.close()


def seed_patient(conn, patient_id, *, dob=None, sex=None, names=(), identifiers=()):
    """Insert projection rows for one patient directly (bypassing submit_event).

    dob/sex: (value, provenance_rank[, precision]) tuples or None.
    names: iterable of (value, provenance_rank). identifiers: iterable of (system, match_key, value).
    """
    import json

    with conn.cursor() as cur:
        if dob is not None:
            value, rank, *rest = dob
            precision = rest[0] if rest else "day"
            cur.execute(
                "INSERT INTO patient_demographic (patient_id, field, value, facets, "
                "provenance, provenance_rank, asserted_hlc_wall, asserted_hlc_count, asserted_origin) "
                "VALUES (%s,'dob',%s,%s,'seed',%s,0,0,'seed')",
                (patient_id, value, json.dumps({"precision": precision}), rank),
            )
        if sex is not None:
            value, rank = sex
            cur.execute(
                "INSERT INTO patient_demographic (patient_id, field, value, facets, "
                "provenance, provenance_rank, asserted_hlc_wall, asserted_hlc_count, asserted_origin) "
                "VALUES (%s,'sex-at-birth',%s,NULL,'seed',%s,0,0,'seed')",
                (patient_id, value, rank),
            )
        for value, rank in names:
            cur.execute(
                "INSERT INTO patient_name (patient_id, use_key, value, use_raw, provenance, "
                "provenance_rank, last_hlc_wall, last_hlc_count, asserted_origin) "
                "VALUES (%s,'legal',%s,'legal','seed',%s,0,0,'seed') ON CONFLICT DO NOTHING",
                (patient_id, value, rank),
            )
        for system, match_key, value in identifiers:
            cur.execute(
                "INSERT INTO patient_identifier (patient_id, system, match_key, value, normalized, "
                "profile, use_type, provenance, asserted_hlc_wall, asserted_hlc_count, asserted_origin) "
                "VALUES (%s,%s,%s,%s,%s,NULL,NULL,'seed',0,0,'seed') ON CONFLICT DO NOTHING",
                (patient_id, system, match_key, value, match_key),
            )
    conn.commit()
```

- [ ] **Step 3: Write the smoke test**

```python
# matcher/tests/test_pipeline_smoke.py
"""Smoke test: the gated fixture applies schema, seeds, and reads back. Proves the
integration substrate works (or skips cleanly with no DB) before the real pipeline tests.
"""

from tests.conftest import seed_patient

PA = "11111111-1111-1111-1111-111111111111"


def test_fixture_seeds_and_reads_back(pg_conn):
    seed_patient(pg_conn, PA, dob=("1980-07-15", 60, "day"), names=[("Alex Smith", 20)])
    with pg_conn.cursor() as cur:
        cur.execute("SELECT value FROM patient_demographic WHERE patient_id = %s AND field='dob'", (PA,))
        assert cur.fetchone()[0] == "1980-07-15"
        cur.execute("SELECT count(*) FROM patient_name WHERE patient_id = %s", (PA,))
        assert cur.fetchone()[0] == 1
```

- [ ] **Step 4: Run (skips without DB, passes with it)**

Run (no DB): `cd matcher && uv run pytest tests/test_pipeline_smoke.py -v`
Expected: SKIPPED ("CAIRN_TEST_PG not set ...").

Run (with DB): `cd matcher && CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" uv run --extra pipeline pytest tests/test_pipeline_smoke.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add matcher/pyproject.toml matcher/tests/conftest.py matcher/tests/test_pipeline_smoke.py
git commit -m "test(matcher): B2 gated integration scaffolding (pipeline extra + conftest)"
```

---

### Task 6: `db.py` + `runner.py` — the IO layer and the end-to-end pipeline

**Files:**
- Create: `matcher/src/cairn_matcher/pipeline/db.py`
- Create: `matcher/src/cairn_matcher/pipeline/runner.py`
- Test: `matcher/tests/test_pipeline_e2e.py`

**Interfaces:**
- Consumes: `adapter.candidate_from_rows`, `banding.{VetoFinding, band, build_payload, Band, Thresholds, DEFAULT_THRESHOLDS}`, `orchestrator.field_comparisons`, `scoring.{score, DEFAULT_WEIGHTS}`.
- Produces:
  - `db.load_candidate(conn, patient_id) -> CandidateRecord`
  - `db.match_veto(conn, a, b) -> list[VetoFinding]`
  - `db.upsert_proposal(conn, low, high, payload: ProposalPayload) -> None`
  - `runner.propose(conn, a, b, *, thresholds=DEFAULT_THRESHOLDS, weights=DEFAULT_WEIGHTS) -> Band | None`

- [ ] **Step 1: Write the failing end-to-end test**

```python
# matcher/tests/test_pipeline_e2e.py
"""End-to-end: seed two patients' projections, run propose(), assert the persisted
match_proposal row. Gated on CAIRN_TEST_PG (skips cleanly without a database).

Covers: a clean strong match -> a persisted proposal; a verified-DOB clash -> the db/016
hard veto caps the band at 'review' (never auto, never dropped); a weak pair -> no row;
re-running preserves a human-set status (latest-wins but status-preserving).
"""

import json

from cairn_matcher.pipeline.banding import Band
from cairn_matcher.pipeline.runner import propose
from tests.conftest import seed_patient

PA = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
PB = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"
LOW, HIGH = (PA, PB) if PA < PB else (PB, PA)


def _row(conn):
    with conn.cursor() as cur:
        cur.execute(
            "SELECT score_total, band, veto_findings, evidence, status FROM match_proposal "
            "WHERE patient_low=%s AND patient_high=%s", (LOW, HIGH))
        return cur.fetchone()


def test_strong_match_persists_a_review_proposal(pg_conn):
    # Shared identifier (8.0 * 0.5 = 4.0) -> crosses review but not auto. No veto.
    for p in (PA, PB):
        seed_patient(pg_conn, p, names=[("Alex Smith", 20)],
                     identifiers=[("mrn:hospital-a", "12345", "12345")])
    result = propose(pg_conn, PA, PB)
    assert result is Band.REVIEW
    row = _row(pg_conn)
    assert row is not None
    assert row[1] == "review"
    assert row[4] == "pending"


def test_verified_dob_clash_caps_at_review(pg_conn):
    # Strong name+id signal, but verified, same-precision, different DOBs -> hard veto.
    seed_patient(pg_conn, PA, dob=("1980-07-15", 60, "day"), names=[("Alex Smith", 60)],
                 identifiers=[("mrn:a", "1", "1")])
    seed_patient(pg_conn, PB, dob=("1990-01-01", 60, "day"), names=[("Alex Smith", 60)],
                 identifiers=[("mrn:a", "1", "1")])
    result = propose(pg_conn, PA, PB)
    assert result is Band.REVIEW  # never AUTO_CANDIDATE under a veto
    row = _row(pg_conn)
    findings = row[2]
    assert any(f["veto_kind"] == "dob" and f["severity"] == "hard_veto" for f in findings)


def test_weak_pair_persists_nothing(pg_conn):
    # Only sex agrees (1.0 * 0.5 = 0.5) -> below review threshold -> no proposal.
    for p in (PA, PB):
        seed_patient(pg_conn, p, sex=("female", 0))
    assert propose(pg_conn, PA, PB) is None
    assert _row(pg_conn) is None


def test_rerun_preserves_human_status(pg_conn):
    for p in (PA, PB):
        seed_patient(pg_conn, p, names=[("Alex Smith", 20)],
                     identifiers=[("mrn:hospital-a", "12345", "12345")])
    propose(pg_conn, PA, PB)
    with pg_conn.cursor() as cur:  # a reviewer accepts it
        cur.execute("UPDATE match_proposal SET status='accepted' WHERE patient_low=%s", (LOW,))
    pg_conn.commit()
    propose(pg_conn, PB, PA)  # re-run, reversed order -> same row
    assert _row(pg_conn)[4] == "accepted"  # status preserved, not clobbered to 'pending'
```

- [ ] **Step 2: Run test to verify it fails**

Run (with DB): `cd matcher && CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" uv run --extra pipeline pytest tests/test_pipeline_e2e.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'cairn_matcher.pipeline.runner'`.

- [ ] **Step 3: Write `db.py`**

```python
# matcher/src/cairn_matcher/pipeline/db.py
"""The only Postgres-touching module in the matcher. Thin: it loads a patient's
projection rows, calls the in-DB veto floor, and upserts a proposal. All scoring and
banding logic lives in the pure modules; this module just moves data.

Requires the optional `pipeline` extra (psycopg). The pure core never imports it.
"""

import json

from psycopg.rows import dict_row

from cairn_matcher.pipeline.adapter import candidate_from_rows
from cairn_matcher.pipeline.banding import ProposalPayload, VetoFinding
from cairn_matcher.records import CandidateRecord


def load_candidate(conn, patient_id) -> CandidateRecord:
    """Read one patient's matching-relevant projection rows and shape a CandidateRecord.

    Reads the winner rows (dob, sex-at-birth) and the retained sets (names, identifiers).
    Pure shaping is delegated to adapter.candidate_from_rows.
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT value, facets, provenance_rank FROM patient_demographic "
                    "WHERE patient_id=%s AND field='dob'", (patient_id,))
        dob_row = cur.fetchone()
        cur.execute("SELECT value, provenance_rank FROM patient_demographic "
                    "WHERE patient_id=%s AND field='sex-at-birth'", (patient_id,))
        sex_row = cur.fetchone()
        cur.execute("SELECT value, provenance_rank FROM patient_name WHERE patient_id=%s",
                    (patient_id,))
        name_rows = cur.fetchall()
        cur.execute("SELECT system, match_key FROM patient_identifier WHERE patient_id=%s",
                    (patient_id,))
        identifier_rows = cur.fetchall()
    return candidate_from_rows(
        dob_row=dob_row, sex_row=sex_row, name_rows=name_rows, identifier_rows=identifier_rows
    )


def match_veto(conn, a, b) -> list[VetoFinding]:
    """Call the safety-critical in-DB hard-veto floor (db/016) and return its rows.

    The matcher NEVER re-implements this; it only consults it. A pair with any finding
    cannot be auto-linked (banding enforces that).
    """
    with conn.cursor() as cur:
        cur.execute("SELECT veto_kind, severity, subject, detail FROM cairn_match_veto(%s, %s)",
                    (a, b))
        return [VetoFinding(*row) for row in cur.fetchall()]


def upsert_proposal(conn, low, high, payload: ProposalPayload) -> None:
    """Write (or refresh) the advisory proposal for a canonical-ordered pair.

    Latest-wins on (patient_low, patient_high), but a non-'pending' status (a human's
    decision) is PRESERVED — a re-run refreshes the score/band/evidence, never a verdict.
    """
    with conn.cursor() as cur:
        cur.execute(
            "INSERT INTO match_proposal "
            "(patient_low, patient_high, score_total, band, veto_findings, evidence, matcher_version) "
            "VALUES (%s,%s,%s,%s,%s,%s,%s) "
            "ON CONFLICT (patient_low, patient_high) DO UPDATE SET "
            "score_total=EXCLUDED.score_total, band=EXCLUDED.band, "
            "veto_findings=EXCLUDED.veto_findings, evidence=EXCLUDED.evidence, "
            "matcher_version=EXCLUDED.matcher_version, updated_at=clock_timestamp()",
            (low, high, payload.score_total, payload.band.value,
             json.dumps(list(payload.veto_findings)), json.dumps(list(payload.evidence)),
             payload.matcher_version),
        )
    conn.commit()
```

- [ ] **Step 4: Write `runner.py`**

```python
# matcher/src/cairn_matcher/pipeline/runner.py
"""Orchestrate one pairwise proposal: load -> score -> veto -> band -> persist.

This is the only place IO (pipeline.db) and the pure core (orchestrator/scoring/banding)
meet. It computes a verdict for a single given pair; finding WHICH pairs to score
(blocking) is B2b. A pair below the review threshold persists nothing — the B3 hub
duplicate-sweep is the declared backstop for any signal missed at the noise floor.
"""

from cairn_matcher.orchestrator import field_comparisons
from cairn_matcher.pipeline import db
from cairn_matcher.pipeline.banding import (
    DEFAULT_THRESHOLDS,
    Band,
    Thresholds,
    band,
    build_payload,
)
from cairn_matcher.scoring import DEFAULT_WEIGHTS, Weights, score


def propose(
    conn,
    a,
    b,
    *,
    thresholds: Thresholds = DEFAULT_THRESHOLDS,
    weights: Weights = DEFAULT_WEIGHTS,
) -> Band | None:
    """Score the pair (a, b), gate on the in-DB veto, and persist a proposal if warranted.

    Returns the Band (AUTO_CANDIDATE | REVIEW) when a proposal is written, or None when
    the pair is below the review threshold (nothing persisted). The pair is stored in
    canonical (low, high) order so the row is symmetric in a and b.
    """
    rec_a = db.load_candidate(conn, a)
    rec_b = db.load_candidate(conn, b)
    comparisons = field_comparisons(rec_a, rec_b)
    match_score = score(comparisons, weights)
    vetoes = db.match_veto(conn, a, b)
    band_value = band(match_score, vetoes, thresholds)
    if band_value is None:
        return None
    low, high = (str(a), str(b)) if str(a) < str(b) else (str(b), str(a))
    payload = build_payload(match_score, vetoes, band_value, weights)
    db.upsert_proposal(conn, low, high, payload)
    return band_value
```

- [ ] **Step 5: Run the end-to-end test to verify it passes**

Run (with DB): `cd matcher && CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" uv run --extra pipeline pytest tests/test_pipeline_e2e.py -v`
Expected: PASS (4 tests).

- [ ] **Step 6: Commit**

```bash
git add matcher/src/cairn_matcher/pipeline/db.py matcher/src/cairn_matcher/pipeline/runner.py matcher/tests/test_pipeline_e2e.py
git commit -m "feat(matcher): B2 db + runner — veto-gated pairwise pipeline end-to-end"
```

---

### Task 7: README + full-suite verification

**Files:**
- Modify: `matcher/README.md`

**Interfaces:** none (documentation + verification).

- [ ] **Step 1: Document the pipeline sub-package + integration tests in the matcher README**

Add a section to `matcher/README.md`:

```markdown
## pipeline/ (piece B2 — advisory pairwise pipeline)

`cairn_matcher.pipeline` connects the pure scoring core to a node's `patient_*`
projections and persists an advisory proposal. It is the only IO-bearing part:

- `adapter.py`, `banding.py` — **pure** (no psycopg); projection rows → `CandidateRecord`,
  and `MatchScore` + db/016 veto findings → a band (`auto_candidate` / `review` / none).
- `db.py`, `runner.py` — Postgres IO; require the optional `pipeline` extra (psycopg).

`runner.propose(conn, a, b)` scores a pair via B1, gates it on the in-DB
`cairn_match_veto` (db/016), and upserts a row into `match_proposal` (db/017). A veto
caps the band at `review` — never an auto-link, never an auto-reject. Below the review
threshold nothing is persisted (the B3 hub duplicate-sweep is the backstop).

### Tests

- Pure suite (no DB): `uv run pytest`
- Integration (gated): needs PostgreSQL ≥ 18 + `cairn_pgx`; skips when `CAIRN_TEST_PG` is unset:
  `CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" uv run --extra pipeline pytest`
```

- [ ] **Step 2: Run the full pure suite (no DB) — everything green and nothing skipped unexpectedly**

Run: `cd matcher && uv run pytest -v`
Expected: all B1 tests + Task 1/2/3 pure tests PASS; the integration tests (Tasks 5–6) SKIP with "CAIRN_TEST_PG not set".

- [ ] **Step 3: Run the full suite WITH a database**

Run: `cd matcher && CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" uv run --extra pipeline pytest -v`
Expected: every test PASSES (pure + integration).

- [ ] **Step 4: Confirm the Rust workspace still builds (SCHEMA change)**

Run: `cargo build -p cairn-node && cargo clippy -p cairn-node --quiet`
Expected: compiles, clippy clean.

- [ ] **Step 5: Commit**

```bash
git add matcher/README.md
git commit -m "docs(matcher): document B2 pipeline sub-package + integration tests"
```

---

## Self-Review

**Spec coverage:** §1 layout → Tasks 1–6 + pyproject (Task 5). §2 adapter (dob/sex/names/identifiers, degrade, MatcherTypeError) → Tasks 1–2. §3.2 banding (veto caps both severities, sub-threshold None, thresholds) → Task 3. §3.1 db.py + §3.3 runner (one path; veto mandatory) → Task 6. §4 db/017 table (canonical order, CHECK, upsert status-preserving, grants) → Tasks 4 & 6. §5 error handling (degrade vs raise; veto mandatory; transaction) → Tasks 1/2 (degrade), 6 (veto/commit). §6 testing (pure + gated integration; the four integration cases) → Tasks 5–7. §7 out-of-scope items are not implemented (correct). §8 footprint (no ADR/spec bump, SCHEMA 15→16, one optional dep, B1 untouched) → respected.

**Refinement vs spec (recorded):** integration tests seed projection rows directly rather than via `submit_event` (Task 5 design note) — faithful because B2/db/016 read only projections, and it keeps the Python suite free of Ed25519/cairn_pgx event machinery. Read grants on `patient_*` already exist (db/010/011/012), so db/017 only grants on `match_proposal` (Task 4) — a simplification from the spec's "grants we add".

**Placeholder scan:** none — every code/SQL step is complete.

**Type consistency:** `VetoFinding` defined in `banding.py` (Task 3), imported by `db.py`/tests (Task 6). `ProposalPayload`/`Band`/`Thresholds` consistent across Tasks 3/6. `candidate_from_rows` keyword signature matches between Task 2 definition and Task 6 `db.load_candidate` call. `parse_dob` signature consistent across Tasks 1–2. `propose` signature consistent between Task 6 definition and Tasks 5–6 tests.
