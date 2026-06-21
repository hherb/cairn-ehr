# First Federating Node — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the first real Cairn node — a `cairn-node` service that provisions its own signing identity, pairs with another node over an out-of-band fingerprint, and converges its identity + peering events by mTLS-gated set-union sync, with admission enforced in Postgres. No EHR surface.

**Architecture:** Node/peering events reuse the existing signed `EventBody`/`cairn_verify` envelope (nil patient, `node.*` types) but live in a new append-only `node_event` table parallel to `actor_event`. The trust set is a `trust_peer` projection over those events. `cairn-node` (new crate) holds the private key off-log, signs node events, and runs federation sync over built-in mTLS whose peer certs are pinned to `trust_peer`. The admission gate (`apply_remote_node_event`) and authoring door (`submit_node_event`) are SECURITY DEFINER PL/pgSQL calling the existing `cairn_verify`/`cairn_body` pgrx functions — no new Rust crypto, no new pgrx functions.

**Tech Stack:** Rust (workspace), `cairn-event` (COSE_Sign1/Ed25519, existing), PostgreSQL ≥ 18 + `cairn_pgx` (pgrx, existing), `rustls` 0.23 + `rcgen` (new, mTLS), `clap` (CLI), `tokio-postgres` (DB client).

## Global Constraints

- **License:** every new file is AGPL-3.0-only (workspace `license = "AGPL-3.0-only"`).
- **Postgres floor:** ≥ 18 (SQL must run on 16 for local pgrx testing — no 18-only syntax; UUIDv7 is minted in Rust, never `uuidv7()` in SQL).
- **§9 blast-radius:** signature/identity/admission code is safety-critical → Rust or in-DB, reviewer-legible. CLI ergonomics / output formatting is fit-for-purpose.
- **Append-only (principle 1/2):** never `UPDATE`/`DELETE` clinical or identity rows; corrections/unpeering are overlay events.
- **node_id is genesis-stable (D7):** `node_id` = `event_address(genesis signed_bytes)` (the `\x1220` sha2-256 multihash), NOT `cairn_actor_id(pinned)`. Do not bake the rotatable key into the identity address.
- **Direct-pairwise trust only:** no CA, no registry, no practice-issuing-key. Trust roots in the out-of-band fingerprint.
- **No clinical events** of any kind in this slice.
- **Product-neutrality:** no record-system product names or prior-project names in any committed file.

---

## File Structure

```
/Cargo.toml                              # MODIFIED at T1: new root workspace
/crates/cairn-event/                     # MOVED at T1; T2 adds node-identity primitives
/crates/cairn-sync/                      # MOVED at T1 (unchanged thereafter this slice)
/crates/cairn-node/                      # NEW crate (T6+): CLI, keystore, transport, federation sync
  ├── Cargo.toml
  ├── src/main.rs                        # clap CLI dispatch
  ├── src/keystore.rs                    # sealed Ed25519 key file (T6)
  ├── src/db.rs                          # tokio-postgres helpers + schema load (T6)
  ├── src/identity.rs                    # provision / identity / node-event authoring (T6/T7)
  ├── src/pairing.rs                     # pair-offer / pair-accept bundle flow (T7)
  ├── src/transport.rs                   # rustls mTLS pinned to trust_peer (T9)
  ├── src/sync.rs                        # node_event set-union over mTLS (T10)
  └── tests/federation.rs               # two-node integration (T12)
/extensions/cairn_pgx/                   # MOVED at T1 (unchanged this slice)
/db/                                     # MOVED at T1; T3-T5,T8 add 007_node_federation.sql
  ├── 007_node_federation.sql            # NEW: node_event, doors, trust_peer, admission
  └── tests/007_node_federation_test.sql # NEW: psql assertion script
/poc/walking-skeleton/                   # FROZEN; Cargo.toml removed at T1, harnesses repoint --bin
```

---

## PHASE 1 — Identity & federation data layer (no wire)

### Task 1: Graduate to a top-level workspace

**Files:**
- Create: `/Cargo.toml`
- Move: `poc/walking-skeleton/crates/cairn-event` → `crates/cairn-event`; `…/crates/cairn-sync` → `crates/cairn-sync`; `…/crates/cairn_pgx` → `extensions/cairn_pgx`; `…/db` → `db`
- Delete: `poc/walking-skeleton/Cargo.toml`, `poc/walking-skeleton/Cargo.lock`

**Interfaces:**
- Produces: a root workspace where `cargo test --workspace` builds `cairn-event` + `cairn-sync`; `extensions/cairn_pgx` excluded (pgrx toolchain separate). The `include_str!("../../../db/00x.sql")` paths in `cairn-sync/src/main.rs` remain valid (db moves up by the same depth the crates do).

- [ ] **Step 1: Move the crates and db with git (preserve history)**

```bash
cd /Users/hherb/src/cairn-ehr/.claude/worktrees/happy-volhard-24f06a
mkdir -p crates extensions
git mv poc/walking-skeleton/crates/cairn-event   crates/cairn-event
git mv poc/walking-skeleton/crates/cairn-sync    crates/cairn-sync
git mv poc/walking-skeleton/crates/cairn_pgx     extensions/cairn_pgx
git mv poc/walking-skeleton/db                    db
git rm poc/walking-skeleton/Cargo.toml poc/walking-skeleton/Cargo.lock
```

- [ ] **Step 2: Write the root workspace manifest**

```toml
# /Cargo.toml
[workspace]
resolver = "2"
members = ["crates/cairn-event", "crates/cairn-sync", "crates/cairn-node"]
exclude = ["extensions/cairn_pgx"]

[workspace.package]
edition = "2021"
rust-version = "1.74"
license = "AGPL-3.0-only"
repository = "https://github.com/cairn-ehr/cairn-ehr"

[profile.release]
opt-level = 3
```

(`crates/cairn-node` does not exist yet; Step 4 verifies the two existing crates. Re-run after Task 6 once the crate exists. If `cargo` errors on the missing member now, temporarily drop `"crates/cairn-node"` from `members` and restore it in Task 6.)

- [ ] **Step 3: Repoint the frozen Python harnesses' default bin path**

In `poc/walking-skeleton/harness/`, the harnesses accept `--bin`; no code change is required. Add a one-line banner to `poc/walking-skeleton/README.md` top:

```markdown
> **FROZEN (2026-06-21):** the Rust crates and `db/` graduated to the repo-root
> workspace (`/crates`, `/extensions`, `/db`). These harnesses are historical
> spike artifacts; run them with `--bin ../../target/debug/cairn-sync` and the
> `db/` SQL now at the repo root. Their recorded results stand; no re-run needed.
```

- [ ] **Step 4: Verify the existing suite stays green after the move**

Run: `cargo test --workspace`
Expected: `cairn-event` tests PASS (the sign/verify/blob/attestation suite), `cairn-sync` builds. No test should reference the old `poc/walking-skeleton/crates` paths.

- [ ] **Step 5: Verify the pgrx extension still builds from its new home**

Run: `cd extensions/cairn_pgx && cargo build`
Expected: compiles (full `cargo pgrx test` is deferred to its own toolchain; a plain build confirms the move didn't break paths).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor: graduate cairn crates + db to the top-level workspace"
```

---

### Task 2: `cairn-event` node-identity primitives — fingerprint + pairing bundle

**Files:**
- Modify: `crates/cairn-event/src/lib.rs` (append new public items + tests)

**Interfaces:**
- Consumes: existing `sign`, `verify_with`, `SigningKey`, `VerifyingKey`, `event_address`, `SHA2_256_MULTIHASH_PREFIX`.
- Produces:
  - `pub fn short_fingerprint(pubkey_hex: &str) -> Result<String, EventError>` — a human-verifiable code derived deterministically from the key (groups of a sha2-256 over the 32 pubkey bytes).
  - `pub struct PairingBundle { pub node_id_hex: String, pub pubkey_hex: String, pub address: String, pub fingerprint: String, pub nonce: String, pub hlc: Hlc }`
  - `pub fn sign_pairing_bundle(b: &PairingBundle, sk: &SigningKey) -> Result<Vec<u8>, EventError>` (COSE_Sign1 over canonical CBOR of the bundle).
  - `pub fn verify_pairing_bundle(token: &[u8]) -> Result<PairingBundle, EventError>` — verifies against the key embedded in the bundle (`pubkey_hex`) and confirms `fingerprint == short_fingerprint(pubkey_hex)` (a bundle that lies about its own fingerprint is rejected).

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `crates/cairn-event/src/lib.rs`:

```rust
#[test]
fn fingerprint_is_deterministic_and_keyed() {
    let (_sk, kid) = generate_key().unwrap();
    let fp1 = short_fingerprint(&kid).unwrap();
    let fp2 = short_fingerprint(&kid).unwrap();
    assert_eq!(fp1, fp2, "same key -> same fingerprint");
    let (_sk2, kid2) = generate_key().unwrap();
    assert_ne!(fp1, short_fingerprint(&kid2).unwrap(), "different key -> different fingerprint");
    assert!(short_fingerprint("not-hex").is_err());
}

#[test]
fn pairing_bundle_roundtrips_and_rejects_tampering() {
    let (sk, kid) = generate_key().unwrap();
    let b = PairingBundle {
        node_id_hex: hex::encode(event_address(b"genesis-bytes")),
        pubkey_hex: kid.clone(),
        address: "10.0.0.2:7800".into(),
        fingerprint: short_fingerprint(&kid).unwrap(),
        nonce: "abcd1234".into(),
        hlc: Hlc { wall: 1, counter: 0, node_origin: "n".into() },
    };
    let token = sign_pairing_bundle(&b, &sk).unwrap();
    assert_eq!(verify_pairing_bundle(&token).unwrap(), b);

    // A bundle that lies about its own fingerprint is rejected.
    let mut liar = b.clone();
    liar.fingerprint = "DEAD-BEEF".into();
    let bad = sign_pairing_bundle(&liar, &sk).unwrap();
    assert!(verify_pairing_bundle(&bad).is_err());

    // Tampered bytes -> reject.
    let mut t = token.clone();
    let m = t.len() / 2; t[m] ^= 0x01;
    assert!(verify_pairing_bundle(&t).is_err());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cairn-event fingerprint_is_deterministic_and_keyed pairing_bundle_roundtrips`
Expected: FAIL — `short_fingerprint`/`PairingBundle`/`sign_pairing_bundle`/`verify_pairing_bundle` not found.

- [ ] **Step 3: Implement the primitives**

Add `#[derive(Clone)]` import note: `PairingBundle` needs `Debug, Clone, Serialize, Deserialize, PartialEq`. Append to `crates/cairn-event/src/lib.rs` (before the `tests` module):

```rust
/// A human-verifiable short fingerprint of an Ed25519 public key (hex): the
/// sha2-256 of the 32 key bytes, rendered as five 4-hex-digit groups. This is the
/// out-of-band code an operator reads aloud / scans to confirm a peer's identity
/// at pairing (the MITM antidote — ADR-0017 §7). Display-only; the DB pins the key.
pub fn short_fingerprint(pubkey_hex: &str) -> Result<String, EventError> {
    use sha2::{Digest, Sha256};
    let raw = hex::decode(pubkey_hex).map_err(|_| EventError::BadKeyId)?;
    if raw.len() != 32 {
        return Err(EventError::BadKeyId);
    }
    let digest = Sha256::digest(&raw);
    let groups: Vec<String> = digest[..10]
        .chunks(2)
        .map(|c| format!("{:02X}{:02X}", c[0], c[1]))
        .collect();
    Ok(groups.join("-"))
}

/// The out-of-band pairing offer (ADR-0017 §7): a signed, operator-carried bundle
/// that introduces one node to another. The fingerprint is the human check; the
/// pubkey is what the trust set pins.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PairingBundle {
    pub node_id_hex: String,
    pub pubkey_hex: String,
    pub address: String,
    pub fingerprint: String,
    pub nonce: String,
    pub hlc: Hlc,
}

/// Sign a pairing bundle as a COSE_Sign1 (Ed25519), reusing the event signing path.
pub fn sign_pairing_bundle(b: &PairingBundle, sk: &SigningKey) -> Result<Vec<u8>, EventError> {
    use coset::{iana, CborSerializable, CoseSign1Builder, HeaderBuilder};
    let mut payload = Vec::new();
    ciborium::into_writer(b, &mut payload).map_err(|e| EventError::Cbor(e.to_string()))?;
    let kid = sk.verifying_key().to_bytes().to_vec();
    let protected = HeaderBuilder::new()
        .algorithm(iana::Algorithm::EdDSA)
        .key_id(kid)
        .build();
    let sign1 = CoseSign1Builder::new()
        .protected(protected)
        .payload(payload)
        .create_signature(b"", |tbs| sk.sign(tbs).to_bytes().to_vec())
        .build();
    sign1.to_vec().map_err(|e| EventError::Cose(e.to_string()))
}

/// Verify a pairing bundle against the key it embeds, and confirm it does not lie
/// about its own fingerprint (the fingerprint must derive from the embedded key).
pub fn verify_pairing_bundle(token: &[u8]) -> Result<PairingBundle, EventError> {
    use coset::{CborSerializable, CoseSign1};
    let sign1 = CoseSign1::from_slice(token).map_err(|e| EventError::Cose(e.to_string()))?;
    let kid = sign1.protected.header.key_id.clone();
    let bytes: [u8; 32] = kid.as_slice().try_into().map_err(|_| EventError::BadKeyId)?;
    let vk = VerifyingKey::from_bytes(&bytes).map_err(|_| EventError::BadKeyId)?;
    sign1
        .verify_signature(b"", |sig, tbs| {
            let signature =
                ed25519_dalek::Signature::from_slice(sig).map_err(|_| EventError::BadSignature)?;
            vk.verify(tbs, &signature).map_err(|_| EventError::BadSignature)
        })
        .map_err(|_| EventError::BadSignature)?;
    let payload = sign1.payload.ok_or(EventError::NoPayload)?;
    let b: PairingBundle =
        ciborium::from_reader(&payload[..]).map_err(|e| EventError::Cbor(e.to_string()))?;
    // The bundle must be honest about the key it carries and that key's fingerprint.
    if b.pubkey_hex != hex::encode(bytes) || b.fingerprint != short_fingerprint(&b.pubkey_hex)? {
        return Err(EventError::SignerKeyMismatch);
    }
    Ok(b)
}
```

`use ed25519_dalek::Verifier;` is already imported at the top of the file (line 20).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cairn-event`
Expected: all PASS (the new two plus the existing suite).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-event/src/lib.rs
git commit -m "feat(cairn-event): node fingerprint + signed pairing bundle"
```

---

### Task 3: `db/007` — the `node_event` append-only table + `node_current` view

**Files:**
- Create: `db/007_node_federation.sql`
- Create: `db/tests/007_node_federation_test.sql`

**Interfaces:**
- Consumes: `pgcrypto` `digest()`; the append-only-trigger pattern from `db/004`.
- Produces: table `node_event(node_event_id UUID PK, op TEXT CHECK enroll|peer|revoke, author_node_id BYTEA, subject_node_id BYTEA, signer_key_id TEXT, peer_pubkey TEXT, fingerprint TEXT, role TEXT, scope_hint TEXT, target_event_id UUID, hlc_wall BIGINT, hlc_counter INT, node_origin TEXT, signed_bytes BYTEA, content_address BYTEA UNIQUE, recorded_at TIMESTAMPTZ)`; view `node_current(node_id, signer_key_id, recorded_at)` mapping a node's current key to its genesis `node_id`.

- [ ] **Step 1: Write the failing test**

Create `db/tests/007_node_federation_test.sql`:

```sql
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

ROLLBACK;
```

- [ ] **Step 2: Run it to verify it fails**

Run: `psql "$CAIRN_TEST_PG" -v ON_ERROR_STOP=1 -f db/tests/007_node_federation_test.sql`
Expected: FAIL — `relation "node_event" does not exist`.
(Set `CAIRN_TEST_PG`, e.g. `export CAIRN_TEST_PG="host=127.0.0.1 user=postgres dbname=cairn_dev"`, against a throwaway DB with `db/001`–`db/006` already loaded.)

- [ ] **Step 3: Write the migration (table + view + triggers)**

Create `db/007_node_federation.sql`:

```sql
-- Cairn — node identity & federation (ADR-0017). The actor-event algebra applied
-- to node-to-node relationships. Parallel to db/004 (actor_event): an append-only,
-- content-addressed, signed log of node enroll / peer / revoke events. node_id is
-- GENESIS-STABLE: it is the content-address of the genesis enroll event's signed
-- bytes (NOT the pinned-key hash db/004 uses for agents), so a future key rotation
-- keeps the node_id. Federation events reuse the cairn-event signed envelope
-- (nil patient, node.* type) but never touch the clinical event_log.

BEGIN;

CREATE TABLE IF NOT EXISTS node_event (
    node_event_id   UUID    PRIMARY KEY,            -- = body.event_id (UUIDv7), inside the signed bytes
    op              TEXT    NOT NULL CHECK (op IN ('enroll','peer','revoke')),
    author_node_id  BYTEA   NOT NULL,               -- node_id of the signer (self, for enroll)
    subject_node_id BYTEA   NOT NULL,               -- enroll: = author; peer/revoke: the peer
    signer_key_id   TEXT    NOT NULL,               -- hex Ed25519 public key of the author
    peer_pubkey     TEXT,                           -- peer/revoke: hex pubkey of the subject peer
    fingerprint     TEXT,                           -- peer: the operator-confirmed short fingerprint
    role            TEXT    CHECK (role IS NULL OR role IN ('upstream','downstream','peer')),
    scope_hint      TEXT,                           -- peer: optional default sync-scope label (ADR-0004)
    target_event_id UUID,                           -- revoke: the peer event it overlays
    hlc_wall        BIGINT  NOT NULL,
    hlc_counter     INTEGER NOT NULL,
    node_origin     TEXT    NOT NULL,
    signed_bytes    BYTEA   NOT NULL,
    content_address BYTEA   NOT NULL UNIQUE,
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT node_event_content_addressed
        CHECK (content_address = '\x1220'::bytea || digest(signed_bytes, 'sha256')),
    CONSTRAINT node_event_hlc_nonneg CHECK (hlc_wall >= 0 AND hlc_counter >= 0)
);

CREATE INDEX IF NOT EXISTS node_event_signer_idx  ON node_event (signer_key_id);
CREATE INDEX IF NOT EXISTS node_event_subject_idx ON node_event (subject_node_id);

CREATE OR REPLACE FUNCTION node_event_is_append_only()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'node_event is append-only: % is not permitted (Cairn principle #1/#2)', TG_OP;
END;
$$;
DROP TRIGGER IF EXISTS node_event_no_update ON node_event;
CREATE TRIGGER node_event_no_update BEFORE UPDATE OR DELETE ON node_event
    FOR EACH ROW EXECUTE FUNCTION node_event_is_append_only();

-- Map a node's CURRENT signing key to its genesis node_id (latest enroll per node,
-- no later revoke of that node). For v1 there is exactly one enroll per node_id.
CREATE OR REPLACE VIEW node_current AS
SELECT DISTINCT ON (ne.subject_node_id)
       ne.subject_node_id AS node_id, ne.signer_key_id, ne.recorded_at
FROM node_event ne
WHERE ne.op = 'enroll'
ORDER BY ne.subject_node_id, ne.recorded_at DESC;

-- This node's own identity (singleton). Set once by submit_node_event on genesis enroll.
CREATE TABLE IF NOT EXISTS local_node (
    id       BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    node_id  BYTEA NOT NULL,
    signer_key_id TEXT NOT NULL
);

COMMIT;
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `psql "$CAIRN_TEST_PG" -v ON_ERROR_STOP=1 -f db/007_node_federation.sql -f db/tests/007_node_federation_test.sql`
Expected: `node_current_maps_key = t`, and the two `RAISE NOTICE ... OK` lines; no error.

- [ ] **Step 5: Commit**

```bash
git add db/007_node_federation.sql db/tests/007_node_federation_test.sql
git commit -m "feat(db): node_event append-only table + node_current view"
```

---

### Task 4: `db/007` — `submit_node_event` authoring door + grant floor

**Files:**
- Modify: `db/007_node_federation.sql` (append the door + role + grants)
- Modify: `db/tests/007_node_federation_test.sql` (append authoring assertions)

**Interfaces:**
- Consumes: `cairn_verify(bytea)`, `cairn_body(bytea)` (existing pgrx); `node_event`, `node_current`, `local_node` (Task 3).
- Produces: `submit_node_event(p_signed BYTEA) RETURNS UUID` (SECURITY DEFINER). It verifies the signature in-DB, derives `op` from `event_type` (`node.enrolled`→enroll, `peer.added`→peer, `peer.revoked`→revoke), and:
  - **enroll:** allowed only when `local_node` is empty; `node_id := content_address`; sets `local_node`; `author = subject = node_id`.
  - **peer/revoke:** signer must equal `local_node.signer_key_id` (you author only your own peering); reads peer fields from `body.payload`; `author := local_node.node_id`.
  - Role `cairn_node` (NOLOGIN); `REVOKE` direct DML on `node_event`/`local_node`; `GRANT EXECUTE` on the door to `cairn_node`.

- [ ] **Step 1: Write the failing tests**

Append to `db/tests/007_node_federation_test.sql` (before the final `ROLLBACK;` — keep one `BEGIN; … ROLLBACK;` block; move the new asserts inside it). These use Rust-produced signed bytes, so they live in the **Rust** integration test instead; here assert only the *grant floor* and *fail-closed* paths reachable without a real signature:

```sql
-- cairn_node may not raw-INSERT into node_event (grant floor).
DO $$ BEGIN
    SET LOCAL ROLE cairn_node;
    BEGIN
        INSERT INTO node_event (node_event_id, op, author_node_id, subject_node_id,
            signer_key_id, hlc_wall, hlc_counter, node_origin, signed_bytes, content_address)
        VALUES (gen_random_uuid(),'enroll','\x00','\x00','k',0,0,'X','b','\x1220'||digest('b','sha256'));
        RESET ROLE; RAISE EXCEPTION 'grant-floor FAILED: raw INSERT succeeded';
    EXCEPTION WHEN insufficient_privilege THEN RESET ROLE; RAISE NOTICE 'grant-floor OK'; END;
END $$;

-- submit_node_event rejects unsigned/malformed bytes with a legible reason (fail closed).
DO $$ BEGIN
    BEGIN
        PERFORM submit_node_event('\xdeadbeef'::bytea);
        RAISE EXCEPTION 'fail-closed FAILED: malformed node event accepted';
    EXCEPTION WHEN others THEN
        IF SQLERRM LIKE '%signature%' OR SQLERRM LIKE '%verify%'
            THEN RAISE NOTICE 'fail-closed OK: %', SQLERRM; ELSE RAISE; END IF;
    END;
END $$;
```

- [ ] **Step 2: Run to verify it fails**

Run: `psql "$CAIRN_TEST_PG" -v ON_ERROR_STOP=1 -f db/007_node_federation.sql -f db/tests/007_node_federation_test.sql`
Expected: FAIL — `function submit_node_event(bytea) does not exist` and `role "cairn_node" does not exist`.

- [ ] **Step 3: Implement the door + grant floor**

Insert before `COMMIT;` in `db/007_node_federation.sql`:

```sql
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'cairn_node') THEN
        CREATE ROLE cairn_node NOLOGIN;
    END IF;
END $$;

-- The ONE local authoring door for node/peering events. Verifies in-DB, derives
-- op from event_type, and enforces: enroll is once-only and self; peer/revoke are
-- authored only by THIS node's current key. Every rejection is legible.
CREATE OR REPLACE FUNCTION submit_node_event(p_signed BYTEA)
RETURNS UUID
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public
AS $$
DECLARE
    b JSONB; v_type TEXT; v_op TEXT; v_ca BYTEA; v_eid UUID;
    v_local_node BYTEA; v_local_key TEXT; v_signer TEXT; v_payload JSONB;
BEGIN
    IF NOT cairn_verify(p_signed) THEN
        RAISE EXCEPTION 'submit_node_event: signature verification failed (unsigned or malformed)';
    END IF;
    b := cairn_body(p_signed);
    IF b IS NULL THEN
        RAISE EXCEPTION 'submit_node_event: body could not be parsed after verify';
    END IF;
    v_type   := b ->> 'event_type';
    v_eid    := (b ->> 'event_id')::uuid;
    v_signer := b ->> 'signer_key_id';
    v_payload := b -> 'payload';
    v_ca     := '\x1220'::bytea || digest(p_signed, 'sha256');
    v_op := CASE v_type
        WHEN 'node.enrolled' THEN 'enroll'
        WHEN 'peer.added'    THEN 'peer'
        WHEN 'peer.revoked'  THEN 'revoke'
        ELSE NULL END;
    IF v_op IS NULL THEN
        RAISE EXCEPTION 'submit_node_event: unknown node event_type % (fail closed)', v_type;
    END IF;

    SELECT node_id, signer_key_id INTO v_local_node, v_local_key FROM local_node WHERE id;

    IF v_op = 'enroll' THEN
        IF v_local_node IS NOT NULL THEN
            RAISE EXCEPTION 'submit_node_event: this node is already enrolled (genesis is once-only)';
        END IF;
        INSERT INTO node_event (node_event_id, op, author_node_id, subject_node_id,
            signer_key_id, hlc_wall, hlc_counter, node_origin, signed_bytes, content_address)
        VALUES (v_eid, 'enroll', v_ca, v_ca, v_signer,
            (b -> 'hlc' ->> 'wall')::bigint, (b -> 'hlc' ->> 'counter')::int,
            b -> 'hlc' ->> 'node_origin', p_signed, v_ca);
        INSERT INTO local_node (id, node_id, signer_key_id) VALUES (TRUE, v_ca, v_signer);
        RETURN v_eid;
    END IF;

    -- peer / revoke: authored only by this node's own current key.
    IF v_local_node IS NULL THEN
        RAISE EXCEPTION 'submit_node_event: node not yet enrolled; cannot author peering';
    END IF;
    IF v_signer <> v_local_key THEN
        RAISE EXCEPTION 'submit_node_event: peering may be authored only by this node (signer % != local %)', v_signer, v_local_key;
    END IF;

    INSERT INTO node_event (node_event_id, op, author_node_id, subject_node_id,
        signer_key_id, peer_pubkey, fingerprint, role, scope_hint, target_event_id,
        hlc_wall, hlc_counter, node_origin, signed_bytes, content_address)
    VALUES (v_eid, v_op, v_local_node,
        decode(v_payload ->> 'peer_node_id_hex','hex'),
        v_signer, v_payload ->> 'peer_pubkey', v_payload ->> 'fingerprint',
        v_payload ->> 'role', v_payload ->> 'scope_hint',
        NULLIF(v_payload ->> 'target_event_id','')::uuid,
        (b -> 'hlc' ->> 'wall')::bigint, (b -> 'hlc' ->> 'counter')::int,
        b -> 'hlc' ->> 'node_origin', p_signed, v_ca)
    ON CONFLICT (node_event_id) DO NOTHING;
    RETURN v_eid;
END;
$$;

REVOKE INSERT, UPDATE, DELETE ON node_event FROM PUBLIC;
REVOKE INSERT, UPDATE, DELETE ON node_event FROM cairn_node;
REVOKE INSERT, UPDATE, DELETE ON local_node FROM PUBLIC, cairn_node;
REVOKE EXECUTE ON FUNCTION submit_node_event(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION submit_node_event(bytea) TO cairn_node;
GRANT SELECT ON node_event, node_current, local_node TO cairn_node;
```

- [ ] **Step 4: Run to verify it passes**

Run: `psql "$CAIRN_TEST_PG" -v ON_ERROR_STOP=1 -f db/007_node_federation.sql -f db/tests/007_node_federation_test.sql`
Expected: `grant-floor OK` and `fail-closed OK` notices; no error. (Positive authoring is covered by the Rust integration test in Task 6/7, which can produce real signatures.)

- [ ] **Step 5: Commit**

```bash
git add db/007_node_federation.sql db/tests/007_node_federation_test.sql
git commit -m "feat(db): submit_node_event authoring door + grant floor"
```

---

### Task 5: `db/007` — `trust_peer` projection

**Files:**
- Modify: `db/007_node_federation.sql` (append the view)
- Modify: `db/tests/007_node_federation_test.sql` (append a peer→revoke fold assertion)

**Interfaces:**
- Produces: view `trust_peer(peer_node_id BYTEA, peer_pubkey TEXT, fingerprint TEXT, role TEXT, scope_hint TEXT, status TEXT, last_event_hlc …)` — the local node's currently-trusted peers: `peer` rows authored by `local_node.node_id`, with no later `revoke` of the same `subject_node_id`. A revoked peer appears with `status='revoked'` (row retained, never deleted).

- [ ] **Step 1: Write the failing test**

Append inside the `BEGIN; … ROLLBACK;` block of `db/tests/007_node_federation_test.sql`:

```sql
-- Seed a local node + a peer + then revoke it; trust_peer reflects active->revoked.
INSERT INTO local_node (id, node_id, signer_key_id) VALUES (TRUE, '\x1220'||digest('SELF','sha256'), 'selfkey')
    ON CONFLICT (id) DO NOTHING;
INSERT INTO node_event (node_event_id, op, author_node_id, subject_node_id, signer_key_id,
    peer_pubkey, fingerprint, role, hlc_wall, hlc_counter, node_origin, signed_bytes, content_address)
VALUES (gen_random_uuid(),'peer', '\x1220'||digest('SELF','sha256'), '\x1220'||digest('P','sha256'),
    'selfkey','pkey','AAAA-BBBB-CCCC-DDDD-EEEE','peer',1,0,'SELF','p1','\x1220'||digest('p1','sha256'));
SELECT (status = 'active') AS peer_is_active FROM trust_peer WHERE peer_node_id = '\x1220'||digest('P','sha256');

INSERT INTO node_event (node_event_id, op, author_node_id, subject_node_id, signer_key_id,
    hlc_wall, hlc_counter, node_origin, signed_bytes, content_address)
VALUES (gen_random_uuid(),'revoke', '\x1220'||digest('SELF','sha256'), '\x1220'||digest('P','sha256'),
    'selfkey',2,0,'SELF','p2','\x1220'||digest('p2','sha256'));
SELECT (status = 'revoked') AS peer_is_revoked FROM trust_peer WHERE peer_node_id = '\x1220'||digest('P','sha256');
```

- [ ] **Step 2: Run to verify it fails**

Run: `psql "$CAIRN_TEST_PG" -v ON_ERROR_STOP=1 -f db/007_node_federation.sql -f db/tests/007_node_federation_test.sql`
Expected: FAIL — `relation "trust_peer" does not exist`.

- [ ] **Step 3: Implement the projection**

Insert before `COMMIT;` in `db/007_node_federation.sql`:

```sql
-- The local node's trust set: peer assertions IT authored, graded active/revoked by
-- the latest op per subject. Read by the admission gate (Task 8) and the mTLS
-- cert-pin verifier (Task 9). A revoked peer is retained, never deleted (principle 2).
CREATE OR REPLACE VIEW trust_peer AS
SELECT DISTINCT ON (ne.subject_node_id)
       ne.subject_node_id AS peer_node_id,
       ne.peer_pubkey, ne.fingerprint, ne.role, ne.scope_hint,
       CASE ne.op WHEN 'revoke' THEN 'revoked' ELSE 'active' END AS status,
       ne.hlc_wall, ne.hlc_counter
FROM node_event ne
WHERE ne.op IN ('peer','revoke')
  AND ne.author_node_id = (SELECT node_id FROM local_node WHERE id)
ORDER BY ne.subject_node_id, ne.hlc_wall DESC, ne.hlc_counter DESC, ne.recorded_at DESC;

GRANT SELECT ON trust_peer TO cairn_node;
```

- [ ] **Step 4: Run to verify it passes**

Run: `psql "$CAIRN_TEST_PG" -v ON_ERROR_STOP=1 -f db/007_node_federation.sql -f db/tests/007_node_federation_test.sql`
Expected: `peer_is_active = t`, `peer_is_revoked = t`; all earlier notices still OK.

- [ ] **Step 5: Commit**

```bash
git add db/007_node_federation.sql db/tests/007_node_federation_test.sql
git commit -m "feat(db): trust_peer projection (active/revoked fold)"
```

---

### Task 6: `cairn-node` crate — keystore + `init` + `identity`

**Files:**
- Create: `crates/cairn-node/Cargo.toml`, `crates/cairn-node/src/main.rs`, `crates/cairn-node/src/keystore.rs`, `crates/cairn-node/src/db.rs`, `crates/cairn-node/src/identity.rs`
- Create: `crates/cairn-node/tests/provision.rs`

**Interfaces:**
- Consumes: `cairn_event::{generate_key, sign, EventBody, Hlc, SigningKey, event_address, short_fingerprint}`; `submit_node_event` door; `db/00x.sql`.
- Produces:
  - `keystore::generate_and_seal(path, passphrase: Option<&str>) -> Result<(SigningKey, String)>` and `keystore::load(path, passphrase) -> Result<SigningKey>` (OS perms 0600). **Recovery/escrow is an explicit stub** — a `// HONEST GAP (ADR-0026): no recovery-secret escrow; a lost key file = a lost node identity, recovered only by re-provisioning + supersede.` comment.
  - `identity::provision(db, sk, key_id, display_name, address) -> Result<NodeId>` — builds a `node.enrolled` `EventBody` (nil patient, `payload={display_name,address}`), signs it, calls `submit_node_event`, returns `node_id = hex(event_address(signed_bytes))`.
  - `identity::Identity { node_id_hex, pubkey_hex, fingerprint, address }` and `identity::load_local(db) -> Result<Identity>`.

- [ ] **Step 1: Write the crate manifest and the failing integration test**

`crates/cairn-node/Cargo.toml`:

```toml
[package]
name = "cairn-node"
version = "0.1.0"
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
cairn-event = { path = "../cairn-event" }
clap = { version = "4", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
tokio-postgres = "0.7"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
hex = "0.4"
uuid = { version = "1", features = ["v7"] }
thiserror = "1"
anyhow = "1"

[dev-dependencies]
# integration tests connect to a local Postgres via $CAIRN_TEST_PG
```

`crates/cairn-node/tests/provision.rs`:

```rust
// Gated on a live Postgres (set CAIRN_TEST_PG). Loads the schema into a fresh
// throwaway database, provisions a node, and asserts the genesis identity lands.
use cairn_node::{db, identity, keystore};

fn conn_str() -> Option<String> { std::env::var("CAIRN_TEST_PG").ok() }

#[tokio::test]
async fn provision_writes_genesis_identity() {
    let Some(cs) = conn_str() else { eprintln!("skipped: set CAIRN_TEST_PG"); return; };
    let client = db::connect_and_load_schema(&cs).await.unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let keypath = tmp.path().join("node.key");
    let (sk, kid) = keystore::generate_and_seal(&keypath, None).unwrap();

    let node_id = identity::provision(&client, &sk, &kid, "Clinic-A", "127.0.0.1:7800").await.unwrap();
    let loaded = identity::load_local(&client).await.unwrap();

    assert_eq!(loaded.node_id_hex, node_id);
    assert_eq!(loaded.pubkey_hex, kid);
    assert_eq!(loaded.fingerprint, cairn_event::short_fingerprint(&kid).unwrap());

    // Genesis is once-only: a second provision must error.
    assert!(identity::provision(&client, &sk, &kid, "Clinic-A", "127.0.0.1:7800").await.is_err());
}
```

Add `tempfile = "3"` to `[dev-dependencies]`. Create `crates/cairn-node/src/main.rs` with `pub mod db; pub mod identity; pub mod keystore;` plus a `lib.rs` re-exporting them (so the integration test can `use cairn_node::…`). Simplest: make it a lib+bin crate — add `src/lib.rs` with `pub mod db; pub mod identity; pub mod keystore;` and have `main.rs` `use cairn_node::…`.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cairn-node`
Expected: FAIL to compile — modules/functions not defined.

- [ ] **Step 3: Implement keystore, db, identity, and the CLI skeleton**

`crates/cairn-node/src/keystore.rs`:

```rust
use std::path::Path;
use cairn_event::{generate_key, SigningKey};

#[derive(thiserror::Error, Debug)]
pub enum KeystoreError {
    #[error("io: {0}")] Io(#[from] std::io::Error),
    #[error("key material: {0}")] Key(String),
}

// HONEST GAP (ADR-0026): v1 has NO recovery-secret escrow and NO passphrase KDF
// hardening beyond a raw at-rest file. A lost key file = a lost node identity,
// recoverable only by re-provisioning and a future `supersede`. `passphrase` is
// accepted but, for v1, the file is written with 0600 perms and no encryption;
// wiring a real KDF/seal is the ADR-0026 follow-on. This is surfaced in `status`.
pub fn generate_and_seal(path: &Path, _passphrase: Option<&str>) -> Result<(SigningKey, String), KeystoreError> {
    let (sk, kid) = generate_key().map_err(|e| KeystoreError::Key(e.to_string()))?;
    write_key_file(path, &sk.to_bytes())?;
    Ok((sk, kid))
}

pub fn load(path: &Path, _passphrase: Option<&str>) -> Result<SigningKey, KeystoreError> {
    let bytes = std::fs::read(path)?;
    let seed: [u8; 32] = bytes.as_slice().try_into().map_err(|_| KeystoreError::Key("not 32 bytes".into()))?;
    Ok(SigningKey::from_bytes(&seed))
}

#[cfg(unix)]
fn write_key_file(path: &Path, bytes: &[u8]) -> Result<(), KeystoreError> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    f.write_all(bytes)?;
    Ok(())
}
#[cfg(not(unix))]
fn write_key_file(path: &Path, bytes: &[u8]) -> Result<(), KeystoreError> {
    std::fs::write(path, bytes)?; Ok(())
}
```

`crates/cairn-node/src/db.rs`:

```rust
use tokio_postgres::{Client, NoTls};

const SCHEMA: [(&str, &str); 7] = [
    ("001_envelope",      include_str!("../../../db/001_envelope.sql")),
    ("002_projection",    include_str!("../../../db/002_projection.sql")),
    ("003_blobs",         include_str!("../../../db/003_blobs.sql")),
    ("004_actors",        include_str!("../../../db/004_actors.sql")),
    ("005_submit",        include_str!("../../../db/005_submit.sql")),
    ("006_recall",        include_str!("../../../db/006_recall.sql")),
    ("007_node_federation", include_str!("../../../db/007_node_federation.sql")),
];

pub async fn connect(conn: &str) -> anyhow::Result<Client> {
    let (client, connection) = tokio_postgres::connect(conn, NoTls).await?;
    tokio::spawn(async move { let _ = connection.await; });
    Ok(client)
}

pub async fn connect_and_load_schema(conn: &str) -> anyhow::Result<Client> {
    let client = connect(conn).await?;
    for (name, sql) in SCHEMA.iter() {
        client.batch_execute(sql).await.map_err(|e| anyhow::anyhow!("loading {name}: {e}"))?;
    }
    Ok(client)
}
```

`crates/cairn-node/src/identity.rs`:

```rust
use cairn_event::{event_address, short_fingerprint, sign, EventBody, Hlc, SigningKey};
use tokio_postgres::Client;

pub const NIL_PATIENT: &str = "00000000-0000-0000-0000-000000000000";

pub struct Identity {
    pub node_id_hex: String,
    pub pubkey_hex: String,
    pub fingerprint: String,
    pub address: String,
}

fn node_event_body(event_type: &str, signer_key_id: &str, node_origin: &str,
                   wall: i64, counter: i32, payload: serde_json::Value) -> EventBody {
    EventBody {
        event_id: uuid::Uuid::now_v7().to_string(),
        patient_id: NIL_PATIENT.into(),
        event_type: event_type.into(),
        schema_version: "node/1".into(),
        hlc: Hlc { wall, counter, node_origin: node_origin.into() },
        t_effective: None,
        signer_key_id: signer_key_id.into(),
        contributors: serde_json::json!([{"actor_id": signer_key_id, "role": "device"}]),
        payload,
        attachments: vec![],
    }
}

/// Author the genesis node.enrolled, submit it, return node_id (hex of its content-address).
pub async fn provision(db: &Client, sk: &SigningKey, key_id: &str, display_name: &str, address: &str)
    -> anyhow::Result<String> {
    let body = node_event_body("node.enrolled", key_id, display_name, 0, 0,
        serde_json::json!({"display_name": display_name, "address": address}));
    let signed = sign(&body, sk)?;
    let signed_bytes = signed.signed_bytes.clone();
    db.execute("SELECT submit_node_event($1)", &[&signed_bytes]).await?;
    Ok(hex::encode(event_address(&signed.signed_bytes)))
}

pub async fn load_local(db: &Client) -> anyhow::Result<Identity> {
    let row = db.query_one(
        "SELECT encode(ln.node_id,'hex') AS node_id_hex, ln.signer_key_id,
                ne.body_address AS addr
         FROM local_node ln
         JOIN LATERAL (SELECT (body -> 'payload' ->> 'address') AS body_address
                       FROM event_for_node_enroll(ln.node_id)) ne ON true
         WHERE ln.id", &[]).await
        // NOTE: the JOIN LATERAL above is illustrative; if you did not add a body
        // column/helper, read address from a node_event payload directly:
        ;
    // Simpler concrete query (use THIS one — no helper needed): node_event stores
    // signed_bytes, not parsed payload, so fetch address from the enroll row's body
    // via cairn_body in SQL is unavailable here; instead store address in local_node.
    let _ = row;
    unimplemented!("replaced by Step 3b")
}
```

- [ ] **Step 3b: Fix `load_local` — store address on `local_node` so it needs no body re-parse**

The migration's `local_node` does not carry `address`. Add it. Append a migration tweak to `db/007_node_federation.sql` (in the `local_node` `CREATE TABLE` add `address TEXT`), and set it in `submit_node_event`'s enroll branch:

```sql
-- in CREATE TABLE local_node, add:   address TEXT,
-- in the enroll INSERT into local_node, add the column + value:
INSERT INTO local_node (id, node_id, signer_key_id, address)
VALUES (TRUE, v_ca, v_signer, v_payload ->> 'address');
```

Then replace `load_local` with the concrete version:

```rust
pub async fn load_local(db: &Client) -> anyhow::Result<Identity> {
    let row = db.query_one(
        "SELECT encode(node_id,'hex') AS node_id_hex, signer_key_id, COALESCE(address,'') AS address
         FROM local_node WHERE id", &[]).await?;
    let pubkey_hex: String = row.get("signer_key_id");
    Ok(Identity {
        node_id_hex: row.get("node_id_hex"),
        fingerprint: short_fingerprint(&pubkey_hex)?,
        pubkey_hex,
        address: row.get("address"),
    })
}
```

(`node.enrolled` payload already carries `address`, so no body re-parse is needed.)

- [ ] **Step 3c: CLI skeleton (`init`, `identity`)**

`crates/cairn-node/src/lib.rs`: `pub mod db; pub mod identity; pub mod keystore;`
`crates/cairn-node/src/main.rs`:

```rust
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "cairn-node", about = "A Cairn federation node")]
struct Cli {
    #[arg(long, env = "CAIRN_CONN")] conn: String,
    #[arg(long, default_value = "node.key")] key: PathBuf,
    #[command(subcommand)] cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Provision this node: mint a keypair and append the genesis enrollment.
    Init { #[arg(long)] name: String, #[arg(long)] address: String },
    /// Print this node's identity (node_id, pubkey, fingerprint, address).
    Identity,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Init { name, address } => {
            let db = cairn_node::db::connect_and_load_schema(&cli.conn).await?;
            let (sk, kid) = cairn_node::keystore::generate_and_seal(&cli.key, None)?;
            let node_id = cairn_node::identity::provision(&db, &sk, &kid, &name, &address).await?;
            println!("provisioned node {node_id}\nfingerprint {}", cairn_event::short_fingerprint(&kid)?);
        }
        Cmd::Identity => {
            let db = cairn_node::db::connect(&cli.conn).await?;
            let id = cairn_node::identity::load_local(&db).await?;
            println!("node_id     {}\npubkey      {}\nfingerprint {}\naddress     {}",
                id.node_id_hex, id.pubkey_hex, id.fingerprint, id.address);
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Run the integration test to verify it passes**

Run: `CAIRN_TEST_PG="host=127.0.0.1 user=postgres dbname=cairn_node_test" cargo test -p cairn-node provision_writes_genesis_identity -- --nocapture`
Expected: PASS (genesis identity round-trips; second provision errors). Requires `cairn_pgx` installed in that DB (so `cairn_verify`/`cairn_body` exist) — see the walking-skeleton README pgrx section; create the extension once: `psql "$CAIRN_TEST_PG" -c "CREATE EXTENSION IF NOT EXISTS cairn_pgx;"`.

- [ ] **Step 5: Restore `crates/cairn-node` to the workspace members (if removed in Task 1) and commit**

```bash
cargo build -p cairn-node
git add crates/cairn-node Cargo.toml db/007_node_federation.sql
git commit -m "feat(cairn-node): keystore + provision + identity (init/identity)"
```

---

### Task 7: `cairn-node` pairing — `pair-offer`, `pair-accept`, `peers`, `unpeer`

**Files:**
- Create: `crates/cairn-node/src/pairing.rs`
- Modify: `crates/cairn-node/src/identity.rs` (add `author_peer`, `author_unpeer`, `list_peers`)
- Modify: `crates/cairn-node/src/main.rs` (wire the four subcommands)
- Create: `crates/cairn-node/tests/pairing.rs`

**Interfaces:**
- Consumes: `cairn_event::{PairingBundle, sign_pairing_bundle, verify_pairing_bundle, short_fingerprint}`; `submit_node_event`; `trust_peer`.
- Produces:
  - `pairing::make_offer(id: &Identity, sk, nonce) -> Result<String>` (base64 of the signed bundle).
  - `pairing::read_offer(b64) -> Result<PairingBundle>`.
  - `identity::author_peer(db, sk, key_id, local_node_origin, peer: &PairingBundle, role: Option<&str>) -> Result<String>` — builds a `peer.added` body whose `payload` = `{peer_node_id_hex, peer_pubkey, fingerprint, role, scope_hint}`, signs, submits.
  - `identity::author_unpeer(db, sk, key_id, local_node_origin, peer_node_id_hex) -> Result<String>` — `peer.revoked` body whose `payload.peer_node_id_hex` = the target; submits.
  - `identity::list_peers(db) -> Result<Vec<PeerRow>>` (`peer_node_id_hex, fingerprint, role, status`).

- [ ] **Step 1: Write the failing integration test**

`crates/cairn-node/tests/pairing.rs`:

```rust
use cairn_node::{db, identity, keystore, pairing};

fn conn_str() -> Option<String> { std::env::var("CAIRN_TEST_PG").ok() }

#[tokio::test]
async fn pairing_records_an_active_peer_and_unpeer_revokes_it() {
    let Some(cs) = conn_str() else { eprintln!("skipped"); return; };
    // Node A in this DB; "node B" is just a second keypair + a hand-built offer.
    let a = db::connect_and_load_schema(&cs).await.unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let (sk_a, kid_a) = keystore::generate_and_seal(&tmp.path().join("a.key"), None).unwrap();
    identity::provision(&a, &sk_a, &kid_a, "A", "127.0.0.1:7800").await.unwrap();
    let id_a = identity::load_local(&a).await.unwrap();

    // Build B's offer (B's genesis node_id is the content-address of ITS genesis;
    // for the test we only need a stable hex + B's pubkey + matching fingerprint).
    let (sk_b, kid_b) = cairn_event::generate_key().unwrap();
    let b_node_id = hex::encode(cairn_event::event_address(b"B-genesis"));
    let offer = pairing::make_offer_for(&b_node_id, &kid_b, "127.0.0.1:7801",
        "nonceB", &sk_b).unwrap();
    let bundle = pairing::read_offer(&offer).unwrap();
    assert_eq!(bundle.fingerprint, cairn_event::short_fingerprint(&kid_b).unwrap());

    identity::author_peer(&a, &sk_a, &kid_a, "A", &bundle, Some("downstream")).await.unwrap();
    let peers = identity::list_peers(&a).await.unwrap();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].status, "active");
    assert_eq!(peers[0].peer_node_id_hex, b_node_id);

    identity::author_unpeer(&a, &sk_a, &kid_a, "A", &b_node_id).await.unwrap();
    let peers = identity::list_peers(&a).await.unwrap();
    assert_eq!(peers[0].status, "revoked");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cairn-node pairing_records_an_active_peer`
Expected: FAIL to compile — `pairing` module / `author_peer` / `list_peers` not found.

- [ ] **Step 3: Implement pairing + peering authorship**

`crates/cairn-node/src/pairing.rs`:

```rust
use cairn_event::{sign_pairing_bundle, verify_pairing_bundle, short_fingerprint, Hlc, PairingBundle, SigningKey};
use base64::{engine::general_purpose::STANDARD, Engine};
use crate::identity::Identity;

pub fn make_offer(id: &Identity, sk: &SigningKey, nonce: &str) -> anyhow::Result<String> {
    make_offer_for(&id.node_id_hex, &id.pubkey_hex, &id.address, nonce, sk)
}

pub fn make_offer_for(node_id_hex: &str, pubkey_hex: &str, address: &str, nonce: &str, sk: &SigningKey)
    -> anyhow::Result<String> {
    let b = PairingBundle {
        node_id_hex: node_id_hex.into(),
        pubkey_hex: pubkey_hex.into(),
        address: address.into(),
        fingerprint: short_fingerprint(pubkey_hex)?,
        nonce: nonce.into(),
        hlc: Hlc { wall: 0, counter: 0, node_origin: node_id_hex.into() },
    };
    Ok(STANDARD.encode(sign_pairing_bundle(&b, sk)?))
}

pub fn read_offer(b64: &str) -> anyhow::Result<PairingBundle> {
    let raw = STANDARD.decode(b64.trim())?;
    Ok(verify_pairing_bundle(&raw)?) // verifies signature + self-consistent fingerprint
}
```

Add `base64 = "0.22"` to `Cargo.toml`. Append to `crates/cairn-node/src/identity.rs`:

```rust
use cairn_event::PairingBundle;

pub struct PeerRow { pub peer_node_id_hex: String, pub fingerprint: String,
                     pub role: Option<String>, pub status: String }

pub async fn author_peer(db: &Client, sk: &SigningKey, key_id: &str, node_origin: &str,
                         peer: &PairingBundle, role: Option<&str>) -> anyhow::Result<String> {
    let body = node_event_body("peer.added", key_id, node_origin, 0, 0, serde_json::json!({
        "peer_node_id_hex": peer.node_id_hex,
        "peer_pubkey": peer.pubkey_hex,
        "fingerprint": peer.fingerprint,
        "role": role,
    }));
    let signed = sign(&body, sk)?;
    let bytes = signed.signed_bytes.clone();
    db.execute("SELECT submit_node_event($1)", &[&bytes]).await?;
    Ok(hex::encode(event_address(&signed.signed_bytes)))
}

pub async fn author_unpeer(db: &Client, sk: &SigningKey, key_id: &str, node_origin: &str,
                           peer_node_id_hex: &str) -> anyhow::Result<String> {
    let body = node_event_body("peer.revoked", key_id, node_origin, 0, 0, serde_json::json!({
        "peer_node_id_hex": peer_node_id_hex,
    }));
    let signed = sign(&body, sk)?;
    let bytes = signed.signed_bytes.clone();
    db.execute("SELECT submit_node_event($1)", &[&bytes]).await?;
    Ok(hex::encode(event_address(&signed.signed_bytes)))
}

pub async fn list_peers(db: &Client) -> anyhow::Result<Vec<PeerRow>> {
    let rows = db.query(
        "SELECT encode(peer_node_id,'hex') AS pid, COALESCE(fingerprint,'') AS fp, role, status
         FROM trust_peer ORDER BY pid", &[]).await?;
    Ok(rows.iter().map(|r| PeerRow {
        peer_node_id_hex: r.get("pid"), fingerprint: r.get("fp"),
        role: r.get("role"), status: r.get("status"),
    }).collect())
}
```

> **HLC note:** v1 uses `wall:0, counter:0` for node events (peering volume is tiny and human-paced; ordering within one node is by `recorded_at`, the tiebreak in `trust_peer`). A real HLC advance per write is a Phase-2 follow-on shared with clinical sync; it is **not** required for correctness of the active/revoked fold here because the projection also tiebreaks on `recorded_at DESC`.

- [ ] **Step 3b: Wire the CLI subcommands**

Add to `Cmd` in `main.rs`: `PairOffer { #[arg(long, default_value="cairn")] nonce: String }`, `PairAccept { offer: String, #[arg(long)] role: Option<String> }`, `Peers`, `Unpeer { node_id: String }`. Each loads the key (`keystore::load`), connects, and calls the matching `identity`/`pairing` function. `PairAccept` must **print the fingerprint and require confirmation** before authoring:

```rust
Cmd::PairAccept { offer, role } => {
    let bundle = cairn_node::pairing::read_offer(&offer)?;
    eprintln!("Peer fingerprint: {}\nConfirm it matches what the peer displays, then type YES:", bundle.fingerprint);
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    if line.trim() != "YES" { anyhow::bail!("pairing aborted: fingerprint not confirmed"); }
    let sk = cairn_node::keystore::load(&cli.key, None)?;
    let db = cairn_node::db::connect(&cli.conn).await?;
    let id = cairn_node::identity::load_local(&db).await?;
    let kid = id.pubkey_hex.clone();
    cairn_node::identity::author_peer(&db, &sk, &kid, &id.node_id_hex, &bundle, role.as_deref()).await?;
    println!("peered with {}", bundle.node_id_hex);
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `CAIRN_TEST_PG="host=127.0.0.1 user=postgres dbname=cairn_node_test2" cargo test -p cairn-node pairing_records_an_active_peer -- --nocapture`
Expected: PASS — one active peer after `author_peer`, `revoked` after `author_unpeer`.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-node Cargo.toml
git commit -m "feat(cairn-node): out-of-band pairing + peer/unpeer authoring"
```

---

## PHASE 2 — Transport & sync (the wire)

### Task 8: `db/007` — the remote-apply admission gate

**Files:**
- Modify: `db/007_node_federation.sql` (append `apply_remote_node_event`)
- Modify: `db/tests/007_node_federation_test.sql` (append admission assertions reachable via SQL)
- Create: `crates/cairn-node/tests/admission.rs` (the signature-dependent positive/negative cases)

**Interfaces:**
- Consumes: `cairn_verify`, `cairn_body`, `node_event`, `node_current`, `trust_peer`, `local_node`.
- Produces: `apply_remote_node_event(p_signed BYTEA) RETURNS UUID` (SECURITY DEFINER). Admits an inbound node event **iff** it verifies AND:
  - **enroll:** its `content_address` equals a `trust_peer.peer_node_id` that is `active` **and** its `signer_key_id` equals that peer's recorded `peer_pubkey` (the out-of-band-confirmed identity matches the synced genesis). On admit, the row's `node_current` lets later peer/revoke from that node resolve.
  - **peer/revoke:** its `signer_key_id` resolves via `node_current` to a `node_id` that is `active` in `trust_peer`.
  - Idempotent (`ON CONFLICT (node_event_id) DO NOTHING`); legible rejection otherwise. Grant `EXECUTE` to `cairn_node`.

- [ ] **Step 1: Write the failing SQL test (the reject-by-default path)**

Append inside the `BEGIN; … ROLLBACK;` block of `db/tests/007_node_federation_test.sql`:

```sql
-- A well-formed but UNSIGNED blob is rejected by the admission gate (fail closed).
DO $$ BEGIN
    BEGIN
        PERFORM apply_remote_node_event('\xdeadbeef'::bytea);
        RAISE EXCEPTION 'admission FAILED: malformed remote event accepted';
    EXCEPTION WHEN others THEN
        IF SQLERRM LIKE '%verify%' OR SQLERRM LIKE '%signature%'
            THEN RAISE NOTICE 'admission fail-closed OK'; ELSE RAISE; END IF;
    END;
END $$;
```

- [ ] **Step 2: Run to verify it fails**

Run: `psql "$CAIRN_TEST_PG" -v ON_ERROR_STOP=1 -f db/007_node_federation.sql -f db/tests/007_node_federation_test.sql`
Expected: FAIL — `function apply_remote_node_event(bytea) does not exist`.

- [ ] **Step 3: Implement the admission gate**

Insert before `COMMIT;` in `db/007_node_federation.sql`:

```sql
-- The federation admission seam (ADR-0017 §8): the one safety-critical gate. An
-- inbound, peer-authored node event enters the log only if it verifies AND its
-- author is an out-of-band-confirmed, currently-active peer. Reject is legible.
CREATE OR REPLACE FUNCTION apply_remote_node_event(p_signed BYTEA)
RETURNS UUID
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public
AS $$
DECLARE
    b JSONB; v_type TEXT; v_op TEXT; v_ca BYTEA; v_eid UUID; v_signer TEXT;
    v_payload JSONB; v_author_node BYTEA;
BEGIN
    IF NOT cairn_verify(p_signed) THEN
        RAISE EXCEPTION 'apply_remote_node_event: signature verification failed';
    END IF;
    b := cairn_body(p_signed);
    v_type := b ->> 'event_type'; v_eid := (b ->> 'event_id')::uuid;
    v_signer := b ->> 'signer_key_id'; v_payload := b -> 'payload';
    v_ca := '\x1220'::bytea || digest(p_signed, 'sha256');
    v_op := CASE v_type WHEN 'node.enrolled' THEN 'enroll' WHEN 'peer.added' THEN 'peer'
                        WHEN 'peer.revoked' THEN 'revoke' ELSE NULL END;
    IF v_op IS NULL THEN
        RAISE EXCEPTION 'apply_remote_node_event: unknown node event_type % (fail closed)', v_type;
    END IF;

    IF v_op = 'enroll' THEN
        -- The genesis must match an active, out-of-band-confirmed peer: its
        -- content-address is the node_id we trust, and its key is the pubkey we pinned.
        IF NOT EXISTS (SELECT 1 FROM trust_peer
                       WHERE peer_node_id = v_ca AND status = 'active' AND peer_pubkey = v_signer) THEN
            RAISE EXCEPTION 'apply_remote_node_event: genesis from an un-trusted or mismatched node (deny-all default)';
        END IF;
        INSERT INTO node_event (node_event_id, op, author_node_id, subject_node_id,
            signer_key_id, hlc_wall, hlc_counter, node_origin, signed_bytes, content_address)
        VALUES (v_eid, 'enroll', v_ca, v_ca, v_signer,
            (b -> 'hlc' ->> 'wall')::bigint, (b -> 'hlc' ->> 'counter')::int,
            b -> 'hlc' ->> 'node_origin', p_signed, v_ca)
        ON CONFLICT (node_event_id) DO NOTHING;
        RETURN v_eid;
    END IF;

    -- peer/revoke: the author must be a currently-trusted peer (resolved by key).
    SELECT node_id INTO v_author_node FROM node_current WHERE signer_key_id = v_signer;
    IF v_author_node IS NULL THEN
        RAISE EXCEPTION 'apply_remote_node_event: author key % maps to no known node', v_signer;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM trust_peer WHERE peer_node_id = v_author_node AND status = 'active') THEN
        RAISE EXCEPTION 'apply_remote_node_event: author % is not an active peer (deny-all)', encode(v_author_node,'hex');
    END IF;
    INSERT INTO node_event (node_event_id, op, author_node_id, subject_node_id,
        signer_key_id, peer_pubkey, fingerprint, role, scope_hint, target_event_id,
        hlc_wall, hlc_counter, node_origin, signed_bytes, content_address)
    VALUES (v_eid, v_op, v_author_node,
        decode(COALESCE(v_payload ->> 'peer_node_id_hex','00'),'hex'),
        v_signer, v_payload ->> 'peer_pubkey', v_payload ->> 'fingerprint',
        v_payload ->> 'role', v_payload ->> 'scope_hint',
        NULLIF(v_payload ->> 'target_event_id','')::uuid,
        (b -> 'hlc' ->> 'wall')::bigint, (b -> 'hlc' ->> 'counter')::int,
        b -> 'hlc' ->> 'node_origin', p_signed, v_ca)
    ON CONFLICT (node_event_id) DO NOTHING;
    RETURN v_eid;
END;
$$;

REVOKE EXECUTE ON FUNCTION apply_remote_node_event(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION apply_remote_node_event(bytea) TO cairn_node;
```

- [ ] **Step 4: Write the signature-dependent Rust admission test**

`crates/cairn-node/tests/admission.rs` — two real nodes' DBs in one test, exercising admit + the two hostile rejects (un-peered signer, revoked peer):

```rust
use cairn_node::{db, identity, keystore};
use cairn_event::{sign, EventBody, Hlc, event_address};

fn cs() -> Option<String> { std::env::var("CAIRN_TEST_PG").ok() }

#[tokio::test]
async fn admission_admits_trusted_peer_genesis_and_rejects_strangers() {
    let Some(base) = cs() else { eprintln!("skipped"); return; };
    let a = db::connect_and_load_schema(&base).await.unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let (sk_a, kid_a) = keystore::generate_and_seal(&tmp.path().join("a.key"), None).unwrap();
    identity::provision(&a, &sk_a, &kid_a, "A", "127.0.0.1:7800").await.unwrap();

    // B's genesis (authored against B's own key), captured as signed bytes.
    let (sk_b, kid_b) = cairn_event::generate_key().unwrap();
    let body_b = EventBody {
        event_id: uuid::Uuid::now_v7().to_string(), patient_id: identity::NIL_PATIENT.into(),
        event_type: "node.enrolled".into(), schema_version: "node/1".into(),
        hlc: Hlc { wall: 0, counter: 0, node_origin: "B".into() }, t_effective: None,
        signer_key_id: kid_b.clone(),
        contributors: serde_json::json!([{"actor_id": kid_b, "role": "device"}]),
        payload: serde_json::json!({"display_name":"B","address":"127.0.0.1:7801"}), attachments: vec![],
    };
    let signed_b = sign(&body_b, &sk_b).unwrap();
    let b_node_id = hex::encode(event_address(&signed_b.signed_bytes));

    // Before A peers with B, B's genesis is rejected (deny-all).
    let bytes = signed_b.signed_bytes.clone();
    let r = a.execute("SELECT apply_remote_node_event($1)", &[&bytes]).await;
    assert!(r.is_err(), "un-trusted genesis must be rejected");

    // A pairs with B (records peer.added with B's real node_id + pubkey + fingerprint).
    let bundle = cairn_event::PairingBundle {
        node_id_hex: b_node_id.clone(), pubkey_hex: kid_b.clone(),
        address: "127.0.0.1:7801".into(),
        fingerprint: cairn_event::short_fingerprint(&kid_b).unwrap(),
        nonce: "n".into(), hlc: Hlc { wall: 0, counter: 0, node_origin: b_node_id.clone() },
    };
    identity::author_peer(&a, &sk_a, &kid_a, "A", &bundle, Some("peer")).await.unwrap();

    // Now B's genesis is admitted.
    let bytes = signed_b.signed_bytes.clone();
    a.execute("SELECT apply_remote_node_event($1)", &[&bytes]).await.unwrap();

    // After unpeering B, a NEW B-authored peer event is rejected.
    identity::author_unpeer(&a, &sk_a, &kid_a, "A", &b_node_id).await.unwrap();
    let body_b2 = EventBody { event_id: uuid::Uuid::now_v7().to_string(),
        event_type: "peer.added".into(),
        payload: serde_json::json!({"peer_node_id_hex":"aa","peer_pubkey":"bb","fingerprint":"X"}),
        ..body_b.clone() };
    let signed_b2 = sign(&body_b2, &sk_b).unwrap();
    let bytes = signed_b2.signed_bytes.clone();
    assert!(a.execute("SELECT apply_remote_node_event($1)", &[&bytes]).await.is_err(),
        "events from a revoked peer must be rejected");
}
```

- [ ] **Step 5: Run both tests to verify they pass, then commit**

Run: `psql "$CAIRN_TEST_PG" -v ON_ERROR_STOP=1 -f db/007_node_federation.sql -f db/tests/007_node_federation_test.sql`
Run: `CAIRN_TEST_PG="host=127.0.0.1 user=postgres dbname=cairn_admit_test" cargo test -p cairn-node admission_admits_trusted_peer -- --nocapture`
Expected: both PASS — fail-closed notice in SQL; admit + two rejects in Rust.

```bash
git add db/007_node_federation.sql db/tests/007_node_federation_test.sql crates/cairn-node/tests/admission.rs
git commit -m "feat(db): remote-apply admission gate (deny-all, peer-pinned)"
```

---

### Task 9: `cairn-node` transport — mTLS pinned to `trust_peer`

**Files:**
- Create: `crates/cairn-node/src/transport.rs`
- Modify: `crates/cairn-node/Cargo.toml` (rustls/rcgen deps)
- Create: `crates/cairn-node/tests/transport.rs`

**Interfaces:**
- Consumes: the node's `SigningKey` (Ed25519); `trust_peer` (for pin lookups, via a `TrustStore` closure).
- Produces:
  - `transport::node_cert(sk: &SigningKey) -> Result<(rustls::pki_types::CertificateDer, rustls::pki_types::PrivateKeyDer)>` — a self-signed Ed25519 cert whose SPKI key IS the node's signing key (via `rcgen` from the existing keypair).
  - `transport::server_config(sk, trust: TrustStore) -> Result<Arc<rustls::ServerConfig>>` — presents the node cert; a custom `ClientCertVerifier` admits a client iff its cert's Ed25519 SPKI public key (hex) is `active` in `trust_peer`.
  - `transport::client_config(sk, trust: TrustStore) -> Result<Arc<rustls::ClientConfig>>` — presents the node cert; a custom `ServerCertVerifier` pins the server's key the same way.
  - `type TrustStore = Arc<dyn Fn(&str /*pubkey_hex*/) -> bool + Send + Sync>` — backed by `SELECT 1 FROM trust_peer WHERE peer_pubkey=$1 AND status='active'`.

> **Crate-version note:** confirm the current `rustls` (0.23) `ClientCertVerifier`/`ServerCertVerifier` trait surface and `rcgen` Ed25519-from-existing-key API with context7 (`/rustls/rustls`, `/rustls/rcgen`) before writing the verifier bodies — the trait method names move between minor versions. The pinning *logic* (extract SPKI Ed25519 public key from the presented cert DER, hex-encode, ask `TrustStore`) is stable regardless.

- [ ] **Step 1: Write the failing test (round-trip a pinned mTLS session)**

`crates/cairn-node/tests/transport.rs`:

```rust
use cairn_node::transport;
use std::sync::Arc;

#[tokio::test]
async fn mtls_accepts_pinned_peer_and_rejects_unpinned() {
    let (sk_a, kid_a) = cairn_event::generate_key().unwrap();
    let (sk_b, kid_b) = cairn_event::generate_key().unwrap();
    let (_sk_c, kid_c) = cairn_event::generate_key().unwrap();

    // A trusts B only.
    let kid_b2 = kid_b.clone();
    let trust_a: transport::TrustStore = Arc::new(move |pk: &str| pk == kid_b2);
    // B trusts A only.
    let kid_a2 = kid_a.clone();
    let trust_b: transport::TrustStore = Arc::new(move |pk: &str| pk == kid_a2);

    // A serves; B connects -> handshake succeeds (mutually pinned).
    let server = transport::server_config(&sk_a, trust_a.clone()).unwrap();
    let client_b = transport::client_config(&sk_b, trust_b).unwrap();
    assert!(transport::test_handshake(server.clone(), client_b).await.is_ok(),
        "mutually-pinned peers must handshake");

    // C (untrusted by A) connects -> A's ClientCertVerifier rejects.
    let trust_c_sees_a: transport::TrustStore = Arc::new(move |pk: &str| pk == kid_a);
    let client_c = transport::client_config(&_sk_c, trust_c_sees_a).unwrap();
    let _ = kid_c; let _ = kid_b;
    assert!(transport::test_handshake(server, client_c).await.is_err(),
        "an unpinned client must be rejected at the TLS layer");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cairn-node mtls_accepts_pinned_peer`
Expected: FAIL to compile — `transport` module not found.

- [ ] **Step 3: Add deps and implement the transport module**

Add to `crates/cairn-node/Cargo.toml`:

```toml
rustls = "0.23"
tokio-rustls = "0.26"
rcgen = "0.13"
x509-parser = "0.16"
```

Implement `crates/cairn-node/src/transport.rs` with: `node_cert` (use `rcgen` to build a self-signed cert from the node's Ed25519 `SigningKey` — `rcgen::KeyPair::from_pkcs8`/`from_der` against the PKCS#8 encoding of the seed, or `rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519)` reusing the same seed bytes); the two custom verifiers, each of which uses `x509-parser` to pull the SubjectPublicKeyInfo, hex-encode the 32 raw Ed25519 bytes, and return `Ok(...)` only when `trust(&pubkey_hex)` is true (else a `rustls::Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure)`); and a `test_handshake(server, client)` helper that runs a `tokio::io::duplex` pair through `tokio_rustls::TlsAcceptor`/`TlsConnector` and returns `Ok(())` iff both directions complete the handshake.

> The full verifier bodies depend on the rustls 0.23 trait surface (`danger::ServerCertVerifier`, `server::danger::ClientCertVerifier`). Fetch the exact signatures with context7 and implement the four required methods (`verify_server_cert`/`verify_client_cert` do the pin check; `verify_tls12_signature`/`verify_tls13_signature` delegate to `rustls::crypto::verify_tls13_signature` etc.; `supported_verify_schemes` returns `ED25519`). Keep each verifier under ~60 lines and reviewer-legible (§9).

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p cairn-node mtls_accepts_pinned_peer -- --nocapture`
Expected: PASS — pinned handshake succeeds, unpinned is rejected.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-node/src/transport.rs crates/cairn-node/Cargo.toml crates/cairn-node/tests/transport.rs
git commit -m "feat(cairn-node): mTLS transport pinned to the trust set"
```

---

### Task 10: `cairn-node` federation sync — `node_event` set-union over mTLS

**Files:**
- Create: `crates/cairn-node/src/sync.rs`
- Modify: `crates/cairn-node/src/main.rs` (`serve`, `run` subcommands)
- Create: `crates/cairn-node/tests/federation.rs` (placeholder; the full two-node E2E is Task 12)

**Interfaces:**
- Consumes: `transport::{server_config, client_config, TrustStore}`; `apply_remote_node_event`; `node_event`.
- Produces:
  - `sync::serve(listen: SocketAddr, db, sk, trust) -> Future` — accepts mTLS sessions; on a `{op:"NodeEventsAfter", after_id: Option<Uuid>}` request, streams every `node_event.signed_bytes` (length-framed, raw bytes; ordered by `recorded_at, node_event_id`).
  - `sync::pull_once(peer_addr, db, sk, trust) -> Result<PullStats>` — connects, requests all node events, calls `apply_remote_node_event` for each (idempotent), returns `{received, admitted, rejected}` (rejections are logged with the legible DB reason, never fatal — deny-all is normal).
  - The wire protocol reuses the `cairn-sync` length-prefixed framing (`write_frame`/`read_frame`) but over the `tokio_rustls` stream; node events ship as raw `signed_bytes` (small, but binary — no hex).

- [ ] **Step 1: Write the failing test (a pull applies a trusted peer's events)**

In `crates/cairn-node/tests/federation.rs` write a single-process test that stands up a `serve` task bound to `127.0.0.1:0` holding node A's events, and a second DB acting as B that has peered with A, then calls `sync::pull_once` and asserts B admitted A's genesis. (Full bidirectional convergence is Task 12; this proves one direction end-to-end.) Use two databases via `CAIRN_TEST_PG` + `CAIRN_TEST_PG2`.

```rust
// crates/cairn-node/tests/federation.rs (Task 10 portion)
// Asserts: B, having peered with A, pulls and admits A's genesis over mTLS.
// (Skips unless both CAIRN_TEST_PG and CAIRN_TEST_PG2 are set.)
```

(Write the concrete body following the `admission.rs` setup pattern: provision A in DB1, provision B in DB2, have each `author_peer` the other from a hand-built bundle, start `sync::serve` for A, `sync::pull_once` from B, then `SELECT count(*) FROM node_event WHERE op='enroll'` on B equals 2.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cairn-node --test federation`
Expected: FAIL to compile — `sync` module not found.

- [ ] **Step 3: Implement `sync.rs` (serve + pull) and wire `serve`/`run`**

Implement the framed request/response over `tokio_rustls` streams. `serve` loop: `accept` → TLS handshake (rejects unpinned clients via the Task 9 `ClientCertVerifier`) → read one request frame → if `NodeEventsAfter`, `SELECT signed_bytes FROM node_event ORDER BY recorded_at, node_event_id` → write each as a frame → close. `pull_once`: connect + TLS → write `NodeEventsAfter{after_id:None}` → read frames until EOF → for each, `SELECT apply_remote_node_event($1)` (catch + log per-event errors, increment `rejected`). The `TrustStore` closures wrap a `tokio_postgres::Client` query `SELECT 1 FROM trust_peer WHERE peer_pubkey=$1 AND status='active'`.

`run` is `serve` + a `pull_once` loop on an interval that survives connect errors (log a partition, keep going) — mirroring `cairn-sync run`.

- [ ] **Step 4: Run to verify it passes**

Run: `CAIRN_TEST_PG=… CAIRN_TEST_PG2=… cargo test -p cairn-node --test federation -- --nocapture`
Expected: PASS — B holds 2 enroll rows after the pull (its own + A's, admitted over mTLS).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-node/src/sync.rs crates/cairn-node/src/main.rs crates/cairn-node/tests/federation.rs
git commit -m "feat(cairn-node): node_event set-union sync over mTLS"
```

---

### Task 11: `cairn-node status` — honest assembly state

**Files:**
- Modify: `crates/cairn-node/src/identity.rs` (add `status`)
- Modify: `crates/cairn-node/src/main.rs` (`Status` subcommand)
- Create: `crates/cairn-node/tests/status.rs`

**Interfaces:**
- Produces: `identity::status(db, key_path) -> Result<Status>` returning `{ node_id_hex, peers_active, peers_revoked, keystore_ok, dr_escrow: "STUBBED (ADR-0026)" }`. `keystore_ok` = the key file exists and loads; if not, status still renders and flags **"cannot author (keystore unreadable)"** rather than erroring (honest degradation).

- [ ] **Step 1: Write the failing test**

`crates/cairn-node/tests/status.rs`: provision a node, add one active + one revoked peer (reuse the `admission.rs` helpers), assert `status` reports `peers_active==…`, `peers_revoked==1`, `keystore_ok==true`, and `dr_escrow` contains `"STUBBED"`. Then point `key_path` at a missing file and assert `keystore_ok==false` **without** the call erroring.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cairn-node --test status`
Expected: FAIL — `status` not found.

- [ ] **Step 3: Implement `status`**

Query `trust_peer` grouped by `status`; check `keystore::load(key_path, None).is_ok()`; hard-code `dr_escrow: "STUBBED (ADR-0026): no recovery escrow; key loss = node loss"`. Wire the `Status` subcommand to print each field, one per line.

- [ ] **Step 4: Run to verify it passes**

Run: `CAIRN_TEST_PG=… cargo test -p cairn-node --test status -- --nocapture`
Expected: PASS — counts correct; missing-key path flags `keystore_ok=false` without panicking.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-node
git commit -m "feat(cairn-node): honest-assembly status (peers, keystore, DR stub)"
```

---

### Task 12: End-to-end — two nodes converge, strangers rejected

**Files:**
- Modify: `crates/cairn-node/tests/federation.rs` (add the full bidirectional E2E)

**Interfaces:**
- Consumes: everything above. No new production code — this task is the acceptance test for the slice.

- [ ] **Step 1: Write the end-to-end test**

In `crates/cairn-node/tests/federation.rs`, add `two_nodes_converge_then_unpeer_and_a_stranger_is_rejected`:
1. Provision A (DB1) and B (DB2) with `sync::serve` tasks on ephemeral ports.
2. Exchange offers (`pairing::make_offer`), each `read_offer` + `author_peer` the other (confirming fingerprints — call `author_peer` directly in-test, bypassing the stdin prompt).
3. `pull_once` A→B and B→A. Assert both DBs hold **2 enroll rows + 2 peer rows** (set-union convergence) and `trust_peer` on each shows the other `active`.
4. Provision a third node C (DB3) that nobody peered with; start its `serve`. From B, `pull_once` against C. Assert B admits **nothing** from C (C is un-peered → C's `serve` rejects B's unpinned cert at TLS, OR B rejects C's; either way zero rows transfer) and B's row counts are unchanged.
5. On A, `author_unpeer(B)`; `pull_once` A from B again; assert A now **rejects** any new B-authored peer event (admission `rejected` count ≥ 1, row count for that event stays 0).

```rust
// Asserts the full slice acceptance criteria. Skips unless CAIRN_TEST_PG,
// CAIRN_TEST_PG2, CAIRN_TEST_PG3 are set to three throwaway databases, each with
// cairn_pgx installed.
```

- [ ] **Step 2: Run to verify it fails (before wiring helpers), then passes**

Run: `CAIRN_TEST_PG=… CAIRN_TEST_PG2=… CAIRN_TEST_PG3=… cargo test -p cairn-node --test federation two_nodes_converge -- --nocapture`
Expected: PASS — convergence holds, stranger transfers zero rows, post-unpeer events rejected.

- [ ] **Step 3: Full-suite green**

Run: `cargo test --workspace` (the non-DB units) and the DB-gated `cargo test -p cairn-node` with the three test DBs set.
Expected: all PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-node/tests/federation.rs
git commit -m "test(cairn-node): two-node federation E2E (converge, unpeer, stranger-reject)"
```

---

## Self-Review

**Spec coverage** (each spec §, mapped to a task):
- §3.1 graduation → T1. §3.2 blast-radius → enforced across T3–T10 (in-DB doors + Rust). §3.3 event algebra → T3/T4/T7. §3.4 trust_peer → T5. §3.5 mTLS pinned → T9. §3.6 admission gate → T8. §4 ceremonies: `init`/`identity` → T6, `pair-offer`/`pair-accept`/`peers`/`unpeer` → T7, `serve`/`run` → T10, `status` → T11. §5 error/honest-assembly → T11 (keystore degradation), T10 (partition non-fatal), T8 (legible rejects). §6 testing → unit (T2), DB (T3–T5,T8), in-DB hostile (T8), E2E (T12). §7 traceability → preserved in code comments citing ADRs. Non-goals (no EHR, no other anchors, no rotation/DR) → respected; DR stub explicit in T6/T11.
- **Gap fixed during review:** `load_local` needed the node address; added `local_node.address` (T6 Step 3b) so no body re-parse is required.
- **Gap noted:** v1 node-event HLC is `(0,0)`; the `trust_peer` fold tiebreaks on `recorded_at`, so correctness holds. A real HLC advance is a Phase-2-shared follow-on, called out in T7 — not silently skipped.

**Placeholder scan:** no "TBD/TODO/handle edge cases". The two places that defer to current crate docs (rustls 0.23 verifier trait bodies, rcgen Ed25519 API in T9) give the exact pinning logic + the context7 lookup to confirm signatures — concrete intent, not a placeholder. The `unimplemented!` in T6 Step 3 is deliberately replaced by Step 3b in the same task.

**Type consistency:** `submit_node_event(bytea)`/`apply_remote_node_event(bytea)` signatures match between SQL and the Rust `db.execute("SELECT …($1)", &[&bytes])` calls; `PeerRow`/`Identity`/`PairingBundle` fields are used consistently; `node_id` is hex everywhere it crosses the Rust/SQL boundary (`encode(...,'hex')` in SQL, `hex::encode(event_address(...))` in Rust).

---

## Open dependencies for the implementer

1. **A local Postgres with `cairn_pgx` installed** in each test database (`CREATE EXTENSION cairn_pgx;`) — `submit_node_event`/`apply_remote_node_event` call `cairn_verify`/`cairn_body`. Build/install per the walking-skeleton README pgrx section (now at `extensions/cairn_pgx`).
2. **rustls 0.23 / rcgen 0.13 API confirmation** via context7 before T9's verifier bodies (`/rustls/rustls`, `/rustls/rcgen`).
3. Three throwaway databases (`CAIRN_TEST_PG`, `…2`, `…3`) for the DB-gated tests; the non-DB unit tests (T2) run with plain `cargo test --workspace`.
