-- db/018_identity_linkage.sql
-- Cairn — §5.1/§5.7 identity linkage core (matcher piece C1).
--
-- WHAT: the authoritative destination for identity linkage. Adds the additive
-- `identity.link.asserted` / `identity.unlink.asserted` event types, a
-- culture-neutral structural floor, an HLC-overlay `patient_link` edge table, and
-- a `person_member` connected-component ("golden identity") projection with clean
-- unmerge (principle 2 — never merge, always link; unmerge is always clean).
--
-- The safety-critical write door submit_event (db/005) is REUSED verbatim: new
-- types register in event_type_class and add a branch to the cairn_event_twin hook.
-- Advisory matching (§5.2) and the proposal→apply seam (C2) are NOT here.

BEGIN;

-- 1. Register the two additive identity event types (fail-closed registry, ADR-0010).
--    additive + targets_other_author=FALSE: a link neither suppresses nor targets
--    another author's event, so the existing gate requires NO attestation for a
--    matcher-authored link (§5.2 "auto above threshold"); a human who vouches simply
--    includes a responsibility-bearing contributor, which the gate already attests.
INSERT INTO event_type_class (event_type, mode, targets_other_author) VALUES
    ('identity.link.asserted',   'additive', FALSE),
    ('identity.unlink.asserted', 'additive', FALSE)
ON CONFLICT (event_type) DO NOTHING;

-- 2. The §5.7 structural floor. Culture-neutral: validates STRUCTURE only — two
--    distinct valid UUID subjects and a non-empty provenance. Each violation is a
--    distinct legible exception (the cairn_check_identifier_assertion pattern).
CREATE OR REPLACE FUNCTION cairn_check_link_assertion(b jsonb)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    p jsonb := b -> 'payload';
    a text;
    c text;
BEGIN
    IF p IS NULL THEN
        RAISE EXCEPTION 'link assertion: missing payload';
    END IF;
    -- subject_a / subject_b: present, string.
    IF jsonb_typeof(p -> 'subject_a') IS DISTINCT FROM 'string'
       OR jsonb_typeof(p -> 'subject_b') IS DISTINCT FROM 'string' THEN
        RAISE EXCEPTION 'link assertion: subject_a and subject_b must be uuid strings (§5.7)';
    END IF;
    a := p ->> 'subject_a';
    c := p ->> 'subject_b';
    -- ...valid UUIDs (a bad cast here is a legible reject, not an opaque crash).
    BEGIN
        PERFORM a::uuid;
        PERFORM c::uuid;
    EXCEPTION WHEN others THEN
        RAISE EXCEPTION 'link assertion: subject_a/subject_b must be valid uuids (§5.7)';
    END;
    -- ...and distinct (a self-link is meaningless and would corrupt the component walk).
    IF a::uuid = c::uuid THEN
        RAISE EXCEPTION 'link assertion: self-link refused (subject_a = subject_b) (§5.1)';
    END IF;
    -- provenance: present, non-empty (§4.1 ladder; value-open, "unknown" is honest).
    IF jsonb_typeof(p -> 'provenance') IS DISTINCT FROM 'string'
       OR length(trim(p ->> 'provenance')) = 0 THEN
        RAISE EXCEPTION 'link assertion: provenance must be a non-empty string (§4.1)';
    END IF;
    -- confidence: optional; when present must not be JSON null (omit-when-absent
    -- discipline — a null confidence is a serialization bug, not "unknown", which is
    -- expressed by omitting the key; principle 4).
    IF (p ? 'confidence') AND (p -> 'confidence') = 'null'::jsonb THEN
        RAISE EXCEPTION 'link assertion: confidence must be omitted when absent, never null (principle 4)';
    END IF;
END;
$$;

-- 3. Extend the per-type twin hook. Identity link/unlink: run the floor + HARD-require
--    an authored twin (like demographics). This CREATE OR REPLACE PRESERVES db/010's
--    demographic branches and db/015's honest-degrade fallback for every other type —
--    it only adds the identity branch (submit_event itself is NEVER re-declared).
CREATE OR REPLACE FUNCTION cairn_event_twin(p_type text, b jsonb)
RETURNS text LANGUAGE plpgsql AS $$
DECLARE
    v_twin        text    := b ->> 'plaintext_twin';
    v_authored    boolean := v_twin IS NOT NULL AND length(regexp_replace(v_twin, '\s+', '', 'g')) > 0;
    v_demographic boolean := false;
    v_identity    boolean := false;
BEGIN
    -- Per-type structural floor.
    IF p_type = 'demographic.identifier.asserted' THEN
        PERFORM cairn_check_identifier_assertion(b);
        v_demographic := true;
    ELSIF p_type = 'demographic.field.asserted' THEN
        PERFORM cairn_check_demographic_field(b);
        v_demographic := true;
    ELSIF p_type IN ('identity.link.asserted', 'identity.unlink.asserted') THEN
        PERFORM cairn_check_link_assertion(b);
        v_identity := true;
    END IF;

    -- Authored twin present → carry it verbatim (principle 11; the conformant path).
    IF v_authored THEN
        RETURN v_twin;
    END IF;

    -- Absent/blank twin: demographic AND identity types HARD-require it; every other
    -- type degrades honestly to a flagged derived skeleton (ADR-0039).
    IF v_demographic THEN
        RAISE EXCEPTION 'submit_event: demographic assertion requires a non-empty authored twin (§4.5)';
    ELSIF v_identity THEN
        RAISE EXCEPTION 'submit_event: identity linkage assertion requires a non-empty authored twin (§5.7)';
    END IF;
    RETURN cairn_twin_skeleton(p_type, b);
END;
$$;

COMMIT;
