"""Tests for the eval CLI. Run via main() in-process (no subprocess needed)."""

import json

from cairn_matcher.eval.__main__ import main


def test_cli_runs_bundled_gold_and_prints_scorer_report(capsys, monkeypatch):
    monkeypatch.delenv("CAIRN_TEST_PG", raising=False)  # force the pure path
    rc = main([])
    out = capsys.readouterr().out
    assert rc == 0
    assert "Scorer eval" in out
    assert "auto_false_link_rate" in out


def test_cli_runs_a_named_dataset_file(tmp_path, capsys, monkeypatch):
    monkeypatch.delenv("CAIRN_TEST_PG", raising=False)
    ds = {"name": "mini", "entities": [
        {"entity_id": "e", "records": [{"record_id": "r1"}, {"record_id": "r2"}]}]}
    p = tmp_path / "mini.json"
    p.write_text(json.dumps(ds), encoding="utf-8")
    rc = main([str(p)])
    out = capsys.readouterr().out
    assert rc == 0
    assert "mini" in out


def test_cli_reports_a_bad_dataset_with_nonzero_exit(tmp_path, capsys, monkeypatch):
    monkeypatch.delenv("CAIRN_TEST_PG", raising=False)
    p = tmp_path / "bad.json"
    p.write_text('{"name": "x"}', encoding="utf-8")  # no 'entities'
    rc = main([str(p)])
    assert rc != 0
    assert "error" in capsys.readouterr().err.lower()
