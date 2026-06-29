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
