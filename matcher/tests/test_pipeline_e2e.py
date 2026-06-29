# matcher/tests/test_pipeline_e2e.py
"""End-to-end: seed two patients' projections, run propose(), assert the persisted
match_proposal row. Gated on CAIRN_TEST_PG (skips cleanly without a database).

Covers: a clean strong match -> a persisted proposal; a verified-DOB clash -> the db/016
hard veto caps the band at 'review' (never auto, never dropped); a weak pair -> no row;
re-running preserves a human-set status (latest-wins but status-preserving).
"""

from cairn_matcher.pipeline.banding import Band
from cairn_matcher.pipeline.runner import propose
from tests.conftest import seed_patient

PA = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
PB = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"
LOW, HIGH = (PA, PB) if PA < PB else (PB, PA)


def _row(conn):
    with conn.cursor() as cur:
        cur.execute(
            "SELECT score_total, band, veto_findings, evidence, status FROM match_proposal "
            "WHERE patient_low=%s AND patient_high=%s", (LOW, HIGH))
        return cur.fetchone()


def test_strong_match_persists_a_review_proposal(pg_conn):
    # Shared identifier (8.0 * 0.5 = 4.0) -> crosses review but not auto. No veto.
    for p in (PA, PB):
        seed_patient(pg_conn, p, names=[("Alex Smith", 20)],
                     identifiers=[("mrn:hospital-a", "12345", "12345")])
    result = propose(pg_conn, PA, PB)
    assert result is Band.REVIEW
    row = _row(pg_conn)
    assert row is not None
    assert row[1] == "review"
    assert row[4] == "pending"


def test_verified_dob_clash_caps_at_review(pg_conn):
    # Strong name+id signal, but verified, same-precision, different DOBs -> hard veto.
    seed_patient(pg_conn, PA, dob=("1980-07-15", 60, "day"), names=[("Alex Smith", 60)],
                 identifiers=[("mrn:a", "1", "1")])
    seed_patient(pg_conn, PB, dob=("1990-01-01", 60, "day"), names=[("Alex Smith", 60)],
                 identifiers=[("mrn:a", "1", "1")])
    result = propose(pg_conn, PA, PB)
    assert result is Band.REVIEW  # never AUTO_CANDIDATE under a veto
    row = _row(pg_conn)
    findings = row[2]
    assert any(f["veto_kind"] == "dob" and f["severity"] == "hard_veto" for f in findings)


def test_weak_pair_persists_nothing(pg_conn):
    # Only sex agrees (1.0 * 0.5 = 0.5) -> below review threshold -> no proposal.
    for p in (PA, PB):
        seed_patient(pg_conn, p, sex=("female", 0))
    assert propose(pg_conn, PA, PB) is None
    assert _row(pg_conn) is None


def test_rerun_preserves_human_status(pg_conn):
    for p in (PA, PB):
        seed_patient(pg_conn, p, names=[("Alex Smith", 20)],
                     identifiers=[("mrn:hospital-a", "12345", "12345")])
    propose(pg_conn, PA, PB)
    with pg_conn.cursor() as cur:  # a reviewer accepts it
        cur.execute("UPDATE match_proposal SET status='accepted' WHERE patient_low=%s", (LOW,))
    pg_conn.commit()
    propose(pg_conn, PB, PA)  # re-run, reversed order -> same row
    assert _row(pg_conn)[4] == "accepted"  # status preserved, not clobbered to 'pending'
