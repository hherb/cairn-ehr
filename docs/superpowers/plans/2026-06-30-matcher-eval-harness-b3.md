# Matcher Eval Harness (B3 keystone) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a labelled-dataset eval harness for the §5.2 advisory matcher that measures scorer/banding quality and blocking recall, unblocking the measurement-driven B3 items (compound blocking keys, weight-learning).

**Architecture:** A new `cairn_matcher/eval/` sub-package beside `pipeline/`, mirroring the established pure-core / optional-DB split. A dataset record mirrors the projection-row shape so both consumers derive from one shape: the **pure** scorer eval reuses the real `candidate_from_rows` adapter; the **DB-gated** blocking eval seeds `patient_*` and calls the real `generate_candidate_pairs`. No parallel blocking implementation. A thin CLI (`python -m cairn_matcher.eval`) prints a report.

**Tech Stack:** Python ≥3.11, stdlib only for the pure core (`json`, `uuid`, `statistics`, `dataclasses`, `itertools`); `psycopg` (existing optional `pipeline` extra) for the blocking layer; pytest; uv.

## Global Constraints

- **AGPL-3.0**; every dependency AGPL-3.0-compatible. **No new dependency** (pure core: stdlib only; blocking layer: existing `pipeline` extra / psycopg).
- **TDD**: failing test first, then minimal code. Run via `uv` only — never venv/pip. Pure suite: `cd matcher && uv run pytest`. DB-gated: `cd matcher && CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" uv run --extra pipeline pytest`.
- **Pure core stays pure**: modules under `eval/` except `blocking_eval.py` and the lazy CLI branch MUST NOT import psycopg, directly or transitively. `cairn_matcher.pipeline.banding` is safe to import (it is psycopg-free; `pipeline/__init__.py` is docstring-only).
- **Junior-legible inline docs** on every non-trivial function (house rule #3). **Pure functions, small focused files** under 500 lines (house rules #1, #4).
- **Advisory only**: a defect yields a wrong metric a human reads, never record corruption. No `db/` floor file, no SCHEMA bump, no spec/ADR change (implements settled §5.2/§5.13/ADR-0014).
- Working directory for all commands is `matcher/` unless stated. New package dir: `matcher/src/cairn_matcher/eval/`. Tests: `matcher/tests/`.

---

### Task 1: Dataset value types + loader

**Files:**
- Create: `matcher/src/cairn_matcher/eval/__init__.py`
- Create: `matcher/src/cairn_matcher/eval/dataset.py`
- Test: `matcher/tests/test_eval_dataset.py`

**Interfaces:**
- Consumes: nothing (leaf).
- Produces:
  - `DatasetError(ValueError)`
  - `DatasetRecord(record_id: str, dob: Mapping | None = None, sex_at_birth: Mapping | None = None, names: tuple[Mapping, ...] = (), identifiers: tuple[Mapping, ...] = ())` — frozen.
  - `EntityCluster(entity_id: str, records: tuple[DatasetRecord, ...])` — frozen.
  - `LabelledDataset(name: str, entities: tuple[EntityCluster, ...], description: str = "")` — frozen, with method `all_records() -> tuple[DatasetRecord, ...]`.
  - `load_dataset(obj: Mapping) -> LabelledDataset`.

- [ ] **Step 1: Write the failing test**

Create `matcher/tests/test_eval_dataset.py`:

```python
"""Pure tests for the eval dataset value types and loader."""

import pytest

from cairn_matcher.eval.dataset import (
    DatasetError,
    DatasetRecord,
    EntityCluster,
    LabelledDataset,
    load_dataset,
)

_MINIMAL = {
    "name": "tiny",
    "entities": [
        {"entity_id": "e1", "records": [
            {"record_id": "r1", "dob": {"value": "1990-05-12", "precision": "day", "provenance_rank": 70}},
            {"record_id": "r2", "names": [{"value": "Alex Nguyen", "provenance_rank": 30}]},
        ]},
        {"entity_id": "e2", "records": [{"record_id": "r3"}]},
    ],
}


def test_load_dataset_builds_typed_tree():
    ds = load_dataset(_MINIMAL)
    assert isinstance(ds, LabelledDataset)
    assert ds.name == "tiny"
    assert len(ds.entities) == 2
    assert isinstance(ds.entities[0], EntityCluster)
    assert isinstance(ds.entities[0].records[0], DatasetRecord)
    assert ds.entities[0].records[0].record_id == "r1"
    assert ds.entities[0].records[0].dob == {"value": "1990-05-12", "precision": "day", "provenance_rank": 70}


def test_all_records_flattens_in_order():
    ds = load_dataset(_MINIMAL)
    assert [r.record_id for r in ds.all_records()] == ["r1", "r2", "r3"]


def test_missing_record_id_raises():
    bad = {"name": "x", "entities": [{"entity_id": "e", "records": [{"dob": {}}]}]}
    with pytest.raises(DatasetError):
        load_dataset(bad)


def test_duplicate_record_id_raises():
    bad = {"name": "x", "entities": [
        {"entity_id": "e1", "records": [{"record_id": "dup"}]},
        {"entity_id": "e2", "records": [{"record_id": "dup"}]},
    ]}
    with pytest.raises(DatasetError):
        load_dataset(bad)


def test_missing_entities_key_raises():
    with pytest.raises(DatasetError):
        load_dataset({"name": "x"})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_eval_dataset.py -q`
Expected: FAIL — `ModuleNotFoundError: No module named 'cairn_matcher.eval'`.

- [ ] **Step 3: Write minimal implementation**

Create `matcher/src/cairn_matcher/eval/__init__.py`:

```python
"""Cairn matcher eval harness — measurement substrate for the §5.2 advisory matcher.

Pure by default (stdlib only): dataset format, scorer/banding metrics, and a CLI. The
blocking-recall layer (`blocking_eval`) is the one DB-touching module and needs the
optional `pipeline` extra (psycopg). This package ships NO clinical floor and makes NO
link decision — a defect yields a wrong metric a human reads, never record corruption.
"""
```

Create `matcher/src/cairn_matcher/eval/dataset.py`:

```python
"""The labelled-dataset format the harness measures, plus its loader.

Ground truth is expressed as ENTITY CLUSTERS: records grouped by the real person they
describe. Within-cluster record pairs are true matches; cross-cluster pairs are true
non-matches. That avoids hand-labelling O(n^2) pairs.

A dataset record deliberately mirrors the projection-row SHAPE the matcher already
operates over, so the pure scorer eval and the DB blocking eval both derive from one
shape with no parallel construction logic (see record_to_candidate / blocking_eval).
"""

from collections.abc import Mapping, Sequence
from dataclasses import dataclass


class DatasetError(ValueError):
    """The dataset JSON is structurally invalid (missing/duplicate ids, wrong shape).

    Raised loudly rather than silently tolerated (house rule #5): a malformed eval set
    would otherwise produce quietly-wrong metrics.
    """


@dataclass(frozen=True)
class DatasetRecord:
    """One patient record as projection-shaped field dicts. Every field is optional
    except record_id; absence is a safe, gradeable absence (principle 4), not an error.

    dob: {"value": ISO str, "precision": "year"|"month"|"day", "provenance_rank": int}
    sex_at_birth: {"value": str, "provenance_rank": int}
    names: tuple of {"value": display str, "provenance_rank": int}
    identifiers: tuple of {"system": str, "match_key": str, "value": str}
    """

    record_id: str
    dob: Mapping | None = None
    sex_at_birth: Mapping | None = None
    names: tuple[Mapping, ...] = ()
    identifiers: tuple[Mapping, ...] = ()


@dataclass(frozen=True)
class EntityCluster:
    """All records that describe ONE real person — the ground-truth grouping."""

    entity_id: str
    records: tuple[DatasetRecord, ...]


@dataclass(frozen=True)
class LabelledDataset:
    """A named set of entity clusters: the unit the harness evaluates."""

    name: str
    entities: tuple[EntityCluster, ...]
    description: str = ""

    def all_records(self) -> tuple[DatasetRecord, ...]:
        """Every record across all clusters, in cluster-then-record declaration order."""
        return tuple(r for e in self.entities for r in e.records)


def _record_from(obj: Mapping) -> DatasetRecord:
    """Shape one record dict into a DatasetRecord; require a non-empty record_id."""
    record_id = obj.get("record_id")
    if not isinstance(record_id, str) or not record_id:
        raise DatasetError(f"each record needs a non-empty string record_id, got {obj!r}")
    return DatasetRecord(
        record_id=record_id,
        dob=obj.get("dob"),
        sex_at_birth=obj.get("sex_at_birth"),
        names=tuple(obj.get("names", ())),
        identifiers=tuple(obj.get("identifiers", ())),
    )


def load_dataset(obj: Mapping) -> LabelledDataset:
    """Parse an in-memory dataset mapping (already JSON-decoded) into typed clusters.

    Validates the two invariants the harness depends on: there is an `entities` list,
    and every record_id is unique across the whole dataset (pairs are keyed by id).
    """
    entities_raw = obj.get("entities")
    if not isinstance(entities_raw, Sequence) or isinstance(entities_raw, (str, bytes)):
        raise DatasetError("dataset needs an 'entities' list")

    seen_ids: set[str] = set()
    entities: list[EntityCluster] = []
    for ent in entities_raw:
        entity_id = ent.get("entity_id")
        if not isinstance(entity_id, str) or not entity_id:
            raise DatasetError(f"each entity needs a non-empty entity_id, got {ent!r}")
        records = tuple(_record_from(r) for r in ent.get("records", ()))
        for r in records:
            if r.record_id in seen_ids:
                raise DatasetError(f"duplicate record_id across dataset: {r.record_id!r}")
            seen_ids.add(r.record_id)
        entities.append(EntityCluster(entity_id=entity_id, records=records))

    return LabelledDataset(
        name=str(obj.get("name", "")),
        description=str(obj.get("description", "")),
        entities=tuple(entities),
    )
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_eval_dataset.py -q`
Expected: PASS (5 passed).

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/eval/__init__.py matcher/src/cairn_matcher/eval/dataset.py matcher/tests/test_eval_dataset.py
git commit -m "feat(matcher): eval dataset value types + loader (B3)"
```

---

### Task 2: record_to_candidate + ground-truth pair derivation

**Files:**
- Modify: `matcher/src/cairn_matcher/eval/dataset.py` (append functions)
- Test: `matcher/tests/test_eval_truth.py`

**Interfaces:**
- Consumes: `DatasetRecord`, `LabelledDataset` (Task 1); `cairn_matcher.pipeline.adapter.candidate_from_rows`; `cairn_matcher.records.CandidateRecord`.
- Produces:
  - `record_to_candidate(rec: DatasetRecord) -> CandidateRecord`
  - `canonical_label_pair(a: str, b: str) -> tuple[str, str]` (lexical low, high)
  - `truth_pairs(ds: LabelledDataset) -> frozenset[tuple[str, str]]`
  - `all_pairs(ds: LabelledDataset) -> list[tuple[str, str]]`

- [ ] **Step 1: Write the failing test**

Create `matcher/tests/test_eval_truth.py`:

```python
"""Pure tests for record->CandidateRecord mapping and ground-truth pair derivation."""

from cairn_matcher.eval.dataset import (
    all_pairs,
    canonical_label_pair,
    load_dataset,
    record_to_candidate,
    truth_pairs,
)
from cairn_matcher.pipeline.adapter import candidate_from_rows

_DS = {
    "name": "t",
    "entities": [
        {"entity_id": "e1", "records": [{"record_id": "r1"}, {"record_id": "r2"}, {"record_id": "r3"}]},
        {"entity_id": "e2", "records": [{"record_id": "r4"}]},
    ],
}


def test_canonical_label_pair_orders_lexically():
    assert canonical_label_pair("b", "a") == ("a", "b")
    assert canonical_label_pair("a", "b") == ("a", "b")


def test_truth_pairs_are_within_cluster_only():
    ds = load_dataset(_DS)
    # e1 has C(3,2)=3 within-cluster pairs; e2 (singleton) has none.
    assert truth_pairs(ds) == frozenset({("r1", "r2"), ("r1", "r3"), ("r2", "r3")})


def test_all_pairs_is_the_full_universe_canonical_and_unique():
    ds = load_dataset(_DS)
    pairs = all_pairs(ds)
    assert len(pairs) == 6  # C(4,2)
    assert len(set(pairs)) == 6
    for low, high in pairs:
        assert low < high


def test_record_to_candidate_matches_a_directly_built_record():
    rec = load_dataset({
        "name": "t",
        "entities": [{"entity_id": "e", "records": [{
            "record_id": "r1",
            "dob": {"value": "1990-05-12", "precision": "day", "provenance_rank": 70},
            "sex_at_birth": {"value": "female", "provenance_rank": 70},
            "names": [{"value": "Alex Nguyen", "provenance_rank": 30}],
            "identifiers": [{"system": "au-medicare", "match_key": "12345", "value": "12345"}],
        }]}],
    }).entities[0].records[0]

    got = record_to_candidate(rec)
    expected = candidate_from_rows(
        dob_row={"value": "1990-05-12", "facets": {"precision": "day"}, "provenance_rank": 70},
        sex_row={"value": "female", "provenance_rank": 70},
        name_rows=[{"value": "Alex Nguyen", "provenance_rank": 30}],
        identifier_rows=[{"system": "au-medicare", "match_key": "12345"}],
    )
    assert got == expected


def test_record_to_candidate_handles_total_absence():
    rec = load_dataset({"name": "t", "entities": [
        {"entity_id": "e", "records": [{"record_id": "r1"}]}]}).entities[0].records[0]
    got = record_to_candidate(rec)
    assert got.dob is None and got.sex_at_birth is None and got.names is None
    assert got.identifiers == {}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_eval_truth.py -q`
Expected: FAIL — `ImportError: cannot import name 'record_to_candidate'`.

- [ ] **Step 3: Write minimal implementation**

Append to `matcher/src/cairn_matcher/eval/dataset.py`:

```python
import itertools

from cairn_matcher.pipeline.adapter import candidate_from_rows
from cairn_matcher.records import CandidateRecord


def record_to_candidate(rec: DatasetRecord) -> CandidateRecord:
    """Map a dataset record to a B1 CandidateRecord via the REAL projection adapter.

    The eval scores the same path production does: the only transform here is reshaping
    the dataset's flat dob dict into the projection's {value, facets:{precision}, ...}
    row shape candidate_from_rows expects. Everything else (DOB precision-gating, name
    token-bagging, identifier keying, safe degrade on absence) is the adapter's, reused
    verbatim so the eval can never drift from the production mapping.
    """
    dob_row = None
    if rec.dob is not None:
        dob_row = {
            "value": rec.dob.get("value"),
            "facets": {"precision": rec.dob.get("precision")},
            "provenance_rank": rec.dob.get("provenance_rank", 0),
        }
    sex_row = None
    if rec.sex_at_birth is not None:
        sex_row = {
            "value": rec.sex_at_birth.get("value"),
            "provenance_rank": rec.sex_at_birth.get("provenance_rank", 0),
        }
    name_rows = [
        {"value": n["value"], "provenance_rank": n.get("provenance_rank", 0)} for n in rec.names
    ]
    identifier_rows = [
        {"system": i["system"], "match_key": i["match_key"]} for i in rec.identifiers
    ]
    return candidate_from_rows(
        dob_row=dob_row, sex_row=sex_row, name_rows=name_rows, identifier_rows=identifier_rows
    )


def canonical_label_pair(a: str, b: str) -> tuple[str, str]:
    """Order two record_id labels (low, high) so a pair has one identity regardless of
    argument order. Lexical order on labels — the blocking layer maps to uuid order and
    reverse-maps back, so the two spaces never need to agree on ordering."""
    return (a, b) if a < b else (b, a)


def truth_pairs(ds: LabelledDataset) -> frozenset[tuple[str, str]]:
    """Every true-match pair: all within-cluster unordered record pairs, canonicalised.

    Cross-cluster pairs are, by construction, the non-matches; we never enumerate them
    here (the universe is all_pairs; non-matches = all_pairs - truth_pairs).
    """
    out: set[tuple[str, str]] = set()
    for ent in ds.entities:
        ids = [r.record_id for r in ent.records]
        for a, b in itertools.combinations(ids, 2):
            out.add(canonical_label_pair(a, b))
    return frozenset(out)


def all_pairs(ds: LabelledDataset) -> list[tuple[str, str]]:
    """The full comparison universe: every unordered record pair, canonicalised."""
    ids = [r.record_id for r in ds.all_records()]
    return [canonical_label_pair(a, b) for a, b in itertools.combinations(ids, 2)]
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_eval_truth.py -q`
Expected: PASS (5 passed).

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/eval/dataset.py matcher/tests/test_eval_truth.py
git commit -m "feat(matcher): eval record->candidate mapping + ground-truth pairs (B3)"
```

---

### Task 3: Scorer metrics

**Files:**
- Create: `matcher/src/cairn_matcher/eval/metrics.py`
- Test: `matcher/tests/test_eval_metrics.py`

**Interfaces:**
- Consumes: `cairn_matcher.pipeline.banding.Band` (psycopg-free import).
- Produces:
  - `PairOutcome(is_match: bool, score_total: float, band: Band | None)` — frozen.
  - `OperatingPoint(name: str, precision: float, recall: float, f1: float)` — frozen.
  - `ScoreStats(count: int, minimum: float, median: float, maximum: float)` — frozen.
  - `Confusion(match_auto, match_review, match_none, nonmatch_auto, nonmatch_review, nonmatch_none: int)` — frozen.
  - `ScorerMetrics(confusion: Confusion, strict: OperatingPoint, lenient: OperatingPoint, auto_false_link_rate: float, missed_match_rate: float, match_scores: ScoreStats, nonmatch_scores: ScoreStats, pair_count: int)` — frozen.
  - `scorer_metrics(outcomes: Sequence[PairOutcome]) -> ScorerMetrics`.

- [ ] **Step 1: Write the failing test**

Create `matcher/tests/test_eval_metrics.py`:

```python
"""Pure tests for the scorer metric math (no scoring, no DB — hand-built outcomes)."""

from cairn_matcher.eval.metrics import PairOutcome, scorer_metrics
from cairn_matcher.pipeline.banding import Band


def _o(is_match, total, band):
    return PairOutcome(is_match=is_match, score_total=total, band=band)


def test_confusion_counts_each_cell():
    outcomes = [
        _o(True, 10.0, Band.AUTO_CANDIDATE),
        _o(True, 5.0, Band.REVIEW),
        _o(True, 1.0, None),
        _o(False, 9.0, Band.AUTO_CANDIDATE),
        _o(False, 4.0, Band.REVIEW),
        _o(False, 0.0, None),
    ]
    m = scorer_metrics(outcomes)
    c = m.confusion
    assert (c.match_auto, c.match_review, c.match_none) == (1, 1, 1)
    assert (c.nonmatch_auto, c.nonmatch_review, c.nonmatch_none) == (1, 1, 1)
    assert m.pair_count == 6


def test_strict_and_lenient_operating_points():
    # 2 true matches: one auto, one review. 1 non-match: auto (a false link).
    outcomes = [
        _o(True, 10.0, Band.AUTO_CANDIDATE),
        _o(True, 5.0, Band.REVIEW),
        _o(False, 9.0, Band.AUTO_CANDIDATE),
    ]
    m = scorer_metrics(outcomes)
    # strict: positive == auto. TP=1, FP=1, FN=1 -> P=0.5, R=0.5
    assert m.strict.precision == 0.5
    assert m.strict.recall == 0.5
    # lenient: positive == auto|review. TP=2, FP=1, FN=0 -> P=2/3, R=1.0
    assert abs(m.lenient.precision - 2 / 3) < 1e-9
    assert m.lenient.recall == 1.0


def test_auto_false_link_and_missed_match_rates():
    outcomes = [
        _o(True, 10.0, Band.AUTO_CANDIDATE),
        _o(True, 1.0, None),                 # a missed true match
        _o(False, 9.0, Band.AUTO_CANDIDATE), # a false auto-link
    ]
    m = scorer_metrics(outcomes)
    assert m.auto_false_link_rate == 0.5      # 1 of 2 auto pairs is a non-match
    assert m.missed_match_rate == 0.5         # 1 of 2 true matches banded None


def test_score_separation_stats_per_class():
    outcomes = [
        _o(True, 10.0, Band.AUTO_CANDIDATE),
        _o(True, 6.0, Band.REVIEW),
        _o(False, 2.0, None),
    ]
    m = scorer_metrics(outcomes)
    assert m.match_scores.count == 2
    assert m.match_scores.minimum == 6.0
    assert m.match_scores.maximum == 10.0
    assert m.match_scores.median == 8.0
    assert m.nonmatch_scores.count == 1


def test_zero_denominators_yield_zero_not_nan():
    # No predicted positives, no true matches: every guarded ratio must be 0.0.
    m = scorer_metrics([PairOutcome(is_match=False, score_total=0.0, band=None)])
    assert m.strict.precision == 0.0
    assert m.strict.recall == 0.0
    assert m.strict.f1 == 0.0
    assert m.auto_false_link_rate == 0.0
    assert m.missed_match_rate == 0.0


def test_empty_outcomes_are_safe():
    m = scorer_metrics([])
    assert m.pair_count == 0
    assert m.match_scores.count == 0
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_eval_metrics.py -q`
Expected: FAIL — `ModuleNotFoundError: No module named 'cairn_matcher.eval.metrics'`.

- [ ] **Step 3: Write minimal implementation**

Create `matcher/src/cairn_matcher/eval/metrics.py`:

```python
"""Turn a list of per-pair outcomes into scorer/banding quality metrics.

Pure: no scoring, no DB. The scorer eval (scorer_eval.py) produces the PairOutcome list
by running the real pipeline over a dataset; this module just counts and divides. It
imports only Band (from the psycopg-free banding module), keeping the metric core pure.

Zero-denominator convention (no NaNs ever): precision is 0.0 when nothing is predicted
positive; recall is 0.0 when there are no true matches; F1 is 0.0 when precision+recall
is 0; each rate is 0.0 when its denominator is 0.
"""

import statistics
from collections.abc import Sequence
from dataclasses import dataclass

from cairn_matcher.pipeline.banding import Band


@dataclass(frozen=True)
class PairOutcome:
    """One evaluated pair: whether it is truly a match, its score, and its band."""

    is_match: bool
    score_total: float
    band: Band | None


@dataclass(frozen=True)
class OperatingPoint:
    """Precision/recall/F1 at one band cut-off (strict = auto; lenient = auto|review)."""

    name: str
    precision: float
    recall: float
    f1: float


@dataclass(frozen=True)
class ScoreStats:
    """Score spread for one truth class — the overlap a weight change must reduce."""

    count: int
    minimum: float
    median: float
    maximum: float


@dataclass(frozen=True)
class Confusion:
    """The 2x3 truth (match/nonmatch) x band (auto/review/none) contingency table."""

    match_auto: int
    match_review: int
    match_none: int
    nonmatch_auto: int
    nonmatch_review: int
    nonmatch_none: int


@dataclass(frozen=True)
class ScorerMetrics:
    """Everything the scorer report shows; all derived purely from the outcome list."""

    confusion: Confusion
    strict: OperatingPoint
    lenient: OperatingPoint
    auto_false_link_rate: float
    missed_match_rate: float
    match_scores: ScoreStats
    nonmatch_scores: ScoreStats
    pair_count: int


def _ratio(numerator: float, denominator: float) -> float:
    """Guarded division: 0.0 when the denominator is 0 (never a NaN/ZeroDivisionError)."""
    return numerator / denominator if denominator else 0.0


def _band_label(band: Band | None) -> str:
    """Collapse a Band (or None) to one of the three confusion column keys."""
    if band is Band.AUTO_CANDIDATE:
        return "auto"
    if band is Band.REVIEW:
        return "review"
    return "none"


def _operating_point(name: str, tp: int, fp: int, fn: int) -> OperatingPoint:
    """Precision/recall/F1 from true/false positives and false negatives, all guarded."""
    precision = _ratio(tp, tp + fp)
    recall = _ratio(tp, tp + fn)
    f1 = _ratio(2 * precision * recall, precision + recall)
    return OperatingPoint(name=name, precision=precision, recall=recall, f1=f1)


def _score_stats(scores: Sequence[float]) -> ScoreStats:
    """min/median/max over one class's scores; all-zero on an empty class (safe)."""
    if not scores:
        return ScoreStats(count=0, minimum=0.0, median=0.0, maximum=0.0)
    return ScoreStats(
        count=len(scores),
        minimum=min(scores),
        median=statistics.median(scores),
        maximum=max(scores),
    )


def scorer_metrics(outcomes: Sequence[PairOutcome]) -> ScorerMetrics:
    """Aggregate per-pair outcomes into the full scorer metric bundle.

    Two operating points are reported because the matcher is two-tiered: 'strict' counts
    only AUTO_CANDIDATE as a predicted link (the aggressive end), 'lenient' also counts
    REVIEW (a human will look). auto_false_link_rate is the dangerous one — the fraction
    of auto-banded pairs that are actually non-matches; it should be ~0 for a sane config.
    """
    cells = {(m, lbl): 0 for m in (True, False) for lbl in ("auto", "review", "none")}
    match_scores: list[float] = []
    nonmatch_scores: list[float] = []
    for o in outcomes:
        cells[(o.is_match, _band_label(o.band))] += 1
        (match_scores if o.is_match else nonmatch_scores).append(o.score_total)

    confusion = Confusion(
        match_auto=cells[(True, "auto")],
        match_review=cells[(True, "review")],
        match_none=cells[(True, "none")],
        nonmatch_auto=cells[(False, "auto")],
        nonmatch_review=cells[(False, "review")],
        nonmatch_none=cells[(False, "none")],
    )

    strict = _operating_point(
        "strict",
        tp=confusion.match_auto,
        fp=confusion.nonmatch_auto,
        fn=confusion.match_review + confusion.match_none,
    )
    lenient = _operating_point(
        "lenient",
        tp=confusion.match_auto + confusion.match_review,
        fp=confusion.nonmatch_auto + confusion.nonmatch_review,
        fn=confusion.match_none,
    )

    total_auto = confusion.match_auto + confusion.nonmatch_auto
    total_true = confusion.match_auto + confusion.match_review + confusion.match_none

    return ScorerMetrics(
        confusion=confusion,
        strict=strict,
        lenient=lenient,
        auto_false_link_rate=_ratio(confusion.nonmatch_auto, total_auto),
        missed_match_rate=_ratio(confusion.match_none, total_true),
        match_scores=_score_stats(match_scores),
        nonmatch_scores=_score_stats(nonmatch_scores),
        pair_count=len(outcomes),
    )
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_eval_metrics.py -q`
Expected: PASS (6 passed).

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/eval/metrics.py matcher/tests/test_eval_metrics.py
git commit -m "feat(matcher): eval scorer metrics (confusion, P/R/F1, rates) (B3)"
```

---

### Task 4: Scorer evaluation driver

**Files:**
- Create: `matcher/src/cairn_matcher/eval/scorer_eval.py`
- Test: `matcher/tests/test_eval_scorer_driver.py`

**Interfaces:**
- Consumes: `load_dataset`, `record_to_candidate`, `truth_pairs`, `all_pairs` (Tasks 1–2); `field_comparisons`/`DEFAULT_CONFIG` (orchestrator); `score`/`DEFAULT_WEIGHTS` (scoring); `band`/`DEFAULT_THRESHOLDS` (banding); `PairOutcome`/`scorer_metrics`/`ScorerMetrics` (Task 3).
- Produces: `evaluate_scorer(ds: LabelledDataset, *, weights: Weights = DEFAULT_WEIGHTS, thresholds: Thresholds = DEFAULT_THRESHOLDS, config: ComparatorConfig = DEFAULT_CONFIG) -> ScorerMetrics`.

- [ ] **Step 1: Write the failing test**

Create `matcher/tests/test_eval_scorer_driver.py`:

```python
"""Pure end-to-end test of evaluate_scorer over a tiny inline dataset."""

from cairn_matcher.eval.dataset import load_dataset
from cairn_matcher.eval.scorer_eval import evaluate_scorer

# Two records of the SAME person sharing a strong identifier and an exact high-rank DOB
# (-> AUTO), plus a third unrelated person sharing nothing (-> the non-match pairs).
_DS = load_dataset({
    "name": "driver",
    "entities": [
        {"entity_id": "p", "records": [
            {"record_id": "p-1",
             "dob": {"value": "1990-05-12", "precision": "day", "provenance_rank": 70},
             "identifiers": [{"system": "mrn", "match_key": "K1", "value": "K1"}]},
            {"record_id": "p-2",
             "dob": {"value": "1990-05-12", "precision": "day", "provenance_rank": 70},
             "identifiers": [{"system": "mrn", "match_key": "K1", "value": "K1"}]},
        ]},
        {"entity_id": "q", "records": [
            {"record_id": "q-1",
             "dob": {"value": "1970-01-01", "precision": "day", "provenance_rank": 70},
             "identifiers": [{"system": "mrn", "match_key": "K9", "value": "K9"}]},
        ]},
    ],
})


def test_evaluate_scorer_counts_all_pairs_and_finds_the_match():
    m = evaluate_scorer(_DS)
    assert m.pair_count == 3  # C(3,2): one true match (p-1,p-2) + two non-matches
    # The strong same-person pair is auto-banded; no non-match reaches auto.
    assert m.confusion.match_auto == 1
    assert m.auto_false_link_rate == 0.0


def test_evaluate_scorer_respects_a_custom_threshold():
    # With an absurdly high auto threshold nothing is auto-banded.
    from cairn_matcher.pipeline.banding import Thresholds
    m = evaluate_scorer(_DS, thresholds=Thresholds(review=3.0, auto=999.0))
    assert m.confusion.match_auto == 0
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_eval_scorer_driver.py -q`
Expected: FAIL — `ModuleNotFoundError: No module named 'cairn_matcher.eval.scorer_eval'`.

- [ ] **Step 3: Write minimal implementation**

Create `matcher/src/cairn_matcher/eval/scorer_eval.py`:

```python
"""Run the real scoring+banding pipeline over a labelled dataset -> ScorerMetrics.

Pure: it reuses the production scoring path (orchestrator -> scoring -> banding), so the
metrics describe the real matcher, not a stand-in. weights/thresholds/config are
PARAMETERS — sweeping them is exactly how weight-learning will use this harness.

Caveat (documented in the spec): banding is called with NO vetoes here. The pure eval
measures scorer+threshold quality in isolation; the in-DB veto can cap a high score at
REVIEW, so these metrics are slightly optimistic vs the end-to-end banded outcome. A
veto-aware mode is a later, additive extension.

Complexity is O(N^2) in records (every pair scored). Fine for the small gold set; that
O(N^2) is precisely what the blocking layer measures how to avoid at scale.
"""

from cairn_matcher.eval.dataset import (
    LabelledDataset,
    all_pairs,
    record_to_candidate,
    truth_pairs,
)
from cairn_matcher.eval.metrics import PairOutcome, ScorerMetrics, scorer_metrics
from cairn_matcher.orchestrator import DEFAULT_CONFIG, ComparatorConfig, field_comparisons
from cairn_matcher.pipeline.banding import DEFAULT_THRESHOLDS, Thresholds, band
from cairn_matcher.scoring import DEFAULT_WEIGHTS, Weights, score


def evaluate_scorer(
    ds: LabelledDataset,
    *,
    weights: Weights = DEFAULT_WEIGHTS,
    thresholds: Thresholds = DEFAULT_THRESHOLDS,
    config: ComparatorConfig = DEFAULT_CONFIG,
) -> ScorerMetrics:
    """Score every record pair, band it, and aggregate against ground truth.

    Candidates are built once per record (not once per pair) so the O(N^2) loop does only
    the comparison work, not repeated adapter work.
    """
    candidates = {r.record_id: record_to_candidate(r) for r in ds.all_records()}
    truth = truth_pairs(ds)

    outcomes: list[PairOutcome] = []
    for low, high in all_pairs(ds):
        comparisons = field_comparisons(candidates[low], candidates[high], config)
        match_score = score(comparisons, weights)
        outcomes.append(
            PairOutcome(
                is_match=(low, high) in truth,
                score_total=match_score.total,
                band=band(match_score, (), thresholds),
            )
        )
    return scorer_metrics(outcomes)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_eval_scorer_driver.py -q`
Expected: PASS (2 passed).

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/eval/scorer_eval.py matcher/tests/test_eval_scorer_driver.py
git commit -m "feat(matcher): eval scorer-evaluation driver (B3)"
```

---

### Task 5: Gold fixture + scorer eval over it

**Files:**
- Create: `matcher/src/cairn_matcher/eval/fixtures/gold_v1.json`
- Create: `matcher/src/cairn_matcher/eval/loader.py`
- Test: `matcher/tests/test_eval_gold.py`

**Interfaces:**
- Consumes: `load_dataset`, `evaluate_scorer`, `record_to_candidate`, `field_comparisons`, `score`, `band` (earlier tasks).
- Produces:
  - `load_bundled_gold() -> LabelledDataset` (in `loader.py`) — reads the packaged `fixtures/gold_v1.json`.
  - `GOLD_PATH` (a `pathlib.Path` to the bundled fixture) in `loader.py`.

**Note on packaging:** hatchling includes non-`.py` files under the package directory by default, so `fixtures/gold_v1.json` ships in the wheel with no extra config. Tests and the CLI read it from disk via `GOLD_PATH`.

- [ ] **Step 1: Write the failing test**

Create `matcher/tests/test_eval_gold.py`:

```python
"""The gold fixture is the regression gate: specific, robust band assertions.

These metrics are a regression/tuning instrument, NOT a statistical accuracy claim — the
set is tiny and hand-authored. Assertions are chosen to be robust to comparator nuance:
- alex-1/alex-2 reach AUTO via shared identifier (~4.0) + exact high-rank DOB (6.0) alone.
- garcia-1/smith-1 share nothing comparable -> NONE.
- rev-a/rev-b (different people) share only an exact DOB + agreeing sex -> 7.0 -> REVIEW,
  demonstrating 'weak coincidence is reviewed, never auto-linked'.
- No cross-entity pair reaches AUTO -> auto_false_link_rate == 0.
"""

from cairn_matcher.eval.dataset import record_to_candidate
from cairn_matcher.eval.loader import load_bundled_gold
from cairn_matcher.eval.scorer_eval import evaluate_scorer
from cairn_matcher.orchestrator import DEFAULT_CONFIG, field_comparisons
from cairn_matcher.pipeline.banding import DEFAULT_THRESHOLDS, Band, band
from cairn_matcher.scoring import score


def _band_of(ds, id_a, id_b):
    recs = {r.record_id: r for r in ds.all_records()}
    cmp = field_comparisons(
        record_to_candidate(recs[id_a]), record_to_candidate(recs[id_b]), DEFAULT_CONFIG
    )
    return band(score(cmp), (), DEFAULT_THRESHOLDS)


def test_gold_loads():
    ds = load_bundled_gold()
    assert ds.name == "gold_v1"
    assert len(ds.all_records()) == 10


def test_strong_duplicate_is_auto():
    assert _band_of(load_bundled_gold(), "alex-1", "alex-2") is Band.AUTO_CANDIDATE


def test_unrelated_people_band_to_none():
    assert _band_of(load_bundled_gold(), "garcia-1", "smith-1") is None


def test_weak_coincidence_is_review_never_auto():
    assert _band_of(load_bundled_gold(), "rev-a", "rev-b") is Band.REVIEW


def test_no_non_match_is_auto_linked():
    m = evaluate_scorer(load_bundled_gold())
    assert m.auto_false_link_rate == 0.0
    assert m.confusion.nonmatch_auto == 0
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_eval_gold.py -q`
Expected: FAIL — `ModuleNotFoundError: No module named 'cairn_matcher.eval.loader'`.

- [ ] **Step 3: Write minimal implementation**

Create `matcher/src/cairn_matcher/eval/fixtures/gold_v1.json`:

```json
{
  "name": "gold_v1",
  "description": "Hand-authored culture-plural gold set for the matcher eval harness. Tiny: a regression/tuning instrument, NOT a statistical accuracy claim. Spans mononym, patronymic, multi-token, and transliteration name shapes.",
  "entities": [
    {
      "entity_id": "alex",
      "records": [
        {
          "record_id": "alex-1",
          "dob": {"value": "1990-05-12", "precision": "day", "provenance_rank": 70},
          "sex_at_birth": {"value": "female", "provenance_rank": 70},
          "names": [{"value": "Alex Nguyen", "provenance_rank": 30}],
          "identifiers": [{"system": "au-medicare", "match_key": "12345", "value": "12345"}]
        },
        {
          "record_id": "alex-2",
          "dob": {"value": "1990-05-12", "precision": "day", "provenance_rank": 70},
          "names": [{"value": "Nguyen Van Alex", "provenance_rank": 20}],
          "identifiers": [{"system": "au-medicare", "match_key": "12345", "value": "12345"}]
        }
      ]
    },
    {
      "entity_id": "suharto",
      "records": [
        {
          "record_id": "mono-1",
          "dob": {"value": "1955-06-08", "precision": "day", "provenance_rank": 40},
          "sex_at_birth": {"value": "male", "provenance_rank": 40},
          "names": [{"value": "Suharto", "provenance_rank": 30}]
        },
        {
          "record_id": "mono-2",
          "dob": {"value": "1955-06-08", "precision": "day", "provenance_rank": 20},
          "names": [{"value": "Suharto", "provenance_rank": 20}],
          "identifiers": [{"system": "national-id", "match_key": "ID-7", "value": "ID-7"}]
        }
      ]
    },
    {
      "entity_id": "einarsson",
      "records": [
        {
          "record_id": "pat-1",
          "dob": {"value": "1978-03-03", "precision": "day", "provenance_rank": 40},
          "names": [{"value": "Jon Einarsson", "provenance_rank": 30}],
          "identifiers": [{"system": "kennitala", "match_key": "070378", "value": "070378"}]
        },
        {
          "record_id": "pat-2",
          "dob": {"value": "1978-03-03", "precision": "day", "provenance_rank": 20},
          "names": [{"value": "Jón Einarsson", "provenance_rank": 20}],
          "identifiers": [{"system": "kennitala", "match_key": "070378", "value": "070378"}]
        }
      ]
    },
    {
      "entity_id": "garcia",
      "records": [
        {
          "record_id": "garcia-1",
          "dob": {"value": "2001-11-30", "precision": "day", "provenance_rank": 70},
          "sex_at_birth": {"value": "female", "provenance_rank": 70},
          "names": [{"value": "Maria Garcia", "provenance_rank": 30}],
          "identifiers": [{"system": "au-medicare", "match_key": "99999", "value": "99999"}]
        }
      ]
    },
    {
      "entity_id": "smith",
      "records": [
        {
          "record_id": "smith-1",
          "dob": {"value": "1965-01-01", "precision": "day", "provenance_rank": 40},
          "sex_at_birth": {"value": "male", "provenance_rank": 40},
          "names": [{"value": "Robin Smith", "provenance_rank": 30}]
        }
      ]
    },
    {
      "entity_id": "coincidence-a",
      "records": [
        {
          "record_id": "rev-a",
          "dob": {"value": "2010-10-10", "precision": "day", "provenance_rank": 70},
          "sex_at_birth": {"value": "female", "provenance_rank": 70}
        }
      ]
    },
    {
      "entity_id": "coincidence-b",
      "records": [
        {
          "record_id": "rev-b",
          "dob": {"value": "2010-10-10", "precision": "day", "provenance_rank": 70},
          "sex_at_birth": {"value": "female", "provenance_rank": 70}
        }
      ]
    }
  ]
}
```

Create `matcher/src/cairn_matcher/eval/loader.py`:

```python
"""Locate and load the bundled gold dataset shipped inside the package.

Kept separate from dataset.py so the pure value types carry no filesystem dependency:
dataset.load_dataset takes an already-decoded mapping; this module is the thin I/O edge
that reads JSON from disk.
"""

import json
from pathlib import Path

from cairn_matcher.eval.dataset import LabelledDataset, load_dataset

GOLD_PATH = Path(__file__).resolve().parent / "fixtures" / "gold_v1.json"


def load_dataset_file(path: Path | str) -> LabelledDataset:
    """Read a dataset JSON file from disk and parse it into a LabelledDataset."""
    with open(path, encoding="utf-8") as fh:
        return load_dataset(json.load(fh))


def load_bundled_gold() -> LabelledDataset:
    """Load the package's bundled gold_v1 dataset (the default CLI target)."""
    return load_dataset_file(GOLD_PATH)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_eval_gold.py -q`
Expected: PASS (5 passed).

If `test_strong_duplicate_is_auto` or `test_weak_coincidence_is_review_never_auto` fails because a pair lands in a neighbouring band, the **fixture** is the free variable, not the assertion: adjust that record's `provenance_rank`/`precision` (never the asserted intent) until the intended band is realised under the shipped `DEFAULT_WEIGHTS`/`DEFAULT_THRESHOLDS`. Re-run.

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/eval/fixtures/gold_v1.json matcher/src/cairn_matcher/eval/loader.py matcher/tests/test_eval_gold.py
git commit -m "feat(matcher): gold_v1 fixture + bundled loader + scorer regression gate (B3)"
```

---

### Task 6: Report formatting

**Files:**
- Create: `matcher/src/cairn_matcher/eval/report.py`
- Modify: `matcher/src/cairn_matcher/eval/__init__.py` (add public exports)
- Test: `matcher/tests/test_eval_report.py`

**Interfaces:**
- Consumes: `ScorerMetrics` (Task 3); `BlockingMetrics` is referenced only by name in `format_blocking` (defined in Task 8) — `format_blocking` takes the object and reads documented attributes, so it does not import the class.
- Produces:
  - `format_scorer(metrics: ScorerMetrics, *, dataset_name: str = "") -> str`
  - `format_blocking(metrics) -> str` (duck-typed on the Task 8 `BlockingMetrics` fields).

- [ ] **Step 1: Write the failing test**

Create `matcher/tests/test_eval_report.py`:

```python
"""Pure tests for the plain-text report formatter."""

from cairn_matcher.eval.report import format_scorer
from cairn_matcher.eval.scorer_eval import evaluate_scorer
from cairn_matcher.eval.loader import load_bundled_gold


def test_scorer_report_mentions_key_metrics_and_the_caveat():
    text = format_scorer(evaluate_scorer(load_bundled_gold()), dataset_name="gold_v1")
    assert "gold_v1" in text
    assert "auto_false_link_rate" in text
    assert "precision" in text
    # The honest caveat must be in the printed report, not just the docs.
    assert "regression" in text.lower() or "not a statistical" in text.lower()


def test_scorer_report_is_a_single_string():
    text = format_scorer(evaluate_scorer(load_bundled_gold()))
    assert isinstance(text, str)
    assert text.strip()
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_eval_report.py -q`
Expected: FAIL — `ModuleNotFoundError: No module named 'cairn_matcher.eval.report'`.

- [ ] **Step 3: Write minimal implementation**

Create `matcher/src/cairn_matcher/eval/report.py`:

```python
"""Render metric bundles to a plain-text report.

Pure string formatting — no scoring, no DB. Kept separate from the metric computation so
the numbers can be consumed programmatically (weight-learning) without the prose, and the
prose can change without touching the math.
"""

from cairn_matcher.eval.metrics import OperatingPoint, ScorerMetrics

_CAVEAT = (
    "NOTE: on a small hand-authored set these numbers are a regression/tuning "
    "instrument, not a statistical accuracy claim."
)


def _op_line(op: OperatingPoint) -> str:
    """One operating-point row: precision / recall / F1 to three decimals."""
    return (f"  {op.name:<8} precision={op.precision:.3f} "
            f"recall={op.recall:.3f} f1={op.f1:.3f}")


def format_scorer(metrics: ScorerMetrics, *, dataset_name: str = "") -> str:
    """Render scorer metrics: confusion, both operating points, the danger rates, spread."""
    c = metrics.confusion
    title = f"Scorer eval — {dataset_name}" if dataset_name else "Scorer eval"
    lines = [
        title,
        f"  pairs evaluated: {metrics.pair_count}",
        "  confusion (truth x band):",
        f"    match    : auto={c.match_auto} review={c.match_review} none={c.match_none}",
        f"    non-match: auto={c.nonmatch_auto} review={c.nonmatch_review} none={c.nonmatch_none}",
        _op_line(metrics.strict),
        _op_line(metrics.lenient),
        f"  auto_false_link_rate={metrics.auto_false_link_rate:.3f}  "
        f"missed_match_rate={metrics.missed_match_rate:.3f}",
        f"  match scores    : n={metrics.match_scores.count} "
        f"min={metrics.match_scores.minimum:.2f} med={metrics.match_scores.median:.2f} "
        f"max={metrics.match_scores.maximum:.2f}",
        f"  non-match scores: n={metrics.nonmatch_scores.count} "
        f"min={metrics.nonmatch_scores.minimum:.2f} med={metrics.nonmatch_scores.median:.2f} "
        f"max={metrics.nonmatch_scores.maximum:.2f}",
        _CAVEAT,
    ]
    return "\n".join(lines)


def format_blocking(metrics) -> str:
    """Render blocking metrics. Duck-typed on the BlockingMetrics fields (Task 8) so this
    pure module never imports the psycopg-adjacent blocking layer."""
    lines = [
        "Blocking eval",
        f"  pair_completeness={metrics.pair_completeness:.3f} (blocking recall ceiling)",
        f"  reduction_ratio={metrics.reduction_ratio:.3f}",
        f"  generated_pairs={metrics.generated_pairs} of {metrics.total_pairs} possible",
        f"  skipped oversized blocks: {len(metrics.skipped_blocks)} "
        f"(dropped_pair_estimate={metrics.dropped_pair_estimate})",
        f"  dropped TRUE matches (blocking misses): {len(metrics.dropped_true_matches)}",
    ]
    for low, high in metrics.dropped_true_matches:
        lines.append(f"    - {low} / {high}")
    return "\n".join(lines)
```

Modify `matcher/src/cairn_matcher/eval/__init__.py` — append after the module docstring:

```python
from cairn_matcher.eval.dataset import LabelledDataset, load_dataset
from cairn_matcher.eval.loader import load_bundled_gold, load_dataset_file
from cairn_matcher.eval.metrics import ScorerMetrics, scorer_metrics
from cairn_matcher.eval.report import format_scorer
from cairn_matcher.eval.scorer_eval import evaluate_scorer

__all__ = [
    "LabelledDataset",
    "load_dataset",
    "load_dataset_file",
    "load_bundled_gold",
    "ScorerMetrics",
    "scorer_metrics",
    "evaluate_scorer",
    "format_scorer",
]
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_eval_report.py -q`
Expected: PASS (2 passed).

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/eval/report.py matcher/src/cairn_matcher/eval/__init__.py matcher/tests/test_eval_report.py
git commit -m "feat(matcher): eval report formatter + package exports (B3)"
```

---

### Task 7: CLI runner

**Files:**
- Create: `matcher/src/cairn_matcher/eval/__main__.py`
- Test: `matcher/tests/test_eval_cli.py`

**Interfaces:**
- Consumes: `load_bundled_gold`/`load_dataset_file` (Task 5), `evaluate_scorer` (Task 4), `format_scorer` (Task 6).
- Produces: `main(argv: list[str] | None = None) -> int`. Blocking eval is imported LAZILY inside `main` and only when `CAIRN_TEST_PG` is set, so a pure invocation never imports psycopg.

- [ ] **Step 1: Write the failing test**

Create `matcher/tests/test_eval_cli.py`:

```python
"""Tests for the eval CLI. Run via main() in-process (no subprocess needed)."""

import json

from cairn_matcher.eval.__main__ import main


def test_cli_runs_bundled_gold_and_prints_scorer_report(capsys, monkeypatch):
    monkeypatch.delenv("CAIRN_TEST_PG", raising=False)  # force the pure path
    rc = main([])
    out = capsys.readouterr().out
    assert rc == 0
    assert "Scorer eval" in out
    assert "auto_false_link_rate" in out


def test_cli_runs_a_named_dataset_file(tmp_path, capsys, monkeypatch):
    monkeypatch.delenv("CAIRN_TEST_PG", raising=False)
    ds = {"name": "mini", "entities": [
        {"entity_id": "e", "records": [{"record_id": "r1"}, {"record_id": "r2"}]}]}
    p = tmp_path / "mini.json"
    p.write_text(json.dumps(ds), encoding="utf-8")
    rc = main([str(p)])
    out = capsys.readouterr().out
    assert rc == 0
    assert "mini" in out


def test_cli_reports_a_bad_dataset_with_nonzero_exit(tmp_path, capsys, monkeypatch):
    monkeypatch.delenv("CAIRN_TEST_PG", raising=False)
    p = tmp_path / "bad.json"
    p.write_text('{"name": "x"}', encoding="utf-8")  # no 'entities'
    rc = main([str(p)])
    assert rc != 0
    assert "error" in capsys.readouterr().err.lower()
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_eval_cli.py -q`
Expected: FAIL — `ModuleNotFoundError: No module named 'cairn_matcher.eval.__main__'`.

- [ ] **Step 3: Write minimal implementation**

Create `matcher/src/cairn_matcher/eval/__main__.py`:

```python
"""`python -m cairn_matcher.eval [dataset.json]` — print the matcher eval report.

Runs the pure scorer eval always. If CAIRN_TEST_PG is set, ALSO runs the DB-gated
blocking eval (imported lazily so a pure run never needs psycopg) and appends its report.
"""

import argparse
import os
import sys

from cairn_matcher.eval.dataset import DatasetError
from cairn_matcher.eval.loader import load_bundled_gold, load_dataset_file
from cairn_matcher.eval.report import format_scorer
from cairn_matcher.eval.scorer_eval import evaluate_scorer


def main(argv: list[str] | None = None) -> int:
    """Parse args, run the eval(s), print the report. Returns a process exit code."""
    parser = argparse.ArgumentParser(prog="cairn_matcher.eval", description=__doc__)
    parser.add_argument(
        "dataset", nargs="?",
        help="path to a dataset JSON file; default: the bundled gold_v1 set",
    )
    parser.add_argument(
        "--max-block-size", type=int, default=100,
        help="blocking cap (only used when CAIRN_TEST_PG is set)",
    )
    args = parser.parse_args(argv)

    try:
        ds = load_dataset_file(args.dataset) if args.dataset else load_bundled_gold()
    except (DatasetError, OSError, ValueError) as exc:
        print(f"error: could not load dataset: {exc}", file=sys.stderr)
        return 2

    print(format_scorer(evaluate_scorer(ds), dataset_name=ds.name))

    dsn = os.environ.get("CAIRN_TEST_PG")
    if dsn:
        # Lazy import: psycopg + the blocking layer are only touched when a DB is offered.
        import psycopg

        from cairn_matcher.eval.blocking_eval import evaluate_blocking
        from cairn_matcher.eval.report import format_blocking

        with psycopg.connect(dsn, autocommit=False) as conn:
            metrics = evaluate_blocking(conn, ds, max_block_size=args.max_block_size)
        print()
        print(format_blocking(metrics))

    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_eval_cli.py -q`
Expected: PASS (3 passed). (These tests unset `CAIRN_TEST_PG`, so the lazy DB branch is not taken.)

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/eval/__main__.py matcher/tests/test_eval_cli.py
git commit -m "feat(matcher): eval CLI runner (B3)"
```

---

### Task 8: DB-gated blocking eval + README

**Files:**
- Create: `matcher/src/cairn_matcher/eval/blocking_eval.py`
- Modify: `matcher/tests/conftest.py` (add `match_proposal` is already truncated; no change needed — verify only)
- Test: `matcher/tests/test_eval_blocking.py`
- Modify: `matcher/README.md` (add an "Eval harness" section)

**Interfaces:**
- Consumes: `LabelledDataset`, `truth_pairs`, `canonical_label_pair`, `all_pairs` (dataset); `cairn_matcher.pipeline.db.generate_candidate_pairs` (lazy import inside `evaluate_blocking`); psycopg connection from the caller.
- Produces:
  - `BlockingMetrics(pair_completeness: float, reduction_ratio: float, generated_pairs: int, total_pairs: int, skipped_blocks: tuple[tuple[str, str, int], ...], dropped_pair_estimate: int, dropped_true_matches: tuple[tuple[str, str], ...])` — frozen.
  - `record_uuid(label: str) -> str` (deterministic uuid5 of the label).
  - `seed_dataset(conn, ds: LabelledDataset) -> dict[str, str]` (returns uuid→label reverse map; commits).
  - `evaluate_blocking(conn, ds: LabelledDataset, *, max_block_size: int = 100) -> BlockingMetrics`.

- [ ] **Step 1: Write the failing test**

Create `matcher/tests/test_eval_blocking.py`:

```python
"""DB-gated tests for the blocking eval (pair-completeness / reduction-ratio).

Gated on CAIRN_TEST_PG via the shared pg_conn fixture (skipped cleanly without a DB).
"""

from cairn_matcher.eval.blocking_eval import evaluate_blocking
from cairn_matcher.eval.dataset import load_dataset
from cairn_matcher.eval.loader import load_bundled_gold


def test_gold_blocking_recall_is_total(pg_conn):
    # Every true-match pair in gold_v1 shares an identifier or a name token AND a DOB,
    # so blocking must generate all of them: pair_completeness == 1.0, no dropped matches.
    m = evaluate_blocking(pg_conn, load_bundled_gold())
    assert m.pair_completeness == 1.0
    assert m.dropped_true_matches == ()
    assert m.reduction_ratio > 0.0  # blocking generated fewer than all C(10,2)=45 pairs


def test_oversized_block_is_skipped_and_estimated(pg_conn):
    # Three records sharing one DOB; cap=2 -> that block (size 3) is skipped, dropping
    # C(3,2)=3 candidate pairs, reported via dropped_pair_estimate.
    ds = load_dataset({"name": "big", "entities": [
        {"entity_id": "e", "records": [
            {"record_id": f"r{i}",
             "dob": {"value": "2000-01-01", "precision": "day", "provenance_rank": 40}}
            for i in range(3)
        ]},
    ]})
    m = evaluate_blocking(pg_conn, ds, max_block_size=2)
    assert any(pn == "dob" and sz == 3 for pn, _key, sz in m.skipped_blocks)
    assert m.dropped_pair_estimate == 3
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" uv run --extra pipeline pytest tests/test_eval_blocking.py -q`
Expected: FAIL — `ModuleNotFoundError: No module named 'cairn_matcher.eval.blocking_eval'`.
(Without `CAIRN_TEST_PG` the tests SKIP — confirm with `cd matcher && uv run pytest tests/test_eval_blocking.py -q` showing `2 skipped`.)

- [ ] **Step 3: Write minimal implementation**

Create `matcher/src/cairn_matcher/eval/blocking_eval.py`:

```python
"""DB-gated blocking-recall eval: how well candidate generation covers true matches.

The one DB-touching eval module (needs the optional `pipeline` extra, psycopg). It seeds
a dataset into the patient_* projections, calls the REAL generate_candidate_pairs, and
measures pair-completeness (blocking recall) and reduction-ratio against ground truth.
No parallel blocking implementation — the SQL stays the source of truth.

Dataset record_ids are readable labels; the projection key is a uuid. We derive a stable
uuid5 per label (deterministic, so a re-run is reproducible) and reverse-map the
generated uuid pairs back to labels to compare against the label-space ground truth.
"""

import uuid
from dataclasses import dataclass

from cairn_matcher.eval.dataset import (
    LabelledDataset,
    all_pairs,
    canonical_label_pair,
    truth_pairs,
)

# A fixed namespace so label -> uuid is stable across runs (reproducible eval seeding).
_LABEL_NS = uuid.UUID("6f9b4c2e-1d3a-4e5f-8a7b-0c1d2e3f4a5b")


@dataclass(frozen=True)
class BlockingMetrics:
    """Blocking-recall metrics for one dataset under one blocking cap."""

    pair_completeness: float          # |generated & true| / |true|  (the recall ceiling)
    reduction_ratio: float            # 1 - |generated| / |all pairs|
    generated_pairs: int
    total_pairs: int
    skipped_blocks: tuple[tuple[str, str, int], ...]  # (pass_name, key, size) over cap
    dropped_pair_estimate: int        # sum of C(size,2) over skipped blocks
    dropped_true_matches: tuple[tuple[str, str], ...]  # true matches blocking missed


def record_uuid(label: str) -> str:
    """Deterministic uuid (text) for a record label — stable across runs."""
    return str(uuid.uuid5(_LABEL_NS, label))


def seed_dataset(conn, ds: LabelledDataset) -> dict[str, str]:
    """Insert every dataset record into the patient_* projections; commit.

    Mirrors tests/conftest.seed_patient but reads the dataset's dict fields. Returns the
    uuid->label reverse map the caller uses to translate generated pairs back to labels.
    """
    reverse: dict[str, str] = {}
    with conn.cursor() as cur:
        for rec in ds.all_records():
            pid = record_uuid(rec.record_id)
            reverse[pid] = rec.record_id
            if rec.dob is not None:
                import json
                cur.execute(
                    "INSERT INTO patient_demographic (patient_id, field, value, facets, "
                    "provenance, provenance_rank, asserted_hlc_wall, asserted_hlc_count, "
                    "asserted_origin) VALUES (%s,'dob',%s,%s,'seed',%s,0,0,'seed')",
                    (pid, rec.dob.get("value"),
                     json.dumps({"precision": rec.dob.get("precision")}),
                     rec.dob.get("provenance_rank", 0)),
                )
            if rec.sex_at_birth is not None:
                cur.execute(
                    "INSERT INTO patient_demographic (patient_id, field, value, facets, "
                    "provenance, provenance_rank, asserted_hlc_wall, asserted_hlc_count, "
                    "asserted_origin) VALUES (%s,'sex-at-birth',%s,NULL,'seed',%s,0,0,'seed')",
                    (pid, rec.sex_at_birth.get("value"),
                     rec.sex_at_birth.get("provenance_rank", 0)),
                )
            for n in rec.names:
                cur.execute(
                    "INSERT INTO patient_name (patient_id, use_key, value, use_raw, "
                    "provenance, provenance_rank, last_hlc_wall, last_hlc_count, "
                    "asserted_origin) VALUES (%s,'legal',%s,'legal','seed',%s,0,0,'seed') "
                    "ON CONFLICT DO NOTHING",
                    (pid, n["value"], n.get("provenance_rank", 0)),
                )
            for i in rec.identifiers:
                cur.execute(
                    "INSERT INTO patient_identifier (patient_id, system, match_key, value, "
                    "normalized, profile, use_type, provenance, asserted_hlc_wall, "
                    "asserted_hlc_count, asserted_origin) VALUES "
                    "(%s,%s,%s,%s,%s,NULL,NULL,'seed',0,0,'seed') ON CONFLICT DO NOTHING",
                    (pid, i["system"], i["match_key"], i["value"], i["match_key"]),
                )
    conn.commit()
    return reverse


def evaluate_blocking(conn, ds: LabelledDataset, *, max_block_size: int = 100) -> BlockingMetrics:
    """Seed the dataset, run the real blocking, and measure recall/reduction.

    Calls generate_candidate_pairs (lazy import: keeps the module importable without the
    function name leaking into the pure path) then rolls back the read snapshot, mirroring
    the sweep's xmin-horizon discipline.
    """
    from cairn_matcher.pipeline.db import generate_candidate_pairs

    reverse = seed_dataset(conn, ds)
    uuid_pairs, skipped = generate_candidate_pairs(conn, max_block_size=max_block_size)
    conn.rollback()

    generated = {
        canonical_label_pair(reverse[low], reverse[high]) for low, high in uuid_pairs
    }
    truth = truth_pairs(ds)
    total = len(all_pairs(ds))

    dropped_true = tuple(sorted(truth - generated))
    return BlockingMetrics(
        pair_completeness=(len(generated & truth) / len(truth)) if truth else 0.0,
        reduction_ratio=(1.0 - len(generated) / total) if total else 0.0,
        generated_pairs=len(generated),
        total_pairs=total,
        skipped_blocks=tuple(skipped),
        dropped_pair_estimate=sum(s * (s - 1) // 2 for _pn, _key, s in skipped),
        dropped_true_matches=dropped_true,
    )
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" uv run --extra pipeline pytest tests/test_eval_blocking.py -q`
Expected: PASS (2 passed).
Also confirm the no-DB skip: `cd matcher && uv run pytest tests/test_eval_blocking.py -q` → `2 skipped`.

- [ ] **Step 5: Add the README section + commit**

Append to `matcher/README.md` an "Eval harness" section:

```markdown
## Eval harness (B3)

Measure the matcher against a labelled dataset (entity clusters = ground truth):

```bash
# scorer/banding metrics over the bundled gold set (pure, no DB):
uv run python -m cairn_matcher.eval

# a named dataset:
uv run python -m cairn_matcher.eval path/to/dataset.json

# also run blocking-recall metrics (needs a DB + the pipeline extra):
CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" \
  uv run --extra pipeline python -m cairn_matcher.eval
```

Scorer metrics (precision/recall/F1 at strict and lenient bands, auto-false-link rate,
missed-match rate, score separation) are the lever for **weight-learning**; blocking
metrics (pair-completeness, reduction-ratio, dropped-true-matches) are the lever for
**compound blocking keys**. On a small hand-authored set these are a regression/tuning
instrument, not a statistical accuracy claim. Dataset format: see
`src/cairn_matcher/eval/fixtures/gold_v1.json`.
```

```bash
git add matcher/src/cairn_matcher/eval/blocking_eval.py matcher/tests/test_eval_blocking.py matcher/README.md
git commit -m "feat(matcher): DB-gated blocking-recall eval + README (B3)"
```

---

### Task 9: Full-suite green + whole-branch review prep

**Files:** none (verification + docs).

- [ ] **Step 1: Run the full pure suite**

Run: `cd matcher && uv run pytest -q`
Expected: all pure tests pass; the blocking tests show as skipped (no `CAIRN_TEST_PG`). Record the counts.

- [ ] **Step 2: Run the full DB-gated suite**

Run: `cd matcher && CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" uv run --extra pipeline pytest -q`
Expected: every test passes (pure + DB-gated). Record the counts.

- [ ] **Step 3: Confirm the pure path imports no psycopg**

Run: `cd matcher && uv run python -c "import cairn_matcher.eval, cairn_matcher.eval.scorer_eval, cairn_matcher.eval.report, sys; assert 'psycopg' not in sys.modules, sorted(m for m in sys.modules if 'psyco' in m); print('pure-clean')"`
Expected: prints `pure-clean` (importing the pure eval surface pulls in no psycopg).

- [ ] **Step 4: Update HANDOVER.md + ROADMAP.md**

Record the eval-harness slice (B3 keystone) in `docs/HANDOVER.md` (new "this session" entry) and the matcher line in `docs/ROADMAP.md` Phase 4. Keep both concise and under 500 lines (prune older condensed entries as needed). Note what is unblocked (weight-learning, compound blocking keys) and what stays deferred (synthetic generator; the compound-key and weight-learning slices themselves; veto-aware scorer mode).

- [ ] **Step 5: Commit the docs**

```bash
git add docs/HANDOVER.md docs/ROADMAP.md
git commit -m "docs: record matcher eval harness (B3 keystone) in HANDOVER + ROADMAP"
```

---

## Self-Review

**Spec coverage:**
- Full split (pure + DB-gated blocking) — Tasks 1–7 (pure) + Task 8 (DB-gated). ✓
- One record shape, two consumers (reuse `candidate_from_rows` / seed `patient_*`) — Task 2 `record_to_candidate`; Task 8 `seed_dataset`. ✓
- Dataset format (entity clusters, projection-shaped fields, culture-plural) — Task 1 types/loader; Task 5 gold fixture. ✓
- Scorer metrics (confusion, P/R/F1 strict+lenient, auto-false-link, missed-match, score separation, zero-denominator convention) — Task 3. ✓
- Blocking metrics (pair-completeness, reduction-ratio, block-size/skipped, dropped-pair estimate, dropped-true-matches) — Task 8. ✓
- Thin CLI (`python -m cairn_matcher.eval`, scorer always, blocking when `CAIRN_TEST_PG`) — Task 7. ✓
- Caveats surfaced in the printed report — Task 6 `_CAVEAT`; vetoes-absent documented in Task 4. ✓
- No new dependency; pure core stays psycopg-free — Task 9 Step 3 asserts it. ✓

**Placeholder scan:** No TBD/TODO; every code step shows complete code; the gold-fixture tuning note names the exact free variables and keeps assertions fixed. ✓

**Type consistency:** `record_to_candidate`, `truth_pairs`, `all_pairs`, `canonical_label_pair` defined in Task 2 and used with the same signatures in Tasks 4/8. `PairOutcome`/`ScorerMetrics`/`scorer_metrics` defined in Task 3, consumed in Task 4. `Band` imported from `cairn_matcher.pipeline.banding` consistently. `BlockingMetrics` fields named in Task 8 match exactly what `format_blocking` reads in Task 6 (`pair_completeness`, `reduction_ratio`, `generated_pairs`, `total_pairs`, `skipped_blocks`, `dropped_pair_estimate`, `dropped_true_matches`). `evaluate_blocking(conn, ds, *, max_block_size=100)` signature matches the CLI call in Task 7. ✓
