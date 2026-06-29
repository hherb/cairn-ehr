# matcher/src/cairn_matcher/pipeline/db.py
"""The only Postgres-touching module in the matcher. Thin: it loads a patient's
projection rows, calls the in-DB veto floor, and upserts a proposal. All scoring and
banding logic lives in the pure modules; this module just moves data.

Requires the optional `pipeline` extra (psycopg). The pure core never imports it.
"""

import json

from psycopg.rows import dict_row

from cairn_matcher.pipeline.adapter import candidate_from_rows
from cairn_matcher.pipeline.banding import ProposalPayload, VetoFinding
from cairn_matcher.records import CandidateRecord


def load_candidate(conn, patient_id) -> CandidateRecord:
    """Read one patient's matching-relevant projection rows and shape a CandidateRecord.

    Reads the winner rows (dob, sex-at-birth) and the retained sets (names, identifiers).
    Pure shaping is delegated to adapter.candidate_from_rows.
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT value, facets, provenance_rank FROM patient_demographic "
                    "WHERE patient_id=%s AND field='dob'", (patient_id,))
        dob_row = cur.fetchone()
        cur.execute("SELECT value, provenance_rank FROM patient_demographic "
                    "WHERE patient_id=%s AND field='sex-at-birth'", (patient_id,))
        sex_row = cur.fetchone()
        cur.execute("SELECT value, provenance_rank FROM patient_name WHERE patient_id=%s",
                    (patient_id,))
        name_rows = cur.fetchall()
        cur.execute("SELECT system, match_key FROM patient_identifier WHERE patient_id=%s",
                    (patient_id,))
        identifier_rows = cur.fetchall()
    return candidate_from_rows(
        dob_row=dob_row, sex_row=sex_row, name_rows=name_rows, identifier_rows=identifier_rows
    )


def match_veto(conn, a, b) -> list[VetoFinding]:
    """Call the safety-critical in-DB hard-veto floor (db/016) and return its rows.

    The matcher NEVER re-implements this; it only consults it. A pair with any finding
    cannot be auto-linked (banding enforces that).
    """
    with conn.cursor() as cur:
        cur.execute("SELECT veto_kind, severity, subject, detail FROM cairn_match_veto(%s, %s)",
                    (a, b))
        return [VetoFinding(*row) for row in cur.fetchall()]


# The three blocking passes share one shape: group patients by a blocking value, keep
# groups with >= 2 members, and emit every within-group pair. Group-based (not direct
# self-joins) because Task 2's oversized-block guard needs each group's member count.
#
# Canonical order is enforced in SQL by m1 < m2 on the uuid VALUES (Postgres uuid byte
# order == uuid.UUID 128-bit order == runner.canonical_pair), so a pair is one stable row.
# Blocking is RECALL-oriented and advisory: the SQL name tokenizer is deliberately simple
# (lower + whitespace split); the Python scorer remains the source of truth for comparison.
_CANDIDATE_SQL = """
WITH ident_groups AS (
    SELECT array_agg(patient_id) AS members
    FROM patient_identifier
    WHERE system <> 'unknown'
    GROUP BY system, match_key
    HAVING count(DISTINCT patient_id) >= 2
),
dob_groups AS (
    SELECT array_agg(patient_id) AS members
    FROM patient_demographic
    WHERE field = 'dob'
    GROUP BY value
    HAVING count(DISTINCT patient_id) >= 2
),
name_tokens AS (
    SELECT DISTINCT patient_id, token
    FROM patient_name, regexp_split_to_table(lower(value), '\\s+') AS token
    WHERE token <> ''
),
name_groups AS (
    SELECT array_agg(patient_id) AS members
    FROM name_tokens
    GROUP BY token
    HAVING count(*) >= 2
),
all_groups AS (
    SELECT members FROM ident_groups
    UNION ALL SELECT members FROM dob_groups
    UNION ALL SELECT members FROM name_groups
)
SELECT DISTINCT m1::text AS patient_low, m2::text AS patient_high
FROM all_groups g, unnest(g.members) m1, unnest(g.members) m2
WHERE m1 < m2
"""


def generate_candidate_pairs(conn, *, max_block_size: int = 100):
    """Generate the canonical candidate pairs worth scoring, via three blocking passes.

    Returns (pairs, skipped_blocks): `pairs` is a list of unique canonical
    (patient_low, patient_high) lowercase-uuid-text tuples; `skipped_blocks` reports
    oversized blocks excluded from generation (empty until the cap is wired in).

    Read-only — opens a read transaction the CALLER must close (sweep does conn.rollback
    before its write loop, so a long sweep does not pin the xmin horizon).
    """
    with conn.cursor() as cur:
        cur.execute(_CANDIDATE_SQL)
        pairs = [(low, high) for low, high in cur.fetchall()]
    skipped_blocks: list[tuple[str, str, int]] = []
    return pairs, skipped_blocks


def upsert_proposal(conn, low, high, payload: ProposalPayload) -> None:
    """Write (or refresh) the advisory proposal for a canonical-ordered pair.

    Latest-wins on (patient_low, patient_high), but a non-'pending' status (a human's
    decision) is PRESERVED — a re-run refreshes the score/band/evidence, never a verdict.

    Does NOT commit. The caller owns the transaction boundary.
    """
    with conn.cursor() as cur:
        cur.execute(
            "INSERT INTO match_proposal "
            "(patient_low, patient_high, score_total, band, veto_findings, evidence, matcher_version) "
            "VALUES (%s,%s,%s,%s,%s,%s,%s) "
            "ON CONFLICT (patient_low, patient_high) DO UPDATE SET "
            "score_total=EXCLUDED.score_total, band=EXCLUDED.band, "
            "veto_findings=EXCLUDED.veto_findings, evidence=EXCLUDED.evidence, "
            "matcher_version=EXCLUDED.matcher_version, updated_at=clock_timestamp()",
            (low, high, payload.score_total, payload.band.value,
             json.dumps(list(payload.veto_findings)), json.dumps(list(payload.evidence)),
             payload.matcher_version),
        )
