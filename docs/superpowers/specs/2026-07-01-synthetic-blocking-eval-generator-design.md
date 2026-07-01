# Design — synthetic blocking-eval dataset generator (§5.2 matcher, B3)

**Date:** 2026-07-01 · **Tier:** advisory (fit-for-purpose, §9) · **Spec/ADR:** no change
(extends the B3 eval harness under settled §5.2/§5.13/[ADR-0014](../../spec/decisions/0014-locale-pluggable-matcher-comparators.md)).

## Problem

The B3 eval harness (`cairn_matcher/eval/`) measures blocking recall (`pair_completeness`,
`reduction_ratio`, dropped-true-matches) — but only against the tiny hand-authored
`gold_v1.json` (3-ish clusters). At that size the numbers cannot show whether a **compound
blocking key** actually earns its keep: the just-merged `name+year` pass, and future keys
(`dob+first-initial`, `name+sex`), need a **volume** labelled set with realistic duplicate
records to produce an interpretable before/after recall curve.

This slice builds a **synthetic corruption / volume generator**: it synthesizes clean seed
identities, then produces corrupted within-cluster near-duplicates at volume, deterministically,
emitting the **existing dataset JSON format** so the existing harness consumes it unchanged and
ground truth comes free from the entity grouping.

## Scope

**In:** blocking-only measurement substrate — entity clusters whose within-cluster records are
corrupted near-duplicates (true matches). Cross-cluster pairs are incidental non-matches; no
engineered collisions.

**Out (non-goals this slice):**

- Hard negatives / scorer precision-recall curves (distinct people colliding on name+DOB).
- An A/B toggle to disable a blocking pass *inside* `generate_candidate_pairs` — git-revert gives
  before/after today; a real `passes=[...]` selector is its own future slice (touches `pipeline/`,
  not `eval/`).
- Deliberately-*unrecoverable* duplicate pairs (modelling the hub-sweep floor) — a deferred knob.
- **Variable cluster size (>2 records/entity)** — this slice fixes each entity at exactly
  *seed + one clone*, so the only within-cluster pair is `seed↔clone` and the recoverability
  invariant below is exactly the all-pairs invariant. With 3+ records, two clones can each be
  recoverable-to-seed yet share no blocking key with *each other*, reintroducing unrecoverable
  pairs; supporting that cleanly is a later knob.
- Volume/perf tuning.

## Approach

Pure-function generator with **curated culture-plural pools + a seeded PRNG**, emitting the
existing dataset JSON.

Rejected alternatives:

- **faker** — adds a runtime dependency *and* carries Western cultural bias; both violate the
  mission (zero-dep pure core; anti-cultural-capture, principle-4 comparators).
- **hypothesis / property-based** — adds a dependency and yields random-per-run data, where we want
  reproducible fixed datasets for stable metrics.

## Architecture

Mirrors the eval package's existing pure-core ↔ I/O-edge split (`dataset.py` ↔ `loader.py`).

| Module | Tier | Responsibility |
|---|---|---|
| `eval/generator.py` | pure (stdlib `random`, `dataclasses` only) | `GenSpec` config + `generate_dataset(spec) -> dict`; the four corruption operators; the recoverability guard. Filesystem-free. |
| `eval/generate.py` | I/O edge | `write_dataset(path, mapping)` + a `python -m cairn_matcher.eval.generate` CLI. The only new module that touches disk. |

No change to `dataset.py`, `loader.py`, `blocking_eval.py`, or the `python -m cairn_matcher.eval`
CLI. No `db/` floor file, no SCHEMA bump, no new dependency.

### Output contract

`generate_dataset(spec: GenSpec) -> dict` returns a JSON-shaped mapping
(`{"name", "description", "entities": [...]}`) that **round-trips through the real
`load_dataset`** — reuse, no parallel schema. Each record uses the projection-shaped field dicts
`DatasetRecord` already documents (`dob`, `sex_at_birth`, `names`, `identifiers`). Records that
describe one synthetic person are grouped under one `entity_id`, so `truth_pairs(ds)` derives the
ground truth with no extra labelling.

### Base identities (culture-plural, zero-dep)

Curated in-module pools spanning the gold set's shape diversity:

- **mononym** (e.g. single-token names),
- **patronymic + diacritic** (given + patronymic, some diacritic-bearing),
- **multi-token given + family**.

A birth-date synthesizer draws ISO dates across a plausible range. Everything is drawn from
`random.Random(spec.seed)`, so a `(seed, spec)` pair reproduces a byte-identical dataset — essential
for stable metrics and for TDD.

### Corruption operators

Four pure `(record, rng) -> record` functions, one per selected family. Each returns a **new**
record (frozen-dataclass discipline); none mutate input.

1. **DOB format / precision** — restring the same birth-year in a different exact form
   (ISO `1990-05-12` ↔ day-first `12/05/1990`), or downgrade precision to year-only (`1990`).
   *Exact-DOB block misses; name+year catches* — the money case for the merged compound key.
2. **DOB digit typo / transpose** — perturb one digit or transpose two. If the perturbation hits the
   year, the pair **honestly degrades** (name+year lost; other keys must carry it).
3. **Name typo / diacritics** — strip a diacritic, transpose two letters, or drop/reorder a token.
   Breaks the exact shared-name-token block for the affected token.
4. **Identifier presence / typo** — drop the shared identifier on the clone, or mistype its
   `match_key`. Shared-identifier block misses; the pair falls through to DOB/name blocks.

### The recoverability invariant (load-bearing correctness property)

For a *blocking-only* number to be interpretable, every within-cluster duplicate must stay
recoverable by **≥ 1 blocking pass** — otherwise it is an impossible pair that depresses recall as
pure noise, and the metric no longer measures blocking-key coverage.

The generator therefore guarantees: the corruptions applied to a clone never simultaneously destroy
**all** of the surviving blocking keys shared with its seed. Concretely, after corrupting a clone, a
pure predicate checks that at least one of the **three base blocking keys** still holds between
clone and seed:

- shared identifier (`(system, match_key)` still equal to the seed's, non-`unknown`),
- exact DOB (`value` string still equal),
- shared name token (token-bag intersection non-empty).

(The fourth pass, `name+year`, is *subsumed* by shared-name-token — it requires a shared token, on
which the plain `name` pass already groups — so it adds nothing to *recoverability*; it adds recall
only under the block-size cap, which is exactly what the volume eval measures.) If none of the three
holds, the generator **repairs** the clone by appending the seed's primary name (verbatim) to its
retained name set, guaranteeing a shared name token. Since every seed carries ≥ 1 name, the repair
always restores recoverability. This is a **pure, testable** property.

A deliberately-unrecoverable fraction (to model the residual the hub duplicate-sweep exists to
catch) is explicitly a **deferred knob**, not this slice — including it now would make the headline
`pair_completeness` number ambiguous.

### Config — `GenSpec`

A frozen dataclass:

- `seed: int` — PRNG seed (reproducibility).
- `n_entities: int` — number of distinct synthetic people (each yields one `seed↔clone` true pair).
- per-family corruption probabilities (`p_dob_format`, `p_dob_typo`, `p_name`, `p_identifier`).
- (cluster size is fixed at 2 this slice — see non-goals.)

Defaults are tuned to produce a **mission-relevant mix**, notably a healthy share of cross-format
DOB duplicates so the merged name+year key is exercised at volume.

### CLI

```
python -m cairn_matcher.eval.generate --entities N --seed S [--out path]
```

Writes the dataset JSON to `--out` (stdout if omitted). Then the **existing** command consumes it:

```
CAIRN_TEST_PG="host=… port=5532 …" python -m cairn_matcher.eval that.json
```

which prints the volume blocking report. Two composable commands; no change to the existing CLI.

## Testing (TDD)

Pure tests (dependency-free, run under `uv run pytest`):

- **Determinism** — same `(seed, spec)` → identical dataset; different seed → different.
- **Round-trip** — `generate_dataset` output loads cleanly via the real `load_dataset`; every
  `record_id` unique across the dataset; entity grouping intact; `truth_pairs` non-empty.
- **Each corruption operator** — pure (input unmutated); changes the intended field; preserves the
  recoverability invariant.
- **Recoverability invariant** — for every within-cluster pair in a generated set, the pure
  predicate confirms ≥ 1 surviving blocking key.
- **Purity probe** — `generator.py` imports no psycopg (extends the existing eval purity probe).

DB-gated test (`pipeline` extra, `CAIRN_TEST_PG`):

- **Volume sanity** — `evaluate_blocking` on a generated ~200-entity set yields
  `pair_completeness ≥ threshold` (recoverable by construction), a reported `reduction_ratio`, and
  0 unexpectedly-dropped true matches. Uses the existing rollback-guarded blocking-eval substrate
  (leaves no synthetic patients).

## House-rules check

- **AGPL / no new dep** — pure core stdlib-only; nothing added. ✅
- **TDD** — red-first tests above drive every unit. ✅
- **Junior-legible docs** — module/function docstrings explain *why* (as the existing eval modules do). ✅
- **Pure, reusable functions** — the generator is a pure function of `(spec)`; operators are pure
  `(record, rng) -> record`. ✅
- **File size** — target < 500 lines; split generator (pure) from generate (I/O) keeps both small. ✅
- **No silent defects** — malformed output would fail the round-trip test loudly, not silently. ✅
