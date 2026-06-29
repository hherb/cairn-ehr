"""Cairn advisory patient-matcher — pure scoring core (piece B1).

This package is the *advisory* (fit-for-purpose, §9 blast-radius) half of the §5.2
matching pipeline. It turns two already-projected patient records into a match SCORE
with per-field evidence. It is pure: no Postgres, no I/O, no thresholds, no link
decisions. The safety-critical hard-veto floor lives in the database (db/016); the
conservative auto-link threshold and the proposal -> link apply seam are separate
slices. A defect here yields a bad *proposal* a human reviews, never record corruption.
"""

__version__ = "0.1.0"
