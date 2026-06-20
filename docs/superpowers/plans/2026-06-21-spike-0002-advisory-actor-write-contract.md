# Spike 0002 — Advisory-Actor Write Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Demonstrate that an external advisory agent authors an additive, un-attested, provenance-anchored, recallable clinical advisory into Cairn **through the validated submit surface**, and that the in-DB safety floor rejects everything a buggy or hostile agent must not do — even with direct DB access (the C1–C5 pass/fail table).

**Architecture:** Extend the Spike 0001 walking skeleton (`poc/walking-skeleton`). A new `pgrx` extension (`cairn_pgx`) wraps the existing `cairn-event` crate to put Ed25519 verify, body-parse, the pinned-actor-id hash, and attestation-token verify **in the database**. A new `submit_event` PL/pgSQL `SECURITY DEFINER` function is the single write door; a grant model (`REVOKE INSERT … ; GRANT EXECUTE submit_event`) makes raw DB access safe by construction. A Python agent stand-in (`uv`) authors one advisory through that door and a hostile-agent test battery proves the floor holds.

**Tech Stack:** Rust 1.96 (`cairn-event`, `cairn-sync`, `cairn_pgx`/pgrx 0.12), PostgreSQL 16.13 (Postgres.app), PL/pgSQL, Python 3.12 via `uv`.

## Global Constraints

- **License:** every file/component is **AGPL-3.0-only** (workspace `license = "AGPL-3.0-only"`).
- **Python:** manage the environment with **`uv`** — never `venv`/`pip` directly.
- **§9 blast-radius rule:** safety-critical logic (verify, parse, actor registry, `submit_event`, classification gate, recall overlay) is **Rust or in-DB**; only the agent stand-in + its urgency score are **fit-for-purpose Python**.
- **No `is_ai` boolean anywhere** — "AI-generated / un-vouched" must be **emergent** from the contributor set (role `triaged`, no `responsibility`).
- **Never erase, always overlay** (principle 2): recall marks affected events via an overlay event; it never deletes or updates `event_log`.
- **Every floor rejection raises a distinct, legible `RAISE EXCEPTION` reason** — a buggy agent gets a clear error, never silent corruption.
- **Contributor `role` ∈ the ADR-0028 closed enum** — bearing: `authored, ordered, attested, co-signed, witnessed, dictated`; contributory: `drafted, transcribed, graded, triaged, suggested`.
- **Extension name is `cairn_pgx`** (underscore — no hyphen, so `CREATE EXTENSION cairn_pgx` needs no quoting). The crate is **excluded from the cargo workspace** so the existing `cargo test --workspace` stays green without `cargo-pgrx` installed.
- **No PG18-only syntax** in SQL (UUIDv7 minted in Rust, as in Spike 0001), so the skeleton keeps running on PG16.

---

### Task 1: pgrx toolchain checkpoint — `cairn_pgx` extension with `cairn_verify`

The early-risk-first checkpoint: prove `cargo-pgrx` builds against Postgres.app PG16 and that a signed event verifies **from SQL** before building anything on top. If this fights the toolchain, the design's documented fallback is a Homebrew/PGDG PG16 dev install — surface it here, not late.

**Files:**
- Create: `poc/walking-skeleton/crates/cairn_pgx/Cargo.toml`
- Create: `poc/walking-skeleton/crates/cairn_pgx/src/lib.rs`
- Modify: `poc/walking-skeleton/Cargo.toml` (add `exclude`)
- Modify: `poc/walking-skeleton/README.md` (a short "pgrx extension" build note)

**Interfaces:**
- Produces: SQL function `cairn_verify(bytea) -> bool` (true iff the bytes are a valid COSE_Sign1/Ed25519 event whose self-described key verifies).

- [ ] **Step 1: Add the exclude to the workspace**

Modify `poc/walking-skeleton/Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/cairn-event", "crates/cairn-sync"]
exclude = ["crates/cairn_pgx"]
```

- [ ] **Step 2: Write the pgrx crate manifest**

Create `poc/walking-skeleton/crates/cairn_pgx/Cargo.toml`:

```toml
[package]
name = "cairn_pgx"
version = "0.1.0"
edition = "2021"
license = "AGPL-3.0-only"

[lib]
crate-type = ["cdylib", "lib"]

[features]
default = ["pg16"]
pg16 = ["pgrx/pg16"]
pg_test = []

[dependencies]
pgrx = "=0.12.9"
cairn-event = { path = "../cairn-event" }
serde_json = "1"
hex = "0.4"

[dev-dependencies]
pgrx-tests = "=0.12.9"

[profile.dev]
panic = "unwind"
[profile.release]
panic = "unwind"
opt-level = 3
```

- [ ] **Step 3: Write the failing pg_test for `cairn_verify`**

Create `poc/walking-skeleton/crates/cairn_pgx/src/lib.rs`:

```rust
//! cairn_pgx — the in-database safety floor (Spike 0002 §4.3).
//!
//! A thin pgrx wrapper over the existing `cairn-event` crate so there is ONE
//! verify/parse implementation, not two. This is the ADR-0002 production move
//! ("the verify gate moves in-DB so no unverified row can enter the log") made
//! real for the spike. Safety-critical Rust per the §9 blast-radius rule.

use pgrx::prelude::*;

::pgrx::pg_module_magic!();

/// True iff `signed` is a valid COSE_Sign1/Ed25519 event that verifies against
/// its self-described key. The C5.1 floor: an unsigned or malformed event is
/// rejected in-DB, even for a caller with direct DB access.
#[pg_extern(immutable, parallel_safe)]
fn cairn_verify(signed: &[u8]) -> bool {
    cairn_event::verify_self_described(signed).is_ok()
}

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use pgrx::prelude::*;

    // A signed event verifies; one flipped byte does not — the Bet A2 invariant,
    // now checked from inside PostgreSQL.
    #[pg_test]
    fn verify_accepts_good_rejects_tampered() {
        let (sk, kid) = cairn_event::generate_key().unwrap();
        let body = cairn_event::EventBody {
            event_id: "00000000-0000-7000-8000-000000000001".into(),
            patient_id: "00000000-0000-7000-8000-000000000002".into(),
            event_type: "advisory.added".into(),
            schema_version: "advisory/1".into(),
            hlc: cairn_event::Hlc { wall: 1, counter: 0, node_origin: "t".into() },
            t_effective: None,
            signer_key_id: kid,
            contributors: serde_json::json!([]),
            payload: serde_json::json!({"k": "v"}),
            attachments: vec![],
        };
        let signed = cairn_event::sign(&body, &sk).unwrap();
        assert!(crate::cairn_verify(&signed.signed_bytes));

        let mut bad = signed.signed_bytes.clone();
        let m = bad.len() / 2;
        bad[m] ^= 0x01;
        assert!(!crate::cairn_verify(&bad));
    }
}

#[cfg(test)]
pub mod pg_test {
    pub fn setup(_options: Vec<&str>) {}
    pub fn postgresql_conf_options() -> Vec<&'static str> {
        vec![]
    }
}
```

- [ ] **Step 4: Install the pgrx toolchain (one-time; slow) and run the test**

Run, from `poc/walking-skeleton/crates/cairn_pgx`:

```bash
cargo install --locked cargo-pgrx --version 0.12.9
cargo pgrx init --pg16 "$(which pg_config)"      # uses Postgres.app's PG16
cargo pgrx test pg16
```

Expected: the `verify_accepts_good_rejects_tampered` test PASSES.
If `cargo pgrx init` cannot use Postgres.app's `pg_config`, STOP and report — the fallback is a Homebrew/PGDG PG16 dev install (`brew install postgresql@16`), then re-run `init` against its `pg_config`. This is an environment swap, not a design change.

- [ ] **Step 5: Verify it works against the real Postgres.app instance**

Run:

```bash
cargo pgrx install --pg-config "$(which pg_config)"
psql "host=127.0.0.1 user=postgres dbname=postgres" -c "CREATE EXTENSION IF NOT EXISTS cairn_pgx;" \
  -c "SELECT cairn_verify('\x00'::bytea) AS should_be_false;"
```

Expected: `CREATE EXTENSION` succeeds; `should_be_false` is `f`.

- [ ] **Step 6: Add a README note and commit**

Add to `poc/walking-skeleton/README.md` under "Build & test" a short paragraph: the `cairn_pgx` extension is built with `cargo pgrx` (not the workspace build), requires `cargo-pgrx` + `cargo pgrx init --pg16`, and is excluded from `cargo test --workspace` so the Bet A/B harness stays green without the pgrx toolchain.

```bash
git add poc/walking-skeleton/Cargo.toml poc/walking-skeleton/crates/cairn_pgx poc/walking-skeleton/README.md
git commit -m "feat(spike-0002): cairn_pgx extension with in-DB cairn_verify (toolchain checkpoint)"
```

---

### Task 2: `cairn-event` — canonical-JSON content address (the actor-id hash)

The mechanism behind C4: an actor's identity **is** the content-address of its pinned-determinant set, computed deterministically. Pure Rust, unit-testable with plain `cargo test` (no pgrx).

**Files:**
- Modify: `poc/walking-skeleton/crates/cairn-event/src/lib.rs`

**Interfaces:**
- Produces: `pub fn canonical_json_address(v: &serde_json::Value) -> Vec<u8>` — a `0x1220`-prefixed sha2-256 multihash of the canonical (recursively key-sorted) CBOR encoding of `v`. Stable under object-key reordering.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `poc/walking-skeleton/crates/cairn-event/src/lib.rs`:

```rust
#[test]
fn canonical_json_address_is_stable_under_key_order() {
    let a = canonical_json_address(&json!({"model": "m", "version": "1", "skill_epoch": "e"}));
    let b = canonical_json_address(&json!({"version": "1", "skill_epoch": "e", "model": "m"}));
    assert_eq!(a, b, "address must not depend on key order");
    assert_eq!(a[0..2], SHA2_256_MULTIHASH_PREFIX);
    assert_eq!(a.len(), 34);

    // A different pinned value yields a different actor identity (the C4 supersede trigger).
    let c = canonical_json_address(&json!({"model": "m", "version": "1", "skill_epoch": "e2"}));
    assert_ne!(a, c);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p cairn-event canonical_json_address -- --nocapture`
Expected: FAIL — `canonical_json_address` not found.

- [ ] **Step 3: Implement `canonical_json_address`**

Add to the function area of `poc/walking-skeleton/crates/cairn-event/src/lib.rs`:

```rust
/// Recursively sort object keys so the encoding is canonical regardless of input
/// key order, then return the value re-built with BTreeMap-ordered objects.
fn canonicalize(v: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::Object(m) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), canonicalize(&m[k]));
            }
            Value::Object(sorted)
        }
        Value::Array(a) => Value::Array(a.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}

/// Content-address of an arbitrary JSON value: the `0x1220` sha2-256 multihash of
/// its canonical CBOR encoding. Used to derive an actor's identity from its pinned
/// determinant set (Spike 0002 / ADR-0011), so identity is the *hash of what is
/// pinned* — bumping any determinant (incl. skill_epoch) yields a new identity.
pub fn canonical_json_address(v: &serde_json::Value) -> Vec<u8> {
    let canon = canonicalize(v);
    let mut cbor = Vec::new();
    ciborium::into_writer(&canon, &mut cbor).expect("canonical json encodes to CBOR");
    use sha2::{Digest, Sha256};
    let mut out = SHA2_256_MULTIHASH_PREFIX.to_vec();
    out.extend_from_slice(&Sha256::digest(&cbor));
    out
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p cairn-event canonical_json_address`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add poc/walking-skeleton/crates/cairn-event/src/lib.rs
git commit -m "feat(spike-0002): canonical_json_address for pinned actor identity (C4)"
```

---

### Task 3: `cairn-event` — the attestation token (sign + verify)

The mechanism behind C2/C5.2: a human "vouches" by producing an Ed25519-signed token bound to the event's content-address. Pure Rust, unit-testable with plain `cargo test`.

**Files:**
- Modify: `poc/walking-skeleton/crates/cairn-event/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub struct AttestationBody { pub content_address_hex: String, pub attester_key_id: String, pub role: String }`
  - `pub fn sign_attestation(content_address: &[u8], attester_key_id: &str, role: &str, sk: &SigningKey) -> Result<Vec<u8>, EventError>` — a COSE_Sign1 token.
  - `pub fn verify_attestation(token: &[u8], content_address: &[u8], vk: &VerifyingKey) -> bool` — true iff the token verifies against `vk` AND its bound content-address equals `content_address`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:

```rust
#[test]
fn attestation_binds_key_and_content_address() {
    let (sk, kid) = generate_key().unwrap();
    let vk = sk.verifying_key();
    let ca = event_address(b"some signed event bytes");

    let token = sign_attestation(&ca, &kid, "attested", &sk).unwrap();
    assert!(verify_attestation(&token, &ca, &vk), "valid token for right key + address");

    // Wrong content-address -> reject (a token cannot be replayed onto another event).
    let other = event_address(b"a different event");
    assert!(!verify_attestation(&token, &other, &vk));

    // Wrong key -> reject (a forged attester does not verify).
    let (_sk2, _kid2) = generate_key().unwrap();
    let other_vk = SigningKey::from_bytes(&[5u8; 32]).verifying_key();
    assert!(!verify_attestation(&token, &ca, &other_vk));

    // Tampered token bytes -> reject.
    let mut bad = token.clone();
    let m = bad.len() / 2;
    bad[m] ^= 0x01;
    assert!(!verify_attestation(&bad, &ca, &vk));
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p cairn-event attestation_binds -- --nocapture`
Expected: FAIL — `sign_attestation` not found.

- [ ] **Step 3: Implement the attestation token**

Add to `poc/walking-skeleton/crates/cairn-event/src/lib.rs`:

```rust
/// The payload of an attestation token: a human (or attesting actor) binds their
/// key and a responsibility-bearing role to a specific event's content-address.
/// Signed as a COSE_Sign1, verified in-DB by cairn_pgx (ADR-0008: the token, never
/// the DB session, is what confers responsibility / stops a forged human author).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AttestationBody {
    pub content_address_hex: String,
    pub attester_key_id: String,
    pub role: String,
}

/// Sign an attestation token over `content_address` (a COSE_Sign1, Ed25519).
pub fn sign_attestation(
    content_address: &[u8],
    attester_key_id: &str,
    role: &str,
    sk: &SigningKey,
) -> Result<Vec<u8>, EventError> {
    use coset::{iana, CborSerializable, CoseSign1Builder, HeaderBuilder};
    let body = AttestationBody {
        content_address_hex: hex::encode(content_address),
        attester_key_id: attester_key_id.to_string(),
        role: role.to_string(),
    };
    let mut payload = Vec::new();
    ciborium::into_writer(&body, &mut payload).map_err(|e| EventError::Cbor(e.to_string()))?;
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

/// Verify an attestation token against `vk` and confirm it binds `content_address`.
pub fn verify_attestation(token: &[u8], content_address: &[u8], vk: &VerifyingKey) -> bool {
    use coset::{CborSerializable, CoseSign1};
    let sign1 = match CoseSign1::from_slice(token) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let verified = sign1
        .verify_signature(b"", |sig, tbs| {
            let signature =
                ed25519_dalek::Signature::from_slice(sig).map_err(|_| EventError::BadSignature)?;
            vk.verify(tbs, &signature).map_err(|_| EventError::BadSignature)
        })
        .is_ok();
    if !verified {
        return false;
    }
    let payload = match sign1.payload {
        Some(p) => p,
        None => return false,
    };
    let body: AttestationBody = match ciborium::from_reader(&payload[..]) {
        Ok(b) => b,
        Err(_) => return false,
    };
    body.content_address_hex == hex::encode(content_address)
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p cairn-event attestation_binds`
Expected: PASS. Also run `cargo test --workspace` and confirm the existing skeleton tests still pass.

- [ ] **Step 5: Commit**

```bash
git add poc/walking-skeleton/crates/cairn-event/src/lib.rs
git commit -m "feat(spike-0002): Ed25519 attestation token bound to content-address (C2/C5.2)"
```

---

### Task 4: `cairn_pgx` — expose `cairn_body`, `cairn_actor_id`, `cairn_attestation_ok`

Make the Task 2/3 helpers callable from `submit_event`. Thin pgrx wrappers; tested with `cargo pgrx test`.

**Files:**
- Modify: `poc/walking-skeleton/crates/cairn_pgx/src/lib.rs`

**Interfaces:**
- Produces:
  - `cairn_body(bytea) -> jsonb` — the parsed `EventBody` as JSONB (verifies first; returns NULL if invalid).
  - `cairn_actor_id(jsonb) -> bytea` — `canonical_json_address` of the pinned set.
  - `cairn_attestation_ok(token bytea, content_address bytea, attester_key bytea) -> bool`.

- [ ] **Step 1: Write the failing pg_tests**

Add to the `tests` module in `poc/walking-skeleton/crates/cairn_pgx/src/lib.rs`:

```rust
    #[pg_test]
    fn body_returns_parsed_event_and_actor_id_is_stable() {
        let (sk, kid) = cairn_event::generate_key().unwrap();
        let body = cairn_event::EventBody {
            event_id: "00000000-0000-7000-8000-000000000010".into(),
            patient_id: "00000000-0000-7000-8000-000000000011".into(),
            event_type: "advisory.added".into(),
            schema_version: "advisory/1".into(),
            hlc: cairn_event::Hlc { wall: 5, counter: 0, node_origin: "t".into() },
            t_effective: None,
            signer_key_id: kid.clone(),
            contributors: serde_json::json!([{"actor_id": "x", "role": "triaged"}]),
            payload: serde_json::json!({"urgency": 3}),
            attachments: vec![],
        };
        let signed = cairn_event::sign(&body, &sk).unwrap();
        let parsed = crate::cairn_body(&signed.signed_bytes).expect("verifies");
        assert_eq!(parsed.0["event_type"], serde_json::json!("advisory.added"));

        // Invalid bytes -> NULL.
        assert!(crate::cairn_body(b"not an event").is_none());

        // actor_id is stable under key reorder (C4).
        let id1 = crate::cairn_actor_id(pgrx::JsonB(serde_json::json!({"model": "m", "skill_epoch": "e"})));
        let id2 = crate::cairn_actor_id(pgrx::JsonB(serde_json::json!({"skill_epoch": "e", "model": "m"})));
        assert_eq!(id1, id2);
    }

    #[pg_test]
    fn attestation_ok_checks_key_and_address() {
        let (sk, kid) = cairn_event::generate_key().unwrap();
        let ca = cairn_event::event_address(b"evt");
        let token = cairn_event::sign_attestation(&ca, &kid, "attested", &sk).unwrap();
        let pubkey = hex::decode(&kid).unwrap();
        assert!(crate::cairn_attestation_ok(&token, &ca, &pubkey));
        let other = cairn_event::event_address(b"other");
        assert!(!crate::cairn_attestation_ok(&token, &other, &pubkey));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run (in `crates/cairn_pgx`): `cargo pgrx test pg16`
Expected: FAIL — `cairn_body` / `cairn_actor_id` / `cairn_attestation_ok` not found.

- [ ] **Step 3: Implement the three functions**

Add to `poc/walking-skeleton/crates/cairn_pgx/src/lib.rs` (after `cairn_verify`):

```rust
use pgrx::JsonB;

/// Verify and parse an event's signed bytes into its EventBody as JSONB. Returns
/// NULL when the bytes do not verify — submit_event calls cairn_verify first for a
/// legible rejection, then this to read the body PL/pgSQL cannot parse (COSE/CBOR).
#[pg_extern(immutable, parallel_safe)]
fn cairn_body(signed: &[u8]) -> Option<JsonB> {
    let body = cairn_event::verify_self_described(signed).ok()?;
    let value = serde_json::to_value(&body).ok()?;
    Some(JsonB(value))
}

/// Content-address (0x1220 sha2-256 multihash) of a pinned-determinant set. An
/// actor's identity IS this hash, so bumping any pinned field mints a new actor (C4).
#[pg_extern(immutable, parallel_safe)]
fn cairn_actor_id(pinned: JsonB) -> Vec<u8> {
    cairn_event::canonical_json_address(&pinned.0)
}

/// True iff `token` is a valid attestation by `attester_key` bound to `content_address`.
#[pg_extern(immutable, parallel_safe)]
fn cairn_attestation_ok(token: &[u8], content_address: &[u8], attester_key: &[u8]) -> bool {
    let bytes: [u8; 32] = match attester_key.try_into() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let vk = match cairn_event::VerifyingKey::from_bytes(&bytes) {
        Ok(v) => v,
        Err(_) => return false,
    };
    cairn_event::verify_attestation(token, content_address, &vk)
}
```

Add `use hex;` is already covered by the `hex` dependency; reference it as `hex::decode` in tests.

- [ ] **Step 4: Run to verify it passes, then reinstall**

Run:

```bash
cargo pgrx test pg16
cargo pgrx install --pg-config "$(which pg_config)"
```

Expected: both pg_tests PASS; install succeeds (so the dev DB gets the new functions on `ALTER EXTENSION cairn_pgx UPDATE` or a drop/recreate).

- [ ] **Step 5: Commit**

```bash
git add poc/walking-skeleton/crates/cairn_pgx/src/lib.rs
git commit -m "feat(spike-0002): cairn_body / cairn_actor_id / cairn_attestation_ok in-DB"
```

---

### Task 5: `db/004_actors.sql` — the append-only actor registry + grant scaffolding

**Files:**
- Create: `poc/walking-skeleton/db/004_actors.sql`
- Create: `poc/walking-skeleton/db/tests/004_actors_test.sql`

**Interfaces:**
- Produces: table `actor_event`, view `actor_current`, function `enroll_actor(kind, pinned jsonb, signing_key_id text) -> bytea` (returns the derived `actor_id`), the append-only trigger, and a `cairn_agent` role with **no** `event_log` write privilege.

- [ ] **Step 1: Write the failing SQL test**

Create `poc/walking-skeleton/db/tests/004_actors_test.sql`:

```sql
-- Run with:  psql "$CONN" -v ON_ERROR_STOP=1 -f db/004_actors.sql -f db/tests/004_actors_test.sql
\set ON_ERROR_STOP on
BEGIN;

-- Enroll an agent; its actor_id is the hash of its pinned set (C4).
SELECT enroll_actor('agent',
    '{"model":"triage-stub","version":"1","skill_epoch":"epoch-a"}'::jsonb,
    'deadbeef') AS aid \gset
SELECT count(*) = 1 AS enrolled_one FROM actor_current WHERE actor_id = :'aid'::bytea;

-- Bumping skill_epoch mints a DIFFERENT actor_id (the supersede trigger for C4).
SELECT enroll_actor('agent',
    '{"model":"triage-stub","version":"1","skill_epoch":"epoch-b"}'::jsonb,
    'deadbeef') AS aid2 \gset
SELECT (:'aid'::bytea <> :'aid2'::bytea) AS epoch_bump_is_new_actor;

-- The registry is append-only: UPDATE/DELETE must raise.
DO $$ BEGIN
    BEGIN
        UPDATE actor_event SET op = 'revoke';
        RAISE EXCEPTION 'append-only check FAILED: update succeeded';
    EXCEPTION WHEN others THEN
        IF SQLERRM LIKE '%append-only%' THEN RAISE NOTICE 'append-only OK'; ELSE RAISE; END IF;
    END;
END $$;

ROLLBACK;
```

- [ ] **Step 2: Run it to verify it fails**

Run: `psql "$CONN" -v ON_ERROR_STOP=1 -f poc/walking-skeleton/db/tests/004_actors_test.sql`
Expected: FAIL — relation `actor_event` / function `enroll_actor` does not exist. (`$CONN` = `host=127.0.0.1 user=postgres dbname=skeleton_a`.)

- [ ] **Step 3: Write `db/004_actors.sql`**

Create `poc/walking-skeleton/db/004_actors.sql`:

```sql
-- Cairn walking skeleton — the append-only actor registry (Spike 0002 §4.1).
--
-- ADR-0011: actor identity is version-pinned and immutable. An actor_id IS the
-- content-address of its pinned-determinant set (computed by cairn_pgx), so
-- bumping any determinant (incl. skill_epoch) mints a new actor via a fresh
-- enroll/supersede row — never an edit (principle 2). The closed actor-event
-- algebra is enroll | supersede | revoke.

BEGIN;

CREATE TABLE IF NOT EXISTS actor_event (
    actor_event_id  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_id        BYTEA   NOT NULL,           -- content-address of the pinned set
    op              TEXT    NOT NULL CHECK (op IN ('enroll','supersede','revoke')),
    kind            TEXT    CHECK (kind IN ('human','agent','device')),
    pinned          JSONB,                       -- the version-pinned determinant set
    signing_key_id  TEXT,                        -- hex Ed25519 public key
    superseded_by   BYTEA,                       -- for supersede: the new actor_id
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE INDEX IF NOT EXISTS actor_event_actor_idx ON actor_event (actor_id);
CREATE INDEX IF NOT EXISTS actor_event_key_idx ON actor_event (signing_key_id);

-- Append-only: refuse UPDATE and DELETE (principle 1), same pattern as event_log.
CREATE OR REPLACE FUNCTION actor_event_is_append_only()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'actor_event is append-only: % is not permitted (Cairn principle #1)', TG_OP;
END;
$$;
DROP TRIGGER IF EXISTS actor_event_no_update ON actor_event;
CREATE TRIGGER actor_event_no_update BEFORE UPDATE OR DELETE ON actor_event
    FOR EACH ROW EXECUTE FUNCTION actor_event_is_append_only();

-- Current, non-revoked identities: the latest enroll/supersede per actor_id with
-- no later revoke.
CREATE OR REPLACE VIEW actor_current AS
SELECT DISTINCT ON (ae.actor_id)
       ae.actor_id, ae.kind, ae.pinned, ae.signing_key_id, ae.recorded_at
FROM actor_event ae
WHERE ae.op IN ('enroll','supersede')
  AND NOT EXISTS (
      SELECT 1 FROM actor_event r
      WHERE r.actor_id = ae.actor_id AND r.op = 'revoke' AND r.recorded_at >= ae.recorded_at)
ORDER BY ae.actor_id, ae.recorded_at DESC;

-- Enroll an actor; its identity is derived in-DB from the pinned set (cairn_pgx),
-- so "identity = hash of what is pinned" is enforced, not asserted.
CREATE OR REPLACE FUNCTION enroll_actor(p_kind TEXT, p_pinned JSONB, p_key TEXT)
RETURNS BYTEA LANGUAGE plpgsql AS $$
DECLARE aid BYTEA;
BEGIN
    aid := cairn_actor_id(p_pinned);
    INSERT INTO actor_event (actor_id, op, kind, pinned, signing_key_id)
    VALUES (aid, 'enroll', p_kind, p_pinned, p_key);
    RETURN aid;
END;
$$;

-- The agent's DB role: it may EXECUTE the submit door and READ projections, but
-- has NO write privilege on the event log (the C5.4 grant floor; granted in 005).
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'cairn_agent') THEN
        CREATE ROLE cairn_agent NOLOGIN;
    END IF;
END $$;

COMMIT;
```

- [ ] **Step 4: Run the test to verify it passes**

Run:

```bash
CONN="host=127.0.0.1 user=postgres dbname=skeleton_a"
psql "$CONN" -v ON_ERROR_STOP=1 -c "CREATE EXTENSION IF NOT EXISTS cairn_pgx;" \
  -f poc/walking-skeleton/db/004_actors.sql \
  -f poc/walking-skeleton/db/tests/004_actors_test.sql
```

Expected: `enrolled_one = t`, `epoch_bump_is_new_actor = t`, `NOTICE: append-only OK`.

- [ ] **Step 5: Commit**

```bash
git add poc/walking-skeleton/db/004_actors.sql poc/walking-skeleton/db/tests/004_actors_test.sql
git commit -m "feat(spike-0002): append-only actor registry + derived actor_id (C4)"
```

---

### Task 6: typed contributor set on `EventBody` (Rust) + plaintext-twin still derives

Give the contributor set a typed shape and keep the existing skeleton green. Small, isolated Rust change.

**Files:**
- Modify: `poc/walking-skeleton/crates/cairn-event/src/lib.rs`

**Interfaces:**
- Produces: `pub struct Contributor { pub actor_id: String, pub role: String, #[serde(skip_serializing_if = "Option::is_none")] pub responsibility: Option<String> }` and a helper `pub fn contributors_json(set: &[Contributor]) -> serde_json::Value`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:

```rust
#[test]
fn agent_contributor_is_unvouched_by_construction() {
    let set = vec![Contributor {
        actor_id: "agent-aid".into(),
        role: "triaged".into(),
        responsibility: None,
    }];
    let v = contributors_json(&set);
    // role present, NO responsibility key, NO is_ai flag anywhere (C1).
    assert_eq!(v[0]["role"], json!("triaged"));
    assert!(v[0].get("responsibility").is_none());
    assert!(v[0].get("is_ai").is_none());
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p cairn-event agent_contributor_is_unvouched -- --nocapture`
Expected: FAIL — `Contributor` not found.

- [ ] **Step 3: Implement the type + helper**

Add to `poc/walking-skeleton/crates/cairn-event/src/lib.rs`:

```rust
/// A §3.9 contributor: who contributed, in what role, and — only when an
/// attestation token backs it — whether they bear responsibility. The agent
/// authors with role `triaged` and `responsibility = None`, so "AI-generated /
/// un-vouched" is emergent (C1): there is no `is_ai` flag anywhere.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Contributor {
    pub actor_id: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsibility: Option<String>,
}

/// Render a contributor set as the JSON that rides in the signed body's
/// `contributors` field (and lands in `event_log.contributors`).
pub fn contributors_json(set: &[Contributor]) -> serde_json::Value {
    serde_json::to_value(set).expect("contributor set serializes")
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p cairn-event agent_contributor_is_unvouched` then `cargo test --workspace`
Expected: PASS; the existing skeleton tests still pass.

- [ ] **Step 5: Commit**

```bash
git add poc/walking-skeleton/crates/cairn-event/src/lib.rs
git commit -m "feat(spike-0002): typed contributor set; un-vouched-by-construction (C1)"
```

---

### Task 7: `db/005_submit.sql` — `submit_event` pipeline + classification + grant floor

The single write door. Verifies in-DB, classifies, gates attestation, binds provenance, appends — and revokes all other write paths.

**Files:**
- Create: `poc/walking-skeleton/db/005_submit.sql`
- Create: `poc/walking-skeleton/db/tests/005_submit_test.sql`

**Interfaces:**
- Consumes: `cairn_verify`, `cairn_body`, `cairn_attestation_ok` (Task 4); `event_log` (001); `cairn_agent` role (004).
- Produces:
  - table `event_type_class(event_type TEXT PK, mode TEXT CHECK (mode IN ('additive','suppressing')), targets_other_author BOOLEAN DEFAULT FALSE)`
  - `submit_event(p_signed bytea, p_attestation bytea DEFAULT NULL, p_attester_key bytea DEFAULT NULL) RETURNS uuid` — `SECURITY DEFINER`; the only writer of `event_log`.

- [ ] **Step 1: Write the failing SQL test (the floor in miniature)**

Create `poc/walking-skeleton/db/tests/005_submit_test.sql`:

```sql
\set ON_ERROR_STOP on
-- Helper: assert that a statement raises with a message matching a pattern.
-- Usage relies on DO blocks; each negative case below is self-checking.

-- C5.4: the agent role cannot raw-INSERT into event_log.
DO $$ BEGIN
    SET LOCAL ROLE cairn_agent;
    BEGIN
        INSERT INTO event_log (event_id, patient_id, event_type, schema_version,
            hlc_wall, hlc_counter, node_origin, signed_bytes, content_address, body,
            contributors, signer_key_id, plaintext_twin)
        VALUES (gen_random_uuid(), gen_random_uuid(), 'x','x',0,0,'n','\x00','\x1220'||digest('\x00','sha256'),
            '{}','[]','k','t');
        RESET ROLE;
        RAISE EXCEPTION 'C5.4 FAILED: agent raw INSERT succeeded';
    EXCEPTION WHEN insufficient_privilege THEN
        RESET ROLE;
        RAISE NOTICE 'C5.4 OK: raw INSERT denied to cairn_agent';
    END;
END $$;

-- C5.1: submit_event rejects unsigned/malformed bytes with a legible reason.
DO $$ BEGIN
    BEGIN
        PERFORM submit_event('\xdeadbeef'::bytea);
        RAISE EXCEPTION 'C5.1 FAILED: malformed event accepted';
    EXCEPTION WHEN others THEN
        IF SQLERRM LIKE '%signature%' OR SQLERRM LIKE '%verify%'
            THEN RAISE NOTICE 'C5.1 OK: % ', SQLERRM; ELSE RAISE; END IF;
    END;
END $$;
```

- [ ] **Step 2: Run it to verify it fails**

Run: `psql "$CONN" -v ON_ERROR_STOP=1 -f poc/walking-skeleton/db/tests/005_submit_test.sql`
Expected: FAIL — function `submit_event` does not exist.

- [ ] **Step 3: Write `db/005_submit.sql`**

Create `poc/walking-skeleton/db/005_submit.sql`:

```sql
-- Cairn walking skeleton — the validated submit surface (Spike 0002 §4.4 / ADR-0022).
--
-- submit_event is the ONE generic write door. It runs the write-time seams in-DB,
-- atomically: verify (cairn_pgx) -> resolve actor -> classify additive/suppressing
-- -> gate attestation -> owner-gate cross-author overlays -> bind provenance ->
-- append. The grant floor (REVOKE INSERT on event_log; GRANT EXECUTE here) makes
-- direct DB access safe by construction (ADR-0021). Every rejection is legible.

BEGIN;

-- Additive vs suppressing classification (ADR-0010). A new event type adds a row
-- here (additive-only registry); unknown types are rejected (fail closed).
CREATE TABLE IF NOT EXISTS event_type_class (
    event_type            TEXT PRIMARY KEY,
    mode                  TEXT NOT NULL CHECK (mode IN ('additive','suppressing')),
    targets_other_author  BOOLEAN NOT NULL DEFAULT FALSE
);
INSERT INTO event_type_class (event_type, mode, targets_other_author) VALUES
    ('patient.created', 'additive',    FALSE),
    ('patient.amended', 'additive',    FALSE),
    ('note.added',      'additive',    FALSE),
    ('advisory.added',  'additive',    FALSE),
    ('salience.downgrade','suppressing', TRUE),
    ('visibility.suppress','suppressing', TRUE)
ON CONFLICT (event_type) DO NOTHING;

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
    v_target_origin TEXT;
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

    -- 5. Owner-gate: a suppressing overlay that targets another author's event must
    --    be attested by a human (already enforced in step 4); record the linkage.
    --    (The skeleton stores the target in the body as `target_event_id`.)
    IF v_targets_other AND (b -> 'payload' ? 'target_event_id') THEN
        v_target_id := (b -> 'payload' ->> 'target_event_id')::uuid;
        SELECT node_origin INTO v_target_origin FROM event_log WHERE event_id = v_target_id;
        IF v_target_origin IS NULL THEN
            RAISE EXCEPTION 'submit_event: overlay targets unknown event %', v_target_id;
        END IF;
    END IF;

    -- 6. Provenance binding (C3): an advisory must cite its source blob's address.
    IF v_type = 'advisory.added' THEN
        IF jsonb_array_length(COALESCE(b -> 'attachments', '[]'::jsonb)) = 0 THEN
            RAISE EXCEPTION 'submit_event: advisory.added must carry a provenance attachment reference';
        END IF;
    END IF;

    -- 7. Derive the plaintext twin (mechanical; the §3.13 substrate) and append.
    v_twin := format('[%s] %s for patient %s', v_type, b ->> 'schema_version', b ->> 'patient_id');

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
    ON CONFLICT DO NOTHING;

    -- Learn any attachment references (reference-eager, byte-lazy).
    FOR c IN SELECT * FROM jsonb_array_elements(COALESCE(b -> 'attachments','[]'::jsonb)) LOOP
        PERFORM blob_note_reference(decode(c ->> 'digest_hex','hex'), c ->> 'media_type',
                                    (c ->> 'byte_len')::bigint);
    END LOOP;

    RETURN v_event_id;
END;
$$;

-- The grant floor (C5.4 / ADR-0021): no direct event_log writes; the only door is
-- submit_event. The agent reads projections + the log, executes the door, nothing else.
REVOKE INSERT, UPDATE, DELETE ON event_log FROM PUBLIC;
REVOKE INSERT, UPDATE, DELETE ON event_log FROM cairn_agent;
GRANT EXECUTE ON FUNCTION submit_event(bytea, bytea, bytea) TO cairn_agent;
GRANT SELECT ON event_log, patient_chart, actor_current TO cairn_agent;

COMMIT;
```

- [ ] **Step 4: Run the test to verify it passes**

Run:

```bash
psql "$CONN" -v ON_ERROR_STOP=1 -f poc/walking-skeleton/db/005_submit.sql \
  -f poc/walking-skeleton/db/tests/005_submit_test.sql
```

Expected: `NOTICE: C5.4 OK: raw INSERT denied to cairn_agent` and `NOTICE: C5.1 OK: …signature…`.

- [ ] **Step 5: Wire 004 + 005 into `cairn-sync init` and confirm the schema loads**

Modify `poc/walking-skeleton/crates/cairn-sync/src/main.rs`: extend the `SCHEMA` const and have `cmd_init` create the extension first.

```rust
const SCHEMA: [(&str, &str); 5] = [
    ("001_envelope", include_str!("../../../db/001_envelope.sql")),
    ("002_projection", include_str!("../../../db/002_projection.sql")),
    ("003_blobs", include_str!("../../../db/003_blobs.sql")),
    ("004_actors", include_str!("../../../db/004_actors.sql")),
    ("005_submit", include_str!("../../../db/005_submit.sql")),
];
```

In `cmd_init`, before the loop:

```rust
fn cmd_init(conn: &str) -> R<()> {
    let mut client = postgres::Client::connect(conn, postgres::NoTls)?;
    // 004/005 call cairn_pgx functions; the extension must exist first.
    client.batch_execute("CREATE EXTENSION IF NOT EXISTS cairn_pgx;")?;
    for (name, sql) in SCHEMA {
        client.batch_execute(sql)?;
        eprintln!("applied {name}");
    }
    Ok(())
}
```

Run: `cargo build -p cairn-sync && target/debug/cairn-sync init --conn "host=127.0.0.1 user=postgres dbname=skeleton_c"`
Expected: prints `applied 001_envelope` … `applied 005_submit` with no error (requires `cairn_pgx` installed via Task 4).

- [ ] **Step 6: Commit**

```bash
git add poc/walking-skeleton/db/005_submit.sql poc/walking-skeleton/db/tests/005_submit_test.sql poc/walking-skeleton/crates/cairn-sync/src/main.rs
git commit -m "feat(spike-0002): submit_event pipeline + classification + grant floor (C5)"
```

---

### Task 8: the agent stand-in — `harness/agent_standin.py` (`uv`)

A fit-for-purpose Python actor that authors one advisory through `submit_event`, signing client-side with its own key. To avoid re-implementing COSE_Sign1 byte-for-byte in Python, it shells out to a tiny `cairn-sync sign-stdin` helper (Rust does the canonical encode + sign; Python drives the contract).

**Files:**
- Create: `poc/walking-skeleton/crates/cairn-sync/src/main.rs` change (add `sign-stdin` subcommand)
- Create: `poc/walking-skeleton/harness/agent_standin.py`
- Create: `poc/walking-skeleton/harness/pyproject.toml`

**Interfaces:**
- Consumes: `submit_event(bytea, bytea, bytea)` (Task 7), `enroll_actor` (Task 5).
- Produces: `cairn-sync sign-stdin --key PATH` reads a JSON `EventBody` (without signature) on stdin and writes hex COSE_Sign1 to stdout; `agent_standin.py author --conn … --blob-addr …` authors one advisory and prints the new `event_id`.

- [ ] **Step 1: Add the `sign-stdin` subcommand (failing build first)**

Add a function to `poc/walking-skeleton/crates/cairn-sync/src/main.rs`:

```rust
/// Sign an EventBody supplied as JSON on stdin and emit hex COSE_Sign1 on stdout.
/// Lets a non-Rust client (the Python agent stand-in) drive the write contract
/// while Rust owns the canonical encoding + signature (one signer implementation).
fn cmd_sign_stdin(key_path: &str) -> R<()> {
    let (sk, _kid) = load_or_create_key(key_path)?;
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let body: EventBody = serde_json::from_str(&input)?;
    let signed = sign(&body, &sk)?;
    println!("{}", hex::encode(&signed.signed_bytes));
    Ok(())
}
```

Wire it into `main`'s match and `usage()`:

```rust
        "sign-stdin" => cmd_sign_stdin(
            &flag(&args, "--key").unwrap_or_else(|| "agent.key".into()),
        )?,
```

Run: `cargo build -p cairn-sync`
Expected: compiles. Then verify the public key:

```bash
target/debug/cairn-sync sign-stdin --key agent.key < /dev/null ; echo "exit=$?"
```

Expected: exits non-zero (empty stdin is not a valid body) — proves the command is wired.

- [ ] **Step 2: Write the agent stand-in (failing test = it errors before enrollment)**

Create `poc/walking-skeleton/harness/pyproject.toml`:

```toml
[project]
name = "cairn-spike-0002-harness"
version = "0.1.0"
requires-python = ">=3.12"
dependencies = ["psycopg[binary]>=3.2"]
```

Create `poc/walking-skeleton/harness/agent_standin.py`:

```python
"""Spike 0002 advisory-agent stand-in (fit-for-purpose Python, §9.1).

Mimics the kastellan integration *contract*: it loads its actor identity + skill
epoch, reads a provenance blob reference, computes a trivial urgency score, signs
the event with its own Ed25519 key (via `cairn-sync sign-stdin`, so Rust owns the
canonical COSE encoding), and authors the advisory ONLY through submit_event. It
never touches event_log directly.
"""
import json
import subprocess
import sys
import uuid

import psycopg


def agent_public_key(bin_path: str, key_path: str) -> str:
    """Derive the agent's hex public key by signing an empty-marker body and
    reading the kid back is overkill; instead we read the key file the daemon
    writes (hex seed) and reuse its kid via a dedicated probe."""
    # The daemon prints "generated new signing key … (kid …)" on first creation;
    # simplest stable path: sign a throwaway body and read signer_key_id from it.
    body = _body("probe.added", str(uuid.uuid4()), "probe/1", {}, [], kid="")
    signed_hex = _sign(bin_path, key_path, body)
    # signer_key_id is embedded; re-derive via the daemon is unnecessary — the kid
    # is the COSE protected key_id. We fetch it through the DB instead (see author()).
    return signed_hex  # not used directly; kept for clarity


def _body(event_type, patient_id, schema, payload, attachments, kid):
    return {
        "event_id": str(uuid.uuid4()),  # UUIDv7 minted in Rust normally; v4 ok for the spike body
        "patient_id": patient_id,
        "event_type": event_type,
        "schema_version": schema,
        "hlc": {"wall": 1, "counter": 0, "node_origin": "agent"},
        "t_effective": None,
        "signer_key_id": kid,
        "contributors": [{"actor_id": "agent", "role": "triaged"}],
        "payload": payload,
        "attachments": attachments,
    }


def _sign(bin_path, key_path, body):
    p = subprocess.run([bin_path, "sign-stdin", "--key", key_path],
                       input=json.dumps(body).encode(), capture_output=True)
    if p.returncode != 0:
        raise RuntimeError(f"sign-stdin failed: {p.stderr.decode()}")
    return p.stdout.decode().strip()


def author(conn_str, bin_path, key_path, blob_addr_hex, patient_id):
    """Author one advisory through submit_event. Returns the new event_id."""
    # The agent must sign with the kid it is enrolled under; read it from the key
    # by signing a probe and extracting the COSE key_id via the DB's cairn_body.
    with psycopg.connect(conn_str, autocommit=True) as db:
        probe = _body("advisory.added", patient_id, "advisory/1", {}, [], kid="")
        # First pass: sign to learn our kid (cairn_body exposes signer_key_id).
        signed_hex = _sign(bin_path, key_path, probe)
        row = db.execute("SELECT cairn_body(decode(%s,'hex')) ->> 'signer_key_id'",
                         (signed_hex,)).fetchone()
        kid = row[0]

        # urgency score = a trivial deterministic function of the blob address.
        urgency = (int(blob_addr_hex[:2], 16) % 5) + 1
        body = _body(
            "advisory.added", patient_id, "advisory/1",
            {"urgency": urgency, "summary": "triage advisory (stand-in)"},
            [{"alg": "blake3", "digest_hex": blob_addr_hex,
              "media_type": "message/rfc822", "descriptor": "source mail", "byte_len": 1}],
            kid=kid,
        )
        signed_hex = _sign(bin_path, key_path, body)
        row = db.execute("SELECT submit_event(decode(%s,'hex'))", (signed_hex,)).fetchone()
        return row[0]


if __name__ == "__main__":
    # CLI: author --conn … --bin … --key … --blob-addr … --patient …
    args = dict(zip(sys.argv[2::2], sys.argv[3::2])) if len(sys.argv) > 2 else {}
    if sys.argv[1] == "author":
        eid = author(args["--conn"], args["--bin"], args["--key"],
                     args["--blob-addr"], args["--patient"])
        print(eid)
```

- [ ] **Step 3: Run it to verify it fails without an enrolled actor**

Run:

```bash
cd poc/walking-skeleton/harness
uv run python agent_standin.py author \
  --conn "host=127.0.0.1 user=postgres dbname=skeleton_c" \
  --bin ../../../poc/walking-skeleton/target/debug/cairn-sync \
  --key /tmp/agent.key --blob-addr "1e20$(printf '00%.0s' {1..32})" \
  --patient "$(psql "host=127.0.0.1 user=postgres dbname=skeleton_c" -tAc 'select gen_random_uuid()')"
```

Expected: a `submit_event` exception "signer … is not an enrolled, non-revoked actor" — the agent must be enrolled first (done by the harness in Task 10). This confirms the floor rejects an unknown signer.

- [ ] **Step 4: Commit**

```bash
git add poc/walking-skeleton/crates/cairn-sync/src/main.rs poc/walking-skeleton/harness/agent_standin.py poc/walking-skeleton/harness/pyproject.toml
git commit -m "feat(spike-0002): agent stand-in authors via submit_event (sign-stdin helper)"
```

---

### Task 9: recall — `db/006_recall.sql` (query + contamination overlay)

C4's recall half: find an actor's events under a skill-epoch, and mark them via an append-only overlay — never erase.

**Files:**
- Create: `poc/walking-skeleton/db/006_recall.sql`
- Modify: `poc/walking-skeleton/crates/cairn-sync/src/main.rs` (`SCHEMA` array length 5 → 6)
- Create: `poc/walking-skeleton/db/tests/006_recall_test.sql`

**Interfaces:**
- Produces:
  - `events_by_actor_epoch(p_key text, p_epoch text) RETURNS TABLE(event_id uuid, event_type text)` — events authored by the actor whose pinned `skill_epoch` matches.
  - table `recall_overlay(recall_id uuid pk, target_event_id uuid, reason text, recorded_at)` (append-only) + `recall_event(target uuid, reason text)`.

- [ ] **Step 1: Write the failing SQL test**

Create `poc/walking-skeleton/db/tests/006_recall_test.sql`:

```sql
\set ON_ERROR_STOP on
-- recall_event marks a target without deleting it (principle 2).
DO $$
DECLARE n_before bigint; n_after bigint; tgt uuid;
BEGIN
    SELECT count(*) INTO n_before FROM event_log;
    SELECT event_id INTO tgt FROM event_log LIMIT 1;
    IF tgt IS NOT NULL THEN
        PERFORM recall_event(tgt, 'skill-epoch contamination test');
        SELECT count(*) INTO n_after FROM event_log;
        IF n_after <> n_before THEN RAISE EXCEPTION 'recall ERASED data: % -> %', n_before, n_after; END IF;
        IF NOT EXISTS (SELECT 1 FROM recall_overlay WHERE target_event_id = tgt)
            THEN RAISE EXCEPTION 'recall overlay missing'; END IF;
        RAISE NOTICE 'recall OK: overlay added, no data erased';
    END IF;
END $$;
```

- [ ] **Step 2: Run it to verify it fails**

Run: `psql "$CONN" -v ON_ERROR_STOP=1 -f poc/walking-skeleton/db/tests/006_recall_test.sql`
Expected: FAIL — function `recall_event` does not exist.

- [ ] **Step 3: Write `db/006_recall.sql`**

Create `poc/walking-skeleton/db/006_recall.sql`:

```sql
-- Cairn walking skeleton — recall + contamination overlay (Spike 0002 §4.6 / C4).
-- An actor recall marks affected events via an append-only overlay; it NEVER edits
-- or deletes event_log (principle 2: never erase, always overlay).

BEGIN;

CREATE TABLE IF NOT EXISTS recall_overlay (
    recall_id       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    target_event_id UUID NOT NULL,
    reason          TEXT NOT NULL,
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

CREATE OR REPLACE FUNCTION recall_overlay_is_append_only()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'recall_overlay is append-only: % not permitted (principle #2)', TG_OP;
END;
$$;
DROP TRIGGER IF EXISTS recall_overlay_no_update ON recall_overlay;
CREATE TRIGGER recall_overlay_no_update BEFORE UPDATE OR DELETE ON recall_overlay
    FOR EACH ROW EXECUTE FUNCTION recall_overlay_is_append_only();

-- Events authored by the actor(s) whose pinned skill_epoch matches (C4 recall query).
CREATE OR REPLACE FUNCTION events_by_actor_epoch(p_key TEXT, p_epoch TEXT)
RETURNS TABLE(event_id UUID, event_type TEXT) LANGUAGE sql STABLE AS $$
    SELECT el.event_id, el.event_type
    FROM event_log el
    JOIN actor_current ac ON ac.signing_key_id = el.signer_key_id
    WHERE el.signer_key_id = p_key
      AND ac.pinned ->> 'skill_epoch' = p_epoch;
$$;

-- Mark one event recalled (append-only overlay, never erase).
CREATE OR REPLACE FUNCTION recall_event(p_target UUID, p_reason TEXT)
RETURNS UUID LANGUAGE plpgsql AS $$
DECLARE rid UUID;
BEGIN
    INSERT INTO recall_overlay (target_event_id, reason)
    VALUES (p_target, p_reason) RETURNING recall_id INTO rid;
    RETURN rid;
END;
$$;

COMMIT;
```

- [ ] **Step 4: Add to the `SCHEMA` array and run the test**

Modify `poc/walking-skeleton/crates/cairn-sync/src/main.rs`: change the array size to 6 and add the row:

```rust
const SCHEMA: [(&str, &str); 6] = [
    ("001_envelope", include_str!("../../../db/001_envelope.sql")),
    ("002_projection", include_str!("../../../db/002_projection.sql")),
    ("003_blobs", include_str!("../../../db/003_blobs.sql")),
    ("004_actors", include_str!("../../../db/004_actors.sql")),
    ("005_submit", include_str!("../../../db/005_submit.sql")),
    ("006_recall", include_str!("../../../db/006_recall.sql")),
];
```

Run: `cargo build -p cairn-sync && psql "$CONN" -v ON_ERROR_STOP=1 -f poc/walking-skeleton/db/006_recall.sql -f poc/walking-skeleton/db/tests/006_recall_test.sql`
Expected: `NOTICE: recall OK: overlay added, no data erased`.

- [ ] **Step 5: Commit**

```bash
git add poc/walking-skeleton/db/006_recall.sql poc/walking-skeleton/db/tests/006_recall_test.sql poc/walking-skeleton/crates/cairn-sync/src/main.rs
git commit -m "feat(spike-0002): recall query + append-only contamination overlay (C4)"
```

---

### Task 10: `harness/spike_0002.py` — the C1–C5 selftest table

The end-to-end integration test that drives everything and prints the pass/fail table, mirroring `bet_a.py`'s shape and `--force` guard.

**Files:**
- Create: `poc/walking-skeleton/harness/spike_0002.py`
- Modify: `poc/walking-skeleton/README.md` (a "Spike 0002 harness" section)

**Interfaces:**
- Consumes: `cairn-sync init`/`sign-stdin`, `enroll_actor`, `submit_event`, `events_by_actor_epoch`, `recall_event`, `agent_standin.author`.

- [ ] **Step 1: Write the harness with the C1–C5 checks**

Create `poc/walking-skeleton/harness/spike_0002.py`:

```python
"""Spike 0002 — the C1-C5 advisory-actor write-contract pass/fail table.

Self-contained selftest against ONE local database. Drives the agent stand-in
through submit_event and runs the five hostile-agent attacks; prints C1-C5 and
exits 0 iff all PASS. selftest DROPs+recreates the Cairn tables, so it requires
--force (guards a mistyped --conn), exactly like bet_a.py.
"""
import argparse
import json
import subprocess
import sys
import uuid

import psycopg
import agent_standin as agent

BIN_DEFAULT = "../target/debug/cairn-sync"


def sh(bin_path, *a, stdin=None):
    p = subprocess.run([bin_path, *a], input=stdin, capture_output=True)
    return p.returncode, p.stdout.decode(), p.stderr.decode()


def expect_raises(db, sql, params, needle, label):
    """Return True iff `sql` raises an error whose message contains `needle`."""
    try:
        db.execute(sql, params)
        return False, f"{label}: NO error raised (floor breached)"
    except psycopg.Error as e:
        msg = str(e)
        ok = needle.lower() in msg.lower()
        return ok, f"{label}: {'OK' if ok else 'WRONG ERROR'} — {msg.splitlines()[0]}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["selftest"])
    ap.add_argument("--conn", required=True)
    ap.add_argument("--bin", default=BIN_DEFAULT)
    ap.add_argument("--force", action="store_true")
    args = ap.parse_args()
    if not args.force:
        sys.exit("refusing to DROP/recreate without --force")

    # Fresh schema.
    with psycopg.connect(args.conn, autocommit=True) as db:
        for t in ["recall_overlay","event_type_class","blob_chunk","blob_store",
                  "patient_chart","actor_event","event_log","hlc_state","sync_state"]:
            db.execute(f"DROP TABLE IF EXISTS {t} CASCADE")
    sh(args.bin, "init", "--conn", args.conn)

    results = {}
    pid = str(uuid.uuid4())
    with psycopg.connect(args.conn, autocommit=True) as db:
        # Enroll a human attester and the agent (distinct keys).
        human_key = _enroll(db, args.bin, "human", "/tmp/human.key",
                            {"role": "clinician"})
        agent_key = _enroll(db, args.bin, "agent", "/tmp/agent.key",
                            {"model": "triage-stub", "version": "1", "skill_epoch": "epoch-a"})
        # A patient + a provenance blob the advisory can cite.
        db.execute("SELECT blob_note_reference(decode(%s,'hex'),%s,%s)",
                   ("1e20"+"11"*32, "message/rfc822", 1))
        blob_addr = "1e20" + "11"*32

        # ---- C1 + C3: the agent authors an additive, un-attested, provenance advisory.
        eid = agent.author(args.conn, args.bin, "/tmp/agent.key", blob_addr, pid)
        row = db.execute("SELECT contributors, attachments FROM event_log WHERE event_id=%s",
                         (eid,)).fetchone()
        contributors, attachments = row
        c1 = (any(c.get("role") == "triaged" and "responsibility" not in c for c in contributors)
              and not any("is_ai" in c for c in contributors))
        results["C1 additive, un-attested (no is_ai, no responsibility)"] = c1
        c3 = len(attachments) == 1 and attachments[0]["digest_hex"] == blob_addr
        results["C3 provenance-anchored"] = c3

        # ---- C2: an identical SUPPRESSING event authored un-attested is rejected.
        supp = _agent_body("salience.downgrade", pid, {"target_event_id": eid}, [], agent_key)
        signed = agent._sign(args.bin, "/tmp/agent.key", supp)
        ok, detail = expect_raises(db, "SELECT submit_event(decode(%s,'hex'))", (signed,),
                                   "requires attestation", "C2 suppress-un-attested rejected")
        results["C2 additive accepted; suppressing-un-attested rejected"] = c1 and ok
        print("   ", detail)

        # ---- C4: recall query returns exactly this advisory; recall overlays, never erases.
        found = db.execute("SELECT event_id FROM events_by_actor_epoch(%s,%s)",
                           (agent_key, "epoch-a")).fetchall()
        n_before = db.execute("SELECT count(*) FROM event_log").fetchone()[0]
        db.execute("SELECT recall_event(%s,%s)", (eid, "epoch recall"))
        n_after = db.execute("SELECT count(*) FROM event_log").fetchone()[0]
        # Bumping skill_epoch mints a new actor_id (distinct from epoch-a's).
        aid_a = db.execute("SELECT cairn_actor_id(%s)",
                          (json.dumps({"model":"triage-stub","version":"1","skill_epoch":"epoch-a"}),)).fetchone()[0]
        aid_b = db.execute("SELECT cairn_actor_id(%s)",
                          (json.dumps({"model":"triage-stub","version":"1","skill_epoch":"epoch-b"}),)).fetchone()[0]
        results["C4 version-pinned + recallable (overlay, no erase)"] = (
            any(str(f[0]) == eid for f in found) and n_after == n_before and aid_a != aid_b)

        # ---- C5: the five hostile attacks all fail closed with legible reasons.
        c5_checks = []
        # C5.1 unsigned/malformed
        c5_checks.append(expect_raises(db, "SELECT submit_event(%s)", (b"\xde\xad",),
                                       "signature", "C5.1 unsigned/malformed"))
        # C5.4 raw INSERT as the agent role
        c5_checks.append(_raw_insert_denied(db))
        # C5.2 forged human author (responsibility claimed, no token)
        forged = _agent_body("advisory.added", pid, {"x": 1},
                             [{"alg":"blake3","digest_hex":blob_addr,"media_type":"m","descriptor":"d","byte_len":1}],
                             agent_key, responsibility="attested")
        signed = agent._sign(args.bin, "/tmp/agent.key", forged)
        c5_checks.append(expect_raises(db, "SELECT submit_event(decode(%s,'hex'))", (signed,),
                                       "attestation", "C5.2 forged human author"))
        # C5.3 == C2 (suppress-un-attested) already covered; re-assert here for the table.
        c5_checks.append((results["C2 additive accepted; suppressing-un-attested rejected"],
                          "C5.3 suppressing-un-attested (see C2)"))
        # C5.5 salience downgrade of another author's event, un-attested
        downgrade = _agent_body("salience.downgrade", pid, {"target_event_id": eid}, [], agent_key)
        signed = agent._sign(args.bin, "/tmp/agent.key", downgrade)
        c5_checks.append(expect_raises(db, "SELECT submit_event(decode(%s,'hex'))", (signed,),
                                       "attestation", "C5.5 cross-author salience downgrade"))
        for ok, detail in c5_checks:
            print("   ", detail)
        # Committed-event set unchanged by the attacks (only the C1 advisory + patient exist).
        committed = db.execute("SELECT count(*) FROM event_log WHERE event_type='advisory.added'").fetchone()[0]
        results["C5 floor holds against hostile agent"] = all(ok for ok, _ in c5_checks) and committed == 1

    print("\n  Spike 0002 — C1-C5")
    all_pass = True
    for k, v in results.items():
        print(f"  [{'PASS' if v else 'FAIL'}] {k}")
        all_pass = all_pass and v
    sys.exit(0 if all_pass else 1)


def _enroll(db, bin_path, kind, key_path, pinned):
    """Create the key (sign a throwaway body), learn its kid, enroll it, return the kid."""
    body = _agent_body("probe.added", str(uuid.uuid4()), {}, [], "")
    signed = agent._sign(bin_path, key_path, body)
    kid = db.execute("SELECT cairn_body(decode(%s,'hex')) ->> 'signer_key_id'", (signed,)).fetchone()[0]
    db.execute("SELECT enroll_actor(%s,%s,%s)", (kind, json.dumps(pinned), kid))
    return kid


def _agent_body(event_type, patient_id, payload, attachments, kid, responsibility=None):
    contrib = {"actor_id": "agent", "role": "triaged"}
    if responsibility:
        contrib = {"actor_id": "agent", "role": "attested", "responsibility": responsibility}
    return {
        "event_id": str(uuid.uuid4()), "patient_id": patient_id,
        "event_type": event_type, "schema_version": "advisory/1",
        "hlc": {"wall": 1, "counter": 0, "node_origin": "agent"},
        "t_effective": None, "signer_key_id": kid,
        "contributors": [contrib], "payload": payload, "attachments": attachments,
    }


def _raw_insert_denied(db):
    try:
        db.execute("SET ROLE cairn_agent")
        try:
            db.execute("""INSERT INTO event_log (event_id,patient_id,event_type,schema_version,
                hlc_wall,hlc_counter,node_origin,signed_bytes,content_address,body,contributors,
                signer_key_id,plaintext_twin) VALUES (gen_random_uuid(),gen_random_uuid(),'x','x',
                0,0,'n','\\x00','\\x1220'||digest('\\x00','sha256'),'{}','[]','k','t')""")
            db.execute("RESET ROLE")
            return False, "C5.4 raw INSERT: NOT denied (floor breached)"
        except psycopg.errors.InsufficientPrivilege as e:
            return True, f"C5.4 raw INSERT denied — {str(e).splitlines()[0]}"
    finally:
        try:
            db.execute("RESET ROLE")
        except psycopg.Error:
            pass


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Run the full selftest**

Run:

```bash
cd poc/walking-skeleton/harness
uv run python spike_0002.py selftest --conn "host=127.0.0.1 user=postgres dbname=skeleton_c" \
  --bin ../target/debug/cairn-sync --force
```

Expected: the C1–C5 table prints `[PASS]` on every row; exit code 0. Investigate any `FAIL`/`WRONG ERROR` line — per the spike §6, a genuine FAIL is design feedback (e.g. an ADR-0022 submit-surface gap), not something to paper over.

- [ ] **Step 3: Document and commit**

Add a "Spike 0002 harness" section to `poc/walking-skeleton/README.md` (the `uv run python spike_0002.py selftest …` invocation + the C1–C5 meaning + the pgrx prerequisite).

```bash
git add poc/walking-skeleton/harness/spike_0002.py poc/walking-skeleton/README.md
git commit -m "feat(spike-0002): C1-C5 advisory-actor write-contract selftest harness"
```

- [ ] **Step 4: Update the spike doc status**

Modify `docs/spikes/0002-advisory-actor-write-contract.md`: change the Status line from **Proposed** to **Run YYYY-MM-DD — C1–C5 result** with the outcome, and (if C1–C5 PASS) note that the two follow-on ADRs are now unblocked. Commit.

```bash
git add docs/spikes/0002-advisory-actor-write-contract.md
git commit -m "docs(spike-0002): record C1-C5 run result"
```

---

## Self-Review

**Spec coverage** (against the design doc §4–§7 and the spike C1–C6):
- §4.1 actor registry → Task 5. §4.2 contributor set → Task 6. §4.3 pgrx (`cairn_verify`/`cairn_body`/`cairn_actor_id`/`cairn_attestation_ok`) → Tasks 1, 4 (+ crypto in Tasks 2, 3). §4.4 `submit_event` + grant floor → Task 7. §4.5 agent stand-in → Task 8. §4.6 recall → Task 9. §5 C1–C5 harness → Task 10.
- C1 → Task 10 (contributor check, no `is_ai`). C2 → Task 7 + Task 10. C3 → Task 7 (provenance gate) + Task 10. C4 → Tasks 5, 9 + Task 10. C5.1–C5.5 → Task 7 + Task 10. C6 deferred (design §3) — **not** in scope; recorded here so the omission is intentional, not a gap.
- Global constraints (AGPL, `uv`, §9 mapping, no `is_ai`, never-erase, legible rejections, ADR-0028 roles, excluded pgrx crate, no PG18 syntax) — each appears in the tasks that touch them.

**Placeholder scan:** no "TBD/TODO/handle edge cases" — every code step shows complete code; every command shows expected output.

**Type consistency:** `submit_event(bytea, bytea, bytea)` signature is identical in Task 7 (definition), the grant, and Tasks 8/10 (calls, using the 1-arg default form). `cairn_body`/`cairn_actor_id`/`cairn_attestation_ok` signatures match between Task 4 (definition) and Tasks 5/7/10 (calls). `enroll_actor(kind, pinned, key)` matches between Task 5 and Task 10's `_enroll`. `events_by_actor_epoch(key, epoch)` and `recall_event(target, reason)` match between Task 9 and Task 10. `Contributor { actor_id, role, responsibility? }` (Task 6) matches the JSON the harness asserts (Task 10).

**One known integration risk carried from the design:** byte-identical COSE_Sign1 from Python is sidestepped entirely by the `sign-stdin` helper (Task 8) — Rust owns the encoding, so there is no second signer implementation to keep in sync.
