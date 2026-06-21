use cairn_node::{db, identity, keystore};
use cairn_event::{sign, EventBody, Hlc, event_address};

fn cs() -> Option<String> { std::env::var("CAIRN_TEST_PG").ok() }

#[tokio::test]
async fn admission_admits_trusted_peer_genesis_and_rejects_strangers() {
    let Some(base) = cs() else { eprintln!("skipped"); return; };
    let a = db::connect_and_load_schema(&base).await.unwrap();
    // Re-runnable: truncate before provisioning.
    a.batch_execute("TRUNCATE node_event, local_node").await.ok();

    let tmp = tempfile::tempdir().unwrap();
    let (sk_a, kid_a) = keystore::generate_and_seal(&tmp.path().join("a.key"), None).unwrap();
    identity::provision(&a, &sk_a, &kid_a, "A", "127.0.0.1:7800").await.unwrap();

    // B's genesis (authored against B's own key), captured as signed bytes.
    let (sk_b, kid_b) = cairn_event::generate_key().unwrap();
    let body_b = EventBody {
        event_id: uuid::Uuid::now_v7().to_string(), patient_id: identity::NIL_PATIENT.into(),
        event_type: "node.enrolled".into(), schema_version: "node/1".into(),
        hlc: Hlc { wall: 0, counter: 0, node_origin: "B".into() }, t_effective: None,
        signer_key_id: kid_b.clone(),
        contributors: serde_json::json!([{"actor_id": kid_b, "role": "device"}]),
        payload: serde_json::json!({"display_name":"B","address":"127.0.0.1:7801"}), attachments: vec![],
    };
    let signed_b = sign(&body_b, &sk_b).unwrap();
    let b_node_id = hex::encode(event_address(&signed_b.signed_bytes));

    // Before A peers with B, B's genesis is rejected (deny-all).
    let bytes = signed_b.signed_bytes.clone();
    let r = a.execute("SELECT apply_remote_node_event($1)", &[&bytes]).await;
    assert!(r.is_err(), "un-trusted genesis must be rejected");
    eprintln!("REJECT 1 (un-peered): {:?}", r.unwrap_err());

    // A pairs with B (records peer.added with B's real node_id + pubkey + fingerprint).
    let bundle = cairn_event::PairingBundle {
        node_id_hex: b_node_id.clone(), pubkey_hex: kid_b.clone(),
        address: "127.0.0.1:7801".into(),
        fingerprint: cairn_event::short_fingerprint(&kid_b).unwrap(),
        nonce: "n".into(), hlc: Hlc { wall: 0, counter: 0, node_origin: b_node_id.clone() },
    };
    identity::author_peer(&a, &sk_a, &kid_a, "A", &bundle, Some("peer")).await.unwrap();

    // Now B's genesis is admitted.
    let bytes = signed_b.signed_bytes.clone();
    a.execute("SELECT apply_remote_node_event($1)", &[&bytes]).await.unwrap();
    eprintln!("ADMIT: B genesis accepted after peering");

    // After unpeering B, a NEW B-authored peer event is rejected.
    identity::author_unpeer(&a, &sk_a, &kid_a, "A", &b_node_id).await.unwrap();
    let body_b2 = EventBody { event_id: uuid::Uuid::now_v7().to_string(),
        event_type: "peer.added".into(),
        payload: serde_json::json!({"peer_node_id_hex":"aa","peer_pubkey":"bb","fingerprint":"X"}),
        ..body_b.clone() };
    let signed_b2 = sign(&body_b2, &sk_b).unwrap();
    let bytes = signed_b2.signed_bytes.clone();
    let r2 = a.execute("SELECT apply_remote_node_event($1)", &[&bytes]).await;
    assert!(r2.is_err(), "events from a revoked peer must be rejected");
    eprintln!("REJECT 2 (revoked peer): {:?}", r2.unwrap_err());
}
