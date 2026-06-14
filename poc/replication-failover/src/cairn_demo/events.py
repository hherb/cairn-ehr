"""Appending events to a node's immutable log.

Every write in Cairn is an append of a signed, immutable event. Here we keep
the essentials: a globally-unique ``event_id`` (so sync is a safe set-union),
an HLC timestamp drawn from the node's own clock state, and the originating
node id. Corrections never edit; they append a new overlay event.
"""

from __future__ import annotations

import time
import uuid
from typing import Any

import psycopg
from psycopg.types.json import Jsonb

from .hlc import HLC


def _now_ms() -> int:
    return int(time.time() * 1000)


def _next_hlc(conn: psycopg.Connection) -> HLC:
    """Advance and persist the node's HLC for a locally originated event.

    Done inside the caller's transaction with a row lock so concurrent appends
    on the same node still get a strict order.
    """
    row = conn.execute(
        "SELECT hlc_wall, hlc_counter FROM hlc_state WHERE id IS TRUE FOR UPDATE"
    ).fetchone()
    current = HLC(row[0], row[1])
    nxt = current.tick(_now_ms())
    conn.execute(
        "UPDATE hlc_state SET hlc_wall = %s, hlc_counter = %s WHERE id IS TRUE",
        (nxt.wall, nxt.counter),
    )
    return nxt


def append_event(
    conn: psycopg.Connection,
    *,
    patient_id: uuid.UUID,
    event_type: str,
    payload: dict[str, Any],
    node_origin: str,
) -> dict[str, Any]:
    """Append one immutable event to this node's log. Returns the stored row."""
    event_id = uuid.uuid4()
    with conn.transaction():
        hlc = _next_hlc(conn)
        conn.execute(
            """
            INSERT INTO event_log
                (event_id, patient_id, event_type, payload,
                 hlc_wall, hlc_counter, node_origin)
            VALUES (%s, %s, %s, %s, %s, %s, %s)
            """,
            (event_id, patient_id, event_type, Jsonb(payload),
             hlc.wall, hlc.counter, node_origin),
        )
    return {
        "event_id": event_id,
        "patient_id": patient_id,
        "event_type": event_type,
        "payload": payload,
        "hlc_wall": hlc.wall,
        "hlc_counter": hlc.counter,
        "node_origin": node_origin,
    }


def create_patient(
    conn: psycopg.Connection,
    *,
    node_origin: str,
    name: str,
    dob: str | None = None,
    sex: str | None = None,
) -> uuid.UUID:
    """Register a new patient (an immortal patient UUID) via a created event."""
    patient_id = uuid.uuid4()
    append_event(
        conn,
        patient_id=patient_id,
        event_type="patient.created",
        payload={"name": name, "dob": dob, "sex": sex},
        node_origin=node_origin,
    )
    return patient_id


def amend_patient(
    conn: psycopg.Connection,
    *,
    node_origin: str,
    patient_id: uuid.UUID,
    **fields: Any,
) -> None:
    """Overlay a demographics correction. Never edits the original event."""
    append_event(
        conn,
        patient_id=patient_id,
        event_type="patient.amended",
        payload=fields,
        node_origin=node_origin,
    )


def add_note(
    conn: psycopg.Connection,
    *,
    node_origin: str,
    patient_id: uuid.UUID,
    text: str,
) -> uuid.UUID:
    """Append a free-text clinical note — the atomic health-record component."""
    return append_event(
        conn,
        patient_id=patient_id,
        event_type="note.added",
        payload={"text": text},
        node_origin=node_origin,
    )["event_id"]
