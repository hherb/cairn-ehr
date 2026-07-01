# Identity C2 — match_proposal → apply seam Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn a human-accepted `match_proposal` row into a real, human-attested `identity.link.asserted` event through the existing C1 door, projecting the link into `patient_link` / `person_member`.

**Architecture:** A Rust seam in `cairn-node` reads `match_proposal WHERE status='accepted'`, assembles the link `EventBody` (pure), signs + attests with the accepting human's key, calls the existing 3-arg `submit_event` (which already runs the C1 identity floor + the ADR-0030 attestation gate + the `patient_link_apply` trigger), and marks the proposal applied — all in one transaction. No `submit_event` change, no new event type, no spec/ADR change. One additive migration adds an `applied_event_id` column.

**Tech Stack:** Rust (`cairn-node`, `cairn-event`), `tokio_postgres`, PostgreSQL 18 + `cairn_pgx`.

## Global Constraints

- **Licensing:** AGPL-3.0; every dependency AGPL-3.0-compatible. No new dependency is needed for this plan.
- **TDD:** failing test first, then minimal code. No production code without a test that drove it.
- **Reviewer-legible, junior-readable inline docs** on every non-trivial function/module (§9 house rule).
- **Files under 500 lines** where feasible.
- **All tests pass before committing.**
- **Safety-critical tier (§9):** event construction + signing + the submit call live in Rust (`cairn-node`/`cairn-event`) — never re-serialized in Python.
- **Additive-only:** `db/019` is additive DDL (a nullable column). No `submit_event` re-declaration, no new event type, no SCHEMA-floor version bump.
- **DB-gated tests** require `CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test"` (PG18 + `cairn_pgx`); they self-serialize via `db::test_serial_guard` and no-op (early `return`) when `$CAIRN_TEST_PG` is unset.

## Exact signatures this plan reuses (verbatim, do not guess)

```rust
// cairn-event
pub struct EventBody {
    pub event_id: String, pub patient_id: String, pub event_type: String,
    pub schema_version: String, pub hlc: Hlc, pub t_effective: Option<String>,
    pub signer_key_id: String, pub contributors: serde_json::Value,
    pub payload: serde_json::Value, pub attachments: Vec<AttachmentRef>,
    pub plaintext_twin: Option<String>,
}
pub struct Hlc { pub wall: i64, pub counter: i64, pub node_origin: String }  // fields as used in tests
pub fn generate_key() -> Result<(SigningKey, String)>;      // (key, hex key-id)
pub fn sign(body: &EventBody, sk: &SigningKey) -> Result<SignedEvent, EventError>; // .signed_bytes: Vec<u8>
pub fn event_address(signed_bytes: &[u8]) -> Vec<u8>;
pub fn sign_attestation(content_address: &[u8], attester_key_id: &str, role: &str, sk: &SigningKey) -> Result<Vec<u8>, EventError>;
// SigningKey exposes .verifying_key().to_bytes().to_vec()  (see attestation.rs)

// cairn-event::identity  (C1 builders — already exist)
pub struct LinkAssertion<'a> { pub subject_a: &'a str, pub subject_b: &'a str, pub provenance: &'a str, pub confidence: Option<&'a str> }
pub fn link_assertion_body(a: &LinkAssertion) -> serde_json::Value;
pub fn render_link_twin(a: &LinkAssertion) -> String;   // "link: {a} ↔ {b} ({provenance})"

// cairn-node::db
pub async fn connect_and_load_schema(conn: &str) -> anyhow::Result<tokio_postgres::Client>;
pub async fn test_serial_guard(conn: &str) -> anyhow::Result<tokio_postgres::Client>;
```

The 3-arg door is called as `SELECT submit_event($1,$2,$3)` with params `(&signed_bytes, &token, &attester_vk)` (see `crates/cairn-node/tests/attestation.rs`).

---

## File Structure

- **Create** `db/019_apply_proposal.sql` — additive `applied_event_id` column on `match_proposal`.
- **Modify** `crates/cairn-node/src/db.rs` — extend the `SCHEMA` array (`; 17` → `; 18`, add the `019` entry).
- **Create** `crates/cairn-node/src/apply_proposal.rs` — pure `compose_provenance` + `build_attested_link_body`; IO `apply_accepted_proposal`. Target < 300 lines.
- **Modify** `crates/cairn-node/src/lib.rs` — add `pub mod apply_proposal;`.
- **Create** `crates/cairn-node/tests/apply_proposal.rs` — DB-gated integration tests (happy path, idempotency, forgery-refused, non-accepted-skipped).

---

## Task 1: Additive migration — `applied_event_id` column

**Files:**
- Create: `db/019_apply_proposal.sql`
- Modify: `crates/cairn-node/src/db.rs` (the `SCHEMA` array, currently `[(&str, &str); 17]`)
- Test: `crates/cairn-node/tests/apply_proposal.rs` (one column-exists test to start the file)

**Interfaces:**
- Consumes: `db/017_match_proposal.sql`'s `match_proposal` table.
- Produces: `match_proposal.applied_event_id UUID` (nullable); schema loads cleanly via `connect_and_load_schema`.

- [ ] **Step 1: Write the failing test** — create `crates/cairn-node/tests/apply_proposal.rs`:

```rust
//! Integration coverage for the §5.2/§5.7 C2 apply seam: a human-accepted
//! match_proposal becomes a human-attested identity.link.asserted event through
//! the existing submit_event door, projecting the link into patient_link /
//! person_member. Real Postgres, gated on $CAIRN_TEST_PG, serialized cluster-wide
//! via db::test_serial_guard. No submit_event change is exercised here — C2 only
//! composes the C1 identity floor (db/018) and the ADR-0030 attestation gate (db/005).
use cairn_node::db;
use tokio_postgres::Client;

fn cs() -> Option<String> { std::env::var("CAIRN_TEST_PG").ok() }

#[tokio::test]
async fn migration_adds_applied_event_id_column() {
    let Some(base) = cs() else { return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c: Client = db::connect_and_load_schema(&base).await.unwrap();
    let n: i64 = c
        .query_one(
            "SELECT count(*) FROM information_schema.columns \
             WHERE table_name='match_proposal' AND column_name='applied_event_id'",
            &[],
        )
        .await.unwrap().get(0);
    assert_eq!(n, 1, "db/019 must add match_proposal.applied_event_id");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd crates/cairn-node && CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" cargo test --test apply_proposal migration_adds_applied_event_id_column`
Expected: FAIL — `assert_eq!(n, 1)` fails with `n == 0` (column absent; `db/019` not loaded yet).

- [ ] **Step 3: Create the migration** — `db/019_apply_proposal.sql`:

```sql
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
```

- [ ] **Step 4: Wire it into the schema loader** — in `crates/cairn-node/src/db.rs`, change the array length and append the entry:

```rust
const SCHEMA: [(&str, &str); 18] = [
    // ... existing 17 entries unchanged ...
    ("018_identity_linkage", include_str!("../../../db/018_identity_linkage.sql")),
    ("019_apply_proposal", include_str!("../../../db/019_apply_proposal.sql")),
];
```

(Only two edits: `; 17]` → `; 18]` on the declaration, and the new final tuple after the `018` line.)

- [ ] **Step 5: Run test to verify it passes**

Run: `cd crates/cairn-node && CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" cargo test --test apply_proposal migration_adds_applied_event_id_column`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add db/019_apply_proposal.sql crates/cairn-node/src/db.rs crates/cairn-node/tests/apply_proposal.rs
git commit -m "feat(identity): db/019 additive applied_event_id column on match_proposal (C2)"
```

---

## Task 2: Pure link-body assembly

**Files:**
- Create: `crates/cairn-node/src/apply_proposal.rs`
- Modify: `crates/cairn-node/src/lib.rs` (add `pub mod apply_proposal;`)
- Test: `#[cfg(test)]` unit tests inside `apply_proposal.rs`

**Interfaces:**
- Consumes: `cairn_event::identity::{LinkAssertion, link_assertion_body, render_link_twin}`, `cairn_event::{EventBody, Hlc}`, `uuid::Uuid`.
- Produces:
  - `pub fn compose_provenance(matcher_version: &str, human_kid: &str) -> String`
  - `pub fn build_attested_link_body(event_id: Uuid, low: Uuid, high: Uuid, provenance: &str, confidence: Option<&str>, human_kid: &str, hlc: Hlc) -> EventBody`

- [ ] **Step 1: Write the failing tests** — create `crates/cairn-node/src/apply_proposal.rs`:

```rust
//! §5.2/§5.7 C2 apply seam: turn a human-ACCEPTED match_proposal (db/017) into a
//! human-ATTESTED `identity.link.asserted` event through the existing submit_event
//! door. This module owns the seam; it changes no floor. The link event is *additive*
//! but carries a responsibility-bearing contributor (the accepting human), which trips
//! the existing db/005 attestation gate — so submit_event requires a valid human token
//! bound to this event. The event construction lives here (Rust, §9 safety-critical
//! tier) and reuses cairn-event's serialization verbatim — never re-serialized elsewhere.
//!
//! Split: pure body-assembly (unit-testable, no DB) + one IO function that reads the
//! proposal, signs, attests, submits, and marks the proposal applied in one transaction.

use cairn_event::identity::{link_assertion_body, render_link_twin, LinkAssertion};
use cairn_event::{EventBody, Hlc};
use uuid::Uuid;

/// The schema_version string for a link event (mirrors the C1 test convention).
const LINK_SCHEMA_VERSION: &str = "identity.link/1";

/// Compose the §4.1 provenance string for a matcher-proposed, human-accepted link.
/// Non-empty by construction (the db/018 floor requires it) and legible: it records
/// both the ADR-0014 matcher config digest AND that a specific human vouched.
pub fn compose_provenance(matcher_version: &str, human_kid: &str) -> String {
    format!("matcher:{matcher_version} accepted-by:{human_kid}")
}

/// Assemble the `identity.link.asserted` EventBody for an accepted proposal. Pure:
/// `event_id` is supplied by the caller (so this stays deterministic and testable, and
/// the caller can reuse the same id as match_proposal.applied_event_id). `low`/`high`
/// are the canonical pair (low < high); subject_a := low, subject_b := high. The
/// accepting human is the sole contributor and carries a `responsibility` marker — this
/// is what makes submit_event demand a valid human attestation token.
pub fn build_attested_link_body(
    event_id: Uuid,
    low: Uuid,
    high: Uuid,
    provenance: &str,
    confidence: Option<&str>,
    human_kid: &str,
    hlc: Hlc,
) -> EventBody {
    let low_s = low.to_string();
    let high_s = high.to_string();
    let la = LinkAssertion { subject_a: &low_s, subject_b: &high_s, provenance, confidence };
    EventBody {
        event_id: event_id.to_string(),
        patient_id: low_s.clone(), // C1 convention: an identity event is "about" subject_a
        event_type: "identity.link.asserted".into(),
        schema_version: LINK_SCHEMA_VERSION.into(),
        hlc,
        t_effective: None,
        signer_key_id: human_kid.into(),
        // Responsibility-bearing contributor -> trips the db/005 attestation gate.
        contributors: serde_json::json!([
            {"actor_id": human_kid, "role": "attested", "responsibility": "attested"}
        ]),
        payload: link_assertion_body(&la),
        attachments: vec![],
        plaintext_twin: Some(render_link_twin(&la)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> (Uuid, Uuid, Uuid) {
        // Fixed, ordered UUIDs so low < high is stable and assertions are deterministic.
        let a = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let b = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
        let eid = Uuid::parse_str("11111111-0000-0000-0000-000000000000").unwrap();
        (eid, a, b)
    }

    #[test]
    fn provenance_is_nonempty_and_names_matcher_and_human() {
        let p = compose_provenance("cfg@abc", "humankid");
        assert!(p.contains("cfg@abc"));
        assert!(p.contains("humankid"));
        assert!(!p.trim().is_empty());
    }

    #[test]
    fn body_carries_responsibility_bearing_human_contributor() {
        let (eid, a, b) = ids();
        let body = build_attested_link_body(eid, a, b, "matcher:x accepted-by:h", None, "h", Hlc { wall: 5, counter: 0, node_origin: "n".into() });
        let c = &body.contributors[0];
        assert_eq!(c["actor_id"], "h");
        assert_eq!(c["role"], "attested");
        assert!(c.get("responsibility").is_some(), "must carry a responsibility marker to trip the attestation gate");
    }

    #[test]
    fn body_is_a_link_event_with_authored_twin_and_canonical_subjects() {
        let (eid, a, b) = ids();
        let body = build_attested_link_body(eid, a, b, "matcher:x accepted-by:h", Some("0.910"), "h", Hlc { wall: 5, counter: 0, node_origin: "n".into() });
        assert_eq!(body.event_type, "identity.link.asserted");
        assert_eq!(body.payload["subject_a"], a.to_string());
        assert_eq!(body.payload["subject_b"], b.to_string());
        assert_eq!(body.payload["confidence"], "0.910");
        let twin = body.plaintext_twin.as_deref().unwrap();
        assert!(twin.starts_with("link: "), "authored twin required by the db/018 floor");
    }
}
```

- [ ] **Step 2: Add the module** — in `crates/cairn-node/src/lib.rs`, add (keeping alphabetical order near the other `apply`/`backup` modules):

```rust
pub mod apply_proposal;
```

- [ ] **Step 3: Run tests to verify they fail then pass**

Run: `cd crates/cairn-node && cargo test --lib apply_proposal`
Expected: compiles and the three unit tests PASS (pure, no DB needed). If the module was just created with the code above, they pass directly; the "failing" state is the pre-module compile error you resolve by adding the code.

- [ ] **Step 4: Clippy**

Run: `cd crates/cairn-node && cargo clippy --lib`
Expected: no warnings on the new module.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-node/src/apply_proposal.rs crates/cairn-node/src/lib.rs
git commit -m "feat(identity): pure attested-link body assembly for the C2 apply seam"
```

---

## Task 3: IO apply function + happy-path integration test

**Files:**
- Modify: `crates/cairn-node/src/apply_proposal.rs` (add the IO function + imports)
- Test: `crates/cairn-node/tests/apply_proposal.rs` (add setup helpers + happy-path test)

**Interfaces:**
- Consumes: `compose_provenance`, `build_attested_link_body` (Task 2); `cairn_event::{generate_key, sign, event_address, sign_attestation, SigningKey, Hlc}`; a `&mut tokio_postgres::Client`.
- Produces: `pub async fn apply_accepted_proposal(client: &mut tokio_postgres::Client, low: Uuid, high: Uuid, human_sk: &SigningKey, human_kid: &str, hlc: Hlc) -> anyhow::Result<Uuid>` — returns the applied link event id; errors (rolling back) if the proposal is absent or not `status='accepted'`, or if `submit_event` rejects.

- [ ] **Step 1: Write the failing integration test** — append to `crates/cairn-node/tests/apply_proposal.rs`:

```rust
use cairn_event::{generate_key, Hlc, SigningKey};
use cairn_node::apply_proposal::apply_accepted_proposal;
use uuid::Uuid;

/// Truncate the tables this seam touches and enroll one human actor (the accepting
/// reviewer). Returns (human signing key, human hex key-id). patient_link/person_member
/// are guarded by to_regclass so this stays correct as db/018 grows.
async fn setup(c: &Client) -> (SigningKey, String) {
    c.batch_execute(
        "TRUNCATE event_log, actor_event, patient_chart, match_proposal CASCADE")
        .await.unwrap();
    c.batch_execute(
        "DO $$ BEGIN \
           IF to_regclass('public.patient_link')  IS NOT NULL THEN TRUNCATE patient_link;  END IF; \
           IF to_regclass('public.person_member') IS NOT NULL THEN TRUNCATE person_member; END IF; \
         END $$;")
        .await.unwrap();
    let (sk_h, kid_h) = generate_key().unwrap();
    c.execute(
        "SELECT enroll_actor('human', '{\"role\":\"clinician\"}', $1)",
        &[&kid_h],
    ).await.unwrap();
    (sk_h, kid_h)
}

/// Seed one accepted match_proposal for the canonical (low, high) pair. veto_findings /
/// evidence are JSONB NOT NULL, so pass empty arrays.
async fn seed_accepted_proposal(c: &Client, low: Uuid, high: Uuid, status: &str) {
    c.execute(
        "INSERT INTO match_proposal \
           (patient_low, patient_high, score_total, band, veto_findings, evidence, matcher_version, status) \
         VALUES ($1,$2, 0.91, 'review', '[]'::jsonb, '[]'::jsonb, 'cfg@test', $3)",
        &[&low, &high, &status.to_string()],
    ).await.unwrap();
}

/// Order a pair canonically (low < high by uuid value) to match the match_proposal CHECK.
fn canonical(a: Uuid, b: Uuid) -> (Uuid, Uuid) { if a < b { (a, b) } else { (b, a) } }

#[tokio::test]
async fn accepted_proposal_becomes_attested_link_and_projects_person() {
    let Some(base) = cs() else { return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let mut c: Client = db::connect_and_load_schema(&base).await.unwrap();
    let (sk_h, kid_h) = setup(&c).await;
    let (low, high) = canonical(Uuid::now_v7(), Uuid::now_v7());
    seed_accepted_proposal(&c, low, high, "accepted").await;

    let eid = apply_accepted_proposal(&mut c, low, high, &sk_h, &kid_h,
        Hlc { wall: 100, counter: 0, node_origin: "n".into() })
        .await.expect("accepted proposal must apply");

    // The link event was appended.
    let n_ev: i64 = c.query_one(
        "SELECT count(*) FROM event_log WHERE event_type='identity.link.asserted'", &[])
        .await.unwrap().get(0);
    assert_eq!(n_ev, 1, "exactly one link event appended");

    // The standing edge exists.
    let n_edge: i64 = c.query_one(
        "SELECT count(*) FROM patient_link WHERE low=$1 AND high=$2 AND state='link'",
        &[&low, &high]).await.unwrap().get(0);
    assert_eq!(n_edge, 1, "patient_link edge present");

    // Both patients project to the same (min-UUID) person_id.
    let person_low: Uuid = c.query_one(
        "SELECT person_id FROM person_member WHERE patient_id=$1", &[&low])
        .await.unwrap().get(0);
    let person_high: Uuid = c.query_one(
        "SELECT person_id FROM person_member WHERE patient_id=$1", &[&high])
        .await.unwrap().get(0);
    assert_eq!(person_low, person_high, "both members share one person_id");
    assert_eq!(person_low, low, "person_id is the min-UUID representative");

    // The proposal was marked applied, pointing at the link event.
    let (status, applied): (String, Option<Uuid>) = {
        let row = c.query_one(
            "SELECT status, applied_event_id FROM match_proposal WHERE patient_low=$1 AND patient_high=$2",
            &[&low, &high]).await.unwrap();
        (row.get(0), row.get(1))
    };
    assert_eq!(status, "applied");
    assert_eq!(applied, Some(eid), "applied_event_id points at the emitted link event");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd crates/cairn-node && CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" cargo test --test apply_proposal accepted_proposal_becomes_attested_link_and_projects_person`
Expected: FAIL to compile — `apply_accepted_proposal` does not exist yet.

- [ ] **Step 3: Implement the IO function** — first add the imports it needs at the top of `crates/cairn-node/src/apply_proposal.rs`, next to the existing `use cairn_event::...` lines:

```rust
use cairn_event::{event_address, sign, sign_attestation, SigningKey};
```

Then append the function:

```rust
/// Apply one human-ACCEPTED match_proposal: read it, build + sign + attest the link
/// event with the accepting human's key, submit it through the existing 3-arg
/// submit_event door, and mark the proposal applied — all in ONE transaction.
///
/// Atomicity is the idempotency guarantee: if submit_event rejects (e.g. a non-human
/// attester) or any step fails, the whole transaction rolls back, so no link event is
/// written and the proposal stays 'accepted' to be retried. On success the event and
/// the 'applied' transition commit together, and a re-run finds no 'accepted' row.
///
/// Errors (Err, transaction rolled back) if the proposal is absent or its status is not
/// 'accepted' (only a human's acceptance applies), or if the in-DB floor refuses.
pub async fn apply_accepted_proposal(
    client: &mut tokio_postgres::Client,
    low: Uuid,
    high: Uuid,
    human_sk: &SigningKey,
    human_kid: &str,
    hlc: Hlc,
) -> anyhow::Result<Uuid> {
    let tx = client.transaction().await?;

    // 1. Read the proposal and require status='accepted'.
    let row = tx
        .query_opt(
            "SELECT score_total, matcher_version, status FROM match_proposal \
             WHERE patient_low=$1 AND patient_high=$2",
            &[&low, &high],
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("no match_proposal for pair ({low}, {high})"))?;
    let score: f64 = row.get(0);
    let matcher_version: String = row.get(1);
    let status: String = row.get(2);
    if status != "accepted" {
        // Rolls back on drop; nothing was written.
        anyhow::bail!("match_proposal ({low}, {high}) is '{status}', not 'accepted' — refusing to apply");
    }

    // 2. Compose provenance + confidence and build the attested link body.
    let provenance = compose_provenance(&matcher_version, human_kid);
    let confidence = format!("{score:.3}");
    let event_id = Uuid::now_v7();
    let body = build_attested_link_body(
        event_id, low, high, &provenance, Some(&confidence), human_kid, hlc,
    );

    // 3. Sign (human authors) + mint an attestation token (human vouches).
    let signed = sign(&body, human_sk)?;
    let ca = event_address(&signed.signed_bytes);
    let token = sign_attestation(&ca, human_kid, "attested", human_sk)?;
    let attester_vk = human_sk.verifying_key().to_bytes().to_vec();

    // 4. Submit through the existing 3-arg door: db/005 attestation gate + db/018
    //    identity floor + the patient_link_apply trigger all run here.
    tx.execute(
        "SELECT submit_event($1,$2,$3)",
        &[&signed.signed_bytes, &token, &attester_vk],
    )
    .await?;

    // 5. Mark the proposal applied, pointing at the emitted link event.
    tx.execute(
        "UPDATE match_proposal SET status='applied', applied_event_id=$3, updated_at=clock_timestamp() \
         WHERE patient_low=$1 AND patient_high=$2",
        &[&low, &high, &event_id],
    )
    .await?;

    tx.commit().await?;
    Ok(event_id)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd crates/cairn-node && CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" cargo test --test apply_proposal accepted_proposal_becomes_attested_link_and_projects_person`
Expected: PASS.

- [ ] **Step 5: Clippy (lib + tests)**

Run: `cd crates/cairn-node && cargo clippy --lib --tests`
Expected: no warnings. (If clippy flags the transaction-return or unused imports, fix inline — e.g. remove any unused `use`.)

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-node/src/apply_proposal.rs crates/cairn-node/tests/apply_proposal.rs
git commit -m "feat(identity): C2 apply seam — accepted proposal -> attested link event (happy path)"
```

---

## Task 4: Edge-case integration tests (idempotency, forgery-refused, non-accepted-skipped)

**Files:**
- Test: `crates/cairn-node/tests/apply_proposal.rs` (add three tests; also enroll an agent in one)

**Interfaces:**
- Consumes: everything from Task 3 (`apply_accepted_proposal`, `setup`, `seed_accepted_proposal`, `canonical`).
- Produces: no new production code — these tests characterize the seam's guarantees.

- [ ] **Step 1: Write the three failing/covering tests** — append to `crates/cairn-node/tests/apply_proposal.rs`:

```rust
#[tokio::test]
async fn re_applying_is_idempotent_no_second_link_event() {
    let Some(base) = cs() else { return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let mut c: Client = db::connect_and_load_schema(&base).await.unwrap();
    let (sk_h, kid_h) = setup(&c).await;
    let (low, high) = canonical(Uuid::now_v7(), Uuid::now_v7());
    seed_accepted_proposal(&c, low, high, "accepted").await;

    // First apply succeeds.
    apply_accepted_proposal(&mut c, low, high, &sk_h, &kid_h,
        Hlc { wall: 100, counter: 0, node_origin: "n".into() }).await.unwrap();

    // Second apply must refuse: the proposal is now 'applied', not 'accepted'.
    let again = apply_accepted_proposal(&mut c, low, high, &sk_h, &kid_h,
        Hlc { wall: 101, counter: 0, node_origin: "n".into() }).await;
    assert!(again.is_err(), "re-applying an 'applied' proposal must be refused");

    let n_ev: i64 = c.query_one(
        "SELECT count(*) FROM event_log WHERE event_type='identity.link.asserted'", &[])
        .await.unwrap().get(0);
    assert_eq!(n_ev, 1, "exactly one link event exists after a repeated apply");
}

#[tokio::test]
async fn non_human_attester_is_refused_and_nothing_leaks() {
    let Some(base) = cs() else { return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let mut c: Client = db::connect_and_load_schema(&base).await.unwrap();
    let (_sk_h, _kid_h) = setup(&c).await;
    // Enroll an AGENT (non-human) and try to apply with its key: the db/005 gate must
    // refuse the identity link (identity links cannot be forged without a human vouch).
    let (sk_a, kid_a) = generate_key().unwrap();
    c.execute(
        "SELECT enroll_actor('agent', '{\"model\":\"m\",\"version\":\"1\",\"skill_epoch\":\"e\"}', $1)",
        &[&kid_a],
    ).await.unwrap();
    let (low, high) = canonical(Uuid::now_v7(), Uuid::now_v7());
    seed_accepted_proposal(&c, low, high, "accepted").await;

    let r = apply_accepted_proposal(&mut c, low, high, &sk_a, &kid_a,
        Hlc { wall: 100, counter: 0, node_origin: "n".into() }).await;
    assert!(r.is_err(), "a non-human attester must be refused by the floor");
    assert!(format!("{:?}", r.unwrap_err()).contains("not an enrolled human actor"));

    // Nothing appended; the proposal stays 'accepted' for a later human to apply.
    let n_ev: i64 = c.query_one(
        "SELECT count(*) FROM event_log WHERE event_type='identity.link.asserted'", &[])
        .await.unwrap().get(0);
    assert_eq!(n_ev, 0, "no link event leaked through the refused apply");
    let status: String = c.query_one(
        "SELECT status FROM match_proposal WHERE patient_low=$1 AND patient_high=$2",
        &[&low, &high]).await.unwrap().get(0);
    assert_eq!(status, "accepted", "a refused apply leaves the proposal accepted");
}

#[tokio::test]
async fn pending_proposal_is_not_applied() {
    let Some(base) = cs() else { return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let mut c: Client = db::connect_and_load_schema(&base).await.unwrap();
    let (sk_h, kid_h) = setup(&c).await;
    let (low, high) = canonical(Uuid::now_v7(), Uuid::now_v7());
    seed_accepted_proposal(&c, low, high, "pending").await;

    let r = apply_accepted_proposal(&mut c, low, high, &sk_h, &kid_h,
        Hlc { wall: 100, counter: 0, node_origin: "n".into() }).await;
    assert!(r.is_err(), "only status='accepted' applies; 'pending' must be refused");

    let n_ev: i64 = c.query_one(
        "SELECT count(*) FROM event_log WHERE event_type='identity.link.asserted'", &[])
        .await.unwrap().get(0);
    assert_eq!(n_ev, 0, "a pending proposal produces no link event");
}
```

- [ ] **Step 2: Run the whole test file**

Run: `cd crates/cairn-node && CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" cargo test --test apply_proposal`
Expected: all tests PASS (migration, happy path, idempotency, non-human-refused, pending-not-applied).

- [ ] **Step 3: Run the full cairn-node suite + clippy (no regressions)**

Run: `cd crates/cairn-node && CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" cargo test && cargo clippy --tests`
Expected: whole suite green; clippy clean.

- [ ] **Step 4: Commit**

```bash
git add crates/cairn-node/tests/apply_proposal.rs
git commit -m "test(identity): C2 apply seam edge cases — idempotent, non-human refused, pending skipped"
```

---

## Task 5: Docs currency (HANDOVER + ROADMAP)

**Files:**
- Modify: `docs/HANDOVER.md` (add a "This session" C2 entry; move C2 from "Next" to done; keep C3+ as next)
- Modify: `docs/ROADMAP.md` (record the C2 slice under the identity/matcher build order)

**Interfaces:** documentation only.

- [ ] **Step 1:** Update `docs/HANDOVER.md`: add a concise "This session (2026-07-02): built matcher/identity piece **C2 — the match_proposal → apply seam** …" paragraph (what it is: additive `db/019` `applied_event_id`; `cairn-node::apply_proposal` pure body-assembly + IO `apply_accepted_proposal`; reuses submit_event 3-arg + attestation gate + C1 floor with **no floor change**; human-accepted only; 5 tests). Move C2 out of the "Next identity slices" line; keep **C3+** (identify/repudiate/dispute/reattribute) and **C2b** (auto-apply of the `auto_candidate` band) as the recorded next/deferred items. Prune older prose to stay under 500 lines.

- [ ] **Step 2:** Update `docs/ROADMAP.md`: add the C2 slice entry in the identity/matcher section, mirroring the C1 entry's style; note deferred C2b + the matcher-as-compositional-contributor (needs §7.5 registration) + CLI/key-custody.

- [ ] **Step 3: Commit**

```bash
git add docs/HANDOVER.md docs/ROADMAP.md
git commit -m "docs(identity): record C2 apply seam built (HANDOVER + ROADMAP)"
```

---

## Self-Review (completed)

**Spec coverage:** db/019 additive column → Task 1. Pure `build_attested_link_body` + `compose_provenance` → Task 2. IO `apply_accepted_proposal` (read → sign → attest → submit → mark, one txn) → Task 3. All four spec test cases (happy path, idempotency, forgery-refused, non-accepted-skipped) → Tasks 3–4. Provenance/confidence mapping → Task 2/3. Docs currency → Task 5. No spec requirement is unimplemented.

**Placeholder scan:** none — every code step shows complete code; every run step shows the exact command + expected result.

**Type consistency:** `apply_accepted_proposal(&mut Client, Uuid, Uuid, &SigningKey, &str, Hlc) -> anyhow::Result<Uuid>` and `build_attested_link_body(Uuid, Uuid, Uuid, &str, Option<&str>, &str, Hlc) -> EventBody` are used identically in Tasks 2–4. `compose_provenance(&str, &str) -> String` consistent. `submit_event($1,$2,$3)` param order `(signed_bytes, token, attester_vk)` matches `attestation.rs`. `Hlc { wall, counter, node_origin }` and `.verifying_key().to_bytes().to_vec()` match the verified signatures.

**Scope:** single subsystem (the C2 seam); one plan.
