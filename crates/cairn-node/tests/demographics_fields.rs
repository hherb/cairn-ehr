//! Integration coverage for the §4.2 provenance-precedence fields (DOB +
//! sex-at-birth): the in-DB structural floor + the winner-by-(rank,HLC)
//! patient_demographic projection. Real Postgres, gated on `$CAIRN_TEST_PG`,
//! serialized cluster-wide via `db::test_serial_guard`. Matching (§5.2) is a
//! separate subsystem and is NOT exercised here.
use cairn_event::demographics::{
    dob_assertion_body, render_dob_twin, render_sex_at_birth_twin, sex_at_birth_assertion_body,
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

/// Author + sign + submit one demographic.field.asserted event. `payload` is the
/// already-built body (from a Task-1 builder or a raw json! for rejection tests);
/// `twin` is the authored §4.5 twin. Returns the raw submit result.
async fn submit_field(
    c: &Client, sk: &SigningKey, kid: &str, patient: Uuid, wall: i64,
    payload: serde_json::Value, twin: Option<&str>,
) -> Result<u64, tokio_postgres::Error> {
    let body = EventBody {
        event_id: Uuid::now_v7().to_string(),
        patient_id: patient.to_string(),
        event_type: "demographic.field.asserted".into(),
        schema_version: "demographic.field/1".into(),
        hlc: Hlc { wall, counter: 0, node_origin: "n".into() },
        t_effective: None,
        signer_key_id: kid.into(),
        contributors: serde_json::json!([{"actor_id": kid, "role": "recorded"}]),
        payload,
        attachments: vec![],
        plaintext_twin: twin.map(|t| t.to_string()),
    };
    let signed = sign(&body, sk).unwrap();
    c.execute("SELECT submit_event($1)", &[&signed.signed_bytes]).await
}

#[tokio::test]
async fn happy_path_projects_dob_and_sex_with_rank_and_facets() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return; };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let p = Uuid::now_v7();

    submit_field(&c, &sk, &kid, p, 1,
        dob_assertion_body("1980-07-15", "day", Some("document"), "document-verified"),
        Some(&render_dob_twin("1980-07-15", "day", "document-verified"))
    ).await.expect("valid dob accepted");
    submit_field(&c, &sk, &kid, p, 2,
        sex_at_birth_assertion_body("female", "clinician-observed"),
        Some(&render_sex_at_birth_twin("female", "clinician-observed"))
    ).await.expect("valid sex-at-birth accepted");

    let p_str = p.to_string();
    // DOB row: value, cached provenance_rank (document-verified -> 60), facets.precision.
    let row = c.query_one(
        "SELECT value, provenance_rank, facets->>'precision' \
         FROM patient_demographic WHERE patient_id::text=$1 AND field='dob'",
        &[&p_str]).await.unwrap();
    let value: String = row.get(0);
    let rank: i32 = row.get(1);
    let precision: String = row.get(2);
    assert_eq!(value, "1980-07-15");
    assert_eq!(rank, 60);
    assert_eq!(precision, "day");
    // sex-at-birth row exists with the right value.
    let sex: String = c.query_one(
        "SELECT value FROM patient_demographic WHERE patient_id::text=$1 AND field='sex-at-birth'",
        &[&p_str]).await.unwrap().get(0);
    assert_eq!(sex, "female");
    // The AUTHORED dob twin was carried verbatim (cairn_event_twin demographic branch).
    let twin: String = c.query_one(
        "SELECT plaintext_twin FROM event_log WHERE patient_id::text=$1 AND hlc_wall=1",
        &[&p_str]).await.unwrap().get(0);
    assert_eq!(twin, "Date of birth (document-verified): 1980-07-15 (day)");
}

/// Helper: read the current projected dob value for a patient.
async fn dob_value(c: &Client, p: Uuid) -> String {
    let p_str = p.to_string();
    c.query_one(
        "SELECT value FROM patient_demographic WHERE patient_id::text=$1 AND field='dob'",
        &[&p_str]).await.unwrap().get(0)
}

#[tokio::test]
async fn provenance_beats_recency_and_verified_locks() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return; };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let p = Uuid::now_v7();

    // 1) An early patient-stated dob (rank 50).
    submit_field(&c, &sk, &kid, p, 1,
        dob_assertion_body("1979", "year", None, "patient-stated"),
        Some(&render_dob_twin("1979", "year", "patient-stated"))).await.unwrap();
    assert_eq!(dob_value(&c, p).await, "1979");

    // 2) A later document-verified dob (rank 60) — higher provenance wins.
    submit_field(&c, &sk, &kid, p, 2,
        dob_assertion_body("1980-07-15", "day", Some("document"), "document-verified"),
        Some(&render_dob_twin("1980-07-15", "day", "document-verified"))).await.unwrap();
    assert_eq!(dob_value(&c, p).await, "1980-07-15", "higher provenance wins");

    // 3) An EVEN LATER patient-stated dob (rank 50) — must NOT displace the verified
    //    value. "Verified value locks vs. lower provenance."
    submit_field(&c, &sk, &kid, p, 3,
        dob_assertion_body("1981", "year", None, "patient-stated"),
        Some(&render_dob_twin("1981", "year", "patient-stated"))).await.unwrap();
    assert_eq!(dob_value(&c, p).await, "1980-07-15", "verified value locks vs lower provenance");
}

#[tokio::test]
async fn recency_breaks_ties_among_equal_provenance() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return; };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let p = Uuid::now_v7();

    // Two document-verified dobs (equal rank) — the HLC-later one wins.
    submit_field(&c, &sk, &kid, p, 1,
        dob_assertion_body("1980-01-01", "day", Some("document"), "document-verified"),
        Some(&render_dob_twin("1980-01-01", "day", "document-verified"))).await.unwrap();
    // Confirm the FIRST assertion actually projected. Without this, a silent failure of
    // the first submission would leave a one-row table and let the final assertion pass
    // for the wrong reason — never exercising the equal-provenance conflict-resolution path.
    assert_eq!(dob_value(&c, p).await, "1980-01-01", "first equal-provenance assertion projected");
    submit_field(&c, &sk, &kid, p, 2,
        dob_assertion_body("1980-07-15", "day", Some("document"), "document-verified"),
        Some(&render_dob_twin("1980-07-15", "day", "document-verified"))).await.unwrap();
    // hlc_wall=2 > hlc_wall=1 → "1980-07-15" wins among equal provenance.
    assert_eq!(dob_value(&c, p).await, "1980-07-15", "later HLC wins among equal provenance");
}
