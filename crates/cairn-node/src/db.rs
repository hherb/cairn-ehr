use tokio_postgres::{Client, NoTls};

const SCHEMA: [(&str, &str); 7] = [
    ("001_envelope",      include_str!("../../../db/001_envelope.sql")),
    ("002_projection",    include_str!("../../../db/002_projection.sql")),
    ("003_blobs",         include_str!("../../../db/003_blobs.sql")),
    ("004_actors",        include_str!("../../../db/004_actors.sql")),
    ("005_submit",        include_str!("../../../db/005_submit.sql")),
    ("006_recall",        include_str!("../../../db/006_recall.sql")),
    ("007_node_federation", include_str!("../../../db/007_node_federation.sql")),
];

pub async fn connect(conn: &str) -> anyhow::Result<Client> {
    let (client, connection) = tokio_postgres::connect(conn, NoTls).await?;
    tokio::spawn(async move { let _ = connection.await; });
    Ok(client)
}

pub async fn connect_and_load_schema(conn: &str) -> anyhow::Result<Client> {
    let client = connect(conn).await?;
    for (name, sql) in SCHEMA.iter() {
        client.batch_execute(sql).await.map_err(|e| anyhow::anyhow!("loading {name}: {e}"))?;
    }
    Ok(client)
}

/// Test-support: a serialization guard for the DB-gated integration tests. They
/// share Postgres databases and each `TRUNCATE`s its tables on entry, so running
/// them concurrently — across test binaries OR within one binary — races. This
/// acquires a SESSION-level advisory lock on a fixed key; the returned `Client`
/// holds the lock until it is dropped at the end of the test (a panic still drops
/// it, releasing the lock). Every caller must lock against the SAME database
/// (`CAIRN_TEST_PG`) so the guard serializes regardless of whether the server
/// scopes advisory locks per-cluster or per-database. (PR #28 review follow-up.)
pub async fn test_serial_guard(conn: &str) -> anyhow::Result<Client> {
    let client = connect(conn).await?;
    // 0x4341524E = "CARN": a fixed project-specific key shared by every guard.
    client
        .execute("SELECT pg_advisory_lock($1)", &[&0x4341524E_i64])
        .await
        .map_err(|e| anyhow::anyhow!("acquiring test serialization lock: {e}"))?;
    Ok(client)
}
