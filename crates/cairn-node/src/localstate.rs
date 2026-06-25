//! ADR-0026 slice D — the sealed local-state export (container shape).
//!
//! WHY THIS EXISTS: ADR-0026 point 3 requires a node's NON-EVENT, non-signing-key
//! material — the data-at-rest keystore (node-default DEKs + sealed-episode DEKs),
//! node config, and the draft/scratchpad store — to be exportable as an encrypted
//! bundle co-located with the cold-peer backup medium, so a dead disk does not lose
//! it. The signing key is DELIBERATELY EXCLUDED (point 4): a stolen, unsealed artifact
//! must yield read access, never a signing identity.
//!
//! SCOPE (slice D): the federation-node tier has no clinical surface yet, so the bundle
//! is EMPTY today. This module builds the can't-retrofit SHAPE — the format, the
//! dual-recipient secret lifecycle (a long-lived local-state DEK dual-wrapped once at
//! provisioning), the container, and the restore path — with typed empty slots the
//! clinical tier fills later via additive evolution (principle 11). The genuine
//! day-one piece is `establish_lsk`: state accrued before the channel exists has no
//! durability path, so the channel must exist from `init`.

use serde::{Deserialize, Serialize};

#[derive(thiserror::Error, Debug)]
pub enum LocalStateError {
    /// The bytes are not a valid bundle / container / sidecar (bad magic or malformed body).
    #[error("decode: {0}")]
    Decode(String),
    /// A sealing/unsealing step failed (wrong secret, tamper, or entropy failure).
    /// Reachable from `establish_lsk`, `seal_local_state`, and their callers.
    #[error("seal: {0}")]
    Seal(String),
    // NOTE: no `Io` variant — no `localstate` function does file I/O (reads happen in
    // `main.rs` via `anyhow`). Adding it here would be YAGNI; add it when a function
    // here actually touches the filesystem.
}

/// The node-local material ADR-0026 point 3 exports. Every slot is EMPTY at the
/// federation-node tier (no clinical surface yet); the clinical tier fills them via
/// additive evolution. The leaf type is opaque `Vec<u8>` so we reserve the SLOT SHAPE
/// without committing to the clinical tier's internal schema (no speculative generality).
///
/// The signing key is DELIBERATELY ABSENT (ADR-0026 point 4): a stolen, unsealed export
/// must grant read access, never a signing identity. Do not add it here.
///
/// `serde(default)` on every content field makes this ADDITIVELY evolvable (principle 11):
/// a bundle written before a field existed still deserializes, with that field defaulted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalState {
    /// Bundle format version (bump only on a NON-additive change, which we avoid).
    /// NOT `#[serde(default)]`: absence of a version is always a malformed bundle —
    /// we must refuse it rather than silently assume v1.
    pub version: u8,
    /// Node-default data-at-rest keys. Empty today.
    #[serde(default)]
    pub node_default_deks: Vec<Vec<u8>>,
    /// Sealed-episode DEKs (minus any erased — ADR-0026 point 6). Empty today.
    #[serde(default)]
    pub episode_deks: Vec<Vec<u8>>,
    /// Node config blob. None today.
    #[serde(default)]
    pub config: Option<Vec<u8>>,
    /// Draft / scratchpad store. Empty today.
    #[serde(default)]
    pub drafts: Vec<Vec<u8>>,
}

impl LocalState {
    /// The empty bundle a federation-tier node exports today.
    pub fn empty() -> Self {
        LocalState {
            version: 1,
            node_default_deks: Vec::new(),
            episode_deks: Vec::new(),
            config: None,
            drafts: Vec::new(),
        }
    }

    /// True iff the bundle carries no content (the only valid state at this tier).
    pub fn is_empty(&self) -> bool {
        self.node_default_deks.is_empty()
            && self.episode_deks.is_empty()
            && self.config.is_none()
            && self.drafts.is_empty()
    }
}

/// Serialize a bundle to CBOR. Pure. (No magic header — the bundle is always carried
/// INSIDE a sealed container, which has its own magic; this is the plaintext that gets
/// encrypted.)
pub fn to_cbor(ls: &LocalState) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(ls, &mut out).expect("CBOR serialization of LocalState cannot fail");
    out
}

/// Parse a bundle from CBOR. Errors (never panics) on a malformed body.
pub fn from_cbor(bytes: &[u8]) -> Result<LocalState, LocalStateError> {
    ciborium::from_reader(bytes).map_err(|e| LocalStateError::Decode(e.to_string()))
}

use crate::seal::{self, aead_decrypt, aead_encrypt, normalize_recovery_code, rand_bytes,
                  ArgonParams, Wrap};

/// The dual-wraps of a long-lived local-state DEK (LSK), established ONCE at provisioning
/// (the can't-retrofit day-one piece). A random 32-byte LSK is wrapped under a KEK from the
/// operational passphrase AND a KEK from the recovery code; either secret recovers it.
/// This is the `.lsk` sidecar's payload. `Debug` is intentionally NOT derived (mirrors
/// `SealedKey`) so a stray `{:?}` cannot dump wrapped key material.
#[derive(Clone, Serialize, Deserialize)]
pub struct LskWraps {
    pub argon: ArgonParams,
    pub salt_op: [u8; 16],
    pub salt_rec: [u8; 16],
    pub wrap_op: Wrap,
    pub wrap_rec: Wrap,
}

/// A sealed local-state export: the stable LSK wraps PLUS this export's freshly-encrypted
/// bundle. Self-contained — an off-site restore needs only this (the recovery code unwraps
/// the LSK, which decrypts the payload). `Debug` deliberately not derived.
#[derive(Clone, Serialize, Deserialize)]
pub struct SealedLocalState {
    pub wraps: LskWraps,
    pub payload_nonce: [u8; 24],
    pub payload_ct: Vec<u8>,
}

/// Establish the long-lived local-state DEK and dual-wrap it. Called ONCE at provisioning
/// (`init`/`seal-key`/`establish-local-state-key`) when BOTH secrets are in hand. The LSK
/// itself is discarded after wrapping — every later export re-derives it from the op-pass.
/// Reuses `seal::wrap_dek` (the same audited Argon2id+AEAD wrap the signing key uses).
pub fn establish_lsk(op_pass: &str, recovery_code: &str) -> Result<LskWraps, LocalStateError> {
    let argon = ArgonParams::default();
    let lsk = rand_bytes::<32>().map_err(|e| LocalStateError::Seal(e.to_string()))?;
    let salt_op = rand_bytes::<16>().map_err(|e| LocalStateError::Seal(e.to_string()))?;
    let salt_rec = rand_bytes::<16>().map_err(|e| LocalStateError::Seal(e.to_string()))?;
    let wrap_op = seal::wrap_dek(&lsk, op_pass, &salt_op, &argon)
        .map_err(|e| LocalStateError::Seal(e.to_string()))?;
    // Normalize the recovery code so any spacing/case the human re-types still unseals.
    let wrap_rec = seal::wrap_dek(&lsk, &normalize_recovery_code(recovery_code), &salt_rec, &argon)
        .map_err(|e| LocalStateError::Seal(e.to_string()))?;
    Ok(LskWraps { argon, salt_op, salt_rec, wrap_op, wrap_rec })
}

/// Seal the current bundle for export: unwrap the LSK with the op-pass (the unattended,
/// runtime-available secret), then AEAD-encrypt the bundle under the LSK with a fresh nonce.
/// The wraps are carried through unchanged (stable across exports — ADR-0026 point 5).
/// Errors if the op-pass cannot unwrap the LSK (never seals under a wrong/garbage key).
pub fn seal_local_state(wraps: &LskWraps, op_pass: &str, bundle: &[u8])
    -> Result<SealedLocalState, LocalStateError> {
    let lsk = seal::try_unwrap(&wraps.wrap_op, op_pass, &wraps.salt_op, &wraps.argon)
        .ok_or_else(|| LocalStateError::Seal(
            "operational passphrase did not unwrap the local-state key".into()))?;
    let payload_nonce = rand_bytes::<24>().map_err(|e| LocalStateError::Seal(e.to_string()))?;
    let payload_ct = aead_encrypt(&lsk, &payload_nonce, bundle)
        .map_err(|_| LocalStateError::Seal("aead".into()))?;
    Ok(SealedLocalState { wraps: wraps.clone(), payload_nonce, payload_ct })
}

/// Recover the bundle via the operational passphrase (re-export / self-verify path).
pub fn unseal_local_state_op(s: &SealedLocalState, op_pass: &str) -> Option<Vec<u8>> {
    let lsk = seal::try_unwrap(&s.wraps.wrap_op, op_pass, &s.wraps.salt_op, &s.wraps.argon)?;
    aead_decrypt(&lsk, &s.payload_nonce, &s.payload_ct)
}

/// Recover the bundle via the recovery code (the disaster-recovery path — the only
/// guaranteed-available secret after total disk loss). The code is normalized first.
pub fn unseal_local_state_rec(s: &SealedLocalState, recovery_code: &str) -> Option<Vec<u8>> {
    let lsk = seal::try_unwrap(
        &s.wraps.wrap_rec, &normalize_recovery_code(recovery_code), &s.wraps.salt_rec, &s.wraps.argon)?;
    aead_decrypt(&lsk, &s.payload_nonce, &s.payload_ct)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bundle_cbor_roundtrips() {
        let ls = LocalState::empty();
        let bytes = to_cbor(&ls);
        let back = from_cbor(&bytes).expect("an empty bundle must roundtrip");
        assert_eq!(back, ls, "roundtrip must recover the exact bundle");
        assert!(back.is_empty(), "a fresh node's bundle has no content today");
    }

    #[test]
    fn from_cbor_rejects_garbage() {
        assert!(from_cbor(b"not a bundle").is_err());
    }

    #[test]
    fn older_bundle_without_a_later_field_defaults_it() {
        // Additive evolution (principle 11): a bundle serialized by an OLDER node that
        // lacks a field this node knows about must still deserialize, with the missing
        // field defaulted. We simulate "older" by constructing a ciborium Value::Map
        // omitting later fields, then serializing it to CBOR — encode a map missing `drafts`.
        let mut partial = std::collections::BTreeMap::new();
        partial.insert("version".to_string(), ciborium::value::Value::Integer(1.into()));
        // Intentionally omit node_default_deks/episode_deks/config/drafts.
        let val = ciborium::value::Value::Map(
            partial.into_iter()
                .map(|(k, v)| (ciborium::value::Value::Text(k), v))
                .collect(),
        );
        let mut bytes = Vec::new();
        ciborium::into_writer(&val, &mut bytes).unwrap();
        let back = from_cbor(&bytes).expect("a bundle missing later fields must still parse");
        assert!(back.is_empty(), "omitted collections default to empty");
    }

    const OP: &str = "op-pass";
    const REC: &str = "AB12C-D34EF";

    #[test]
    fn lsk_seal_then_unseal_via_both_recipients() {
        let wraps = establish_lsk(OP, REC).unwrap();
        let bundle = to_cbor(&LocalState::empty());
        let sealed = seal_local_state(&wraps, OP, &bundle).unwrap();
        // Either secret recovers the same plaintext bundle.
        assert_eq!(unseal_local_state_op(&sealed, OP).as_deref(), Some(bundle.as_slice()));
        assert_eq!(unseal_local_state_rec(&sealed, REC).as_deref(), Some(bundle.as_slice()),
            "the recovery code (off-node escrow) must unseal — the disaster-recovery path");
    }

    #[test]
    fn lsk_unseal_rejects_wrong_secret_and_tamper() {
        let wraps = establish_lsk(OP, REC).unwrap();
        let sealed = seal_local_state(&wraps, OP, &to_cbor(&LocalState::empty())).unwrap();
        assert_eq!(unseal_local_state_op(&sealed, "nope"), None, "wrong op-pass => None");
        assert_eq!(unseal_local_state_rec(&sealed, "ZZZZZ"), None, "wrong recovery code => None");
        // Flip a byte of the payload ciphertext: AEAD tag must fail.
        let mut t = sealed.clone();
        t.payload_ct[0] ^= 1;
        assert_eq!(unseal_local_state_op(&t, OP), None, "tampered payload must fail unseal");
    }

    #[test]
    fn seal_local_state_needs_the_op_pass_to_unwrap_the_lsk() {
        // seal_local_state unwraps the LSK with the op-pass; a wrong op-pass cannot
        // unwrap it, so sealing must fail rather than silently produce a bundle under a
        // wrong/garbage key.
        let wraps = establish_lsk(OP, REC).unwrap();
        assert!(seal_local_state(&wraps, "wrong-op", &to_cbor(&LocalState::empty())).is_err());
    }

    #[test]
    fn re_export_keeps_wraps_stable_but_refreshes_the_payload() {
        // ADR-0026 point 5 / Approach 1: the LSK (and thus its dual-wraps) is long-lived
        // across exports — only the payload re-encrypts. So two seals over the SAME wraps
        // must carry byte-identical wrap_op/wrap_rec (the recovery code still unseals both)
        // but DIFFERENT payload ciphertext (fresh nonce), and each unseals to its own bundle.
        let wraps = establish_lsk(OP, REC).unwrap();
        let a = seal_local_state(&wraps, OP, b"bundle-A").unwrap();
        let b = seal_local_state(&wraps, OP, b"bundle-B").unwrap();
        assert_eq!(a.wraps.wrap_op.ct, b.wraps.wrap_op.ct, "LSK op-wrap is stable across exports");
        assert_eq!(a.wraps.wrap_rec.ct, b.wraps.wrap_rec.ct, "LSK rec-wrap is stable across exports");
        assert_ne!(a.payload_ct, b.payload_ct, "each export re-encrypts the payload (fresh nonce)");
        assert_eq!(unseal_local_state_rec(&a, REC).as_deref(), Some(b"bundle-A".as_slice()));
        assert_eq!(unseal_local_state_rec(&b, REC).as_deref(), Some(b"bundle-B".as_slice()));
    }
}
