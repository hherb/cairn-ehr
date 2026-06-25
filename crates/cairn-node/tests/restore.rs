//! ADR-0026 slice C — restore (apply) + new-identity supersede.
//! DB-gated: needs CAIRN_TEST_PG (local PG with cairn_pgx installed).

use cairn_event::{sign, EventBody, Hlc, SigningKey};
use cairn_node::{db, identity, keystore};

fn cs() -> Option<String> { std::env::var("CAIRN_TEST_PG").ok() }

/// A live node can author a node.superseded event naming a dead node-id; it lands as an
/// op='supersede' row and node_lineage resolves the edge (new <- old).
#[tokio::test]
async fn author_supersede_records_a_lineage_edge() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let a = db::connect_and_load_schema(&base).await.unwrap();
    db::reset_node_federation_tables(&a).await.ok();

    let tmp = tempfile::tempdir().unwrap();
    let (sk, kid) = keystore::generate_plaintext(&tmp.path().join("a.key")).unwrap();
    identity::provision(&a, &sk, &kid, "A", "127.0.0.1:7920").await.unwrap();
    let id = identity::load_local(&a).await.unwrap();

    // Author supersede naming a fabricated "old" node-id (any hex node-id works here;
    // the full restore flow supplies the real one in Task 5).
    let old_hex = "1220".to_string() + &"ab".repeat(32);
    identity::author_supersede(&a, &sk, &kid, &id.node_id_hex, &old_hex).await.unwrap();

    let row = a.query_one(
        "SELECT encode(superseded_node_id,'hex') AS old, encode(new_node_id,'hex') AS new
         FROM node_lineage", &[]).await.unwrap();
    let old: String = row.get("old");
    let new: String = row.get("new");
    assert_eq!(old, old_hex, "lineage subject == the dead node-id");
    assert_eq!(new, id.node_id_hex, "lineage author == the live (new) node-id");
}

/// Mint a real signed node.enrolled event for an arbitrary key (no DB). Mirrors how
/// identity::provision builds genesis, so its content-address == the node-id.
fn synth_enroll(sk: &SigningKey, name: &str) -> Vec<u8> {
    let kid = hex::encode(sk.verifying_key().to_bytes());
    let body = EventBody {
        event_id: uuid::Uuid::now_v7().to_string(),
        patient_id: identity::NIL_PATIENT.into(),
        event_type: "node.enrolled".into(),
        schema_version: "node/1".into(),
        hlc: Hlc { wall: 1, counter: 0, node_origin: name.into() },
        t_effective: None,
        signer_key_id: kid,
        contributors: serde_json::json!([]),
        payload: serde_json::json!({ "display_name": name, "address": "127.0.0.1:7999" }),
        attachments: vec![],
    };
    sign(&body, sk).unwrap().signed_bytes
}

/// The restore door must be a no-op on a LIVE node: with a genesis present, it raises a
/// legible "already enrolled" error. This is the structural fence that prevents the
/// self-trusting door from ever bypassing peer-admission on a running node.
#[tokio::test]
async fn restore_door_rejects_on_a_live_node() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let a = db::connect_and_load_schema(&base).await.unwrap();
    db::reset_node_federation_tables(&a).await.ok();

    let tmp = tempfile::tempdir().unwrap();
    let (sk, kid) = keystore::generate_plaintext(&tmp.path().join("a.key")).unwrap();
    identity::provision(&a, &sk, &kid, "Live", "127.0.0.1:7921").await.unwrap();

    let other = cairn_event::generate_key().unwrap().0;
    let ev = synth_enroll(&other, "Intruder");
    let err = a.execute("SELECT restore_node_event($1)", &[&ev]).await.unwrap_err();
    let db_msg = err.as_db_error().map(|e| e.message()).unwrap_or("<no db message>");
    assert!(db_msg.contains("already enrolled"),
        "fence must reject on a live node, got: {db_msg}");
}

/// Into a FRESH (un-enrolled) DB, the door applies a validly-signed enroll without any
/// peer-trust — exactly what a node rehydrating its own history needs.
#[tokio::test]
async fn restore_door_accepts_into_an_empty_db() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let a = db::connect_and_load_schema(&base).await.unwrap();
    db::reset_node_federation_tables(&a).await.ok();

    let other = cairn_event::generate_key().unwrap().0;
    let ev = synth_enroll(&other, "Restored");
    a.execute("SELECT restore_node_event($1)", &[&ev]).await
        .expect("door applies into an empty DB without peer-trust");
    let n: i64 = a.query_one("SELECT count(*) FROM node_event WHERE op='enroll'", &[])
        .await.unwrap().get(0);
    assert_eq!(n, 1, "the restored enroll is present");
    // The door must NOT set local_node (only a real new genesis does).
    let ln: i64 = a.query_one("SELECT count(*) FROM local_node", &[]).await.unwrap().get(0);
    assert_eq!(ln, 0, "restore_node_event must never write local_node");
}

/// A tampered/bit-rotted medium event fails the door's signature check — the same
/// invariant slice B proves catches a hostile peer.
#[tokio::test]
async fn restore_door_rejects_a_tampered_event() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let a = db::connect_and_load_schema(&base).await.unwrap();
    db::reset_node_federation_tables(&a).await.ok();

    let other = cairn_event::generate_key().unwrap().0;
    let mut ev = synth_enroll(&other, "Tampered");
    let mid = ev.len() / 2;
    ev[mid] ^= 0x01; // break the signature
    let err = a.execute("SELECT restore_node_event($1)", &[&ev]).await.unwrap_err();
    let db_msg = err.as_db_error().map(|e| e.message()).unwrap_or("<no db message>");
    assert!(db_msg.contains("verification failed"),
        "tampered event must fail the signature check, got: {db_msg}");
}
