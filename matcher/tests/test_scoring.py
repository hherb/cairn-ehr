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
