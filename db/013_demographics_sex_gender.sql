-- Cairn — demographic sex/gender projection policy: administrative-sex + gender-identity
-- (spec §4.2). Slice 4 of the demographics subsystem.
--
-- Adds the other two of the three §4.2 sex/gender fields on the SAME
-- demographic.field.asserted spine (db/011): no new event type, no new door, no floor
-- change (both values are OPEN strings — principle 4). The one new mechanic is a
-- PER-FIELD WINNER POLICY: gender-identity is recency-first (newest wins regardless of
-- provenance — the inverse of slice-2's provenance-first ordering), while
-- administrative-sex joins dob/sex-at-birth as provenance-first (a document-anchored
-- marker an unverified claim must not displace). A single IMMUTABLE classifier is the
-- source of truth for BOTH the projection gate and the winner ordering, so every node
-- converges identically. Matching (§5.2) is a separate, later subsystem.

BEGIN;

-- The per-field winner policy (spec §4.2). Source of truth for the projection: it gates
-- which fields project (NULL => the field is carried in event_log + legible via its twin
-- but never projected — the ADR-0012 federation-forward degrade for a field this node
-- does not recognise) AND selects the winner ordering. IMMUTABLE so it is trigger-safe
-- and every node computes the identical policy. Names (field='name') are deliberately
-- ABSENT — they project through their own db/012 retained-set table, not here.
CREATE OR REPLACE FUNCTION cairn_demographic_field_policy(p_field text)
RETURNS text LANGUAGE sql IMMUTABLE AS $$
    SELECT CASE p_field
        WHEN 'dob'                THEN 'provenance-first'
        WHEN 'sex-at-birth'       THEN 'provenance-first'
        WHEN 'administrative-sex' THEN 'provenance-first'
        WHEN 'gender-identity'    THEN 'recency-first'
        ELSE NULL
    END;
$$;

-- The §4.2 projection, now policy-driven. Supersedes db/011's definition (standard
-- latest-loaded-wins additive migration); db/012/names is untouched (it projects through
-- patient_name, not here). One row per (patient, field) holds the current DISPLAY winner;
-- full assertion history stays in event_log as the matching evidence (principle 2 — an
-- overlay, never an edit). event_log.body holds b->'payload' (see db/005 submit_event).
CREATE OR REPLACE FUNCTION patient_demographic_apply()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    p      jsonb := NEW.body;
    fld    text  := p ->> 'field';
    v_rank int   := cairn_provenance_rank(p ->> 'provenance');
    policy text  := cairn_demographic_field_policy(fld);
BEGIN
    -- Projection gate: a field with no winner policy is not projected (it is still in
    -- event_log and legible via its twin). Replaces slice-2's hard-coded field list.
    IF policy IS NULL THEN
        RETURN NULL;
    END IF;

    INSERT INTO patient_demographic AS pd
        (patient_id, field, value, facets, provenance, provenance_rank,
         asserted_hlc_wall, asserted_hlc_count, asserted_origin)
    VALUES
        (NEW.patient_id, fld, p ->> 'value', p -> 'facets', p ->> 'provenance', v_rank,
         NEW.hlc_wall, NEW.hlc_counter, NEW.node_origin)
    -- Winner ordering by policy. BOTH tuples are TOTAL orders (node_origin is the final
    -- deterministic tiebreak), so every node converges to the same winner regardless of
    -- apply order.
    --   provenance-first: rank leads -> a verified value LOCKS vs lower provenance,
    --     recency breaks equal-provenance ties (dob, sex-at-birth, administrative-sex).
    --   recency-first:    HLC leads  -> newest wins REGARDLESS of provenance, provenance
    --     then origin break equal-HLC ties (gender-identity).
    -- pd.field == EXCLUDED.field (the PK), so the policy is identical on both sides.
    ON CONFLICT (patient_id, field) DO UPDATE SET
        value              = EXCLUDED.value,
        facets             = EXCLUDED.facets,
        provenance         = EXCLUDED.provenance,
        provenance_rank    = EXCLUDED.provenance_rank,
        asserted_hlc_wall  = EXCLUDED.asserted_hlc_wall,
        asserted_hlc_count = EXCLUDED.asserted_hlc_count,
        asserted_origin    = EXCLUDED.asserted_origin,
        updated_at         = clock_timestamp()
    WHERE CASE cairn_demographic_field_policy(pd.field)
        WHEN 'recency-first' THEN
            (EXCLUDED.asserted_hlc_wall, EXCLUDED.asserted_hlc_count,
             EXCLUDED.provenance_rank, EXCLUDED.asserted_origin)
          > (pd.asserted_hlc_wall, pd.asserted_hlc_count,
             pd.provenance_rank, pd.asserted_origin)
        ELSE
            (EXCLUDED.provenance_rank, EXCLUDED.asserted_hlc_wall,
             EXCLUDED.asserted_hlc_count, EXCLUDED.asserted_origin)
          > (pd.provenance_rank, pd.asserted_hlc_wall,
             pd.asserted_hlc_count, pd.asserted_origin)
    END;
    RETURN NULL;
END;
$$;

-- The trigger binding is unchanged from db/011 (same WHEN, same function name); only the
-- function body above changed. Re-create defensively so a fresh load is order-independent.
DROP TRIGGER IF EXISTS patient_demographic_apply_trg ON event_log;
CREATE TRIGGER patient_demographic_apply_trg
    AFTER INSERT ON event_log
    FOR EACH ROW WHEN (NEW.event_type = 'demographic.field.asserted')
    EXECUTE FUNCTION patient_demographic_apply();

COMMIT;
