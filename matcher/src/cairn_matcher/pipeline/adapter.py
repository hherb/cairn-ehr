"""Pure mappers from a node's patient_* projection rows into B1 CandidateRecords.

No I/O, no psycopg. Callers (pipeline.db) hand these functions plain dict rows; these
functions shape them into the value types B1 scores over. Every field degrades safely
on absence or malformed input (principle 4: absence is never disagreement); a
structurally wrong row raises MatcherTypeError elsewhere in this module (house rule #5).
"""

from cairn_matcher.records import DateValue

# The ISO field counts we can extract per declared precision. precision -> how many of
# (year, month, day) the value must supply. We never parse a locale date string; we only
# read the dash-separated ISO fields the cairn-event writer already emits.
_PRECISION_PARTS = {"year": 1, "month": 2, "day": 3}


def parse_dob(value: str | None, precision: str | None) -> DateValue | None:
    """Extract a DateValue from an ISO dob value at the projection's declared precision.

    Returns None (a safe, gradeable absence) when the value is missing, the precision is
    missing or unknown, or the value is not ISO-shaped to at least the declared precision.
    We never coerce a locale string or guess month/day order — that is a B3/locale-pack
    concern; here, an unreadable value simply has no DOB to compare.
    """
    if not value or precision not in _PRECISION_PARTS:
        return None
    parts = value.split("-")
    needed = _PRECISION_PARTS[precision]
    if len(parts) < needed:
        return None  # value is coarser than the precision it claims
    try:
        nums = [int(p) for p in parts[:needed]]
    except ValueError:
        return None  # non-numeric field -> not ISO -> safe degrade
    year = nums[0]
    month = nums[1] if needed >= 2 else None
    day = nums[2] if needed >= 3 else None
    return DateValue(year=year, month=month, day=day)
