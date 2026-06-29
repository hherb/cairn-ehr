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
