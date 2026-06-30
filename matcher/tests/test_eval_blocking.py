"""DB-gated tests for the blocking eval (pair-completeness / reduction-ratio).

Gated on CAIRN_TEST_PG via the shared pg_conn fixture (skipped cleanly without a DB).
"""

from cairn_matcher.eval.blocking_eval import evaluate_blocking
from cairn_matcher.eval.dataset import load_dataset
from cairn_matcher.eval.loader import load_bundled_gold


def test_gold_blocking_recall_is_total(pg_conn):
    # Every true-match pair in gold_v1 shares an identifier or a name token AND a DOB,
    # so blocking must generate all of them: pair_completeness == 1.0, no dropped matches.
    m = evaluate_blocking(pg_conn, load_bundled_gold())
    assert m.pair_completeness == 1.0
    assert m.dropped_true_matches == ()
    assert m.reduction_ratio > 0.0  # blocking generated fewer than all possible pairs


def test_oversized_block_is_skipped_and_estimated(pg_conn):
    # Three records sharing one DOB; cap=2 -> that block (size 3) is skipped, dropping
    # C(3,2)=3 candidate pairs, reported via dropped_pair_estimate.
    ds = load_dataset({"name": "big", "entities": [
        {"entity_id": "e", "records": [
            {"record_id": f"r{i}",
             "dob": {"value": "2000-01-01", "precision": "day", "provenance_rank": 40}}
            for i in range(3)
        ]},
    ]})
    m = evaluate_blocking(pg_conn, ds, max_block_size=2)
    assert any(pn == "dob" and sz == 3 for pn, _key, sz in m.skipped_blocks)
    assert m.dropped_pair_estimate == 3


def test_blocking_eval_is_idempotent_and_leaves_no_rows(pg_conn):
    # Seeding must be ephemeral: the eval rolls back its own seed, so a second run on the
    # same connection (deterministic uuid5 labels) must not hit the patient_demographic
    # PK (patient_id, field), and no synthetic rows may persist afterwards.
    gold = load_bundled_gold()
    first = evaluate_blocking(pg_conn, gold)
    second = evaluate_blocking(pg_conn, gold)  # would raise UniqueViolation if seed committed
    assert first.pair_completeness == second.pair_completeness == 1.0
    with pg_conn.cursor() as cur:
        cur.execute("SELECT count(*) FROM patient_demographic")
        assert cur.fetchone()[0] == 0
