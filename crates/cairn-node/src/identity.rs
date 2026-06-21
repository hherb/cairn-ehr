use cairn_event::{event_address, short_fingerprint, sign, EventBody, Hlc, PairingBundle, SigningKey};
use tokio_postgres::Client;

pub const NIL_PATIENT: &str = "00000000-0000-0000-0000-000000000000";

pub struct Identity {
    pub node_id_hex: String,
    pub pubkey_hex: String,
    pub fingerprint: String,
    pub address: String,
}

fn node_event_body(event_type: &str, signer_key_id: &str, node_origin: &str,
                   wall: i64, counter: i32, payload: serde_json::Value) -> EventBody {
    EventBody {
        event_id: uuid::Uuid::now_v7().to_string(),
        patient_id: NIL_PATIENT.into(),
        event_type: event_type.into(),
        schema_version: "node/1".into(),
        hlc: Hlc { wall, counter, node_origin: node_origin.into() },
        t_effective: None,
        signer_key_id: signer_key_id.into(),
        contributors: serde_json::json!([{"actor_id": signer_key_id, "role": "device"}]),
        payload,
        attachments: vec![],
    }
}

/// Author the genesis node.enrolled, submit it, return node_id (hex of its content-address).
pub async fn provision(db: &Client, sk: &SigningKey, key_id: &str, display_name: &str, address: &str)
    -> anyhow::Result<String> {
    let body = node_event_body("node.enrolled", key_id, display_name, 0, 0,
        serde_json::json!({"display_name": display_name, "address": address}));
    let signed = sign(&body, sk)?;
    let signed_bytes = signed.signed_bytes.clone();
    db.execute("SELECT submit_node_event($1)", &[&signed_bytes]).await?;
    Ok(hex::encode(event_address(&signed.signed_bytes)))
}

pub async fn load_local(db: &Client) -> anyhow::Result<Identity> {
    let row = db.query_one(
        "SELECT encode(node_id,'hex') AS node_id_hex, signer_key_id, COALESCE(address,'') AS address
         FROM local_node WHERE id", &[]).await?;
    let pubkey_hex: String = row.get("signer_key_id");
    Ok(Identity {
        node_id_hex: row.get("node_id_hex"),
        fingerprint: short_fingerprint(&pubkey_hex)?,
        pubkey_hex,
        address: row.get("address"),
    })
}

// ---------------------------------------------------------------------------
// Peering authorship
// ---------------------------------------------------------------------------

/// A row from the `trust_peer` view.
pub struct PeerRow {
    pub peer_node_id_hex: String,
    pub fingerprint: String,
    pub role: Option<String>,
    pub status: String,
}

/// Author a `peer.added` event and submit it.
///
/// The payload includes `peer_node_id_hex` (required by the submit door),
/// `peer_pubkey`, `fingerprint`, and the optional `role`.
pub async fn author_peer(
    db: &Client,
    sk: &SigningKey,
    key_id: &str,
    node_origin: &str,
    peer: &PairingBundle,
    role: Option<&str>,
) -> anyhow::Result<String> {
    let body = node_event_body("peer.added", key_id, node_origin, 0, 0, serde_json::json!({
        "peer_node_id_hex": peer.node_id_hex,
        "peer_pubkey":      peer.pubkey_hex,
        "fingerprint":      peer.fingerprint,
        "role":             role,
    }));
    let signed = sign(&body, sk)?;
    let bytes = signed.signed_bytes.clone();
    db.execute("SELECT submit_node_event($1)", &[&bytes]).await?;
    Ok(hex::encode(event_address(&signed.signed_bytes)))
}

/// Author a `peer.revoked` event and submit it.
///
/// The payload includes `peer_node_id_hex` (required by the submit door).
pub async fn author_unpeer(
    db: &Client,
    sk: &SigningKey,
    key_id: &str,
    node_origin: &str,
    peer_node_id_hex: &str,
) -> anyhow::Result<String> {
    let body = node_event_body("peer.revoked", key_id, node_origin, 0, 0, serde_json::json!({
        "peer_node_id_hex": peer_node_id_hex,
    }));
    let signed = sign(&body, sk)?;
    let bytes = signed.signed_bytes.clone();
    db.execute("SELECT submit_node_event($1)", &[&bytes]).await?;
    Ok(hex::encode(event_address(&signed.signed_bytes)))
}

/// Query the `trust_peer` view and return the current peer set.
pub async fn list_peers(db: &Client) -> anyhow::Result<Vec<PeerRow>> {
    let rows = db.query(
        "SELECT encode(peer_node_id,'hex') AS pid, COALESCE(fingerprint,'') AS fp, role, status
         FROM trust_peer ORDER BY pid",
        &[],
    ).await?;
    Ok(rows.iter().map(|r| PeerRow {
        peer_node_id_hex: r.get("pid"),
        fingerprint:      r.get("fp"),
        role:             r.get("role"),
        status:           r.get("status"),
    }).collect())
}
