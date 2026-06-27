//! Integration coverage for the §4.4 demographic identifier assertion: the in-DB
//! floor + the set-union patient_identifier projection. Real Postgres, gated on
//! $CAIRN_TEST_PG, serialized cluster-wide via db::test_serial_guard (the shared-DB
//! + TRUNCATE pattern, identical to attestation.rs). Matching/veto is a separate
//! subsystem and is NOT exercised here.
use cairn_event::demographics::{identifier_assertion_body, render_identifier_twin, IdentifierAssertion};
use cairn_event::{generate_key, sign, EventBody, Hlc, SigningKey};
use cairn_node::db;
use tokio_postgres::Client;
use uuid::Uuid;

fn cs() -> Option<String> { std::env::var("CAIRN_TEST_PG").ok() }

/// Truncate the clinical tables and enroll one agent signer. Returns (sk, kid).
async fn setup(c: &Client) -> (SigningKey, String) {
    c.batch_execute("TRUNCATE event_log, actor_event, patient_chart, patient_identifier CASCADE")
        .await.unwrap();
    let (sk, kid) = generate_key().unwrap();
    c.execute(
        "SELECT enroll_actor('agent', '{\"model\":\"reg-stub\",\"version\":\"1\",\"skill_epoch\":\"e\"}', $1)",
        &[&kid],
    ).await.unwrap();
    (sk, kid)
}

/// Author + sign + submit one §4.4 identifier assertion for `patient`. Returns the
/// raw submit result so rejection tests (Task 4) can assert the error.
async fn assert_identifier(
    c: &Client, sk: &SigningKey, kid: &str, patient: Uuid, wall: i64,
    a: &IdentifierAssertion<'_>,
) -> Result<u64, tokio_postgres::Error> {
    let body = EventBody {
        event_id: Uuid::now_v7().to_string(),
        patient_id: patient.to_string(),
        event_type: "demographic.identifier.asserted".into(),
        schema_version: "demographic.identifier/1".into(),
        hlc: Hlc { wall, counter: 0, node_origin: "n".into() },
        t_effective: None,
        signer_key_id: kid.into(),
        contributors: serde_json::json!([{"actor_id": kid, "role": "recorded"}]),
        payload: identifier_assertion_body(a),
        attachments: vec![],
        plaintext_twin: Some(render_identifier_twin(a)),
    };
    let signed = sign(&body, sk).unwrap();
    c.execute("SELECT submit_event($1)", &[&signed.signed_bytes]).await
}

#[tokio::test]
async fn happy_path_appends_and_projects_with_authored_twin() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return; };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let p = Uuid::now_v7();
    let a = IdentifierAssertion {
        value: "943 476 5919", system: "nhs-number", provenance: "document-verified",
        normalized: Some("9434765919"), profile: Some("nhs-number@b3-abc"),
        use_: Some("national-id"),
    };
    assert_identifier(&c, &sk, &kid, p, 1, &a).await.expect("valid assertion accepted");

    // Projection: one row, keyed on the normalized match_key. Compare UUID columns
    // as text (cast the column) since tokio-postgres has no uuid ToSql in this project.
    let p_str = p.to_string();
    let row = c.query_one(
        "SELECT match_key, value, profile, provenance, plaintext_twin
           FROM patient_identifier pi JOIN event_log el ON el.patient_id = pi.patient_id
          WHERE pi.patient_id::text = $1", &[&p_str]).await.unwrap();
    let match_key: String = row.get(0);
    let value: String = row.get(1);
    let twin: String = row.get(4);
    assert_eq!(match_key, "9434765919");
    assert_eq!(value, "943 476 5919");
    assert_eq!(twin, "nhs-number, document-verified: 943 476 5919",
               "the AUTHORED twin is stored (cairn_body passed the top-level field through)");
}

#[tokio::test]
async fn set_union_dedups_same_key_keeps_different_key() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return; };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let p = Uuid::now_v7();
    let same = IdentifierAssertion {
        value: "943 476 5919", system: "nhs-number", provenance: "patient-stated",
        normalized: Some("9434765919"), profile: Some("nhs-number@b3-abc"), use_: None,
    };
    let same_formatted = IdentifierAssertion { value: "9434765919", ..same_clone(&same) };
    let other = IdentifierAssertion {
        value: "111 222 3334", system: "nhs-number", provenance: "patient-stated",
        normalized: Some("1112223334"), profile: Some("nhs-number@b3-abc"), use_: None,
    };
    assert_identifier(&c, &sk, &kid, p, 1, &same).await.unwrap();
    assert_identifier(&c, &sk, &kid, p, 2, &same_formatted).await.unwrap(); // same normalized → dedup
    assert_identifier(&c, &sk, &kid, p, 3, &other).await.unwrap();          // different normalized → 2nd row
    let p_str = p.to_string();
    let n: i64 = c.query_one(
        "SELECT count(*) FROM patient_identifier WHERE patient_id::text=$1 AND system='nhs-number'",
        &[&p_str]).await.unwrap().get(0);
    assert_eq!(n, 2, "same-normalized dedups; different-normalized keeps both");
}

// Helper: clone an IdentifierAssertion's borrowed fields (test-only convenience).
fn same_clone<'a>(a: &IdentifierAssertion<'a>) -> IdentifierAssertion<'a> {
    IdentifierAssertion {
        value: a.value, system: a.system, provenance: a.provenance,
        normalized: a.normalized, profile: a.profile, use_: a.use_,
    }
}

#[tokio::test]
async fn honest_degradation_no_normalized_no_profile_accepted() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return; };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let p = Uuid::now_v7();
    let a = IdentifierAssertion {
        value: "OLD-CARD-77", system: "unknown", provenance: "imported",
        normalized: None, profile: None, use_: None,
    };
    assert_identifier(&c, &sk, &kid, p, 1, &a).await.expect("profile-less assertion accepted");
    let p_str = p.to_string();
    let mk: String = c.query_one(
        "SELECT match_key FROM patient_identifier WHERE patient_id::text=$1", &[&p_str])
        .await.unwrap().get(0);
    assert_eq!(mk, "OLD-CARD-77", "match_key falls back to value when normalized absent");
}
