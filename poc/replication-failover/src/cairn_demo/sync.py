"""The sync engine — conflict-free convergence between two nodes.

Because the event log is append-only and every event has a globally-unique id,
synchronising two nodes is a **set union**, not a merge:

    1. Find events node A has that node B lacks, and vice versa.
    2. Copy each missing event to the other node (INSERT ... ON CONFLICT DO
       NOTHING — re-running sync is always safe and idempotent).
    3. Advance each receiving node's Hybrid Logical Clock past what it absorbed.

There is no "last write wins", no field-level merge, and nothing to overwrite,
so there is no possible conflict and no data loss — that is the whole point of
the append-only design. Two nodes that wrote independently while partitioned
end up holding the identical set of events, ordered identically by HLC.
"""

from __future__ import annotations

import time
from dataclasses import dataclass, field

import psycopg
from psycopg.types.json import Jsonb

from .hlc import HLC
from .projections import event_ids


@dataclass
class SyncResult:
    a_to_b: int = 0          # events copied from A into B
    b_to_a: int = 0          # events copied from B into A
    converged: bool = False  # do both nodes now hold the identical event set?
    errors: list[str] = field(default_factory=list)

    @property
    def total_copied(self) -> int:
        return self.a_to_b + self.b_to_a


def _fetch_events(conn: psycopg.Connection, ids: set[str]) -> list[tuple]:
    if not ids:
        return []
    return conn.execute(
        """
        SELECT event_id, patient_id, event_type, payload,
               hlc_wall, hlc_counter, node_origin
        FROM event_log
        WHERE event_id = ANY(%s)
        """,
        (list(ids),),
    ).fetchall()


def _apply_events(conn: psycopg.Connection, rows: list[tuple]) -> int:
    """Insert received events (idempotent) and advance the local HLC past them."""
    if not rows:
        return 0
    with conn.transaction():
        copied = 0
        max_remote = HLC(0, 0)
        for r in rows:
            # r[3] is the payload, which psycopg decoded from jsonb into a dict;
            # wrap it back in Jsonb so it re-adapts on insert.
            params = (r[0], r[1], r[2], Jsonb(r[3]), r[4], r[5], r[6])
            cur = conn.execute(
                """
                INSERT INTO event_log
                    (event_id, patient_id, event_type, payload,
                     hlc_wall, hlc_counter, node_origin)
                VALUES (%s, %s, %s, %s, %s, %s, %s)
                ON CONFLICT (event_id) DO NOTHING
                """,
                params,
            )
            copied += cur.rowcount
            rh = HLC(r[4], r[5])
            if max_remote < rh:
                max_remote = rh
        # Advance our clock so any future local event sorts after what we just
        # absorbed — the HLC "receive" rule, applied once to the batch maximum.
        row = conn.execute(
            "SELECT hlc_wall, hlc_counter FROM hlc_state WHERE id IS TRUE FOR UPDATE"
        ).fetchone()
        merged = HLC(row[0], row[1]).merge(max_remote, now_ms=int(time.time() * 1000))
        conn.execute(
            "UPDATE hlc_state SET hlc_wall = %s, hlc_counter = %s WHERE id IS TRUE",
            (merged.wall, merged.counter),
        )
    return copied


def sync_pair(conn_a: psycopg.Connection, conn_b: psycopg.Connection) -> SyncResult:
    """Synchronise two live node connections by set-union. Idempotent."""
    ids_a = event_ids(conn_a)
    ids_b = event_ids(conn_b)

    missing_on_b = ids_a - ids_b
    missing_on_a = ids_b - ids_a

    result = SyncResult()
    result.a_to_b = _apply_events(conn_b, _fetch_events(conn_a, missing_on_b))
    result.b_to_a = _apply_events(conn_a, _fetch_events(conn_b, missing_on_a))

    # Confirm convergence by re-reading both sides.
    result.converged = event_ids(conn_a) == event_ids(conn_b)
    return result
