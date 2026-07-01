//! Integration coverage for the §5.1/§5.7 identity linkage core (db/018): the
//! link/unlink event types, the structural floor, the patient_link edge overlay,
//! the person_member connected-component projection, and the person_chart
//! unified-read VIEW. Real Postgres, gated on `$CAIRN_TEST_PG`, serialized
//! cluster-wide via `db::test_serial_guard`. The advisory matcher (§5.2, Python)
//! and the proposal→apply seam (C2) are separate subsystems, NOT exercised here.
use cairn_event::identity::{
    link_assertion_body, render_link_twin, render_unlink_twin, unlink_assertion_body, LinkAssertion,
};
use cairn_event::{generate_key, sign, EventBody, Hlc, SigningKey};
use cairn_node::db;
use tokio_postgres::Client;
use uuid::Uuid;

fn cs() -> Option<String> { std::env::var("CAIRN_TEST_PG").ok() }

/// The Postgres error message text for a failed statement. `tokio_postgres::Error`'s
/// Display renders only as the literal "db error" — the actual `RAISE EXCEPTION`
/// message lives in the DbError payload — so every floor-rejection assertion must go
/// through here (project convention: see `twin_globalise.rs`, `admission.rs`).
fn db_msg(e: &tokio_postgres::Error) -> String {
    e.as_db_error().map(|d| d.message().to_string()).unwrap_or_else(|| e.to_string())
}

/// Truncate the clinical + linkage tables and enroll one agent signer. Returns (sk, kid).
/// `patient_link` / `person_member` are created by LATER sections of db/018 (Tasks 3–4),
/// so they are truncated behind a `to_regclass` guard — this keeps the single `setup()`
/// helper correct at every stage as the migration grows, with no cross-task edits.
async fn setup(c: &Client) -> (SigningKey, String) {
    c.batch_execute(
        "TRUNCATE event_log, actor_event, patient_chart, patient_identifier, \
         patient_demographic CASCADE")
        .await.unwrap();
    c.batch_execute(
        "DO $$ BEGIN \
           IF to_regclass('public.patient_link')  IS NOT NULL THEN TRUNCATE patient_link;  END IF; \
           IF to_regclass('public.person_member') IS NOT NULL THEN TRUNCATE person_member; END IF; \
         END $$;")
        .await.unwrap();
    let (sk, kid) = generate_key().unwrap();
    c.execute(
        "SELECT enroll_actor('agent', '{\"model\":\"reg-stub\",\"version\":\"1\",\"skill_epoch\":\"e\"}', $1)",
        &[&kid],
    ).await.unwrap();
    (sk, kid)
}

/// Sign + submit one link OR unlink event through the real submit_event door.
/// `wall` is the HLC wall clock (higher = newer); patient_id is set to subject_a
/// by convention (an identity event is "about" subject_a's linkage). Returns the
/// submit result so a test can assert acceptance or a specific rejection.
async fn submit_link(
    c: &Client, sk: &SigningKey, kid: &str, a: Uuid, b: Uuid, wall: i64, is_link: bool,
) -> Result<u64, tokio_postgres::Error> {
    submit_link_prov(c, sk, kid, a, b, wall, is_link, "matcher:cfg@test").await
}

/// As `submit_link`, but with an explicit provenance string (for the empty-provenance
/// rejection test). Sends the provenance verbatim; an empty string must be rejected.
async fn submit_link_prov(
    c: &Client, sk: &SigningKey, kid: &str, a: Uuid, b: Uuid, wall: i64, is_link: bool, prov: &str,
) -> Result<u64, tokio_postgres::Error> {
    let a_s = a.to_string();
    let b_s = b.to_string();
    let la = LinkAssertion { subject_a: &a_s, subject_b: &b_s, provenance: prov, confidence: None };
    let (etype, sver, payload, twin) = if is_link {
        ("identity.link.asserted", "identity.link/1", link_assertion_body(&la), render_link_twin(&la))
    } else {
        ("identity.unlink.asserted", "identity.unlink/1", unlink_assertion_body(&la), render_unlink_twin(&la))
    };
    let body = EventBody {
        event_id: Uuid::now_v7().to_string(),
        patient_id: a_s.clone(),
        event_type: etype.into(),
        schema_version: sver.into(),
        hlc: Hlc { wall, counter: 0, node_origin: "n".into() },
        t_effective: None,
        signer_key_id: kid.into(),
        contributors: serde_json::json!([{"actor_id": kid, "role": "recorded"}]),
        payload,
        attachments: vec![],
        plaintext_twin: Some(twin),
    };
    let signed = sign(&body, sk).unwrap();
    c.execute("SELECT submit_event($1)", &[&signed.signed_bytes]).await
}

#[tokio::test]
async fn valid_link_is_accepted() {
    let Some(base) = cs() else { return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
    submit_link(&c, &sk, &kid, a, b, 100, true).await.expect("valid link accepted");
}

#[tokio::test]
async fn self_link_is_rejected() {
    let Some(base) = cs() else { return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let a = Uuid::now_v7();
    let err = submit_link(&c, &sk, &kid, a, a, 100, true).await.unwrap_err();
    assert!(db_msg(&err).contains("self-link"), "self-link must be refused: {}", db_msg(&err));
}

#[tokio::test]
async fn empty_provenance_is_rejected() {
    let Some(base) = cs() else { return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
    let err = submit_link_prov(&c, &sk, &kid, a, b, 100, true, "   ").await.unwrap_err();
    assert!(db_msg(&err).contains("provenance"), "empty provenance must be refused: {}", db_msg(&err));
}

/// Read the standing edge state for a pair, or None if no edge row exists.
/// Uses `$1::text::uuid` (project convention, see `match_veto.rs`) since
/// tokio-postgres in this project has no uuid `ToSql` feature enabled.
async fn edge_state(c: &Client, a: Uuid, b: Uuid) -> Option<String> {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    let (lo_s, hi_s) = (lo.to_string(), hi.to_string());
    let row = c.query_opt(
        "SELECT state FROM patient_link WHERE low = $1::text::uuid AND high = $2::text::uuid",
        &[&lo_s, &hi_s],
    ).await.unwrap();
    row.map(|r| r.get::<_, String>(0))
}

#[tokio::test]
async fn link_creates_a_standing_edge() {
    let Some(base) = cs() else { return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
    submit_link(&c, &sk, &kid, a, b, 100, true).await.unwrap();
    assert_eq!(edge_state(&c, a, b).await.as_deref(), Some("link"));
}

#[tokio::test]
async fn newer_unlink_overlays_older_link() {
    let Some(base) = cs() else { return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
    submit_link(&c, &sk, &kid, a, b, 100, true).await.unwrap();   // link @100
    submit_link(&c, &sk, &kid, a, b, 200, false).await.unwrap();  // unlink @200 (newer)
    assert_eq!(edge_state(&c, a, b).await.as_deref(), Some("unlink"));
}

#[tokio::test]
async fn older_link_does_not_overlay_newer_unlink() {
    // Out-of-order arrival must converge: the highest-HLC assertion wins regardless
    // of the order events land (offline-first set-union).
    let Some(base) = cs() else { return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
    submit_link(&c, &sk, &kid, a, b, 200, false).await.unwrap();  // unlink @200 lands first
    submit_link(&c, &sk, &kid, a, b, 100, true).await.unwrap();   // link @100 lands later (older)
    assert_eq!(edge_state(&c, a, b).await.as_deref(), Some("unlink"),
               "older link must not overlay a newer unlink");
}

#[tokio::test]
async fn missing_twin_is_rejected() {
    let Some(base) = cs() else { return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let c = db::connect_and_load_schema(&base).await.unwrap();
    let (sk, kid) = setup(&c).await;
    let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
    // Build a link event with NO authored twin — the identity floor HARD-requires one.
    let (a_s, b_s) = (a.to_string(), b.to_string());
    let la = LinkAssertion { subject_a: &a_s, subject_b: &b_s, provenance: "p", confidence: None };
    let body = EventBody {
        event_id: Uuid::now_v7().to_string(),
        patient_id: a_s.clone(),
        event_type: "identity.link.asserted".into(),
        schema_version: "identity.link/1".into(),
        hlc: Hlc { wall: 100, counter: 0, node_origin: "n".into() },
        t_effective: None,
        signer_key_id: kid.clone(),
        contributors: serde_json::json!([{"actor_id": kid, "role": "recorded"}]),
        payload: link_assertion_body(&la),
        attachments: vec![],
        plaintext_twin: None,
    };
    let signed = sign(&body, &sk).unwrap();
    let err = c.execute("SELECT submit_event($1)", &[&signed.signed_bytes]).await.unwrap_err();
    assert!(db_msg(&err).contains("authored twin"), "twin-less identity event must be refused: {}", db_msg(&err));
}
