import pytest

from cairn_matcher.comparators import jaro_winkler


def approx(x):
    return pytest.approx(x, abs=1e-3)


def test_identical_strings_are_one():
    assert jaro_winkler("martha", "martha") == 1.0


def test_two_empty_strings_are_one_one_empty_is_zero():
    assert jaro_winkler("", "") == 1.0
    assert jaro_winkler("abc", "") == 0.0
    assert jaro_winkler("", "abc") == 0.0


def test_known_reference_values():
    # Published Jaro–Winkler reference pairs (prefix scale 0.1).
    assert jaro_winkler("martha", "marhta") == approx(0.961)
    assert jaro_winkler("dwayne", "duane") == approx(0.840)
    assert jaro_winkler("dixon", "dicksonx") == approx(0.813)


def test_is_symmetric():
    assert jaro_winkler("dwayne", "duane") == jaro_winkler("duane", "dwayne")
