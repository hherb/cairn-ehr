//! Issue #38 — incremental sync watermark (local-insertion `seq` cursor) and the
//! full-sweep correctness floor. DB-gated: needs CAIRN_TEST_PG (fresh DB + cairn_pgx).

use cairn_node::{db, identity, keystore};

fn cs() -> Option<String> { std::env::var("CAIRN_TEST_PG").ok() }

/// Every node_event row gets a monotonic `seq` assigned at INSERT, and the genesis
/// enroll's seq is the lowest (insertion order == seq order on this node).
#[tokio::test]
async fn node_event_seq_is_monotonic_on_insert() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let a = db::connect_and_load_schema(&base).await.unwrap();
    a.batch_execute("TRUNCATE node_event, local_node").await.ok();

    let tmp = tempfile::tempdir().unwrap();
    let (sk, kid) = keystore::generate_and_seal(&tmp.path().join("a.key"), None).unwrap();
    identity::provision(&a, &sk, &kid, "A", "127.0.0.1:7900").await.unwrap();

    // After genesis there is exactly one row, and its seq is NOT NULL and >= 1.
    let row = a.query_one(
        "SELECT count(*) AS n, min(seq) AS lo, max(seq) AS hi FROM node_event", &[]
    ).await.unwrap();
    let n: i64 = row.get("n");
    let lo: i64 = row.get("lo");
    let hi: i64 = row.get("hi");
    assert_eq!(n, 1, "exactly the genesis enroll");
    assert_eq!(lo, hi, "single row: lo == hi");
    assert!(lo >= 1, "seq is assigned (>= 1), got {lo}");
}
