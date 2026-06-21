\set ON_ERROR_STOP on
BEGIN;

-- A genesis enroll row maps its signer key to its node_id (= content_address).
INSERT INTO node_event (node_event_id, op, author_node_id, subject_node_id,
    signer_key_id, hlc_wall, hlc_counter, node_origin, signed_bytes, content_address)
VALUES (gen_random_uuid(), 'enroll',
    '\x1220'||digest('A','sha256'), '\x1220'||digest('A','sha256'),
    'aakey', 0, 0, 'A', 'A', '\x1220'||digest('A','sha256'));

SELECT (node_id = '\x1220'||digest('A','sha256')) AS node_current_maps_key
FROM node_current WHERE signer_key_id = 'aakey';

-- The content-address invariant rejects a row whose advertised address lies.
DO $$ BEGIN
    BEGIN
        INSERT INTO node_event (node_event_id, op, author_node_id, subject_node_id,
            signer_key_id, hlc_wall, hlc_counter, node_origin, signed_bytes, content_address)
        VALUES (gen_random_uuid(), 'enroll', '\x00','\x00','k',0,0,'X','realbytes','\x1220'||digest('LIE','sha256'));
        RAISE EXCEPTION 'content-address CHECK FAILED: mismatched row accepted';
    EXCEPTION WHEN check_violation THEN RAISE NOTICE 'content-address CHECK OK'; END;
END $$;

-- Append-only: UPDATE/DELETE must raise.
DO $$ BEGIN
    BEGIN
        UPDATE node_event SET role = 'x';
        RAISE EXCEPTION 'append-only FAILED';
    EXCEPTION WHEN others THEN
        IF SQLERRM LIKE '%append-only%' THEN RAISE NOTICE 'append-only OK'; ELSE RAISE; END IF;
    END;
END $$;

-- C7.1: cairn_node may not raw-INSERT into node_event (grant floor).
DO $$ BEGIN
    SET LOCAL ROLE cairn_node;
    BEGIN
        INSERT INTO node_event (node_event_id, op, author_node_id, subject_node_id,
            signer_key_id, hlc_wall, hlc_counter, node_origin, signed_bytes, content_address)
        VALUES (gen_random_uuid(),'enroll','\x00','\x00','k',0,0,'X','b','\x1220'||digest('b','sha256'));
        RESET ROLE; RAISE EXCEPTION 'grant-floor FAILED: raw INSERT succeeded';
    EXCEPTION WHEN insufficient_privilege THEN RESET ROLE; RAISE NOTICE 'grant-floor OK'; END;
END $$;

-- C7.2: submit_node_event rejects unsigned/malformed bytes with a legible reason (fail closed).
DO $$ BEGIN
    BEGIN
        PERFORM submit_node_event('\xdeadbeef'::bytea);
        RAISE EXCEPTION 'fail-closed FAILED: malformed node event accepted';
    EXCEPTION WHEN others THEN
        IF SQLERRM LIKE '%signature%' OR SQLERRM LIKE '%verify%'
            THEN RAISE NOTICE 'fail-closed OK: %', SQLERRM; ELSE RAISE; END IF;
    END;
END $$;

ROLLBACK;
