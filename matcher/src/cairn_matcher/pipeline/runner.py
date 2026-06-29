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
