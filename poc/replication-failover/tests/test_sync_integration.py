"""Integration tests for the sync engine against the two live demo clusters.

These require both nodes to be running (``bin/setup.sh`` first); otherwise they
skip. They reset shared state, so they run serially and own the databases while
they run — do not run them against anything you care about.
"""

from __future__ import annotations

import uuid

import psycopg
import pytest

from cairn_demo import events as ev
from cairn_demo import projections as proj
from cairn_demo.db import get_node
from cairn_demo.sync import sync_pair

A = get_node("A")
B = get_node("B")

pytestmark = pytest.mark.skipif(
    not (A.is_up() and B.is_up()),
    reason="both demo nodes must be running (run bin/setup.sh)",
)


def _reset(conn: psycopg.Connection) -> None:
    conn.execute("TRUNCATE event_log")
    conn.execute("UPDATE hlc_state SET hlc_wall=0, hlc_counter=0 WHERE id IS TRUE")


@pytest.fixture()
def nodes():
    ca = A.connect_autocommit()
    cb = B.connect_autocommit()
    _reset(ca)
    _reset(cb)
    yield ca, cb
    ca.close()
    cb.close()


def test_one_way_replication(nodes):
    ca, cb = nodes
    ev.create_patient(ca, node_origin="A", name="One Way")
    res = sync_pair(ca, cb)
    assert res.a_to_b == 1 and res.b_to_a == 0
    assert res.converged
    assert [p["name"] for p in proj.patients(cb)] == ["One Way"]


def test_bidirectional_partition_converges_identically(nodes):
    ca, cb = nodes
    pid = ev.create_patient(ca, node_origin="A", name="Bidir")
    sync_pair(ca, cb)

    # Both sides write independently (as if partitioned).
    ev.add_note(ca, node_origin="A", patient_id=pid, text="written on A")
    ev.add_note(cb, node_origin="B", patient_id=pid, text="written on B")

    res = sync_pair(ca, cb)
    assert res.converged
    assert proj.event_ids(ca) == proj.event_ids(cb)

    # Identical causal order on both nodes — the HLC guarantee.
    order_a = [(n["node_origin"], n["text"]) for n in proj.notes(ca)]
    order_b = [(n["node_origin"], n["text"]) for n in proj.notes(cb)]
    assert order_a == order_b
    assert len(order_a) == 2


def test_resync_is_idempotent(nodes):
    ca, cb = nodes
    ev.create_patient(ca, node_origin="A", name="Idem")
    assert sync_pair(ca, cb).total_copied == 1
    assert sync_pair(ca, cb).total_copied == 0  # nothing new the second time
    assert sync_pair(ca, cb).converged


def test_event_log_is_append_only(nodes):
    ca, _ = nodes
    pid = ev.create_patient(ca, node_origin="A", name="Immutable")
    with pytest.raises(psycopg.errors.RaiseException):
        ca.execute("UPDATE event_log SET payload = '{}'::jsonb WHERE patient_id = %s",
                   (pid,))
    with pytest.raises(psycopg.errors.RaiseException):
        ca.execute("DELETE FROM event_log WHERE patient_id = %s", (pid,))


def test_amendment_overlays_not_edits(nodes):
    ca, cb = nodes
    pid = ev.create_patient(ca, node_origin="A", name="Typo Nmae", sex="F")
    ev.amend_patient(ca, node_origin="A", patient_id=pid, name="Correct Name")
    sync_pair(ca, cb)
    # Current projection shows the correction…
    assert proj.patients(cb)[0]["name"] == "Correct Name"
    # …but the original event is still in the log (never erased).
    count = cb.execute(
        "SELECT count(*) FROM event_log WHERE patient_id = %s", (pid,)
    ).fetchone()[0]
    assert count == 2
