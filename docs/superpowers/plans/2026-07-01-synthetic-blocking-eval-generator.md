# Synthetic Blocking-Eval Dataset Generator — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A pure, seeded, culture-plural generator that emits the existing eval dataset JSON at volume — clean seed identities plus corrupted `seed↔clone` near-duplicates — so the B3 harness can measure blocking recall on more than the tiny hand-authored gold set.

**Architecture:** Two new modules in `matcher/src/cairn_matcher/eval/`, mirroring the package's existing pure-core ↔ I/O-edge split (`dataset.py` ↔ `loader.py`): `generator.py` is pure (stdlib `random`/`dataclasses` only) and returns a JSON-shaped dataset **dict** that round-trips through the real `load_dataset`; `generate.py` is the thin disk/CLI edge. No change to any existing module.

**Tech Stack:** Python 3.12, stdlib only (`random`, `dataclasses`, `argparse`, `json`, `unicodedata`). Tests with `pytest`. DB-gated volume test uses the existing `pipeline` extra (psycopg) + `CAIRN_TEST_PG`.

## Global Constraints

- **AGPL-3.0**; **no new dependency** — pure core is stdlib-only; the DB-gated test reuses the existing `pipeline` extra. (copy verbatim from spec)
- **Advisory tier**: no `db/` floor file, no SCHEMA bump, no spec/ADR change (extends the B3 harness under settled §5.2/§5.13/ADR-0014).
- **TDD**: failing test first, then minimal code. Run the pure suite with `uv run pytest` (never venv/pip).
- **Purity**: `generator.py` imports **no** psycopg and no I/O; the disk edge lives only in `generate.py`.
- **Dataset dict shape** each record is (all fields except `record_id` optional):
  ```python
  {"record_id": str,
   "dob": {"value": "YYYY-MM-DD", "precision": "day"|"month"|"year", "provenance_rank": int},
   "sex_at_birth": {"value": str, "provenance_rank": int},
   "names": [{"value": str, "provenance_rank": int}, ...],
   "identifiers": [{"system": str, "match_key": str, "value": str}, ...]}
  ```
- **Cluster size fixed at 2** (seed + one clone) this slice — variable size is deferred.
- **Recoverability invariant** — every `seed↔clone` pair shares ≥ 1 of the three base blocking keys (identifier / exact-DOB / name-token); `name+year` is subsumed by name-token.
- Run commands from `matcher/`. DB-gated: `CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" uv run --extra pipeline pytest`.

---

### Task 1: The recoverability predicate (`shares_blocking_key`)

The load-bearing invariant primitive — a pure function mirroring the three base blocking passes in `pipeline/db.py`'s `_GROUPS_SQL`. Build it first; the generator (Task 4) and its invariant test depend on it.

**Files:**
- Create: `matcher/src/cairn_matcher/eval/generator.py`
- Test: `matcher/tests/test_eval_generator.py`

**Interfaces:**
- Consumes: nothing (stdlib only).
- Produces: `name_tokens(record: dict) -> set[str]`, `shares_blocking_key(a: dict, b: dict) -> bool`.

- [ ] **Step 1: Write the failing test**

```python
# matcher/tests/test_eval_generator.py
"""Tests for the synthetic blocking-eval dataset generator (pure, stdlib-only)."""

from cairn_matcher.eval.generator import name_tokens, shares_blocking_key


def test_name_tokens_lowercases_and_splits_all_names():
    rec = {"names": [{"value": "Alex Nguyen"}, {"value": "NGUYEN Van Alex"}]}
    assert name_tokens(rec) == {"alex", "nguyen", "van"}


def test_name_tokens_empty_when_no_names():
    assert name_tokens({"record_id": "r"}) == set()


def test_shares_key_via_exact_dob():
    a = {"dob": {"value": "1990-05-12"}, "names": [{"value": "Ann"}]}
    b = {"dob": {"value": "1990-05-12"}, "names": [{"value": "Bob"}]}
    assert shares_blocking_key(a, b) is True


def test_shares_key_via_name_token():
    a = {"dob": {"value": "1990-05-12"}, "names": [{"value": "Alex Nguyen"}]}
    b = {"dob": {"value": "1985-01-01"}, "names": [{"value": "Sam Nguyen"}]}
    assert shares_blocking_key(a, b) is True


def test_shares_key_via_identifier_but_not_unknown():
    a = {"identifiers": [{"system": "au-medicare", "match_key": "111"}]}
    b = {"identifiers": [{"system": "au-medicare", "match_key": "111"}]}
    assert shares_blocking_key(a, b) is True
    a_unk = {"identifiers": [{"system": "unknown", "match_key": "111"}]}
    b_unk = {"identifiers": [{"system": "unknown", "match_key": "111"}]}
    assert shares_blocking_key(a_unk, b_unk) is False


def test_no_shared_key_is_false():
    a = {"dob": {"value": "1990-05-12"}, "names": [{"value": "Alex Nguyen"}],
         "identifiers": [{"system": "au-medicare", "match_key": "111"}]}
    b = {"dob": {"value": "12/05/1990"}, "names": [{"value": "Sam Smith"}],
         "identifiers": [{"system": "au-medicare", "match_key": "222"}]}
    assert shares_blocking_key(a, b) is False
```

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/test_eval_generator.py -q`
Expected: FAIL — `ModuleNotFoundError: No module named 'cairn_matcher.eval.generator'`.

- [ ] **Step 3: Write minimal implementation**

```python
# matcher/src/cairn_matcher/eval/generator.py
"""Synthetic blocking-eval dataset generator (pure, stdlib-only).

Emits the eval dataset dict shape (see dataset.py) at volume: clean seed identities
plus one corrupted near-duplicate ("clone") per person. Ground truth is the entity
grouping, so no pair-labelling is needed. Deterministic given a seed.

This module is PURE: stdlib random/dataclasses/unicodedata only, no I/O, no psycopg.
The disk/CLI edge lives in generate.py (the dataset.py <-> loader.py split).
"""

from collections.abc import Mapping, Sequence


def name_tokens(record: Mapping) -> set[str]:
    """Lower-cased whitespace tokens across ALL of a record's names.

    Mirrors the SQL 'name' blocking pass (lower(value) split on whitespace) so this
    predicate agrees with what generate_candidate_pairs actually blocks on.
    """
    tokens: set[str] = set()
    for n in record.get("names", ()):
        tokens.update(str(n["value"]).lower().split())
    return tokens


def _identifier_keys(record: Mapping) -> set[tuple[str, str]]:
    """(system, match_key) pairs excluding the 'unknown' sentinel — the identifier pass."""
    return {
        (i["system"], i["match_key"])
        for i in record.get("identifiers", ())
        if i["system"] != "unknown"
    }


def shares_blocking_key(a: Mapping, b: Mapping) -> bool:
    """True iff records a and b would co-occur in >=1 base blocking pass.

    The three BASE keys (pipeline/db.py _GROUPS_SQL): shared non-unknown identifier,
    equal exact-DOB value, or a shared name token. The fourth pass 'name+year' is
    subsumed by the name-token check (it requires a shared token), so it is not tested
    separately: if name tokens intersect, the plain 'name' pass already groups them.
    """
    if _identifier_keys(a) & _identifier_keys(b):
        return True
    da, db_ = a.get("dob"), b.get("dob")
    if da and db_ and da.get("value") is not None and da.get("value") == db_.get("value"):
        return True
    return bool(name_tokens(a) & name_tokens(b))
```

- [ ] **Step 4: Run test to verify it passes**

Run: `uv run pytest tests/test_eval_generator.py -q`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/eval/generator.py matcher/tests/test_eval_generator.py
git commit -m "feat(matcher): recoverability predicate for synthetic eval generator (B3)"
```

---

### Task 2: Corruption operators

Four pure `(record, rng) -> record` functions, one per selected family. Each returns a NEW record (never mutates input). They operate on the dataset dict shape.

**Files:**
- Modify: `matcher/src/cairn_matcher/eval/generator.py`
- Test: `matcher/tests/test_eval_generator.py`

**Interfaces:**
- Consumes: `name_tokens` (Task 1).
- Produces:
  - `corrupt_dob_format(record: dict, rng) -> dict`
  - `corrupt_dob_typo(record: dict, rng) -> dict`
  - `corrupt_name(record: dict, rng) -> dict`
  - `corrupt_identifier(record: dict, rng) -> dict`

- [ ] **Step 1: Write the failing test**

```python
# append to matcher/tests/test_eval_generator.py
import copy
import random

from cairn_matcher.eval.generator import (
    corrupt_dob_format, corrupt_dob_typo, corrupt_name, corrupt_identifier,
)


def _seed_rec():
    return {
        "record_id": "e0-seed",
        "dob": {"value": "1990-05-12", "precision": "day", "provenance_rank": 40},
        "names": [{"value": "Alex Nguyen", "provenance_rank": 30}],
        "identifiers": [{"system": "au-medicare", "match_key": "12345", "value": "12345"}],
    }


def test_dob_format_keeps_birth_year_changes_value():
    rec = _seed_rec()
    before = copy.deepcopy(rec)
    out = corrupt_dob_format(rec, random.Random(1))
    assert rec == before                       # input unmutated (pure)
    assert out["dob"]["value"] != "1990-05-12" # exact value changed
    assert "1990" in out["dob"]["value"]       # birth-year preserved


def test_dob_typo_changes_value():
    out = corrupt_dob_typo(_seed_rec(), random.Random(2))
    assert out["dob"]["value"] != "1990-05-12"


def test_name_corruption_changes_a_name_value():
    out = corrupt_name(_seed_rec(), random.Random(3))
    assert [n["value"] for n in out["names"]] != ["Alex Nguyen"]


def test_identifier_corruption_drops_or_mistypes():
    out = corrupt_identifier(_seed_rec(), random.Random(4))
    ids = out["identifiers"]
    # either dropped (fewer) or the match_key changed
    assert ids == [] or ids[0]["match_key"] != "12345"


def test_operators_are_noops_when_field_absent():
    bare = {"record_id": "x", "names": [{"value": "Sam"}]}
    r = random.Random(5)
    assert corrupt_dob_format(bare, r) == bare
    assert corrupt_dob_typo(bare, r) == bare
    assert corrupt_identifier(bare, r) == bare
```

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/test_eval_generator.py -q`
Expected: FAIL — `ImportError: cannot import name 'corrupt_dob_format'`.

- [ ] **Step 3: Write minimal implementation**

```python
# append to matcher/src/cairn_matcher/eval/generator.py
import copy
import unicodedata


def _clone(record):
    """A deep copy so an operator can never mutate its input (pure discipline)."""
    return copy.deepcopy(dict(record))


def corrupt_dob_format(record, rng):
    """Re-express the same birth-year in a different exact form: day-first restring
    ("1990-05-12" -> "12/05/1990") or precision downgrade to year-only ("1990").

    Exact-DOB blocking then MISSES the pair while name+year still CATCHES it. No-op if
    the record has no ISO 'YYYY-MM-DD' dob value (safe degrade).
    """
    out = _clone(record)
    dob = out.get("dob")
    if not dob or not isinstance(dob.get("value"), str):
        return out
    parts = dob["value"].split("-")
    if len(parts) != 3:
        return out  # not full ISO -> leave it
    y, m, d = parts
    if rng.random() < 0.5:
        dob["value"] = f"{d}/{m}/{y}"          # day-first re-import; year still present
    else:
        dob["value"] = y                        # precision downgrade
        dob["precision"] = "year"
    return out


def _perturb_digit(text, rng):
    """Transpose two adjacent digits, or bump one digit by 1 (mod 10). Pure given rng."""
    positions = [i for i, c in enumerate(text) if c.isdigit()]
    if not positions:
        return text
    chars = list(text)
    adj = [i for i in positions if i + 1 in positions]
    if adj and rng.random() < 0.5:
        i = rng.choice(adj)
        chars[i], chars[i + 1] = chars[i + 1], chars[i]
    else:
        i = rng.choice(positions)
        chars[i] = str((int(chars[i]) + 1) % 10)
    return "".join(chars)


def corrupt_dob_typo(record, rng):
    """Fat-finger the DOB: transpose or bump a digit. May change the birth-year (then the
    pair honestly degrades off name+year; another key must carry it). No-op if no dob."""
    out = _clone(record)
    dob = out.get("dob")
    if not dob or not isinstance(dob.get("value"), str):
        return out
    dob["value"] = _perturb_digit(dob["value"], rng)
    return out


def _strip_diacritics(text):
    """NFD-decompose and drop combining marks: 'Jón' -> 'Jon'. Culture-neutral."""
    return "".join(c for c in unicodedata.normalize("NFD", text)
                   if not unicodedata.combining(c))


def corrupt_name(record, rng):
    """Corrupt ONE of the record's names: strip diacritics, transpose two letters, or drop
    a token (when the name has >1 token). Breaks the exact shared-name-token block for the
    affected token. No-op if the record has no names."""
    out = _clone(record)
    names = out.get("names", [])
    if not names:
        return out
    idx = rng.randrange(len(names))
    value = str(names[idx]["value"])
    mode = rng.choice(("diacritic", "transpose", "drop"))
    if mode == "diacritic":
        value = _strip_diacritics(value)
    elif mode == "transpose" and len(value) >= 2:
        i = rng.randrange(len(value) - 1)
        chars = list(value)
        chars[i], chars[i + 1] = chars[i + 1], chars[i]
        value = "".join(chars)
    else:  # drop a token when possible, else fall back to transpose handled above
        tokens = value.split()
        if len(tokens) > 1:
            del tokens[rng.randrange(len(tokens))]
            value = " ".join(tokens)
    names[idx] = {**names[idx], "value": value}
    return out


def corrupt_identifier(record, rng):
    """Drop the shared identifier, or mistype its match_key/value. Identifier blocking then
    misses; the pair must fall through to DOB/name. No-op if the record has no identifiers."""
    out = _clone(record)
    ids = out.get("identifiers", [])
    if not ids:
        return out
    idx = rng.randrange(len(ids))
    if rng.random() < 0.5:
        del ids[idx]                            # drop it entirely
    else:
        mistyped = _perturb_digit(str(ids[idx]["match_key"]), rng)
        ids[idx] = {**ids[idx], "match_key": mistyped, "value": mistyped}
    return out
```

- [ ] **Step 4: Run test to verify it passes**

Run: `uv run pytest tests/test_eval_generator.py -q`
Expected: PASS (all Task 1 + Task 2 tests).

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/eval/generator.py matcher/tests/test_eval_generator.py
git commit -m "feat(matcher): four corruption operators for synthetic eval generator (B3)"
```

---

### Task 3: Base-identity synthesis

Curated culture-plural name pools + a pure seed-record synthesizer. No external dep; deterministic given the passed rng.

**Files:**
- Modify: `matcher/src/cairn_matcher/eval/generator.py`
- Test: `matcher/tests/test_eval_generator.py`

**Interfaces:**
- Consumes: nothing new.
- Produces: `synth_seed(rng, index: int) -> dict` (a clean dataset record with a non-empty `record_id`, ≥1 name, and an ISO `dob`; sometimes an identifier and `sex_at_birth`).

- [ ] **Step 1: Write the failing test**

```python
# append to matcher/tests/test_eval_generator.py
from cairn_matcher.eval.generator import synth_seed


def test_synth_seed_is_deterministic_for_same_rng_stream():
    a = synth_seed(random.Random(7), 0)
    b = synth_seed(random.Random(7), 0)
    assert a == b


def test_synth_seed_has_required_shape():
    rec = synth_seed(random.Random(8), 3)
    assert rec["record_id"] == "e3-seed"
    assert rec["names"] and rec["names"][0]["value"].strip()
    assert rec["dob"]["value"].count("-") == 2          # full ISO
    assert rec["dob"]["precision"] == "day"


def test_synth_seed_spans_multiple_name_shapes_across_indices():
    shapes = {len(synth_seed(random.Random(i), i)["names"][0]["value"].split())
              for i in range(40)}
    assert 1 in shapes            # at least one mononym
    assert any(s >= 2 for s in shapes)   # and multi-token names
```

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/test_eval_generator.py -q`
Expected: FAIL — `ImportError: cannot import name 'synth_seed'`.

- [ ] **Step 3: Write minimal implementation**

```python
# append to matcher/src/cairn_matcher/eval/generator.py

# Curated, culture-plural pools. Deliberately small and hand-written (no faker: a dep
# and Western bias would both violate the mission). Blocking keys on tokens/years, not
# name rarity, so a small pool is sufficient and makes tokens recur (realistic collisions).
_MONONYMS = ("Suharto", "Sukarno", "Madonna", "Ronaldinho", "Teresa")
_GIVEN = ("Alex", "Sam", "Mira", "Jon", "Ana", "Wei", "Omar", "Fatima", "Ivan", "Lena")
_FAMILY = ("Nguyen", "Einarsson", "Garcia", "Okafor", "Kowalski", "Haddad", "Silva", "Ali")
_PATRONYMIC = (("Jón", "Einarsson"), ("Ólafur", "Bjarnason"), ("Freyr", "Þórsson"))
_ID_SYSTEMS = ("au-medicare", "national-id", "kennitala", "mrn-local")


def _synth_name(rng):
    """Draw one display name across three culture shapes: mononym, patronymic+diacritic,
    or multi-token given+family. Returns the display string."""
    shape = rng.choice(("mono", "patronymic", "given_family"))
    if shape == "mono":
        return rng.choice(_MONONYMS)
    if shape == "patronymic":
        g, p = rng.choice(_PATRONYMIC)
        return f"{g} {p}"
    return f"{rng.choice(_GIVEN)} {rng.choice(_FAMILY)}"


def _synth_dob(rng):
    """A plausible ISO 'YYYY-MM-DD' at day precision."""
    year = rng.randint(1935, 2015)
    month = rng.randint(1, 12)
    day = rng.randint(1, 28)   # 28 avoids month-length edge cases (not needed for blocking)
    return {"value": f"{year:04d}-{month:02d}-{day:02d}", "precision": "day",
            "provenance_rank": rng.choice((20, 30, 40))}


def synth_seed(rng, index):
    """Build one clean seed record for entity `index`. Always has a name and an ISO dob;
    ~70% carry an identifier, ~50% a sex_at_birth (both inert for blocking but realistic)."""
    rec = {
        "record_id": f"e{index}-seed",
        "dob": _synth_dob(rng),
        "names": [{"value": _synth_name(rng), "provenance_rank": rng.choice((20, 30))}],
    }
    if rng.random() < 0.7:
        key = f"{rng.randint(10000, 99999)}"
        rec["identifiers"] = [{"system": rng.choice(_ID_SYSTEMS),
                               "match_key": key, "value": key}]
    if rng.random() < 0.5:
        rec["sex_at_birth"] = {"value": rng.choice(("male", "female")),
                               "provenance_rank": 40}
    return rec
```

- [ ] **Step 4: Run test to verify it passes**

Run: `uv run pytest tests/test_eval_generator.py -q`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/eval/generator.py matcher/tests/test_eval_generator.py
git commit -m "feat(matcher): culture-plural base-identity synthesis for eval generator (B3)"
```

---

### Task 4: `generate_dataset` + clone construction with repair

Tie it together: per entity, synth a seed and one corrupted clone; repair the clone if corruptions destroyed every base key; group under one `entity_id`. Returns the dataset dict.

**Files:**
- Modify: `matcher/src/cairn_matcher/eval/generator.py`
- Test: `matcher/tests/test_eval_generator.py`

**Interfaces:**
- Consumes: `synth_seed`, the four `corrupt_*`, `shares_blocking_key` (Tasks 1–3).
- Produces: `GenSpec` (frozen dataclass) and `generate_dataset(spec: GenSpec) -> dict`.

- [ ] **Step 1: Write the failing test**

```python
# append to matcher/tests/test_eval_generator.py
import itertools

from cairn_matcher.eval.generator import GenSpec, generate_dataset
from cairn_matcher.eval.dataset import load_dataset, truth_pairs


def test_generate_is_deterministic_for_same_seed():
    spec = GenSpec(seed=42, n_entities=25)
    assert generate_dataset(spec) == generate_dataset(spec)


def test_generate_differs_for_different_seed():
    assert generate_dataset(GenSpec(seed=1, n_entities=25)) != \
           generate_dataset(GenSpec(seed=2, n_entities=25))


def test_output_round_trips_through_real_loader():
    ds = load_dataset(generate_dataset(GenSpec(seed=3, n_entities=30)))
    assert len(ds.entities) == 30
    assert all(len(e.records) == 2 for e in ds.entities)   # seed + one clone
    ids = [r.record_id for e in ds.entities for r in e.records]
    assert len(ids) == len(set(ids))                       # unique record_ids
    assert len(truth_pairs(ds)) == 30                      # one true pair per entity


def test_every_true_pair_shares_a_blocking_key():
    # The recoverability invariant: every within-cluster (seed, clone) pair is blockable.
    ds_dict = generate_dataset(GenSpec(seed=4, n_entities=50))
    for ent in ds_dict["entities"]:
        for a, b in itertools.combinations(ent["records"], 2):
            assert shares_blocking_key(a, b), f"unrecoverable pair in {ent['entity_id']}"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/test_eval_generator.py -q`
Expected: FAIL — `ImportError: cannot import name 'GenSpec'`.

- [ ] **Step 3: Write minimal implementation**

```python
# append to matcher/src/cairn_matcher/eval/generator.py
import random
from dataclasses import dataclass


@dataclass(frozen=True)
class GenSpec:
    """Knobs for one synthetic dataset. Deterministic: (seed, fields) reproduce byte-for-byte.

    Cluster size is fixed at 2 (seed + one clone) this slice, so each entity yields exactly
    one seed<->clone true pair and the recoverability invariant is exactly the all-pairs one.
    """
    seed: int = 0
    n_entities: int = 100
    p_dob_format: float = 0.45
    p_dob_typo: float = 0.2
    p_name: float = 0.5
    p_identifier: float = 0.5


_OPERATORS = (
    ("p_dob_format", corrupt_dob_format),
    ("p_dob_typo", corrupt_dob_typo),
    ("p_name", corrupt_name),
    ("p_identifier", corrupt_identifier),
)


def _repair(seed, clone):
    """Guarantee the seed<->clone pair stays blockable: if corruptions destroyed every base
    key, append the seed's primary name (verbatim) to the clone's retained names, restoring a
    shared name token. Every seed has >=1 name, so this always succeeds. Pure (returns new)."""
    if shares_blocking_key(seed, clone):
        return clone
    out = _clone(clone)
    out.setdefault("names", [])
    out["names"].append(dict(seed["names"][0]))
    return out


def _make_clone(seed, spec, rng, index):
    """One corrupted near-duplicate of `seed`: apply each enabled operator with its
    probability, then repair to satisfy the recoverability invariant."""
    clone = _clone(seed)
    clone["record_id"] = f"e{index}-dup"
    for prob_field, op in _OPERATORS:
        if rng.random() < getattr(spec, prob_field):
            clone = op(clone, rng)
    return _repair(seed, clone)


def generate_dataset(spec):
    """Build the full dataset dict: n_entities clusters, each a seed + one corrupted clone.

    Returns a JSON-shaped mapping that round-trips through eval.dataset.load_dataset. Ground
    truth is the entity grouping; truth_pairs derives the one true pair per cluster for free.
    """
    rng = random.Random(spec.seed)
    entities = []
    for i in range(spec.n_entities):
        seed = synth_seed(rng, i)
        clone = _make_clone(seed, spec, rng, i)
        entities.append({"entity_id": f"e{i}", "records": [seed, clone]})
    return {
        "name": f"synthetic_s{spec.seed}_n{spec.n_entities}",
        "description": (
            "Synthetic blocking-eval set: seed + one corrupted clone per entity. "
            "Every true pair is recoverable by >=1 base blocking key (by construction); "
            "a regression/tuning instrument, not a statistical accuracy claim."
        ),
        "entities": entities,
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `uv run pytest tests/test_eval_generator.py -q`
Expected: PASS (all generator tests).

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/eval/generator.py matcher/tests/test_eval_generator.py
git commit -m "feat(matcher): generate_dataset with recoverability repair (B3 volume generator)"
```

---

### Task 5: I/O edge — `write_dataset` + `python -m cairn_matcher.eval.generate` CLI

The thin disk/CLI edge. Keeps `generator.py` filesystem-free (the `dataset.py` ↔ `loader.py` split).

**Files:**
- Create: `matcher/src/cairn_matcher/eval/generate.py`
- Test: `matcher/tests/test_eval_generate_cli.py`

**Interfaces:**
- Consumes: `GenSpec`, `generate_dataset` (Task 4); `load_dataset_file` (existing loader) in tests.
- Produces: `write_dataset(path, mapping) -> None`; `main(argv: list[str] | None = None) -> int`.

- [ ] **Step 1: Write the failing test**

```python
# matcher/tests/test_eval_generate_cli.py
"""Tests for the generator CLI edge (python -m cairn_matcher.eval.generate)."""

import json

from cairn_matcher.eval.generate import main
from cairn_matcher.eval.loader import load_dataset_file


def test_cli_writes_a_loadable_dataset_file(tmp_path):
    out = tmp_path / "synthetic.json"
    rc = main(["--entities", "20", "--seed", "9", "--out", str(out)])
    assert rc == 0
    ds = load_dataset_file(out)                 # must parse via the real loader
    assert len(ds.entities) == 20


def test_cli_is_deterministic_for_same_seed(tmp_path):
    a, b = tmp_path / "a.json", tmp_path / "b.json"
    main(["--entities", "15", "--seed", "5", "--out", str(a)])
    main(["--entities", "15", "--seed", "5", "--out", str(b)])
    assert a.read_text() == b.read_text()


def test_cli_writes_to_stdout_when_no_out(capsys):
    rc = main(["--entities", "3", "--seed", "1"])
    assert rc == 0
    payload = json.loads(capsys.readouterr().out)
    assert len(payload["entities"]) == 3
```

- [ ] **Step 2: Run test to verify it fails**

Run: `uv run pytest tests/test_eval_generate_cli.py -q`
Expected: FAIL — `ModuleNotFoundError: No module named 'cairn_matcher.eval.generate'`.

- [ ] **Step 3: Write minimal implementation**

```python
# matcher/src/cairn_matcher/eval/generate.py
"""`python -m cairn_matcher.eval.generate` — emit a synthetic blocking-eval dataset JSON.

The disk/CLI edge for generator.py (which stays pure/filesystem-free). Write to --out, or
stdout if omitted. Feed the result to the existing eval CLI:

    python -m cairn_matcher.eval.generate --entities 200 --seed 1 --out synth.json
    CAIRN_TEST_PG="host=... port=5532 ..." python -m cairn_matcher.eval synth.json
"""

import argparse
import json
import sys

from cairn_matcher.eval.generator import GenSpec, generate_dataset


def write_dataset(path, mapping):
    """Write a dataset mapping to `path` as UTF-8 JSON (non-ASCII preserved for legibility)."""
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(mapping, fh, ensure_ascii=False, indent=2, sort_keys=True)


def main(argv=None):
    """Parse args, generate the dataset, write it. Returns a process exit code."""
    parser = argparse.ArgumentParser(prog="cairn_matcher.eval.generate", description=__doc__)
    parser.add_argument("--entities", type=int, default=200, help="number of entities (true pairs)")
    parser.add_argument("--seed", type=int, default=0, help="PRNG seed (reproducibility)")
    parser.add_argument("--out", help="output path; stdout if omitted")
    args = parser.parse_args(argv)

    dataset = generate_dataset(GenSpec(seed=args.seed, n_entities=args.entities))
    if args.out:
        write_dataset(args.out, dataset)
    else:
        json.dump(dataset, sys.stdout, ensure_ascii=False, indent=2, sort_keys=True)
        sys.stdout.write("\n")
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
```

Note: `write_dataset` and the stdout branch both pass `sort_keys=True`, so byte-for-byte
determinism holds regardless of dict insertion order.

- [ ] **Step 4: Run test to verify it passes**

Run: `uv run pytest tests/test_eval_generate_cli.py -q`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add matcher/src/cairn_matcher/eval/generate.py matcher/tests/test_eval_generate_cli.py
git commit -m "feat(matcher): generate CLI edge for synthetic eval datasets (B3)"
```

---

### Task 6: DB-gated volume sanity test

Prove the generator produces a fully-recoverable-by-construction set at volume: with a large cap (no oversized blocks), the real `evaluate_blocking` returns `pair_completeness == 1.0` and drops no true matches. Guarded by `CAIRN_TEST_PG` (skips on the pure path), reusing the rollback-guarded blocking-eval substrate (leaves no synthetic patients).

**Files:**
- Test: `matcher/tests/test_eval_generator_volume.py`

**Interfaces:**
- Consumes: `GenSpec`/`generate_dataset` (Task 4), `load_dataset` (existing), `evaluate_blocking` (existing), the `pg_conn` fixture (existing `conftest.py`).

- [ ] **Step 1: Confirm the existing DB fixture name**

Run: `grep -n "def pg_conn\|CAIRN_TEST_PG\|skip" matcher/tests/conftest.py`
Expected: a `pg_conn` fixture that skips when `CAIRN_TEST_PG` is unset. Use that fixture name below (adjust if it differs).

- [ ] **Step 2: Write the failing test**

```python
# matcher/tests/test_eval_generator_volume.py
"""DB-gated: a generated volume set is fully recoverable by blocking under a large cap.

Confirms the recoverability invariant end-to-end through the REAL generate_candidate_pairs:
with no block over the cap, blocking recall is total and no true match is dropped. Reuses
evaluate_blocking's rollback discipline, so it leaves no synthetic patients behind.
"""

from cairn_matcher.eval.dataset import load_dataset
from cairn_matcher.eval.generator import GenSpec, generate_dataset
from cairn_matcher.eval.blocking_eval import evaluate_blocking


def test_generated_volume_set_is_fully_recoverable(pg_conn):
    ds = load_dataset(generate_dataset(GenSpec(seed=1, n_entities=200)))
    metrics = evaluate_blocking(pg_conn, ds, max_block_size=10_000)
    assert metrics.pair_completeness == 1.0
    assert metrics.dropped_true_matches == ()
    assert metrics.total_pairs > metrics.generated_pairs   # reduction happened
    assert 0.0 < metrics.reduction_ratio <= 1.0
```

- [ ] **Step 3: Run test to verify it fails (or skips) correctly**

Run (pure path): `uv run pytest tests/test_eval_generator_volume.py -q`
Expected: SKIP (no `CAIRN_TEST_PG`).

Run (DB path, BEFORE any code bug is possible — this validates the generator, not new prod code):
`CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" uv run --extra pipeline pytest tests/test_eval_generator_volume.py -q`
Expected: PASS. (If it FAILS with dropped matches, the recoverability invariant or its predicate is wrong — fix `shares_blocking_key`/`_repair`, not the assertion.)

- [ ] **Step 4: Run the full matcher suite (pure + DB) to confirm no regressions**

Run: `uv run pytest -q` → expect `... passed, N skipped`.
Run: `CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" uv run --extra pipeline pytest -q` → expect all passed.

- [ ] **Step 5: Commit**

```bash
git add matcher/tests/test_eval_generator_volume.py
git commit -m "test(matcher): DB-gated volume-recoverability check for synthetic generator (B3)"
```

---

### Task 7: Docs — README + HANDOVER/ROADMAP currency

Record the new capability where the next contributor will look. (Folded into one task: docs change together and need no separate reviewer gate.)

**Files:**
- Modify: `matcher/README.md`
- Modify: `docs/HANDOVER.md`
- Modify: `docs/ROADMAP.md`

- [ ] **Step 1: Update `matcher/README.md`**

Add a short subsection under the eval-harness docs describing the generator: the two commands
(generate → eval), that it's pure/stdlib/deterministic, cluster size 2, the recoverability
invariant, and the deferred knobs (variable cluster size, unrecoverable fraction, hard negatives,
A/B toggle). Keep it a few sentences — match the existing README's density.

- [ ] **Step 2: Update `docs/HANDOVER.md`**

Replace the "This session" block's forward-looking "synthetic corruption / volume generator
(deferred)" mention with a "built this session" summary: pure `eval/generator.py` +
`eval/generate.py` CLI, four corruption families, recoverability invariant + repair, DB-gated
volume-recoverability test, test counts (`uv run pytest` before/after), advisory-only. Move the
generator OUT of the deferred list in the today's-work menu; note what it now unblocks
(quantitative before/after once an A/B pass-toggle exists — still deferred).

- [ ] **Step 3: Update `docs/ROADMAP.md`**

In the §5.2 matcher line, add the volume generator to the BUILT set; leave weight-learning /
further compound keys / A/B toggle in the next set.

- [ ] **Step 4: Verify docs build (mkdocs) is not broken**

Run: `uv run --with mkdocs-material --with mkdocs-callouts --with mkdocs-redirects -- mkdocs build 2>&1 | tail -5`
Expected: build completes without error (warnings about the `site/` dir are fine; never commit `site/`).

- [ ] **Step 5: Commit**

```bash
git add matcher/README.md docs/HANDOVER.md docs/ROADMAP.md
git commit -m "docs(matcher): record synthetic blocking-eval volume generator (B3)"
```

---

## Final verification (after all tasks)

- [ ] `cd matcher && uv run pytest -q` — pure suite green (new generator + CLI tests pass; DB volume test skips).
- [ ] `cd matcher && CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" uv run --extra pipeline pytest -q` — full suite green incl. the volume test.
- [ ] `cd matcher && python -m cairn_matcher.eval.generate --entities 200 --seed 1 --out /tmp/synth.json && CAIRN_TEST_PG="..." python -m cairn_matcher.eval /tmp/synth.json` — end-to-end: the generated set runs through the real eval, printing a volume blocking report with `pair_completeness` near 1.0.
- [ ] `generator.py` stays < 500 lines and imports no psycopg.
- [ ] Request code review (superpowers:requesting-code-review) before opening the PR.

## Self-Review (author, against the spec)

**Spec coverage:** placement/tier (Tasks 1–5, no db/ or SCHEMA touch ✅); output contract round-tripping through `load_dataset` (Task 4/5 tests ✅); culture-plural zero-dep base identities (Task 3 ✅); four corruption operators (Task 2 ✅); recoverability invariant + repair (Task 1 predicate + Task 4 repair + Task 6 E2E ✅); GenSpec config (Task 4 ✅); CLI (Task 5 ✅); tests incl. determinism/round-trip/operators/invariant/volume (Tasks 1–6 ✅); non-goals untouched (no hard negatives, no A/B toggle, no variable cluster size, no unrecoverable fraction ✅).

**Placeholder scan:** no TBD/TODO; every code step shows complete code; every command shows expected output.

**Type consistency:** `shares_blocking_key`, `name_tokens`, `corrupt_dob_format`/`corrupt_dob_typo`/`corrupt_name`/`corrupt_identifier`, `synth_seed`, `GenSpec`, `generate_dataset`, `write_dataset`, `main` — names used identically across tasks. `GenSpec` field names (`seed`, `n_entities`, `p_dob_format`, `p_dob_typo`, `p_name`, `p_identifier`) match between definition (Task 4) and CLI use (Task 5). Record dict keys match the dataset shape in Global Constraints and `dataset.py`.
