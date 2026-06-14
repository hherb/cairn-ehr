"""Demo topology configuration.

The PoC runs in one of two topologies, selected entirely by configuration:

* **Single machine** (default, no config): two nodes A and B on this host at
  127.0.0.1:55432 and 127.0.0.1:55433. Great for development and rehearsal.

* **Two machines** (a ``demo.env`` file, or ``CAIRN_*`` env vars): this host
  runs ONE node (``CAIRN_SELF_NAME``) and reaches the other across the network
  (``CAIRN_PEER_*``). Each machine has its own ``demo.env`` that mirrors the
  other. This is the topology for "pull the network/power cable".

Resolution order for every value: real environment variable > ``demo.env`` in
the project root (or ``CAIRN_DEMO_ENV``) > built-in default. So you can always
override a single value inline without editing the file.
"""

from __future__ import annotations

import os
from pathlib import Path

_PROJECT_ROOT = Path(__file__).resolve().parents[2]


def _load_env_file() -> dict[str, str]:
    """Parse a simple KEY=VALUE file (``#`` comments, blank lines ignored)."""
    path = Path(os.environ.get("CAIRN_DEMO_ENV", _PROJECT_ROOT / "demo.env"))
    values: dict[str, str] = {}
    if not path.exists():
        return values
    for raw in path.read_text().splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, val = line.partition("=")
        # strip optional surrounding quotes and inline trailing comments
        val = val.strip().strip("'\"")
        values[key.strip()] = val
    return values


_FILE = _load_env_file()


def get(key: str, default: str | None = None) -> str | None:
    """Look up a config value: environment first, then demo.env, then default."""
    if key in os.environ:
        return os.environ[key]
    if key in _FILE:
        return _FILE[key]
    return default


def is_networked() -> bool:
    """True when a two-machine topology is configured."""
    return get("CAIRN_SELF_NAME") is not None


def self_name() -> str:
    """The node name this machine should write to by default ('A' or 'B')."""
    return (get("CAIRN_SELF_NAME") or "A").strip().upper()
