//! ADR-0026 slice D — integration tests for the sealed local-state export.
//! DB-gated tests need CAIRN_TEST_PG (local PG with cairn_pgx installed); offline tests
//! always run.

use cairn_node::db;
use cairn_node::localstate::{apply_local_state, read_local_state};

fn cs() -> Option<String> {
    std::env::var("CAIRN_TEST_PG").ok()
}

#[tokio::test]
async fn read_local_state_is_empty_at_the_federation_tier() {
    let Some(base) = cs() else {
        eprintln!("skipped: set CAIRN_TEST_PG");
        return;
    };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let conn = db::connect_and_load_schema(&base).await.unwrap();
    db::reset_node_federation_tables(&conn).await.ok();

    let ls = read_local_state(&conn).await.expect("read must succeed");
    assert!(
        ls.is_empty(),
        "no clinical surface yet => the bundle is empty"
    );
    // Applying an empty bundle is a clean noop (the seam the clinical tier extends).
    apply_local_state(&conn, &ls)
        .await
        .expect("applying an empty bundle is a noop");
}
