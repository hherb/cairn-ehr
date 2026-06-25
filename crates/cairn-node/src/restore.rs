//! ADR-0026 slice C — restore orchestration (apply a backup medium under a new identity).
//!
//! WHY: the live apply_remote_node_event gate is the PEER-admission path and rejects a
//! node rehydrating its OWN history (no trust set in a fresh DB). Restore therefore uses
//! the self-trusting restore_node_event door (db/009), then mints a fresh key and records
//! a node-level supersede (the old signing key is never backed up). This module holds the
//! PURE helpers (dead-node-id resolution, old-genesis metadata) and the thin DB
//! orchestration; main.rs owns key-minting + recovery-code printing (as `init` does).

use cairn_event::{event_address, verify_self_described};

#[derive(thiserror::Error, Debug)]
pub enum RestoreError {
    #[error("decode: {0}")]
    Decode(String),
    #[error("medium has no genesis (node.enrolled) event")]
    NoGenesis,
    #[error("medium carries {0} enrolls; pass --superseded-node <hex> to pick the dead node")]
    Ambiguous(usize),
}

/// Every enroll (node.enrolled) on the medium, as (node_id_hex, body) pairs. A node-id
/// is the content-address of its genesis, so we hash each verified enroll's bytes. Only
/// events that VERIFY are considered (a corrupt enroll cannot name a node).
fn enrolls(events: &[Vec<u8>]) -> Vec<(String, cairn_event::EventBody)> {
    events
        .iter()
        .filter_map(|e| {
            let body = verify_self_described(e).ok()?;
            if body.event_type == "node.enrolled" {
                Some((hex::encode(event_address(e)), body))
            } else {
                None
            }
        })
        .collect()
}

/// Resolve the dead node's id (hex) to supersede on restore.
///
/// - `explicit` (operator's --superseded-node) always wins — it is normalized to lower
///   hex but otherwise trusted (the operator knows which node they are restoring).
/// - else, if the medium has exactly ONE enroll, that is the dead node (the solo-clinic
///   case — ADR-0026's primary deployment).
/// - else it is ambiguous (a federated node whose log holds peers' genesis too) and we
///   fail closed, telling the operator to pass --superseded-node.
pub fn resolve_dead_node_id(
    events: &[Vec<u8>],
    explicit: Option<&str>,
) -> Result<String, RestoreError> {
    if let Some(e) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(e.to_ascii_lowercase());
    }
    let found = enrolls(events);
    match found.len() {
        0 => Err(RestoreError::NoGenesis),
        1 => Ok(found.into_iter().next().unwrap().0),
        n => Err(RestoreError::Ambiguous(n)),
    }
}

/// The (display_name, address) recorded in the enroll whose content-address == node_id.
/// Used so the new genesis keeps the node's name/address (paper-parity: a restored node
/// is the same clinic). Returns None if no such enroll is on the medium.
pub fn old_genesis_meta(events: &[Vec<u8>], node_id_hex: &str) -> Option<(String, String)> {
    let want = node_id_hex.to_ascii_lowercase();
    enrolls(events)
        .into_iter()
        .find(|(id, _)| *id == want)
        .map(|(_, body)| {
            let name = body
                .payload
                .get("display_name")
                .and_then(|v| v.as_str())
                .unwrap_or("restored-node")
                .to_string();
            let addr = body
                .payload
                .get("address")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            (name, addr)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_event::{sign, EventBody, Hlc, SigningKey};

    fn enroll(sk: &SigningKey, name: &str) -> Vec<u8> {
        let kid = hex::encode(sk.verifying_key().to_bytes());
        let body = EventBody {
            event_id: uuid::Uuid::now_v7().to_string(),
            patient_id: crate::identity::NIL_PATIENT.into(),
            event_type: "node.enrolled".into(),
            schema_version: "node/1".into(),
            hlc: Hlc { wall: 1, counter: 0, node_origin: name.into() },
            t_effective: None,
            signer_key_id: kid,
            contributors: serde_json::json!([]),
            payload: serde_json::json!({ "display_name": name, "address": "10.0.0.1:7843" }),
            attachments: vec![],
        };
        sign(&body, sk).unwrap().signed_bytes
    }

    fn sk() -> SigningKey {
        cairn_event::generate_key().unwrap().0
    }
    fn node_id(ev: &[u8]) -> String {
        hex::encode(event_address(ev))
    }

    #[test]
    fn single_enroll_auto_detects_the_dead_node() {
        let k = sk();
        let ev = enroll(&k, "Solo");
        let got = resolve_dead_node_id(std::slice::from_ref(&ev), None).unwrap();
        assert_eq!(got, node_id(&ev), "the sole enroll is the dead node");
    }

    #[test]
    fn multiple_enrolls_require_an_explicit_arg() {
        let a = enroll(&sk(), "A");
        let b = enroll(&sk(), "B");
        let err = resolve_dead_node_id(&[a, b], None).unwrap_err();
        assert!(matches!(err, RestoreError::Ambiguous(2)));
    }

    #[test]
    fn explicit_arg_overrides_auto_detect() {
        let a = enroll(&sk(), "A");
        let b = enroll(&sk(), "B");
        let want = node_id(&b);
        let got = resolve_dead_node_id(&[a, b], Some(&want)).unwrap();
        assert_eq!(got, want);
    }

    #[test]
    fn no_enroll_is_an_error() {
        let err = resolve_dead_node_id(&[], None).unwrap_err();
        assert!(matches!(err, RestoreError::NoGenesis));
    }

    #[test]
    fn old_genesis_meta_reads_name_and_address() {
        let k = sk();
        let ev = enroll(&k, "Clinic-7");
        let (name, addr) = old_genesis_meta(std::slice::from_ref(&ev), &node_id(&ev)).unwrap();
        assert_eq!(name, "Clinic-7");
        assert_eq!(addr, "10.0.0.1:7843");
    }
}
