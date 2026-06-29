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
