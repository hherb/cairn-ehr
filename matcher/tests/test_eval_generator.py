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
