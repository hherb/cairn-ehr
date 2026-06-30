"""`python -m cairn_matcher.eval [dataset.json]` — print the matcher eval report.

Runs the pure scorer eval always. If CAIRN_TEST_PG is set, ALSO runs the DB-gated
blocking eval (imported lazily so a pure run never needs psycopg) and appends its report.
"""

import argparse
import os
import sys

from cairn_matcher.eval.dataset import DatasetError
from cairn_matcher.eval.loader import load_bundled_gold, load_dataset_file
from cairn_matcher.eval.report import format_scorer
from cairn_matcher.eval.scorer_eval import evaluate_scorer


def main(argv: list[str] | None = None) -> int:
    """Parse args, run the eval(s), print the report. Returns a process exit code.

    If no dataset path is given, loads the bundled gold_v1 set. If CAIRN_TEST_PG is
    set in the environment, lazily imports psycopg and the blocking_eval module and
    appends a blocking report; otherwise, only the pure scorer eval is run.
    """
    parser = argparse.ArgumentParser(prog="cairn_matcher.eval", description=__doc__)
    parser.add_argument(
        "dataset", nargs="?",
        help="path to a dataset JSON file; default: the bundled gold_v1 set",
    )
    parser.add_argument(
        "--max-block-size", type=int, default=100,
        help="blocking cap (only used when CAIRN_TEST_PG is set)",
    )
    args = parser.parse_args(argv)

    try:
        ds = load_dataset_file(args.dataset) if args.dataset else load_bundled_gold()
    except (DatasetError, OSError, ValueError) as exc:
        print(f"error: could not load dataset: {exc}", file=sys.stderr)
        return 2

    print(format_scorer(evaluate_scorer(ds), dataset_name=ds.name))

    dsn = os.environ.get("CAIRN_TEST_PG")
    if dsn:
        # Lazy import: psycopg + the blocking layer are only touched when a DB is offered.
        import psycopg

        from cairn_matcher.eval.blocking_eval import evaluate_blocking
        from cairn_matcher.eval.report import format_blocking

        with psycopg.connect(dsn, autocommit=False) as conn:
            metrics = evaluate_blocking(conn, ds, max_block_size=args.max_block_size)
        print()
        print(format_blocking(metrics))

    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
