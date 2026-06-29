"""canonical_pair orders a patient-id pair the same way Postgres orders the uuid columns.

The match_proposal CHECK (patient_low < patient_high) compares normalized `uuid` VALUES,
not their text form. Lower/mixed-case input text-sorts differently from the uuid value
ordering, so a naive string compare can flip the pair (CHECK violation or a duplicate
mirror row). canonical_pair compares uuid.UUID objects (128-bit integer order = Postgres
byte order) and emits the lowercase canonical text. Pure — no database.
"""

import uuid

from cairn_matcher.pipeline.runner import canonical_pair


def test_canonical_pair_orders_by_uuid_value_not_text():
    # 'F...' text-sorts BEFORE lowercase 'a...' (0x46 < 0x61), but as uuid values F > a.
    a = "FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF"
    b = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
    low, high = canonical_pair(a, b)
    assert (low, high) == (b.lower(), a.lower())


def test_canonical_pair_normalizes_input_case_to_lowercase():
    low, high = canonical_pair(
        "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA",
        "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
    )
    assert low == "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
    assert high == "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"


def test_canonical_pair_is_symmetric():
    a = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
    b = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"
    assert canonical_pair(a, b) == canonical_pair(b, a) == (a, b)


def test_canonical_pair_accepts_uuid_objects():
    a = uuid.UUID("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
    b = uuid.UUID("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb")
    assert canonical_pair(b, a) == (str(a), str(b))
