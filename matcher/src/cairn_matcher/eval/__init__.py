"""Cairn matcher eval harness — measurement substrate for the §5.2 advisory matcher.

Pure by default (stdlib only): dataset format, scorer/banding metrics, and a CLI. The
blocking-recall layer (`blocking_eval`) is the one DB-touching module and needs the
optional `pipeline` extra (psycopg). This package ships NO clinical floor and makes NO
link decision — a defect yields a wrong metric a human reads, never record corruption.
"""

from cairn_matcher.eval.dataset import LabelledDataset, load_dataset
from cairn_matcher.eval.loader import load_bundled_gold, load_dataset_file
from cairn_matcher.eval.metrics import ScorerMetrics, scorer_metrics
from cairn_matcher.eval.report import format_scorer
from cairn_matcher.eval.scorer_eval import evaluate_scorer

__all__ = [
    "LabelledDataset",
    "load_dataset",
    "load_dataset_file",
    "load_bundled_gold",
    "ScorerMetrics",
    "scorer_metrics",
    "evaluate_scorer",
    "format_scorer",
]
