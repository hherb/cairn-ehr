"""DB-gated: a generated volume set is fully recoverable by blocking under a large cap.

Confirms the recoverability invariant end-to-end through the REAL generate_candidate_pairs:
with no block over the cap, blocking recall is total and no true match is dropped. Reuses
evaluate_blocking's rollback discipline, so it leaves no synthetic patients behind.
"""

from cairn_matcher.eval.dataset import load_dataset
from cairn_matcher.eval.generator import GenSpec, generate_dataset
from cairn_matcher.eval.blocking_eval import evaluate_blocking


def test_generated_volume_set_is_fully_recoverable(pg_conn):
    ds = load_dataset(generate_dataset(GenSpec(seed=1, n_entities=200)))
    metrics = evaluate_blocking(pg_conn, ds, max_block_size=10_000)
    assert metrics.pair_completeness == 1.0
    assert metrics.dropped_true_matches == ()
    assert metrics.total_pairs > metrics.generated_pairs   # reduction happened
    assert 0.0 < metrics.reduction_ratio <= 1.0
