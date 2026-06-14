"""Reading the derived "current truth" from a node's event log."""

from __future__ import annotations

from typing import Any

import psycopg


def event_ids(conn: psycopg.Connection) -> set[str]:
    """Every event_id this node currently holds."""
    return {str(r[0]) for r in conn.execute("SELECT event_id FROM event_log").fetchall()}


def event_count(conn: psycopg.Connection) -> int:
    return conn.execute("SELECT count(*) FROM event_log").fetchone()[0]


def patients(conn: psycopg.Connection) -> list[dict[str, Any]]:
    rows = conn.execute(
        """
        SELECT patient_id, name, dob, sex, last_event, node_origin
        FROM patient_current
        ORDER BY name
        """
    ).fetchall()
    return [
        {"patient_id": str(r[0]), "name": r[1], "dob": r[2], "sex": r[3],
         "last_event": r[4], "node_origin": r[5]}
        for r in rows
    ]


def notes(conn: psycopg.Connection, patient_id: str | None = None) -> list[dict[str, Any]]:
    sql = """
        SELECT n.text, n.node_origin, n.hlc_wall, n.hlc_counter, p.name
        FROM note_current n
        LEFT JOIN patient_current p ON p.patient_id = n.patient_id
    """
    params: tuple = ()
    if patient_id is not None:
        sql += " WHERE n.patient_id = %s"
        params = (patient_id,)
    sql += " ORDER BY n.hlc_wall, n.hlc_counter, n.node_origin"
    rows = conn.execute(sql, params).fetchall()
    return [
        {"text": r[0], "node_origin": r[1], "hlc_wall": r[2],
         "hlc_counter": r[3], "patient_name": r[4]}
        for r in rows
    ]
