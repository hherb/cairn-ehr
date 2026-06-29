"""parse_dob extracts ISO date fields at the precision the projection declares.

The patient_demographic dob `value` is ISO-8601 by the cairn-event write convention
(`1980-07-15` day, `1980-07` month, `1980` year) and `facets.precision` declares how
precise it is. parse_dob is NOT a locale date parser — it reads the fields ISO already
exposes, gated by the declared precision. Anything it cannot read at that precision
degrades to None (absence is safe; principle 4), never a wrong DateValue.
"""

from cairn_matcher.pipeline.adapter import parse_dob
from cairn_matcher.records import DateValue


def test_day_precision_parses_full_date():
    assert parse_dob("1980-07-15", "day") == DateValue(year=1980, month=7, day=15)


def test_month_precision_parses_year_and_month():
    assert parse_dob("1980-07", "month") == DateValue(year=1980, month=7, day=None)


def test_year_precision_parses_year_only():
    assert parse_dob("1980", "year") == DateValue(year=1980, month=None, day=None)


def test_absent_value_or_precision_is_none():
    assert parse_dob(None, "day") is None
    assert parse_dob("1980-07-15", None) is None


def test_non_iso_value_degrades_to_none():
    # A non-conformant peer wrote a locale string; we never guess. Safe degrade.
    assert parse_dob("15/07/1980", "day") is None
    assert parse_dob("not-a-date", "year") is None


def test_value_too_coarse_for_declared_precision_degrades():
    # precision says day but only a year is present -> cannot honour the claim -> None.
    assert parse_dob("1980", "day") is None


def test_unknown_precision_token_degrades_to_none():
    assert parse_dob("1980-07-15", "hour") is None


def test_out_of_range_month_degrades_to_none():
    # Numeric but not a real month -> a wrong DateValue, so degrade (honour the contract).
    assert parse_dob("1980-13-01", "month") is None
    assert parse_dob("1980-00-01", "month") is None


def test_out_of_range_day_degrades_to_none():
    # Numeric but not a real day-of-month -> degrade rather than emit a wrong DateValue.
    assert parse_dob("1980-07-45", "day") is None
    assert parse_dob("1980-07-00", "day") is None
