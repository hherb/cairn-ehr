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

/// The cursor door is advance-only: a lower observed_seq is a no-op, never a rewind.
#[tokio::test]
async fn checkpoint_sync_cursor_is_advance_only() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let a = db::connect_and_load_schema(&base).await.unwrap();
    a.batch_execute("TRUNCATE node_event, local_node, sync_cursor").await.ok();

    let peer = "127.0.0.1:7901";
    let s1: i64 = a.query_one("SELECT checkpoint_sync_cursor($1,$2)", &[&peer, &10_i64])
        .await.unwrap().get(0);
    assert_eq!(s1, 10, "first checkpoint sets last_seq=10");
    // A lower value must NOT rewind.
    let s2: i64 = a.query_one("SELECT checkpoint_sync_cursor($1,$2)", &[&peer, &5_i64])
        .await.unwrap().get(0);
    assert_eq!(s2, 10, "lower observed_seq is a no-op (advance-only)");
    // A higher value advances.
    let s3: i64 = a.query_one("SELECT checkpoint_sync_cursor($1,$2)", &[&peer, &20_i64])
        .await.unwrap().get(0);
    assert_eq!(s3, 20, "higher observed_seq advances");
}

/// The floor invariant (PR #39) extended to sync_cursor: the unprivileged runtime role
/// CANNOT raw-write the table — it may only go through the validated door. Mirrors
/// `tests/floor_enforced.rs`.
fn conn_as_role(base: &str, role: &str) -> String {
    let kept: Vec<&str> = base.split_whitespace().filter(|kv| !kv.starts_with("user=")).collect();
    format!("{} user={role}", kept.join(" "))
}

#[tokio::test]
async fn runtime_role_cannot_raw_write_sync_cursor_but_door_works() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let owner = db::connect_and_load_schema(&base).await.unwrap();
    owner.batch_execute("TRUNCATE node_event, local_node, sync_cursor").await.ok();

    let role = "cairn_runtime_cursor_test";
    db::provision_runtime_role(&owner, role).await.unwrap();
    let runtime = db::connect(&conn_as_role(&base, role)).await.unwrap();

    // Raw INSERT is denied (42501).
    let raw = runtime
        .execute("INSERT INTO sync_cursor (peer_addr, last_seq) VALUES ('x', 1)", &[])
        .await;
    let err = raw.expect_err("raw INSERT into sync_cursor must be denied for the runtime role");
    assert_eq!(
        err.code(),
        Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE),
        "raw INSERT must fail with 42501, got: {err:?}"
    );
    // The validated door still works for the unprivileged role.
    let v: i64 = runtime
        .query_one("SELECT checkpoint_sync_cursor($1,$2)", &[&"x", &7_i64])
        .await.expect("door must work for the cairn_node-granted role").get(0);
    assert_eq!(v, 7, "door advanced the cursor for the runtime role");

    drop(runtime);
    owner.batch_execute(&format!("DROP ROLE IF EXISTS {role}")).await.ok();
}
