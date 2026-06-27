-- Cairn — demographic identifier assertions (spec §4.1/§4.4/§4.5, ADR-0033/0034).
--
-- The first production clinical surface. Adds the `demographic.identifier.asserted`
-- event type, the §4.4 structural floor (culture-neutral: no profile, no checksum,
-- no format validation — those are advisory and live above the floor), the §4.5
-- authored-twin carry through submit_event, and a set-union `patient_identifier`
-- projection. Matching/veto (§5.2) is a separate, later subsystem and NOT here.

BEGIN;

-- Additive registration: a new event type adds a row (fail-closed registry, ADR-0010).
INSERT INTO event_type_class (event_type, mode, targets_other_author) VALUES
    ('demographic.identifier.asserted', 'additive', FALSE)
ON CONFLICT (event_type) DO NOTHING;

-- The §4.4 structural floor. Enforces ONLY culture-neutral invariants; never holds a
-- profile, runs a checksum, or validates a format (those flag-not-reject above the
-- floor — principle 12 / §4.4). Each violation is a distinct legible exception.
CREATE OR REPLACE FUNCTION cairn_check_identifier_assertion(b jsonb)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    p jsonb := b -> 'payload';
BEGIN
    IF p IS NULL THEN
        RAISE EXCEPTION 'identifier assertion: missing payload';
    END IF;
    -- value: present, string, non-empty (§4.4 mandatory, the evidence facet).
    IF jsonb_typeof(p -> 'value') IS DISTINCT FROM 'string'
       OR length(trim(p ->> 'value')) = 0 THEN
        RAISE EXCEPTION 'identifier assertion: value must be a non-empty string (§4.4 mandatory)';
    END IF;
    -- system: present, string, non-empty (§4.4 mandatory; may be the literal "unknown").
    IF jsonb_typeof(p -> 'system') IS DISTINCT FROM 'string'
       OR length(trim(p ->> 'system')) = 0 THEN
        RAISE EXCEPTION 'identifier assertion: system must be a non-empty string (§4.4 mandatory)';
    END IF;
    -- provenance: present, string, non-empty (§4.1 ladder; value-open, "unknown" is honest).
    IF jsonb_typeof(p -> 'provenance') IS DISTINCT FROM 'string'
       OR length(trim(p ->> 'provenance')) = 0 THEN
        RAISE EXCEPTION 'identifier assertion: provenance must be a non-empty string (§4.1)';
    END IF;
    -- normalized: optional; when present must be a string AND name a profile
    -- (the §4.4 materialised-key rule: a materialised matching key needs the bundle
    -- that produced it, so a profile-less node can trust it).
    IF (p ? 'normalized') AND (p -> 'normalized') IS DISTINCT FROM 'null'::jsonb THEN
        IF jsonb_typeof(p -> 'normalized') IS DISTINCT FROM 'string' THEN
            RAISE EXCEPTION 'identifier assertion: normalized must be a string when present (§4.4)';
        END IF;
        IF jsonb_typeof(p -> 'profile') IS DISTINCT FROM 'string'
           OR length(trim(p ->> 'profile')) = 0 THEN
            RAISE EXCEPTION 'identifier assertion: normalized materialised requires a named profile (§4.4)';
        END IF;
    END IF;
END;
$$;

-- The §4.2 set-union projection: one row per (patient, system, match_key). Identifiers
-- are set-union, never LWW — first-seen wins, re-assertion is a no-op, same-system /
-- different-normalized keeps BOTH rows (the veto SIGNAL preserved as data; the veto
-- itself is out of scope). `use` is a reserved word, so the column is `use_type`.
CREATE TABLE IF NOT EXISTS patient_identifier (
    patient_id         UUID    NOT NULL,
    system             TEXT    NOT NULL,
    match_key          TEXT    NOT NULL,   -- coalesce(normalized, value)
    value              TEXT    NOT NULL,
    normalized         TEXT,
    profile            TEXT,
    use_type           TEXT,
    provenance         TEXT    NOT NULL,
    asserted_hlc_wall  BIGINT  NOT NULL,
    asserted_hlc_count INTEGER NOT NULL,
    asserted_origin    TEXT    NOT NULL,
    first_seen         TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (patient_id, system, match_key)
);

-- Incremental set-union maintenance: fold exactly the one new identifier event into
-- the projection. event_log.body holds b->'payload' (see db/005 submit_event INSERT).
CREATE OR REPLACE FUNCTION patient_identifier_apply()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    p    jsonb := NEW.body;
    norm text  := NULLIF(p ->> 'normalized', '');
BEGIN
    INSERT INTO patient_identifier
        (patient_id, system, match_key, value, normalized, profile, use_type,
         provenance, asserted_hlc_wall, asserted_hlc_count, asserted_origin)
    VALUES
        (NEW.patient_id, p ->> 'system', COALESCE(norm, p ->> 'value'),
         p ->> 'value', norm, p ->> 'profile', p ->> 'use', p ->> 'provenance',
         NEW.hlc_wall, NEW.hlc_counter, NEW.node_origin)
    ON CONFLICT (patient_id, system, match_key) DO NOTHING;
    RETURN NULL;
END;
$$;

DROP TRIGGER IF EXISTS patient_identifier_apply_trg ON event_log;
CREATE TRIGGER patient_identifier_apply_trg
    AFTER INSERT ON event_log
    FOR EACH ROW WHEN (NEW.event_type = 'demographic.identifier.asserted')
    EXECUTE FUNCTION patient_identifier_apply();

GRANT SELECT ON patient_identifier TO cairn_agent;

-- Re-declare submit_event to carry the AUTHORED twin for demographic events (§4.5).
-- This is a byte-faithful copy of db/005_submit.sql with exactly one change: step 7
-- (the twin derivation) gains a demographic branch. All other steps — verify, actor
-- resolve, classify, attestation gate, target gate, provenance binding, INSERT,
-- idempotency, attachment learning — are identical to db/005. The GRANT/REVOKE block
-- from db/005 is NOT restated here (it is already in effect and does not change).
CREATE OR REPLACE FUNCTION submit_event(
    p_signed       BYTEA,
    p_attestation  BYTEA DEFAULT NULL,
    p_attester_key BYTEA DEFAULT NULL
) RETURNS UUID
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
DECLARE
    b              JSONB;
    v_event_id     UUID;
    v_ca           BYTEA;
    v_type         TEXT;
    v_mode         TEXT;
    v_targets_other BOOLEAN;
    v_bears        BOOLEAN;
    v_target_id    UUID;
    v_twin         TEXT;
    c              JSONB;
BEGIN
    -- 1. Signature floor (C5.1). cairn_verify is the in-DB pgrx gate.
    IF NOT cairn_verify(p_signed) THEN
        RAISE EXCEPTION 'submit_event: signature verification failed (unsigned or malformed event)';
    END IF;
    b := cairn_body(p_signed);
    IF b IS NULL THEN
        RAISE EXCEPTION 'submit_event: event body could not be parsed after verify';
    END IF;

    v_event_id := (b ->> 'event_id')::uuid;
    v_type     := b ->> 'event_type';
    -- content_address = sha256 of the signed wire bytes (the COSE envelope), identical to event_address() in cairn-event and the db/001 CHECK. (Distinct from canonical_json_address, which hashes the actor pinned-set body for actor_id.) Attestation tokens bind to THIS value.
    v_ca       := '\x1220'::bytea || digest(p_signed, 'sha256');

    -- 2. Resolve the signer against the actor registry (must be enrolled, non-revoked).
    IF NOT EXISTS (SELECT 1 FROM actor_current WHERE signing_key_id = b ->> 'signer_key_id') THEN
        RAISE EXCEPTION 'submit_event: signer % is not an enrolled, non-revoked actor', b ->> 'signer_key_id';
    END IF;

    -- 3. Classify (fail closed on unknown type).
    SELECT mode, targets_other_author INTO v_mode, v_targets_other
        FROM event_type_class WHERE event_type = v_type;
    IF v_mode IS NULL THEN
        RAISE EXCEPTION 'submit_event: unknown event_type % (no classification — fail closed)', v_type;
    END IF;

    -- Does any contributor claim a responsibility (bearing role with attestation)?
    v_bears := EXISTS (
        SELECT 1 FROM jsonb_array_elements(b -> 'contributors') AS e
        WHERE e ? 'responsibility');

    -- 4. Attestation gate. A suppressing event, OR any asserted responsibility,
    --    requires a valid attestation token bound to THIS event (C2, C5.2, C5.3).
    IF v_mode = 'suppressing' OR v_bears THEN
        IF p_attestation IS NULL OR p_attester_key IS NULL THEN
            RAISE EXCEPTION 'submit_event: % requires attestation (no token presented) — un-vouched suppress/responsibility refused', v_type;
        END IF;
        IF NOT cairn_attestation_ok(p_attestation, v_ca, p_attester_key) THEN
            RAISE EXCEPTION 'submit_event: attestation token invalid or not bound to this event';
        END IF;
        IF NOT EXISTS (SELECT 1 FROM actor_current
                       WHERE signing_key_id = encode(p_attester_key,'hex') AND kind = 'human') THEN
            RAISE EXCEPTION 'submit_event: attester is not an enrolled human actor (forged human author refused)';
        END IF;
    END IF;

    -- 5. Target-existence gate for an overlay on another author's event.
    --    (The skeleton stores the target in the body as `target_event_id`.)
    --
    --    DEFERRED (known limitation, not a fix): this does NOT verify that the
    --    attester is *entitled* to suppress THIS target. Step 4 only requires
    --    *some* enrolled human attester, so any human could downgrade any author's
    --    event. Real owner/authority semantics (target-author vs attester, role
    --    authority, delegation) are an ADR-level design question, not a spike hack;
    --    it is therefore left explicit here. C5.5 only demonstrates the *un-attested*
    --    cross-author downgrade is refused, which is the attestation gate, not an
    --    ownership check.
    IF v_targets_other AND (b -> 'payload' ? 'target_event_id') THEN
        v_target_id := (b -> 'payload' ->> 'target_event_id')::uuid;
        IF NOT EXISTS (SELECT 1 FROM event_log WHERE event_id = v_target_id) THEN
            RAISE EXCEPTION 'submit_event: overlay targets unknown event %', v_target_id;
        END IF;
    END IF;

    -- 6. Provenance binding (C3): an advisory must cite its source blob's address.
    IF v_type = 'advisory.added' THEN
        IF jsonb_array_length(COALESCE(b -> 'attachments', '[]'::jsonb)) = 0 THEN
            RAISE EXCEPTION 'submit_event: advisory.added must carry a provenance attachment reference';
        END IF;
    END IF;

    -- 7. Twin (§4.5) + floor. Demographic assertions carry the AUTHORED twin and pass
    --    the §4.4 structural floor; legacy types keep the derived skeleton twin.
    IF v_type = 'demographic.identifier.asserted' THEN
        PERFORM cairn_check_identifier_assertion(b);
        v_twin := b ->> 'plaintext_twin';
        IF v_twin IS NULL OR length(trim(v_twin)) = 0 THEN
            RAISE EXCEPTION 'submit_event: demographic assertion requires a non-empty authored twin (§4.5)';
        END IF;
    ELSE
        v_twin := format('[%s] %s for patient %s', v_type, b ->> 'schema_version', b ->> 'patient_id'); -- TODO: skeleton twin — spec §3.13/ADR-0012 want the clinical payload rendered too
    END IF;

    INSERT INTO event_log
        (event_id, patient_id, event_type, schema_version, hlc_wall, hlc_counter,
         node_origin, t_effective, signed_bytes, content_address, body, contributors,
         signer_key_id, plaintext_twin, attachments)
    VALUES (
        v_event_id, (b ->> 'patient_id')::uuid, v_type, b ->> 'schema_version',
        (b -> 'hlc' ->> 'wall')::bigint, (b -> 'hlc' ->> 'counter')::int,
        b -> 'hlc' ->> 'node_origin',
        NULLIF(b ->> 't_effective','null')::timestamptz,
        p_signed, v_ca, b -> 'payload', b -> 'contributors',
        b ->> 'signer_key_id', v_twin, COALESCE(b -> 'attachments','[]'::jsonb))
    ON CONFLICT (event_id) DO NOTHING;

    -- Idempotent re-submit of the SAME event is a silent no-op (set-union).
    -- But a DIFFERENT event reusing this event_id (substitution) must not pass
    -- silently: compare the stored content-address to what we just verified.
    IF NOT FOUND THEN
        IF (SELECT content_address FROM event_log WHERE event_id = v_event_id) <> v_ca THEN
            RAISE EXCEPTION 'submit_event: event_id % already exists with different content (substitution refused)', v_event_id;
        END IF;
    END IF;

    -- Learn any attachment references (reference-eager, byte-lazy).
    FOR c IN SELECT * FROM jsonb_array_elements(COALESCE(b -> 'attachments','[]'::jsonb)) LOOP
        PERFORM blob_note_reference(decode(c ->> 'digest_hex','hex'), c ->> 'media_type',
                                    (c ->> 'byte_len')::bigint);
    END LOOP;

    RETURN v_event_id;
END;
$$;

COMMIT;
