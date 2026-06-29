# cairn-matcher

The Cairn advisory patient-matcher's **pure scoring core** (piece B1 of the §5.2
matching pipeline). Comparator API contract + culture-neutral comparators + a
Fellegi–Sunter combiner producing an explainable `MatchScore`.

**This is advisory** (fit-for-purpose, §9). It owns no thresholds, no band
classification, no veto logic (that is the in-DB floor, `db/016`), and no link
decision. It only *scores*.

**Pure functions only** — no Postgres, no I/O. Inputs are plain dataclasses. The DB
adapter and the veto-gate call now live in the `pipeline/` sub-package (piece B2, below);
blocking (candidate-pair generation) and locale comparator packs are still later slices
(B2b/B3). See `docs/superpowers/specs/2026-06-29-matcher-scoring-core-design.md`.

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
  `CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=<your-pg-user> dbname=cairn_test" uv run --extra pipeline pytest`

## Develop

```bash
cd matcher
uv run pytest
```
