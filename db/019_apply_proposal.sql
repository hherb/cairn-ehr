-- db/019_apply_proposal.sql
-- §5.2/§5.7 C2 apply seam — the additive column linking an applied proposal to the
-- identity event it produced.
--
-- WHAT: one nullable column on the advisory match_proposal worklist (db/017). When the
-- C2 seam (cairn-node::apply_proposal) turns a human-ACCEPTED proposal into a real
-- identity.link.asserted event, it records that event's id here and flips status to
-- 'applied' in the SAME transaction as submit_event. This closes the loop (proposal ->
-- which link event) and makes re-application idempotent (only status='accepted' rows
-- are picked up).
--
-- INVARIANT (documented, enforced by the seam's single transaction, not a DB trigger):
--   status='applied'  <=>  applied_event_id IS NOT NULL.
--
-- Additive: no event-format change, no submit_event change, no new event type. The
-- existing GRANT ... UPDATE ON match_proposal TO cairn_agent (db/017) already permits
-- the mark-applied write; no new grant is needed.

ALTER TABLE match_proposal ADD COLUMN IF NOT EXISTS applied_event_id UUID;
