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
