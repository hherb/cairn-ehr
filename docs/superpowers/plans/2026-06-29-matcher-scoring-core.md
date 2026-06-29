# Matcher Scoring Core (piece B1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the advisory matcher's pure Fellegi–Sunter scoring core — a comparator API contract, four culture-neutral comparators, and a combiner that turns two projected patient records into an explainable `MatchScore` — as a new `matcher/` Python project with zero DB/IO.

**Architecture:** Pure functions only. Comparators (`value_a, value_b, context -> AgreementLevel`) grade per-field agreement; a Fellegi–Sunter combiner maps `(field, level)` to log-weights, scales by provenance, and sums to a log-likelihood ratio with a per-field evidence breakdown. No Postgres, no thresholds, no link decisions — those are later slices (B2/in-DB).

**Tech Stack:** Python ≥ 3.11, uv (project + runner), pytest, hatchling build backend. No runtime dependencies (Jaro–Winkler is implemented in-house).

## Global Constraints

- **License:** AGPL-3.0-only; every dependency must be AGPL-3.0-compatible, checked before adding. Target **zero runtime dependencies**.
- **Tooling:** **uv only — never venv/pip** for any env/package operation. Run tests with `uv run pytest`.
- **Python:** `requires-python = ">=3.11"`.
- **Layout:** new top-level `matcher/` project; import package `cairn_matcher`; `src/` layout; hatchling build backend (matches `packaging/pypi`).
- **TDD:** failing test first, then minimal code. All tests green before any commit.
- **Pure functions:** no I/O, no global state, no Postgres anywhere in this slice. `frozen=True` dataclasses.
- **Inline docs:** every module and non-trivial function carries a docstring explaining *why it exists and how it fits*, legible to a junior contributor.
- **Files under 500 lines:** keep modules focused; split if one grows unwieldy.
- **Safety boundary:** B1 is advisory/fit-for-purpose. It owns **no** thresholds, **no** band classification, **no** veto logic (that is `db/016`), and **no** link decision. Output is a score with evidence and nothing more.

---

### Task 1: Project scaffold

**Files:**
- Create: `matcher/pyproject.toml`
- Create: `matcher/README.md`
- Create: `matcher/src/cairn_matcher/__init__.py`
- Create: `matcher/.gitignore`
- Test: `matcher/tests/test_package.py`

**Interfaces:**
- Consumes: nothing.
- Produces: `cairn_matcher.__version__` (str); a working `uv run pytest` in `matcher/`.

- [ ] **Step 1: Write the failing test**

`matcher/tests/test_package.py`:
```python
"""Smoke test: the package imports and exposes a version. Proves the uv project runs."""

import cairn_matcher


def test_package_exposes_version():
    assert isinstance(cairn_matcher.__version__, str)
    assert cairn_matcher.__version__
```

- [ ] **Step 2: Create the project files**

`matcher/pyproject.toml`:
```toml
[project]
name = "cairn-matcher"
version = "0.1.0"
description = "Cairn advisory patient-matching scoring core (piece B1): pure Fellegi–Sunter scoring over projected demographic records."
readme = "README.md"
requires-python = ">=3.11"
license = "AGPL-3.0-only"
authors = [{ name = "Horst Herb" }]
keywords = ["ehr", "record-linkage", "fellegi-sunter", "patient-matching"]
classifiers = [
  "Development Status :: 3 - Alpha",
  "Intended Audience :: Healthcare Industry",
  "Topic :: Scientific/Engineering :: Medical Science Apps.",
]
dependencies = []

[project.urls]
Homepage = "https://cairn-ehr.org"
Repository = "https://github.com/cairn-ehr/cairn-ehr"

[dependency-groups]
dev = ["pytest>=8"]

[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[tool.hatch.build.targets.wheel]
packages = ["src/cairn_matcher"]

[tool.pytest.ini_options]
testpaths = ["tests"]
```

`matcher/src/cairn_matcher/__init__.py`:
```python
"""Cairn advisory patient-matcher — pure scoring core (piece B1).

This package is the *advisory* (fit-for-purpose, §9 blast-radius) half of the §5.2
matching pipeline. It turns two already-projected patient records into a match SCORE
with per-field evidence. It is pure: no Postgres, no I/O, no thresholds, no link
decisions. The safety-critical hard-veto floor lives in the database (db/016); the
conservative auto-link threshold and the proposal -> link apply seam are separate
slices. A defect here yields a bad *proposal* a human reviews, never record corruption.
"""

__version__ = "0.1.0"
```

`matcher/README.md`:
```markdown
# cairn-matcher

The Cairn advisory patient-matcher's **pure scoring core** (piece B1 of the §5.2
matching pipeline). Comparator API contract + culture-neutral comparators + a
Fellegi–Sunter combiner producing an explainable `MatchScore`.

**This is advisory** (fit-for-purpose, §9). It owns no thresholds, no band
classification, no veto logic (that is the in-DB floor, `db/016`), and no link
decision. It only *scores*.

**Pure functions only** — no Postgres, no I/O. Inputs are plain dataclasses; the DB
adapter, blocking, the veto-gate call, and locale comparator packs are later slices
(B2/B3). See `docs/superpowers/specs/2026-06-29-matcher-scoring-core-design.md`.

## Develop

```bash
cd matcher
uv run pytest
```
```

`matcher/.gitignore`:
```
.venv/
__pycache__/
*.pyc
.pytest_cache/
dist/
```

- [ ] **Step 3: Run the test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_package.py -v`
Expected: PASS (uv creates the environment on first run).

- [ ] **Step 4: Commit**

```bash
git add matcher/pyproject.toml matcher/README.md matcher/src/cairn_matcher/__init__.py matcher/.gitignore matcher/tests/test_package.py
git commit -m "feat(matcher): scaffold the cairn-matcher uv project (piece B1)"
```

---

### Task 2: The agreement vocabulary and comparator contract

**Files:**
- Create: `matcher/src/cairn_matcher/agreement.py`
- Test: `matcher/tests/test_agreement.py`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `AgreementLevel(IntEnum)` with members `INSUFFICIENT_DATA=0, DISAGREE=1, PARTIAL=2, EDIT_DISTANCE=3, PHONETIC=4, NICKNAME=5, EXACT=6` (ordinal: higher = stronger agreement).
  - `Context` frozen dataclass: `edit_distance_threshold: float = 0.90`.
  - `Comparator = Callable[[Any, Any, Context], AgreementLevel]`.

- [ ] **Step 1: Write the failing test**

`matcher/tests/test_agreement.py`:
```python
from cairn_matcher.agreement import AgreementLevel, Context


def test_levels_are_ordinal_strongest_is_exact():
    assert AgreementLevel.INSUFFICIENT_DATA < AgreementLevel.DISAGREE
    assert AgreementLevel.DISAGREE < AgreementLevel.PARTIAL
    assert AgreementLevel.PARTIAL < AgreementLevel.EDIT_DISTANCE
    assert AgreementLevel.EDIT_DISTANCE < AgreementLevel.PHONETIC
    assert AgreementLevel.PHONETIC < AgreementLevel.NICKNAME
    assert AgreementLevel.NICKNAME < AgreementLevel.EXACT
    # max() over a set of levels picks the strongest agreement
    assert max(AgreementLevel.DISAGREE, AgreementLevel.EXACT) is AgreementLevel.EXACT


def test_context_has_default_edit_distance_threshold():
    assert Context().edit_distance_threshold == 0.90
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_agreement.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'cairn_matcher.agreement'`.

- [ ] **Step 3: Write the implementation**

`matcher/src/cairn_matcher/agreement.py`:
```python
"""The comparator contract and the graded-agreement vocabulary (ADR-0014 §Decision 2).

A comparator is a PURE, field-typed function returning a graded AgreementLevel, never a
boolean — because Fellegi–Sunter weighs each level of agreement differently. The levels
are ordinal (higher = stronger agreement) so that name-set matching can pick the best
agreement across a cross-product with a plain max().

PHONETIC and NICKNAME exist in the vocabulary as the reserved plug points for locale
packs (a later slice). NO comparator in this core emits them — shipping a phonetic
encoder (Soundex is anglo) or a nickname lexicon (cultural) in the core would be the
"cultural capture" ADR-0014 forbids.
"""

from collections.abc import Callable
from dataclasses import dataclass
from enum import IntEnum
from typing import Any


class AgreementLevel(IntEnum):
    """Graded agreement between two field values. Ordinal: higher == stronger."""

    INSUFFICIENT_DATA = 0  # a side is absent/unknown -> ZERO evidence (not a penalty, §3.7)
    DISAGREE = 1           # both present, no agreement at any level
    PARTIAL = 2            # precision-coarsened / weak (e.g. year-only DOB vs full)
    EDIT_DISTANCE = 3      # agree within an edit-distance band
    PHONETIC = 4           # reserved for locale packs — not emitted by this core
    NICKNAME = 5           # reserved for locale packs — not emitted by this core
    EXACT = 6              # exact agreement


@dataclass(frozen=True)
class Context:
    """Per-comparison facets a comparator may need. Never carries I/O handles.

    edit_distance_threshold is the Jaro–Winkler similarity at or above which
    compare_edit_distance grades EDIT_DISTANCE rather than DISAGREE.
    """

    edit_distance_threshold: float = 0.90


# A comparator: pure, field-typed, returns a graded agreement level.
Comparator = Callable[[Any, Any, Context], AgreementLevel]
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_agreement.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/agreement.py matcher/tests/test_agreement.py
git commit -m "feat(matcher): agreement-level vocabulary + comparator contract"
```

---

### Task 3: Input value types

**Files:**
- Create: `matcher/src/cairn_matcher/records.py`
- Test: `matcher/tests/test_records.py`

**Interfaces:**
- Consumes: `AgreementLevel` (Task 2).
- Produces:
  - `DateValue(year: int|None=None, month: int|None=None, day: int|None=None)` frozen.
  - `Name(tokens: Mapping[str, tuple[str, ...]])` frozen (role -> tokens).
  - `FieldValue(value: Any, provenance_rank: int = 0)` frozen.
  - `CandidateRecord(dob: FieldValue|None=None, sex_at_birth: FieldValue|None=None, names: FieldValue|None=None, identifiers: Mapping[str, frozenset[str]]=<empty>)` frozen.
  - `FieldComparison(field: str, level: AgreementLevel, provenance_rank: int)` frozen.
  - `MatcherTypeError(TypeError)`.

- [ ] **Step 1: Write the failing test**

`matcher/tests/test_records.py`:
```python
from cairn_matcher.agreement import AgreementLevel
from cairn_matcher.records import (
    CandidateRecord,
    DateValue,
    FieldComparison,
    FieldValue,
    MatcherTypeError,
    Name,
)


def test_records_construct_and_are_frozen():
    dob = FieldValue(DateValue(1980, 3, 15), provenance_rank=60)
    name = Name(tokens={"given": ("alex",), "family": ("kim",)})
    rec = CandidateRecord(
        dob=dob,
        names=FieldValue(frozenset({name})),
        identifiers={"au-medicare": frozenset({"2951"})},
    )
    assert rec.dob.value == DateValue(1980, 3, 15)
    assert rec.dob.provenance_rank == 60
    assert next(iter(rec.names.value)).tokens["family"] == ("kim",)


def test_field_value_defaults_provenance_zero_and_identifiers_default_empty():
    assert FieldValue("x").provenance_rank == 0
    assert CandidateRecord().identifiers == {}
    assert CandidateRecord().dob is None


def test_field_comparison_carries_level_and_rank():
    fc = FieldComparison(field="dob", level=AgreementLevel.EXACT, provenance_rank=60)
    assert fc.field == "dob"
    assert fc.level is AgreementLevel.EXACT


def test_matcher_type_error_is_a_type_error():
    assert issubclass(MatcherTypeError, TypeError)
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_records.py -v`
Expected: FAIL with `ModuleNotFoundError`.

- [ ] **Step 3: Write the implementation**

`matcher/src/cairn_matcher/records.py`:
```python
"""Input/output value types the scoring core operates over.

These are plain frozen dataclasses. A later slice (B2) populates a CandidateRecord
from the patient_* projections; this core builds them by hand in tests. Keeping the
types here — separate from the comparison logic — means the comparators and the
combiner depend only on data shapes, not on where the data came from.
"""

from collections.abc import Mapping
from dataclasses import dataclass, field
from typing import Any

from cairn_matcher.agreement import AgreementLevel


class MatcherTypeError(TypeError):
    """A value was structurally the wrong type (an adapter bug), not merely absent.

    Absence is normal and safe — it grades INSUFFICIENT_DATA. This error is for the
    different failure: a str where a DateValue is required, etc. We raise loudly rather
    than fail silently (house rule #5).
    """


@dataclass(frozen=True)
class DateValue:
    """A canonical, already-parsed date. Precision is implied by which parts are present.

    The core never parses a locale date STRING into this — that is locale-specific and
    belongs to B2/locale packs. compare_dob operates only on the parts present here.
    """

    year: int | None = None
    month: int | None = None
    day: int | None = None


@dataclass(frozen=True)
class Name:
    """One asserted name as role-tagged token bags, e.g. {"given": ("alex",), ...}.

    A patient carries a SET of these (the §4.2 retained name history). Comparison is
    order- and role-tolerant: tokens are compared as bags per role, not positionally.
    """

    tokens: Mapping[str, tuple[str, ...]]


@dataclass(frozen=True)
class FieldValue:
    """A single demographic field's value plus the provenance rank behind it.

    provenance_rank is the cached patient_demographic.provenance_rank (the §4.1 ladder
    as an int; 0 = unrecognized). The combiner scales evidence by it.
    """

    value: Any
    provenance_rank: int = 0


@dataclass(frozen=True)
class CandidateRecord:
    """Everything one patient contributes to a comparison. Additive: more fields later."""

    dob: FieldValue | None = None
    sex_at_birth: FieldValue | None = None
    names: FieldValue | None = None  # value is a frozenset[Name] (the history set)
    identifiers: Mapping[str, frozenset[str]] = field(default_factory=dict)


@dataclass(frozen=True)
class FieldComparison:
    """The graded outcome for one field, with the (weaker-side) provenance behind it."""

    field: str
    level: AgreementLevel
    provenance_rank: int
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_records.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/records.py matcher/tests/test_records.py
git commit -m "feat(matcher): input value types (CandidateRecord, DateValue, Name)"
```

---

### Task 4: Jaro–Winkler similarity (in-house, pure)

**Files:**
- Create: `matcher/src/cairn_matcher/comparators.py`
- Test: `matcher/tests/test_jaro_winkler.py`

**Interfaces:**
- Consumes: nothing.
- Produces: `jaro_winkler(s1: str, s2: str, prefix_scale: float = 0.1) -> float` returning a similarity in `[0.0, 1.0]`. Symmetric. `jaro_winkler("x","x") == 1.0`; two empty strings -> `1.0`; one empty -> `0.0`.

- [ ] **Step 1: Write the failing test**

`matcher/tests/test_jaro_winkler.py`:
```python
import pytest

from cairn_matcher.comparators import jaro_winkler


def approx(x):
    return pytest.approx(x, abs=1e-3)


def test_identical_strings_are_one():
    assert jaro_winkler("martha", "martha") == 1.0


def test_two_empty_strings_are_one_one_empty_is_zero():
    assert jaro_winkler("", "") == 1.0
    assert jaro_winkler("abc", "") == 0.0
    assert jaro_winkler("", "abc") == 0.0


def test_known_reference_values():
    # Published Jaro–Winkler reference pairs (prefix scale 0.1).
    assert jaro_winkler("martha", "marhta") == approx(0.961)
    assert jaro_winkler("dwayne", "duane") == approx(0.840)
    assert jaro_winkler("dixon", "dicksonx") == approx(0.813)


def test_is_symmetric():
    assert jaro_winkler("dwayne", "duane") == jaro_winkler("duane", "dwayne")
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_jaro_winkler.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'cairn_matcher.comparators'`.

- [ ] **Step 3: Write the implementation**

`matcher/src/cairn_matcher/comparators.py`:
```python
"""The culture-neutral comparator set + the in-house Jaro–Winkler primitive.

Every comparator is pure and field-typed (agreement.Comparator). Each returns
INSUFFICIENT_DATA when a side is absent — a missing field is ZERO evidence, never a
penalty (§3.7, the no-data-is-never-disagreement principle made mechanical).

Jaro–Winkler is implemented here rather than pulled from a dependency: it is short,
fully testable, reviewer-legible, and keeps the project dependency-free (supply-chain
hygiene, house rule #1).
"""

from cairn_matcher.agreement import AgreementLevel, Context


def jaro_winkler(s1: str, s2: str, prefix_scale: float = 0.1) -> float:
    """Jaro–Winkler string similarity in [0, 1]. Symmetric.

    Jaro measures matching characters within a sliding window minus transpositions;
    Winkler boosts strings sharing a common prefix (up to 4 chars). Two empty strings
    are identical (1.0); one empty is fully dissimilar (0.0).
    """

    if s1 == s2:
        return 1.0
    if not s1 or not s2:
        return 0.0

    len1, len2 = len(s1), len(s2)
    match_window = max(len1, len2) // 2 - 1
    match_window = max(match_window, 0)

    s1_matches = [False] * len1
    s2_matches = [False] * len2
    matches = 0

    # Count matching characters within the window.
    for i in range(len1):
        start = max(0, i - match_window)
        end = min(i + match_window + 1, len2)
        for j in range(start, end):
            if s2_matches[j] or s1[i] != s2[j]:
                continue
            s1_matches[i] = True
            s2_matches[j] = True
            matches += 1
            break

    if matches == 0:
        return 0.0

    # Count transpositions among the matched characters.
    transpositions = 0
    k = 0
    for i in range(len1):
        if not s1_matches[i]:
            continue
        while not s2_matches[k]:
            k += 1
        if s1[i] != s2[k]:
            transpositions += 1
        k += 1
    transpositions //= 2

    m = matches
    jaro = (m / len1 + m / len2 + (m - transpositions) / m) / 3.0

    # Winkler prefix boost (common prefix up to 4 characters).
    prefix = 0
    for c1, c2 in zip(s1, s2):
        if c1 != c2:
            break
        prefix += 1
        if prefix == 4:
            break

    return jaro + prefix * prefix_scale * (1.0 - jaro)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_jaro_winkler.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/comparators.py matcher/tests/test_jaro_winkler.py
git commit -m "feat(matcher): in-house Jaro–Winkler similarity primitive"
```

---

### Task 5: String comparators — exact and edit-distance

**Files:**
- Modify: `matcher/src/cairn_matcher/comparators.py`
- Test: `matcher/tests/test_comparators_string.py`

**Interfaces:**
- Consumes: `AgreementLevel`, `Context` (Task 2), `jaro_winkler` (Task 4), `MatcherTypeError` (Task 3).
- Produces:
  - `compare_exact(a: str|None, b: str|None, ctx: Context) -> AgreementLevel`
  - `compare_edit_distance(a: str|None, b: str|None, ctx: Context) -> AgreementLevel`
  - Both: `None` on either side -> `INSUFFICIENT_DATA`; a non-str, non-None value -> `MatcherTypeError`.

- [ ] **Step 1: Write the failing test**

`matcher/tests/test_comparators_string.py`:
```python
import pytest

from cairn_matcher.agreement import AgreementLevel, Context
from cairn_matcher.comparators import compare_edit_distance, compare_exact
from cairn_matcher.records import MatcherTypeError

CTX = Context()


def test_exact_agrees_after_trim():
    assert compare_exact("  smith ", "smith", CTX) is AgreementLevel.EXACT


def test_exact_disagrees_on_different_values():
    assert compare_exact("smith", "jones", CTX) is AgreementLevel.DISAGREE


def test_exact_does_not_casefold():
    # Casefolding is culture-touching; the core does not do it.
    assert compare_exact("Smith", "smith", CTX) is AgreementLevel.DISAGREE


def test_missing_side_is_insufficient_data():
    assert compare_exact(None, "smith", CTX) is AgreementLevel.INSUFFICIENT_DATA
    assert compare_edit_distance("smith", None, CTX) is AgreementLevel.INSUFFICIENT_DATA


def test_edit_distance_exact_when_identical():
    assert compare_edit_distance("martha", "martha", CTX) is AgreementLevel.EXACT


def test_edit_distance_grades_close_pair_within_band():
    # martha/marhta ~ 0.961 >= 0.90 default threshold
    assert compare_edit_distance("martha", "marhta", CTX) is AgreementLevel.EDIT_DISTANCE


def test_edit_distance_disagrees_below_band():
    assert compare_edit_distance("smith", "jones", CTX) is AgreementLevel.DISAGREE


def test_edit_distance_threshold_is_configurable():
    loose = Context(edit_distance_threshold=0.80)
    # dwayne/duane ~ 0.840: DISAGREE at 0.90, EDIT_DISTANCE at 0.80
    assert compare_edit_distance("dwayne", "duane", CTX) is AgreementLevel.DISAGREE
    assert compare_edit_distance("dwayne", "duane", loose) is AgreementLevel.EDIT_DISTANCE


def test_wrong_type_raises():
    with pytest.raises(MatcherTypeError):
        compare_exact(5, "smith", CTX)
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_comparators_string.py -v`
Expected: FAIL with `ImportError` (functions not defined).

- [ ] **Step 3: Write the implementation** (append to `comparators.py`)

Add this import near the top of `comparators.py` (below the existing imports):
```python
from cairn_matcher.records import MatcherTypeError
```

Append these functions to `comparators.py`:
```python
def _require_str_or_none(value: object, field_name: str) -> str | None:
    """Normalize a string field input: None passes through; a str is trimmed; anything
    else is an adapter bug and raises (we never silently coerce)."""
    if value is None:
        return None
    if not isinstance(value, str):
        raise MatcherTypeError(f"{field_name} expected str or None, got {type(value)!r}")
    return value.strip()


def compare_exact(a: str | None, b: str | None, ctx: Context) -> AgreementLevel:
    """Exact string agreement after trimming surrounding whitespace only.

    Deliberately does NOT casefold or transliterate — those are culture-touching and
    belong to locale packs, not the neutral core.
    """
    sa = _require_str_or_none(a, "compare_exact.a")
    sb = _require_str_or_none(b, "compare_exact.b")
    if sa is None or sb is None:
        return AgreementLevel.INSUFFICIENT_DATA
    return AgreementLevel.EXACT if sa == sb else AgreementLevel.DISAGREE


def compare_edit_distance(a: str | None, b: str | None, ctx: Context) -> AgreementLevel:
    """Graded string agreement by Jaro–Winkler similarity.

    EXACT if identical, EDIT_DISTANCE if similarity >= ctx.edit_distance_threshold,
    else DISAGREE. Missing side -> INSUFFICIENT_DATA.
    """
    sa = _require_str_or_none(a, "compare_edit_distance.a")
    sb = _require_str_or_none(b, "compare_edit_distance.b")
    if sa is None or sb is None:
        return AgreementLevel.INSUFFICIENT_DATA
    if sa == sb:
        return AgreementLevel.EXACT
    if jaro_winkler(sa, sb) >= ctx.edit_distance_threshold:
        return AgreementLevel.EDIT_DISTANCE
    return AgreementLevel.DISAGREE
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_comparators_string.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/comparators.py matcher/tests/test_comparators_string.py
git commit -m "feat(matcher): exact + edit-distance string comparators"
```

---

### Task 6: DOB comparator (precision-aware, parses nothing)

**Files:**
- Modify: `matcher/src/cairn_matcher/comparators.py`
- Test: `matcher/tests/test_comparator_dob.py`

**Interfaces:**
- Consumes: `AgreementLevel`, `Context` (Task 2), `DateValue`, `MatcherTypeError` (Task 3).
- Produces: `compare_dob(a: DateValue|None, b: DateValue|None, ctx: Context) -> AgreementLevel`.

- [ ] **Step 1: Write the failing test**

`matcher/tests/test_comparator_dob.py`:
```python
import pytest

from cairn_matcher.agreement import AgreementLevel, Context
from cairn_matcher.comparators import compare_dob
from cairn_matcher.records import DateValue, MatcherTypeError

CTX = Context()


def test_identical_full_dates_are_exact():
    assert compare_dob(DateValue(1980, 3, 15), DateValue(1980, 3, 15), CTX) is AgreementLevel.EXACT


def test_year_only_vs_full_is_partial():
    # 1980 (year precision) vs 1980-03-15 (day precision): consistent coarsening.
    assert compare_dob(DateValue(1980), DateValue(1980, 3, 15), CTX) is AgreementLevel.PARTIAL


def test_shared_part_differs_is_disagree():
    assert compare_dob(DateValue(1980, 3, 15), DateValue(1980, 3, 16), CTX) is AgreementLevel.DISAGREE
    assert compare_dob(DateValue(1980), DateValue(1981, 3, 15), CTX) is AgreementLevel.DISAGREE


def test_missing_side_is_insufficient_data():
    assert compare_dob(None, DateValue(1980, 3, 15), CTX) is AgreementLevel.INSUFFICIENT_DATA


def test_no_overlapping_parts_is_insufficient_data():
    # One has only a year, the other only a day-of-month: nothing comparable in common.
    assert compare_dob(DateValue(year=1980), DateValue(day=15), CTX) is AgreementLevel.INSUFFICIENT_DATA


def test_is_symmetric():
    a, b = DateValue(1980), DateValue(1980, 3, 15)
    assert compare_dob(a, b, CTX) is compare_dob(b, a, CTX)


def test_wrong_type_raises():
    with pytest.raises(MatcherTypeError):
        compare_dob("1980-03-15", DateValue(1980, 3, 15), CTX)
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_comparator_dob.py -v`
Expected: FAIL with `ImportError`.

- [ ] **Step 3: Write the implementation**

Add `DateValue` to the existing `from cairn_matcher.records import ...` line in `comparators.py` so it reads:
```python
from cairn_matcher.records import DateValue, MatcherTypeError
```

Append to `comparators.py`:
```python
# DOB parts compared, coarsest to finest. Precision = the prefix of these present.
_DOB_PARTS = ("year", "month", "day")


def compare_dob(a: DateValue | None, b: DateValue | None, ctx: Context) -> AgreementLevel:
    """Precision-aware DOB agreement that PARSES NO DATE STRINGS.

    Compares only the parts BOTH sides carry:
      * every shared part equal AND same precision depth -> EXACT
      * every shared part equal BUT different depth (year-only vs full) -> PARTIAL
        (a consistent coarsening; principle 4 — imprecision is partial agreement)
      * any shared part differs -> DISAGREE
      * a side absent, or no part in common -> INSUFFICIENT_DATA (never a penalty)
    """
    if a is None or b is None:
        return AgreementLevel.INSUFFICIENT_DATA
    if not isinstance(a, DateValue) or not isinstance(b, DateValue):
        raise MatcherTypeError("compare_dob expects DateValue or None")

    shared = [p for p in _DOB_PARTS if getattr(a, p) is not None and getattr(b, p) is not None]
    if not shared:
        return AgreementLevel.INSUFFICIENT_DATA

    if any(getattr(a, p) != getattr(b, p) for p in shared):
        return AgreementLevel.DISAGREE

    # All shared parts agree. Same precision depth on both sides -> EXACT, else PARTIAL.
    depth_a = sum(1 for p in _DOB_PARTS if getattr(a, p) is not None)
    depth_b = sum(1 for p in _DOB_PARTS if getattr(b, p) is not None)
    return AgreementLevel.EXACT if depth_a == depth_b else AgreementLevel.PARTIAL
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_comparator_dob.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/comparators.py matcher/tests/test_comparator_dob.py
git commit -m "feat(matcher): precision-aware DOB comparator (no date parsing)"
```

---

### Task 7: Name-set comparator (history set, order/role-tolerant)

**Files:**
- Modify: `matcher/src/cairn_matcher/comparators.py`
- Test: `matcher/tests/test_comparator_name.py`

**Interfaces:**
- Consumes: `AgreementLevel`, `Context` (Task 2), `Name` (Task 3), `compare_exact`, `compare_edit_distance` (Task 5).
- Produces: `compare_name_set(a: frozenset[Name]|None, b: frozenset[Name]|None, ctx: Context) -> AgreementLevel`.

- [ ] **Step 1: Write the failing test**

`matcher/tests/test_comparator_name.py`:
```python
from cairn_matcher.agreement import AgreementLevel, Context
from cairn_matcher.comparators import compare_name_set
from cairn_matcher.records import Name

CTX = Context()


def n(given, family):
    return Name(tokens={"given": tuple(given), "family": tuple(family)})


def test_identical_single_name_is_exact():
    a = frozenset({n(["alex"], ["kim"])})
    b = frozenset({n(["alex"], ["kim"])})
    assert compare_name_set(a, b, CTX) is AgreementLevel.EXACT


def test_given_token_order_is_tolerated():
    a = frozenset({n(["mary", "jane"], ["kim"])})
    b = frozenset({n(["jane", "mary"], ["kim"])})
    assert compare_name_set(a, b, CTX) is AgreementLevel.EXACT


def test_given_family_role_swap_is_tolerated():
    # Same token bag, roles swapped — still matches (role-tolerant bag comparison).
    a = frozenset({n(["kim"], ["alex"])})
    b = frozenset({n(["alex"], ["kim"])})
    assert compare_name_set(a, b, CTX) is AgreementLevel.EXACT


def test_matches_if_any_historical_name_agrees():
    # b's maiden name matches a's only name even though b's current differs.
    a = frozenset({n(["sarah"], ["jones"])})
    b = frozenset({n(["sarah"], ["smith"]), n(["sarah"], ["jones"])})
    assert compare_name_set(a, b, CTX) is AgreementLevel.EXACT


def test_close_tokens_grade_edit_distance():
    a = frozenset({n(["jon"], ["smith"])})
    b = frozenset({n(["john"], ["smith"])})  # jon/john close, smith exact
    assert compare_name_set(a, b, CTX) is AgreementLevel.EDIT_DISTANCE


def test_disjoint_names_disagree():
    a = frozenset({n(["alex"], ["kim"])})
    b = frozenset({n(["chris"], ["jones"])})
    assert compare_name_set(a, b, CTX) is AgreementLevel.DISAGREE


def test_empty_set_is_insufficient_data():
    a = frozenset({n(["alex"], ["kim"])})
    assert compare_name_set(frozenset(), a, CTX) is AgreementLevel.INSUFFICIENT_DATA
    assert compare_name_set(None, a, CTX) is AgreementLevel.INSUFFICIENT_DATA
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_comparator_name.py -v`
Expected: FAIL with `ImportError`.

- [ ] **Step 3: Write the implementation**

Add `Name` to the `from cairn_matcher.records import ...` line in `comparators.py`:
```python
from cairn_matcher.records import DateValue, MatcherTypeError, Name
```

Append to `comparators.py`:
```python
def _name_token_bag(name: Name) -> list[str]:
    """Flatten a role-tagged name into a flat bag of tokens (role-tolerant).

    Role tolerance matters because given/family is often swapped or mis-tagged on entry,
    and many cultures do not split names the way the data-entry form assumes.
    """
    bag: list[str] = []
    for tokens in name.tokens.values():
        bag.extend(tokens)
    return bag


def _compare_two_names(a: Name, b: Name, ctx: Context) -> AgreementLevel:
    """Best agreement between two single names, comparing token bags order-tolerantly.

    Greedy one-to-one token pairing: each a-token claims its best-agreeing unused
    b-token. The name's level is the WEAKEST link across the bag (every token must find
    a partner), and the bags must be the same size — a missing/extra token is a real
    difference, not a free pass.
    """
    bag_a = _name_token_bag(a)
    bag_b = list(_name_token_bag(b))
    if not bag_a or not bag_b or len(bag_a) != len(bag_b):
        return AgreementLevel.DISAGREE

    worst = AgreementLevel.EXACT
    for token in bag_a:
        best_level = AgreementLevel.DISAGREE
        best_idx = -1
        for idx, other in enumerate(bag_b):
            level = compare_exact(token, other, ctx)
            if level is not AgreementLevel.EXACT:
                level = compare_edit_distance(token, other, ctx)
            if level > best_level:
                best_level = level
                best_idx = idx
        if best_idx < 0:
            return AgreementLevel.DISAGREE
        bag_b.pop(best_idx)
        worst = min(worst, best_level)
    return worst


def compare_name_set(
    a: frozenset[Name] | None, b: frozenset[Name] | None, ctx: Context
) -> AgreementLevel:
    """Agreement over two NAME HISTORY SETS — match if ANY historical name pair agrees.

    Operating over the retained set (not just the current display name) is principle-
    bearing (§4.2 / ADR-0014): maiden/married switching, changed family names, and
    discarded aliases still match. Returns the BEST agreement across the cross-product;
    an empty/absent set on either side -> INSUFFICIENT_DATA (zero evidence, not a clash).
    """
    if not a or not b:
        return AgreementLevel.INSUFFICIENT_DATA
    best = AgreementLevel.DISAGREE
    for name_a in a:
        for name_b in b:
            best = max(best, _compare_two_names(name_a, name_b, ctx))
            if best is AgreementLevel.EXACT:
                return best
    return best
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_comparator_name.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/comparators.py matcher/tests/test_comparator_name.py
git commit -m "feat(matcher): name-set comparator (history set, order/role-tolerant)"
```

---

### Task 8: Identifier comparator + the field-comparison orchestrator

**Files:**
- Modify: `matcher/src/cairn_matcher/comparators.py`
- Create: `matcher/src/cairn_matcher/orchestrator.py`
- Test: `matcher/tests/test_comparator_identifier.py`
- Test: `matcher/tests/test_orchestrator.py`

**Interfaces:**
- Consumes: all comparators (Tasks 5–7), `CandidateRecord`, `FieldComparison`, `FieldValue` (Task 3), `Context` (Task 2).
- Produces:
  - `compare_identifier_sets(a: Mapping[str, frozenset[str]], b: Mapping[str, frozenset[str]], ctx: Context) -> AgreementLevel` — **positive-only**: `EXACT` if any shared system shares a value, else `INSUFFICIENT_DATA` (never `DISAGREE` — identifier *mismatch* is the in-DB veto's job, `db/016`, not B1's).
  - `FieldSpec(field: str, comparator: Comparator, get: Callable[[CandidateRecord], tuple[Any, int]], context: Context = Context())` frozen.
  - `ComparatorConfig = tuple[FieldSpec, ...]`.
  - `DEFAULT_CONFIG: ComparatorConfig` covering fields `dob`, `sex-at-birth`, `name`, `identifier`.
  - `field_comparisons(a: CandidateRecord, b: CandidateRecord, config: ComparatorConfig = DEFAULT_CONFIG) -> list[FieldComparison]` — per field: runs the comparator, records `min(rank_a, rank_b)` as the provenance behind the comparison (evidence is only as strong as its weaker side).

- [ ] **Step 1: Write the failing tests**

`matcher/tests/test_comparator_identifier.py`:
```python
from cairn_matcher.agreement import AgreementLevel, Context
from cairn_matcher.comparators import compare_identifier_sets

CTX = Context()


def test_shared_value_in_shared_system_is_exact():
    a = {"au-medicare": frozenset({"2951"})}
    b = {"au-medicare": frozenset({"2951", "3000"})}
    assert compare_identifier_sets(a, b, CTX) is AgreementLevel.EXACT


def test_disjoint_is_insufficient_not_disagree():
    # Identifier MISMATCH is the in-DB veto's job, never a B1 penalty.
    a = {"au-medicare": frozenset({"2951"})}
    b = {"au-medicare": frozenset({"9999"})}
    assert compare_identifier_sets(a, b, CTX) is AgreementLevel.INSUFFICIENT_DATA


def test_no_shared_system_is_insufficient():
    a = {"au-medicare": frozenset({"2951"})}
    b = {"nz-nhi": frozenset({"2951"})}
    assert compare_identifier_sets(a, b, CTX) is AgreementLevel.INSUFFICIENT_DATA


def test_empty_is_insufficient():
    assert compare_identifier_sets({}, {"x": frozenset({"1"})}, CTX) is AgreementLevel.INSUFFICIENT_DATA
```

`matcher/tests/test_orchestrator.py`:
```python
from cairn_matcher.agreement import AgreementLevel
from cairn_matcher.orchestrator import DEFAULT_CONFIG, field_comparisons
from cairn_matcher.records import CandidateRecord, DateValue, FieldValue, Name


def n(given, family):
    return Name(tokens={"given": tuple(given), "family": tuple(family)})


def _rec():
    return CandidateRecord(
        dob=FieldValue(DateValue(1980, 3, 15), provenance_rank=60),
        sex_at_birth=FieldValue("female", provenance_rank=60),
        names=FieldValue(frozenset({n(["alex"], ["kim"])}), provenance_rank=20),
        identifiers={"au-medicare": frozenset({"2951"})},
    )


def test_identical_records_produce_field_agreements():
    comps = {c.field: c for c in field_comparisons(_rec(), _rec())}
    assert comps["dob"].level is AgreementLevel.EXACT
    assert comps["sex-at-birth"].level is AgreementLevel.EXACT
    assert comps["name"].level is AgreementLevel.EXACT
    assert comps["identifier"].level is AgreementLevel.EXACT


def test_provenance_is_the_weaker_of_the_two_sides():
    strong = CandidateRecord(dob=FieldValue(DateValue(1980, 3, 15), provenance_rank=60))
    weak = CandidateRecord(dob=FieldValue(DateValue(1980, 3, 15), provenance_rank=10))
    comps = {c.field: c for c in field_comparisons(strong, weak)}
    assert comps["dob"].provenance_rank == 10


def test_absent_field_grades_insufficient_data():
    empty = CandidateRecord()
    comps = {c.field: c for c in field_comparisons(empty, _rec())}
    assert comps["dob"].level is AgreementLevel.INSUFFICIENT_DATA
    assert comps["name"].level is AgreementLevel.INSUFFICIENT_DATA
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd matcher && uv run pytest tests/test_comparator_identifier.py tests/test_orchestrator.py -v`
Expected: FAIL with `ImportError`.

- [ ] **Step 3: Write the implementations**

Append `compare_identifier_sets` to `comparators.py` (add `from collections.abc import Mapping` at the top if not present):
```python
def compare_identifier_sets(
    a: "Mapping[str, frozenset[str]]", b: "Mapping[str, frozenset[str]]", ctx: Context
) -> AgreementLevel:
    """POSITIVE-ONLY identifier agreement: EXACT if any shared system shares a value.

    A shared strong identifier is powerful POSITIVE evidence. But identifier MISMATCH is
    deliberately NOT a B1 concern: the same-system mismatch veto is the safety-critical
    in-DB floor's job (db/016, cairn_identifier_veto), which knows the normalized form
    and the honest-degradation rules. So a disjoint or non-overlapping comparison grades
    INSUFFICIENT_DATA (no positive evidence), never DISAGREE — B1 never penalises an ID.
    """
    for system, values_a in a.items():
        values_b = b.get(system)
        if values_b and (values_a & values_b):
            return AgreementLevel.EXACT
    return AgreementLevel.INSUFFICIENT_DATA
```

`matcher/src/cairn_matcher/orchestrator.py`:
```python
"""Drive the configured comparator over each field of two records.

This is the registry seam ADR-0014's locale packs will extend: DEFAULT_CONFIG wires the
culture-neutral comparators to fields; a locale pack swaps in phonetic/nickname variants
without touching the combiner. Pure — no I/O.
"""

from collections.abc import Callable
from dataclasses import dataclass, field as dataclass_field
from typing import Any

from cairn_matcher.agreement import Comparator, Context
from cairn_matcher.comparators import (
    compare_dob,
    compare_exact,
    compare_identifier_sets,
    compare_name_set,
)
from cairn_matcher.records import CandidateRecord, FieldComparison


def _field_value(rec: CandidateRecord, attr: str) -> tuple[Any, int]:
    """Pull (value, provenance_rank) for a single-valued field; (None, 0) if absent."""
    fv = getattr(rec, attr)
    return (None, 0) if fv is None else (fv.value, fv.provenance_rank)


def _names(rec: CandidateRecord) -> tuple[Any, int]:
    fv = rec.names
    return (None, 0) if fv is None else (fv.value, fv.provenance_rank)


def _identifiers(rec: CandidateRecord) -> tuple[Any, int]:
    # Identifier match is positive-only and not provenance-tracked in B1 -> rank 0.
    return (rec.identifiers, 0)


@dataclass(frozen=True)
class FieldSpec:
    """One field's comparison recipe: which comparator, and how to extract its inputs."""

    field: str
    comparator: Comparator
    get: Callable[[CandidateRecord], tuple[Any, int]]
    context: Context = dataclass_field(default_factory=Context)


ComparatorConfig = tuple[FieldSpec, ...]


# The shipped culture-neutral configuration. A locale pack (B3) ships its own.
DEFAULT_CONFIG: ComparatorConfig = (
    FieldSpec("dob", compare_dob, lambda r: _field_value(r, "dob")),
    FieldSpec("sex-at-birth", compare_exact, lambda r: _field_value(r, "sex_at_birth")),
    FieldSpec("name", compare_name_set, _names),
    FieldSpec("identifier", compare_identifier_sets, _identifiers),
)


def field_comparisons(
    a: CandidateRecord, b: CandidateRecord, config: ComparatorConfig = DEFAULT_CONFIG
) -> list[FieldComparison]:
    """Run each field's comparator and record its graded outcome.

    The provenance recorded is min(rank_a, rank_b): evidence about a field is only as
    trustworthy as its WEAKER-provenance side (a verified value compared against an
    unverified one is, jointly, unverified-grade).
    """
    out: list[FieldComparison] = []
    for spec in config:
        value_a, rank_a = spec.get(a)
        value_b, rank_b = spec.get(b)
        level = spec.comparator(value_a, value_b, spec.context)
        out.append(FieldComparison(spec.field, level, min(rank_a, rank_b)))
    return out
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd matcher && uv run pytest tests/test_comparator_identifier.py tests/test_orchestrator.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/comparators.py matcher/src/cairn_matcher/orchestrator.py matcher/tests/test_comparator_identifier.py matcher/tests/test_orchestrator.py
git commit -m "feat(matcher): positive-only identifier comparator + field orchestrator"
```

---

### Task 9: The Fellegi–Sunter combiner

**Files:**
- Create: `matcher/src/cairn_matcher/scoring.py`
- Test: `matcher/tests/test_scoring.py`

**Interfaces:**
- Consumes: `AgreementLevel` (Task 2), `FieldComparison` (Task 3).
- Produces:
  - `provenance_factor(rank: int) -> float` — `0.5 + 0.5 * clamp(rank, 0, 70)/70` (rank 0 -> 0.5, rank ≥70 -> 1.0).
  - `FieldWeights(weights: Mapping[AgreementLevel, float])` frozen, `.weight_for(level) -> float` (default 0.0).
  - `Weights(per_field: Mapping[str, FieldWeights])` frozen.
  - `DEFAULT_WEIGHTS: Weights` covering `dob`, `sex-at-birth`, `name`, `identifier`.
  - `FieldEvidence(field, level, provenance_rank, weight_contribution)` frozen.
  - `MatchScore(total: float, fields: tuple[FieldEvidence, ...])` frozen.
  - `score(comparisons: list[FieldComparison], weights: Weights = DEFAULT_WEIGHTS) -> MatchScore`.

- [ ] **Step 1: Write the failing test**

`matcher/tests/test_scoring.py`:
```python
import pytest

from cairn_matcher.agreement import AgreementLevel
from cairn_matcher.records import FieldComparison
from cairn_matcher.scoring import (
    DEFAULT_WEIGHTS,
    FieldWeights,
    MatchScore,
    Weights,
    provenance_factor,
    score,
)


def test_provenance_factor_floor_and_ceiling():
    assert provenance_factor(0) == pytest.approx(0.5)
    assert provenance_factor(70) == pytest.approx(1.0)
    assert provenance_factor(35) == pytest.approx(0.75)
    assert provenance_factor(999) == pytest.approx(1.0)  # clamped
    assert provenance_factor(-5) == pytest.approx(0.5)   # clamped


WEIGHTS = Weights(per_field={
    "dob": FieldWeights({AgreementLevel.EXACT: 8.0, AgreementLevel.DISAGREE: -4.0}),
})


def test_exact_agreement_scaled_by_provenance():
    # rank 70 -> factor 1.0 -> 8.0 * 1.0
    comps = [FieldComparison("dob", AgreementLevel.EXACT, 70)]
    assert score(comps, WEIGHTS).total == pytest.approx(8.0)
    # rank 0 -> factor 0.5 -> 8.0 * 0.5
    comps = [FieldComparison("dob", AgreementLevel.EXACT, 0)]
    assert score(comps, WEIGHTS).total == pytest.approx(4.0)


def test_disagree_contributes_negative():
    comps = [FieldComparison("dob", AgreementLevel.DISAGREE, 70)]
    assert score(comps, WEIGHTS).total == pytest.approx(-4.0)


def test_insufficient_data_contributes_zero():
    comps = [FieldComparison("dob", AgreementLevel.INSUFFICIENT_DATA, 70)]
    s = score(comps, WEIGHTS)
    assert s.total == pytest.approx(0.0)
    assert s.fields[0].weight_contribution == pytest.approx(0.0)


def test_unknown_field_or_level_contributes_zero():
    comps = [FieldComparison("unmapped", AgreementLevel.EXACT, 70)]
    assert score(comps, WEIGHTS).total == pytest.approx(0.0)


def test_per_field_contributions_sum_to_total():
    comps = [
        FieldComparison("dob", AgreementLevel.EXACT, 70),
        FieldComparison("dob", AgreementLevel.DISAGREE, 70),
    ]
    s = score(comps, WEIGHTS)
    assert s.total == pytest.approx(sum(f.weight_contribution for f in s.fields))


def test_default_weights_cover_the_default_fields():
    for fld in ("dob", "sex-at-birth", "name", "identifier"):
        assert fld in DEFAULT_WEIGHTS.per_field


def test_match_score_is_returned():
    assert isinstance(score([], WEIGHTS), MatchScore)
    assert score([], WEIGHTS).total == 0.0
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_scoring.py -v`
Expected: FAIL with `ModuleNotFoundError`.

- [ ] **Step 3: Write the implementation**

`matcher/src/cairn_matcher/scoring.py`:
```python
"""The Fellegi–Sunter combiner: agreement vector + weights -> explainable match score.

Classic Fellegi–Sunter assigns each field, at each agreement level, a log-weight
log2(m/u) — positive when agreement is more likely under a match than a non-match,
negative for disagreement. The total match score is the sum of per-field log-weights: a
log-likelihood ratio. Two Cairn-specific properties:

  * INSUFFICIENT_DATA contributes EXACTLY ZERO — a missing field is never a penalty
    (§3.7, the no-data-is-never-disagreement principle).
  * each weight is scaled by provenance_factor(rank) — a *verified* clash or agreement
    weighs more than an *imported/unknown* one (§4.2, provenance-aware).

This module owns NO threshold and makes NO decision. It returns a score with a per-field
breakdown; banding it against the conservative auto-link threshold is the in-DB floor's
job. The m/u weights here are shipped DEFAULTS; learning them from local adjudication
data is a later slice (B3).
"""

from collections.abc import Mapping
from dataclasses import dataclass

from cairn_matcher.agreement import AgreementLevel
from cairn_matcher.records import FieldComparison

_PROVENANCE_CEILING = 70  # cairn_provenance_rank's top tier (fact-proven), db/011


def provenance_factor(rank: int) -> float:
    """Map a provenance rank to an evidence-strength multiplier in [0.5, 1.0].

    Unknown provenance (rank 0) still contributes — it IS data — but at half strength;
    a fully verified value (rank >= 70) contributes at full strength. Monotonic.
    """
    clamped = max(0, min(rank, _PROVENANCE_CEILING))
    return 0.5 + 0.5 * (clamped / _PROVENANCE_CEILING)


@dataclass(frozen=True)
class FieldWeights:
    """log2(m/u) per agreement level for one field. Missing level -> 0.0 (no evidence)."""

    weights: Mapping[AgreementLevel, float]

    def weight_for(self, level: AgreementLevel) -> float:
        return self.weights.get(level, 0.0)


@dataclass(frozen=True)
class Weights:
    """The deployment's per-field weight table (its locale tuning). Learning is B3."""

    per_field: Mapping[str, FieldWeights]


@dataclass(frozen=True)
class FieldEvidence:
    """One field's contribution to the score — the explainability unit."""

    field: str
    level: AgreementLevel
    provenance_rank: int
    weight_contribution: float


@dataclass(frozen=True)
class MatchScore:
    """A match score (log-likelihood ratio) plus its per-field breakdown.

    sum(f.weight_contribution for f in fields) == total, always.
    """

    total: float
    fields: tuple[FieldEvidence, ...]


# Shipped default weights. Illustrative log2(m/u) magnitudes — B3 learns real ones from
# local data. Stronger, rarer agreements (a shared identifier, an exact DOB) weigh most;
# low-cardinality fields (sex-at-birth) weigh least; disagreements are negative.
DEFAULT_WEIGHTS = Weights(per_field={
    "dob": FieldWeights({
        AgreementLevel.EXACT: 6.0,
        AgreementLevel.PARTIAL: 1.5,
        AgreementLevel.DISAGREE: -4.0,
    }),
    "sex-at-birth": FieldWeights({
        AgreementLevel.EXACT: 1.0,
        AgreementLevel.DISAGREE: -2.0,
    }),
    "name": FieldWeights({
        AgreementLevel.EXACT: 5.0,
        AgreementLevel.EDIT_DISTANCE: 2.5,
        AgreementLevel.DISAGREE: -2.0,
    }),
    "identifier": FieldWeights({
        AgreementLevel.EXACT: 8.0,  # positive-only (the comparator never emits DISAGREE)
    }),
})


def score(comparisons: list[FieldComparison], weights: Weights = DEFAULT_WEIGHTS) -> MatchScore:
    """Combine per-field agreements into a match score with a per-field breakdown."""
    evidence: list[FieldEvidence] = []
    for comp in comparisons:
        if comp.level is AgreementLevel.INSUFFICIENT_DATA:
            contribution = 0.0
        else:
            field_weights = weights.per_field.get(comp.field)
            base = field_weights.weight_for(comp.level) if field_weights else 0.0
            contribution = base * provenance_factor(comp.provenance_rank)
        evidence.append(
            FieldEvidence(comp.field, comp.level, comp.provenance_rank, contribution)
        )
    total = sum(e.weight_contribution for e in evidence)
    return MatchScore(total=total, fields=tuple(evidence))
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd matcher && uv run pytest tests/test_scoring.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/scoring.py matcher/tests/test_scoring.py
git commit -m "feat(matcher): Fellegi–Sunter combiner with provenance scaling"
```

---

### Task 10: End-to-end property tests + public API surface

**Files:**
- Modify: `matcher/src/cairn_matcher/__init__.py`
- Test: `matcher/tests/test_properties.py`

**Interfaces:**
- Consumes: `field_comparisons`, `DEFAULT_CONFIG` (Task 8), `score`, `DEFAULT_WEIGHTS` (Task 9), `CandidateRecord` & friends (Task 3).
- Produces: a curated public API re-exported from `cairn_matcher` (`CandidateRecord`, `DateValue`, `Name`, `FieldValue`, `AgreementLevel`, `field_comparisons`, `score`, `MatchScore`).

- [ ] **Step 1: Write the failing test**

`matcher/tests/test_properties.py`:
```python
"""Whole-pipeline property tests: the principle-bearing invariants, end to end."""

import cairn_matcher as cm
from cairn_matcher.orchestrator import field_comparisons
from cairn_matcher.records import CandidateRecord, DateValue, FieldValue, Name
from cairn_matcher.scoring import score


def n(given, family):
    return Name(tokens={"given": tuple(given), "family": tuple(family)})


def _full_record():
    return CandidateRecord(
        dob=FieldValue(DateValue(1980, 3, 15), provenance_rank=60),
        sex_at_birth=FieldValue("female", provenance_rank=60),
        names=FieldValue(frozenset({n(["alex"], ["kim"])}), provenance_rank=20),
        identifiers={"au-medicare": frozenset({"2951"})},
    )


def _score_of(a, b):
    return score(field_comparisons(a, b)).total


def test_score_is_symmetric():
    a, b = _full_record(), _full_record()
    assert _score_of(a, b) == _score_of(b, a)


def test_identical_strong_records_score_clearly_positive():
    a, b = _full_record(), _full_record()
    assert _score_of(a, b) > 0.0


def test_adding_a_missing_field_never_lowers_the_score():
    # The §3.7 invariant, end to end: a record that is absent a field must not be
    # penalised versus a record that simply never had that field considered.
    a = _full_record()
    full = _full_record()
    # b is identical to `full` but with DOB removed entirely (absent, not different).
    b_absent = CandidateRecord(
        sex_at_birth=full.sex_at_birth, names=full.names, identifiers=full.identifiers
    )
    score_with_absent_dob = _score_of(a, b_absent)
    score_full = _score_of(a, full)
    assert score_with_absent_dob <= score_full
    # And an absent field never makes the total go negative on an otherwise-matching pair.
    assert score_with_absent_dob > 0.0


def test_public_api_surface_is_importable():
    for name in (
        "CandidateRecord", "DateValue", "Name", "FieldValue",
        "AgreementLevel", "field_comparisons", "score", "MatchScore",
    ):
        assert hasattr(cm, name), name
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd matcher && uv run pytest tests/test_properties.py -v`
Expected: FAIL on `test_public_api_surface_is_importable` (the re-exports do not exist yet).

- [ ] **Step 3: Add the public API re-exports**

Replace `matcher/src/cairn_matcher/__init__.py` with (keep the module docstring at top, append the exports):
```python
"""Cairn advisory patient-matcher — pure scoring core (piece B1).

This package is the *advisory* (fit-for-purpose, §9 blast-radius) half of the §5.2
matching pipeline. It turns two already-projected patient records into a match SCORE
with per-field evidence. It is pure: no Postgres, no I/O, no thresholds, no link
decisions. The safety-critical hard-veto floor lives in the database (db/016); the
conservative auto-link threshold and the proposal -> link apply seam are separate
slices. A defect here yields a bad *proposal* a human reviews, never record corruption.
"""

from cairn_matcher.agreement import AgreementLevel, Context
from cairn_matcher.orchestrator import DEFAULT_CONFIG, field_comparisons
from cairn_matcher.records import (
    CandidateRecord,
    DateValue,
    FieldComparison,
    FieldValue,
    Name,
)
from cairn_matcher.scoring import DEFAULT_WEIGHTS, MatchScore, score

__version__ = "0.1.0"

__all__ = [
    "AgreementLevel",
    "Context",
    "CandidateRecord",
    "DateValue",
    "FieldComparison",
    "FieldValue",
    "Name",
    "DEFAULT_CONFIG",
    "field_comparisons",
    "DEFAULT_WEIGHTS",
    "MatchScore",
    "score",
    "__version__",
]
```

- [ ] **Step 4: Run the full suite to verify everything passes**

Run: `cd matcher && uv run pytest -v`
Expected: PASS (every test across all files).

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/__init__.py matcher/tests/test_properties.py
git commit -m "feat(matcher): end-to-end property tests + public API surface"
```

---

## Post-implementation (outside the task loop)

After all tasks pass:

1. **Run the whole suite once more:** `cd matcher && uv run pytest -v` — confirm all green.
2. **Update `docs/HANDOVER.md` and `docs/ROADMAP.md`:** record that piece B1 (the matcher scoring core) is built — new `matcher/` project, the comparator contract + 4 culture-neutral comparators + Fellegi–Sunter combiner; note what is still deferred (B2 PG adapter/blocking/veto-gate; B3 locale packs/learning/sweep; `compare_address`; piece C link seam). Prune both docs to stay concise.
3. **Open a PR** to `main` from `matcher-scoring-core`, describing the slice and linking the design/plan docs and ADR-0014.

## Self-review notes (coverage against the spec)

- Spec §2 (project layout) -> Task 1. §3 (contract + AgreementLevel) -> Task 2. §4 comparators: exact/edit-distance -> Task 5, dob -> Task 6, name_set -> Task 7, Jaro–Winkler -> Task 4. §5 value types -> Task 3, identifiers note + orchestrator/registry -> Task 8. §6 combiner (weights, provenance_factor, MatchScore, explainability) -> Task 9. §7 data flow + §9 symmetry/no-data property -> Task 10. §8 error handling (MatcherTypeError) -> Tasks 3/5/6. §10 deferrals -> recorded in HANDOVER/ROADMAP (post-impl).
- `PHONETIC`/`NICKNAME` are defined (Task 2) but never emitted by a core comparator — verified by absence in Tasks 5–8.
- No safety-critical logic (thresholds, veto, banding, link) appears anywhere — Task 9 explicitly owns none.
