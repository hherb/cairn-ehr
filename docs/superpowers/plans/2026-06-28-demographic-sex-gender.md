# Demographics slice 4 — administrative-sex + gender-identity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the two remaining §4.2 sex/gender fields (`administrative-sex`, `gender-identity`) to the demographics projection through the existing `demographic.field.asserted` spine, via a per-field winner-policy selector.

**Architecture:** No new event type, no new write door, no floor change. One new IMMUTABLE classifier `cairn_demographic_field_policy(field)` is the single source of truth for both the projection gate and the winner ordering; the `patient_demographic_apply()` trigger is rewritten to be policy-driven (provenance-first for dob/sex-at-birth/administrative-sex; recency-first for gender-identity). Two pure Rust builders + two twins mirror the slice-2 `sex_at_birth_*` pair. Karyotype is resolved as a distinct field in docs only (no code).

**Tech Stack:** Rust (`cairn-event` pure builders, `cairn-node` integration tests), PostgreSQL 18 + `cairn_pgx` (PL/pgSQL floor + projection), `tokio-postgres`.

## Global Constraints

- **AGPL-3.0**; every dependency AGPL-3.0-compatible (no new deps in this slice).
- **TDD** — failing test first, then minimal code. Load-bearing on the in-DB safety surface.
- **Inline docs for a junior dev** — every non-trivial function explains *why* and *how it fits*.
- **Files under ~500 lines** where feasible.
- **All tests pass before committing.**
- **Values are open strings** — never a closed sex/gender enum (principle 4: intersex / non-binary / questioning / unknown all recordable).
- **DB-gated tests** need `CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test"` (PG18 + cairn_pgx); they self-serialize via `db::test_serial_guard`. When `$CAIRN_TEST_PG` is unset they skip (print `skipped:`), not fail.
- **Spec home** demographics §4.2; new **ADR-0037**; spec version 0.37 → 0.38.

---

### Task 1: cairn-event builders + twins (pure, no DB)

**Files:**
- Modify: `crates/cairn-event/src/demographics.rs` (add 4 functions + 4 unit tests, after the `sex_at_birth_*` pair)

**Interfaces:**
- Consumes: `demographic_field_body(field, value, facets, provenance) -> serde_json::Value` (already in this module).
- Produces:
  - `administrative_sex_assertion_body(value: &str, provenance: &str) -> serde_json::Value`
  - `gender_identity_assertion_body(value: &str, provenance: &str) -> serde_json::Value`
  - `render_administrative_sex_twin(value: &str, provenance: &str) -> String`
  - `render_gender_identity_twin(value: &str, provenance: &str) -> String`

- [ ] **Step 1: Write the failing tests**

Add inside the existing `mod tests { ... }` block in `crates/cairn-event/src/demographics.rs`:

```rust
    #[test]
    fn administrative_sex_body_has_no_facets() {
        let v = administrative_sex_assertion_body("M", "document-verified");
        assert_eq!(v["field"], "administrative-sex");
        assert_eq!(v["value"], "M");
        assert_eq!(v["provenance"], "document-verified");
        let obj = v.as_object().unwrap();
        assert!(!obj.contains_key("facets"), "administrative-sex carries no facets bag");
    }

    #[test]
    fn gender_identity_body_has_no_facets() {
        let v = gender_identity_assertion_body("non-binary", "patient-stated");
        assert_eq!(v["field"], "gender-identity");
        assert_eq!(v["value"], "non-binary");
        assert_eq!(v["provenance"], "patient-stated");
        let obj = v.as_object().unwrap();
        assert!(!obj.contains_key("facets"), "gender-identity carries no facets bag");
    }

    #[test]
    fn sex_gender_twins_render_profile_independent_plaintext() {
        assert_eq!(
            render_administrative_sex_twin("M", "document-verified"),
            "Administrative sex (document-verified): M"
        );
        assert_eq!(
            render_gender_identity_twin("non-binary", "patient-stated"),
            "Gender identity (patient-stated): non-binary"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cairn-event administrative_sex gender_identity sex_gender_twins`
Expected: FAIL — `cannot find function administrative_sex_assertion_body` (and the others).

- [ ] **Step 3: Write the minimal implementation**

Add to `crates/cairn-event/src/demographics.rs`, immediately after `sex_at_birth_assertion_body` (around line 74) and after `render_sex_at_birth_twin` (around line 107) respectively — group the body builders with the builders and the twins with the twins to match the file's existing layout:

```rust
/// One §4.2 administrative-sex assertion — the legal/forms/billing gender marker
/// (M/F/X on documents). `value` is an OPEN string (principle 4); the projection
/// treats it provenance-first (db/013): a document-anchored marker an unverified
/// self-claim must not displace.
pub fn administrative_sex_assertion_body(value: &str, provenance: &str) -> Value {
    demographic_field_body("administrative-sex", value, None, provenance)
}

/// One §4.2 gender-identity assertion — the patient's stated gender. `value` is an
/// OPEN string (principle 4: non-binary / questioning / unknown all recordable).
/// The projection treats it recency-first (db/013): the newest assertion wins
/// regardless of provenance, so the patient's current stated identity always displays.
pub fn gender_identity_assertion_body(value: &str, provenance: &str) -> Value {
    demographic_field_body("gender-identity", value, None, provenance)
}
```

```rust
/// Render the §4.5 legibility twin for administrative sex:
/// `"Administrative sex (<provenance>): <value>"`.
pub fn render_administrative_sex_twin(value: &str, provenance: &str) -> String {
    format!("Administrative sex ({provenance}): {value}")
}

/// Render the §4.5 legibility twin for gender identity:
/// `"Gender identity (<provenance>): <value>"`.
pub fn render_gender_identity_twin(value: &str, provenance: &str) -> String {
    format!("Gender identity ({provenance}): {value}")
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cairn-event` then `cargo clippy -p cairn-event --all-targets -- -D warnings`
Expected: all `cairn-event` tests PASS (the 4 new + the existing demographics units); clippy clean.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-event/src/demographics.rs
git commit -m "$(cat <<'EOF'
feat(event): §4.2 administrative-sex + gender-identity assertion builders and twins

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: db/013 policy selector + policy-driven projection (integration TDD)

**Files:**
- Create: `db/013_demographics_sex_gender.sql`
- Modify: `crates/cairn-node/src/db.rs` (SCHEMA array: bump size literal `; 11]` → `; 12]`, add the `013` entry)
- Create: `crates/cairn-node/tests/demographics_sex_gender.rs`

**Interfaces:**
- Consumes: `cairn_provenance_rank(text) -> int` (db/011); `submit_event(bytea)` door (db/005); the Task-1 builders/twins; the `patient_demographic` table (db/011, PK `(patient_id, field)`).
- Produces (SQL): `cairn_demographic_field_policy(p_field text) -> text` (`'provenance-first'` | `'recency-first'` | `NULL`); a redefined `patient_demographic_apply()` trigger function.

- [ ] **Step 1: Write the failing integration tests**

Create `crates/cairn-node/tests/demographics_sex_gender.rs`:

```rust
//! Integration coverage for the §4.2 administrative-sex + gender-identity fields:
//! the per-field winner-policy selector (db/013) over the slice-2 patient_demographic
//! projection. administrative-sex is provenance-first (like dob/sex-at-birth);
//! gender-identity is recency-first (the inverse ordering). Real Postgres, gated on
//! `$CAIRN_TEST_PG`, serialized cluster-wide via `db::test_serial_guard`. Matching
//! (§5.2) is a separate subsystem and is NOT exercised here.
use cairn_event::demographics::{
    administrative_sex_assertion_body, gender_identity_assertion_body,
    render_administrative_sex_twin, render_gender_identity_twin,
};
use cairn_event::{generate_key, sign, EventBody, Hlc, SigningKey};
use cairn_node::db;
use tokio_postgres::Client;
use uuid::Uuid;

fn cs() -> Option<String> { std::env::var("CAIRN_TEST_PG").ok() }

/// Truncate the clinical tables and enroll one agent signer. Returns (sk, kid).
async fn setup(c: &Client) -> (SigningKey, String) {
    c.batch_execute(
        "TRUNCATE event_log, actor_event, patient_chart, patient_identifier, patient_demographic CASCADE")
        .await.unwrap();
    let (sk, kid) = generate_key().unwrap();
    c.execute(
        "SELECT enroll_actor('agent', '{\"model\":\"reg-stub\",\"version\":\"1\",\"skill_epoch\":\"e\"}', $1)",
        &[&kid],
    ).await.unwrap();
    (sk, kid)
}

/// Author + sign + submit one demographic.field.asserted event at HLC (wall, counter).
/// `counter` is explicit so a test can pin two assertions to the same wall and exercise
/// the recency-first sub-tiebreaks. Returns the raw submit result.
async fn submit_field(
    c: &Client, sk: &SigningKey, kid: &str, patient: Uuid, wall: i64, counter: i64,
    payload: serde_json::Value, twin: &str,
) -> Result<u64, tokio_postgres::Error> {
    let body = EventBody {
        event_id: Uuid::now_v7().to_string(),
        patient_id: patient.to_string(),
        event_type: "demographic.field.asserted".into(),
        schema_version: "demographic.field/1".into(),
        hlc: Hlc { wall, counter, node_origin: "n".into() },
        t_effective: None,
        signer_key_id: kid.into(),
        contributors: serde_json::json!([{"actor_id": kid, "role": "recorded"}]),
        payload,
        attachments: vec![],
        plaintext_twin: Some(twin.to_string()),
    };
    let signed = sign(&body, sk).unwrap();
    c.execute("SELECT submit_event($1)", &[&signed.signed_bytes]).await
}

async fn winner(c: &Client, patient: &Uuid, field: &str) -> String {
    let p = patient.to_string();
    c.query_one(
        "SELECT value FROM patient_demographic WHERE patient_id::text=$1 AND field=$2",
        &[&p, &field]).await.unwrap().get(0)
}

#[tokio::test]
async fn administrative_sex_provenance_locks_then_recency_among_equals() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return; };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let p = Uuid::now_v7();

    // document-verified marker first.
    submit_field(&c, &sk, &kid, p, 1, 0,
        administrative_sex_assertion_body("M", "document-verified"),
        &render_administrative_sex_twin("M", "document-verified")).await.unwrap();
    // a LATER patient-stated claim must NOT displace it (provenance-first lock).
    submit_field(&c, &sk, &kid, p, 2, 0,
        administrative_sex_assertion_body("F", "patient-stated"),
        &render_administrative_sex_twin("F", "patient-stated")).await.unwrap();
    assert_eq!(winner(&c, &p, "administrative-sex").await, "M",
        "lower-provenance later claim must not displace a document-verified marker");

    // a later EQUAL-provenance (document-verified) marker DOES win on recency.
    submit_field(&c, &sk, &kid, p, 3, 0,
        administrative_sex_assertion_body("F", "document-verified"),
        &render_administrative_sex_twin("F", "document-verified")).await.unwrap();
    assert_eq!(winner(&c, &p, "administrative-sex").await, "F",
        "a newer equal-provenance marker wins (recency-among-equals)");
}

#[tokio::test]
async fn gender_identity_recency_wins_regardless_of_provenance() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return; };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let p = Uuid::now_v7();

    // a high-provenance value first.
    submit_field(&c, &sk, &kid, p, 1, 0,
        gender_identity_assertion_body("man", "document-verified"),
        &render_gender_identity_twin("man", "document-verified")).await.unwrap();
    // a LATER but LOWER-provenance assertion still wins — recency leads (inverse of dob).
    submit_field(&c, &sk, &kid, p, 2, 0,
        gender_identity_assertion_body("non-binary", "clinician-observed"),
        &render_gender_identity_twin("non-binary", "clinician-observed")).await.unwrap();
    assert_eq!(winner(&c, &p, "gender-identity").await, "non-binary",
        "newest gender-identity wins regardless of provenance");
}

#[tokio::test]
async fn gender_identity_equal_hlc_breaks_on_provenance() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return; };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let p = Uuid::now_v7();

    // Same (wall, counter): the recency-first tuple falls through to provenance_rank.
    submit_field(&c, &sk, &kid, p, 7, 0,
        gender_identity_assertion_body("A", "patient-stated"),
        &render_gender_identity_twin("A", "patient-stated")).await.unwrap();
    submit_field(&c, &sk, &kid, p, 7, 0,
        gender_identity_assertion_body("B", "document-verified"),
        &render_gender_identity_twin("B", "document-verified")).await.unwrap();
    assert_eq!(winner(&c, &p, "gender-identity").await, "B",
        "equal HLC: higher provenance breaks the recency-first tie (convergence)");
}

#[tokio::test]
async fn unknown_field_is_carried_but_not_projected() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return; };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let p = Uuid::now_v7();

    // A field this node has no policy for: passes the floor, lands in event_log, but
    // is NOT projected (the ADR-0012 federation-forward degrade is intact).
    let body = cairn_event::demographics::demographic_field_body(
        "gender-marker-v2", "x", None, "patient-stated");
    submit_field(&c, &sk, &kid, p, 1, 0, body, "Gender marker v2 (patient-stated): x")
        .await.expect("unknown field passes the generic floor");

    let p_str = p.to_string();
    let in_log: i64 = c.query_one(
        "SELECT count(*) FROM event_log WHERE patient_id::text=$1 AND body->>'field'='gender-marker-v2'",
        &[&p_str]).await.unwrap().get(0);
    assert_eq!(in_log, 1, "unknown field is carried in event_log");
    let projected: i64 = c.query_one(
        "SELECT count(*) FROM patient_demographic WHERE patient_id::text=$1 AND field='gender-marker-v2'",
        &[&p_str]).await.unwrap().get(0);
    assert_eq!(projected, 0, "unknown field is not projected");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" cargo test -p cairn-node --test demographics_sex_gender`
Expected: COMPILE (Task-1 builders exist) then FAIL — `administrative_sex`/`gender_identity` events currently hit the slice-2 gate `IN ('dob','sex-at-birth')` and never project, so `winner()` raises "no rows" (`query_one` panics). The `unknown_field` test would pass already, but the two projection tests fail because nothing is projected.

- [ ] **Step 3: Write the db/013 migration**

Create `db/013_demographics_sex_gender.sql`:

```sql
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
```

- [ ] **Step 4: Register db/013 in the schema array**

In `crates/cairn-node/src/db.rs`, bump the SCHEMA array size and add the entry after the `012_demographics_names` line:

```rust
const SCHEMA: [(&str, &str); 12] = [
```

```rust
    ("012_demographics_names",  include_str!("../../../db/012_demographics_names.sql")),
    ("013_demographics_sex_gender", include_str!("../../../db/013_demographics_sex_gender.sql")),
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" cargo test -p cairn-node --test demographics_sex_gender`
Expected: all 4 PASS.

- [ ] **Step 6: Run the full demographics regression + clippy**

Run:
```bash
CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" cargo test -p cairn-node --test demographics --test demographics_fields --test demographics_names --test demographics_sex_gender
cargo clippy -p cairn-node --all-targets -- -D warnings
```
Expected: slices 1–4 all green; clippy clean.

- [ ] **Step 7: Commit**

```bash
git add db/013_demographics_sex_gender.sql crates/cairn-node/src/db.rs crates/cairn-node/tests/demographics_sex_gender.rs
git commit -m "$(cat <<'EOF'
feat(db): §4.2 per-field winner policy — administrative-sex + gender-identity

cairn_demographic_field_policy classifier drives both the projection gate and
the winner ordering; administrative-sex provenance-first, gender-identity
recency-first (inverse of slice-2). Supersedes db/011's patient_demographic_apply.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: ADR-0037 + spec §4.2 / index.md

**Files:**
- Create: `docs/spec/decisions/0037-demographic-administrative-sex-and-per-field-winner-policy.md`
- Modify: `docs/spec/demographics.md` (§4.2 table row for Sex/gender — fill the administrative-sex rule; add a karyotype note)
- Modify: `docs/spec/index.md` (spec version 0.37 → 0.38; add ADR-0037 row to the ADR table)

**Interfaces:** none (docs only). Follow the existing ADR file format (read `0036-demographic-name-display-recency-first.md` first for the exact heading/section style).

- [ ] **Step 1: Write ADR-0037**

Read `docs/spec/decisions/0036-demographic-name-display-recency-first.md` to match house ADR style, then create `docs/spec/decisions/0037-demographic-administrative-sex-and-per-field-winner-policy.md` capturing, in the project's ADR format (Status: Accepted, dated 2026-06-28; Context / Decision / Consequences):

- **Context:** §4.2 names administrative-sex but gives no projection rule; gender-identity is "recency wins"; slice-2 deferred whether a karyotype displaces sex-at-birth.
- **Decision (three parts):**
  1. **administrative-sex = provenance-first** (like dob/sex-at-birth). Rationale: the administrative marker is document-anchored; an unverified self-claim must not displace a document-verified marker; recency still wins among equal provenance (a new legal document flips it). The dignity/recency surface is carried by **gender-identity** (recency-first, patient-authoritative) — the two are deliberately split.
  2. **Per-field winner-policy selector** — a single IMMUTABLE `cairn_demographic_field_policy(field)` returning `provenance-first` / `recency-first` / `NULL`, the source of truth for both the projection gate (NULL ⇒ carried-not-projected, the ADR-0012 degrade) and the winner ordering. Generalises slice-2's hard-coded ordering; future volatile fields (phone) plug in as recency-first.
  3. **Karyotype = distinct field.** sex-at-birth = the sex assigned/observed at birth; a karyotype (chromosomal sex) is a different fact with its own future field, never asserted as a sex-at-birth value (avoids conflating assigned vs chromosomal sex — the AIS/Swyer case). `fact-proven` stays in the ladder for same-field lab confirmation; the projection's fact-proven-displaces-sex-at-birth path stays mechanically present but unexercised by well-formed input — a modeling convention (UI soft-policy), not a floor gate (principle 12).
- **Consequences:** additive (no event type / floor / table-schema change); recency-first is the first non-provenance ordering; refines ADR-0036 (recency precedent) and ADR-0014.

- [ ] **Step 2: Fill the §4.2 table + add the karyotype note**

In `docs/spec/demographics.md`, replace the Sex/gender table row's projection-rule cell (line 17) to state the administrative-sex rule explicitly, and add a karyotype note after the existing names notes (after line 30):

Table cell (Projection rule column) becomes:
```
Sex-at-birth provenance-locked; **administrative sex provenance-first** (document-anchored marker; recency wins among equal provenance) ([ADR-0037](decisions/0037-demographic-administrative-sex-and-per-field-winner-policy.md)); gender identity patient-stated authoritative, recency wins
```

New note:
```
- Sex-at-birth is the sex **assigned/observed at birth**; a **karyotype** (chromosomal
  sex) is a distinct fact with its own field, never asserted as a sex-at-birth value
  (the AIS/Swyer case records two facts, not one overwriting the other)
  ([ADR-0037](decisions/0037-demographic-administrative-sex-and-per-field-winner-policy.md)).
```

- [ ] **Step 3: Bump spec version + add the ADR row**

In `docs/spec/index.md`: change the spec version `0.37` → `0.38`, and add to the ADR index table:
```
| [0037](decisions/0037-demographic-administrative-sex-and-per-field-winner-policy.md) | Administrative-sex provenance-first + per-field winner-policy selector; karyotype = distinct field | §4.2 (refines 0036/0014) |
```

- [ ] **Step 4: Build the docs to verify no broken links/warnings**

Run: `uv run --with mkdocs-material --with mkdocs-callouts --with mkdocs-redirects -- mkdocs build`
Expected: build succeeds; no warning naming `0037` or `demographics.md` (a broken cross-ref would warn).

- [ ] **Step 5: Commit**

```bash
git add docs/spec/decisions/0037-demographic-administrative-sex-and-per-field-winner-policy.md docs/spec/demographics.md docs/spec/index.md
git commit -m "$(cat <<'EOF'
spec(demographics): §4.2 administrative-sex provenance-first + per-field winner policy (ADR-0037)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: HANDOVER / ROADMAP currency + final verification

**Files:**
- Modify: `docs/HANDOVER.md` (new top "This session" block; demographics open-thread updated — admin-sex + gender-identity done, deferred karyotype now resolved; add ADR-0037 row)
- Modify: `docs/ROADMAP.md` (if it tracks demographics slices — update the demographics line)

- [ ] **Step 1: Run the whole workspace suite**

Run:
```bash
CAIRN_TEST_PG="host=127.0.0.1 port=5532 user=hherb dbname=cairn_test" cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
Expected: all green; clippy clean. (Record the cairn-event + cairn-node demographics test counts for the HANDOVER summary.)

- [ ] **Step 2: Update HANDOVER.md**

Add a new top "This session (2026-06-28)" block summarising slice 4 (administrative-sex provenance-first + gender-identity recency-first via the `cairn_demographic_field_policy` selector; karyotype resolved as a distinct field; ADR-0037; spec 0.37 → 0.38; test counts). Move the slice-3 block down. In the demographics open-thread menu, strike admin-sex + gender-identity from "remaining" and remove the now-resolved karyotype deferred-decision note (point to ADR-0037). Add the ADR-0037 row to the index table. Prune to keep the file concise (< 500 lines).

- [ ] **Step 3: Update ROADMAP.md if applicable**

If `docs/ROADMAP.md` carries a demographics slice tracker, update it to reflect slice 4 done and the remaining slices (§4.3 address, §5.2 matcher, twin globalisation). Otherwise no change.

- [ ] **Step 4: Commit**

```bash
git add docs/HANDOVER.md docs/ROADMAP.md
git commit -m "$(cat <<'EOF'
docs: HANDOVER/ROADMAP currency — demographics slice 4 (sex/gender) landed

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 5: Push + open PR**

```bash
git push -u origin demographics-sex-gender
gh pr create --base main --title "Demographics slice 4 — administrative-sex + gender-identity (per-field winner policy)" --body "$(cat <<'EOF'
## Summary
Slice 4 of the demographics subsystem: the two remaining §4.2 sex/gender fields, on the existing `demographic.field.asserted` spine.

- **administrative-sex** → provenance-first (document-anchored marker; recency wins among equal provenance).
- **gender-identity** → recency-first (newest wins regardless of provenance — the inverse of slice-2's ordering; patient's current stated identity always displays).
- **Mechanism:** one IMMUTABLE `cairn_demographic_field_policy(field)` classifier drives both the projection gate and the winner ordering. Additive: no new event type, no floor change, no `patient_demographic` schema change; `db/013` supersedes db/011's trigger.
- **Karyotype resolved** (slice-2 deferral): a distinct field, never displaces sex-at-birth (= assigned at birth). Spec/ADR only, no karyotype code.

New **ADR-0037**; spec 0.37 → 0.38. TDD throughout (4 cairn-event unit + 4 cairn-node integration; slices 1–3 regress green; clippy clean).

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

---

## Self-Review

**Spec coverage:** administrative-sex provenance-first (Task 2 db/013 + Task 2 test 1); gender-identity recency-first (Task 2 db/013 + tests 2/3); per-field policy selector (Task 2); carried-not-projected degrade (Task 2 test 4); builders + twins (Task 1); karyotype distinct-field resolution (Task 3 ADR + §4.2 note); §4.2 table fill + version bump (Task 3); HANDOVER/ROADMAP currency (Task 4). All spec sections covered.

**Placeholder scan:** all code blocks are complete (full SQL, full Rust, full test bodies). The only prose-described steps are docs (ADR-0037 / HANDOVER), where the exact content is enumerated as bullet points to write — appropriate for narrative docs, not code.

**Type consistency:** `administrative_sex_assertion_body` / `gender_identity_assertion_body` / `render_administrative_sex_twin` / `render_gender_identity_twin` are named identically in Task 1 (definition) and Task 2 (test imports). `cairn_demographic_field_policy(p_field text) -> text` and the `patient_demographic` column names (`asserted_hlc_wall`/`asserted_hlc_count`/`asserted_origin`/`provenance_rank`) match db/011 exactly. The `submit_field` helper signature (with explicit `counter`) is self-consistent within Task 2.
