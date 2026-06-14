"""Cairn EHR — offline-first replication & failover proof-of-concept.

A tiny, honest slice of Cairn's architecture: an append-only event log,
Hybrid Logical Clock ordering, and conflict-free set-union sync between two
independent PostgreSQL nodes. See README.md for the demo narrative.
"""

__version__ = "0.1.0"
