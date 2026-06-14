-- Cairn replication/failover PoC — node schema.
--
-- This is a deliberately tiny slice of Cairn's real architecture, just enough
-- to demonstrate offline-first replication and failover honestly:
--
--   * ONE append-only, immutable event log  (governing principle #1)
--   * events carry a Hybrid Logical Clock for a deterministic total order
--   * a globally-unique event_id makes sync a safe SET-UNION, never a merge
--   * "current" state is a PROJECTION over the log; corrections are new
--     overlay events, never edits        (governing principle #2: never erase)
--
-- Loaded identically into every node. A node's identity (A or B) is just a
-- string we stamp onto the events it originates.

BEGIN;

-- The one table that matters. Clinical content is immutable, append-only.
CREATE TABLE IF NOT EXISTS event_log (
    event_id    UUID PRIMARY KEY,          -- globally unique => set-union sync is safe
    patient_id  UUID        NOT NULL,       -- the immortal patient UUID this event is about
    event_type  TEXT        NOT NULL,       -- 'patient.created' | 'patient.amended' | 'note.added'
    payload     JSONB       NOT NULL,       -- the actual clinical/demographic content
    hlc_wall    BIGINT      NOT NULL,       -- Hybrid Logical Clock: physical component (ms)
    hlc_counter INTEGER     NOT NULL,       -- Hybrid Logical Clock: logical tiebreak
    node_origin TEXT        NOT NULL,       -- which node FIRST recorded this event
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()  -- local arrival time (per node)
);

-- Total order of events across all nodes: (wall, counter, node_origin).
CREATE INDEX IF NOT EXISTS event_log_order_idx
    ON event_log (hlc_wall, hlc_counter, node_origin);
CREATE INDEX IF NOT EXISTS event_log_patient_idx
    ON event_log (patient_id);

-- Append-only enforcement: refuse UPDATE and DELETE at the database layer.
-- (In real Cairn this lives in the safety-critical in-database tier; here it
--  makes the immutability claim demonstrable, not just aspirational.)
CREATE OR REPLACE FUNCTION event_log_is_append_only()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'event_log is append-only: % is not permitted (Cairn principle #1)', TG_OP;
END;
$$;

DROP TRIGGER IF EXISTS event_log_no_update ON event_log;
CREATE TRIGGER event_log_no_update BEFORE UPDATE OR DELETE ON event_log
    FOR EACH ROW EXECUTE FUNCTION event_log_is_append_only();

-- Per-node Hybrid Logical Clock state (single row).
CREATE TABLE IF NOT EXISTS hlc_state (
    id          BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),  -- enforce single row
    hlc_wall    BIGINT  NOT NULL DEFAULT 0,
    hlc_counter INTEGER NOT NULL DEFAULT 0
);
INSERT INTO hlc_state (id, hlc_wall, hlc_counter)
    VALUES (TRUE, 0, 0) ON CONFLICT (id) DO NOTHING;

-- ---------------------------------------------------------------------------
-- PROJECTIONS: "current truth" is derived, never stored as mutable rows.
-- ---------------------------------------------------------------------------

-- Latest demographics per patient: the most recent created/amended event wins
-- by HLC order. Earlier versions remain in the log forever (overlay, not edit).
CREATE OR REPLACE VIEW patient_current AS
SELECT DISTINCT ON (patient_id)
       patient_id,
       payload ->> 'name'      AS name,
       payload ->> 'dob'       AS dob,
       payload ->> 'sex'       AS sex,
       event_type              AS last_event,
       hlc_wall, hlc_counter, node_origin
FROM   event_log
WHERE  event_type IN ('patient.created', 'patient.amended')
ORDER BY patient_id, hlc_wall DESC, hlc_counter DESC, node_origin DESC;

-- Every clinical note, in causal order. The "atomic component of a health
-- record" for this PoC is a single immutable free-text note event.
CREATE OR REPLACE VIEW note_current AS
SELECT event_id,
       patient_id,
       payload ->> 'text'  AS text,
       hlc_wall, hlc_counter, node_origin, recorded_at
FROM   event_log
WHERE  event_type = 'note.added'
ORDER BY hlc_wall, hlc_counter, node_origin;

COMMIT;
