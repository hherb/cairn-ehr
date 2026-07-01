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
