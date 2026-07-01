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
