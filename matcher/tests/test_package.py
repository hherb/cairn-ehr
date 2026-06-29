"""Smoke test: the package imports and exposes a version. Proves the uv project runs."""

import cairn_matcher


def test_package_exposes_version():
    assert isinstance(cairn_matcher.__version__, str)
    assert cairn_matcher.__version__
