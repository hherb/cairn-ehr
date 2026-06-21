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
