-- Cairn — node-level supersede + self-trusting restore (ADR-0026 slice C).
--
-- WHY: slice B exports a node's signed node_event set to a cold-peer medium; this is
-- the APPLY half. Restoring a node's OWN history into a fresh DB cannot use the live
-- apply_remote_node_event gate (that is the PEER-admission path — it rejects events
-- whose author is not an already-trusted peer, which a fresh node has none of). So we
-- add a SELF-TRUSTING restore door, fenced so it is a permanent no-op on a live node,
-- plus the node-level `supersede` op (a restored node mints a NEW key — the signing key
-- is never backed up — and records supersede(dead -> new), already the actor-algebra
-- shape for agents, now applied to nodes). See ADR-0026 §7.10 points 1/2/4.

BEGIN;

-- (1) Widen the op CHECK additively (ADR-0012): a superset rejects nothing previously
-- accepted. The constraint is the auto-named column CHECK from db/007's CREATE TABLE.
ALTER TABLE node_event DROP CONSTRAINT IF EXISTS node_event_op_check;
ALTER TABLE node_event ADD CONSTRAINT node_event_op_check
    CHECK (op IN ('enroll','peer','revoke','supersede'));

-- (2) The supersede lineage view: who superseded whom. Read by `status`/audit. A
-- supersede event's author is the NEW (live) node; its subject is the dead node-id.
CREATE OR REPLACE VIEW node_lineage AS
SELECT ne.subject_node_id AS superseded_node_id,
       ne.author_node_id  AS new_node_id,
       ne.hlc_wall, ne.hlc_counter, ne.recorded_at
FROM node_event ne
WHERE ne.op = 'supersede';

GRANT SELECT ON node_lineage TO cairn_node;

COMMIT;
