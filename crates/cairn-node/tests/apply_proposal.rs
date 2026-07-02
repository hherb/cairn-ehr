//! Integration coverage for the §5.2/§5.7 C2 apply seam: a human-accepted
//! match_proposal becomes a human-attested identity.link.asserted event through
//! the existing submit_event door, projecting the link into patient_link /
//! person_member. Real Postgres, gated on $CAIRN_TEST_PG, serialized cluster-wide
//! via db::test_serial_guard. No submit_event change is exercised here — C2 only
//! composes the C1 identity floor (db/018) and the ADR-0030 attestation gate (db/005).
use cairn_event::{generate_key, Hlc, SigningKey};
use cairn_node::apply_proposal::apply_accepted_proposal;
use cairn_node::db;
use tokio_postgres::Client;
use uuid::Uuid;

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
/// evidence are JSONB NOT NULL, so pass empty arrays. UUIDs are passed as text and cast
/// in SQL (`$N::text::uuid`) — this project's tokio-postgres has no uuid ToSql/FromSql
/// (project convention: see `tests/identity_linkage.rs::person_of`).
async fn seed_accepted_proposal(c: &Client, low: Uuid, high: Uuid, status: &str) {
    let (low_s, high_s) = (low.to_string(), high.to_string());
    c.execute(
        "INSERT INTO match_proposal \
           (patient_low, patient_high, score_total, band, veto_findings, evidence, matcher_version, status) \
         VALUES ($1::text::uuid,$2::text::uuid, 0.91, 'review', '[]'::jsonb, '[]'::jsonb, 'cfg@test', $3)",
        &[&low_s, &high_s, &status.to_string()],
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
    let (low_s, high_s) = (low.to_string(), high.to_string());
    let n_edge: i64 = c.query_one(
        "SELECT count(*) FROM patient_link WHERE low=$1::text::uuid AND high=$2::text::uuid AND state='link'",
        &[&low_s, &high_s]).await.unwrap().get(0);
    assert_eq!(n_edge, 1, "patient_link edge present");

    // Both patients project to the same (min-UUID) person_id.
    let person_low: Uuid = c.query_one(
        "SELECT person_id::text FROM person_member WHERE patient_id=$1::text::uuid", &[&low_s])
        .await.unwrap().get::<_, String>(0).parse().unwrap();
    let person_high: Uuid = c.query_one(
        "SELECT person_id::text FROM person_member WHERE patient_id=$1::text::uuid", &[&high_s])
        .await.unwrap().get::<_, String>(0).parse().unwrap();
    assert_eq!(person_low, person_high, "both members share one person_id");
    assert_eq!(person_low, low, "person_id is the min-UUID representative");

    // The proposal was marked applied, pointing at the link event.
    let (status, applied): (String, Option<String>) = {
        let row = c.query_one(
            "SELECT status, applied_event_id::text FROM match_proposal WHERE patient_low=$1::text::uuid AND patient_high=$2::text::uuid",
            &[&low_s, &high_s]).await.unwrap();
        (row.get(0), row.get(1))
    };
    assert_eq!(status, "applied");
    let applied: Option<Uuid> = applied.map(|s| s.parse().unwrap());
    assert_eq!(applied, Some(eid), "applied_event_id points at the emitted link event");
}
