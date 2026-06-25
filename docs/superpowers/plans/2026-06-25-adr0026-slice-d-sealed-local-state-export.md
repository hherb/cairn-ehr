# ADR-0026 Slice D — Sealed Local-State Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the sealed, dual-recipient, cold-peer-co-located local-state export (the `CAIRNL1` sibling) plus its day-one local-state-DEK provisioning, with an empty-but-extensible content bundle — closing the last open ADR-0026 slice.

**Architecture:** A new `localstate.rs` module holds a versioned `LocalState` bundle (typed empty slots), a long-lived local-state DEK (LSK) dual-wrapped at `init` (the can't-retrofit piece), seal/unseal built from the existing `seal.rs` primitives, a `CAIRNL1` container format, a `.lsk` sidecar, DB read/apply seams (empty today), and a status helper. CLI verbs in `main.rs` establish the LSK at provisioning, write the export on `backup`, and consume it on `restore`.

**Tech Stack:** Rust, `argon2` + `chacha20poly1305` + `getrandom` (already deps), `ciborium` (CBOR, already a dep), `serde`, `tokio-postgres`, `clap`, `tempfile` (dev).

## Global Constraints

- **License:** AGPL-3.0; every dependency AGPL-3.0-compatible. No new crates are introduced by this plan (all listed crates are already in `crates/cairn-node/Cargo.toml`).
- **TDD:** failing test first, then the code that passes it. Load-bearing — this is the safety-critical surface (a defect is silent data loss / a forged-read / identity confusion).
- **Inline docs for a junior dev:** every non-trivial fn/module carries *why it exists and how it fits*, not just *what*.
- **Pure functions in reusable modules** over clever complexity; files under ~500 lines.
- **Reviewer-legibility** is the §9 rule for this surface.
- **Signing key is NEVER in the bundle** (ADR-0026 point 4) — documented in code.
- **No DB schema change** in this slice (no `db/010`): content is empty, `read_local_state`/`apply_local_state` are stubs over existing tables.
- **Magic headers** (8 bytes each, mirroring `CAIRNK1\n`/`CAIRNB1\n`): `.lsk` sidecar = `CAIRNX1\n`; export container = `CAIRNL1\n`.
- **Atomic writes** via `crate::fsio::atomic_write(path, bytes, Some(0o600))`.
- Run all node tests with: `cargo test -p cairn-node`. Pure tests need no DB; DB-gated tests self-serialize via `db::test_serial_guard` and need a local PG + `cairn_pgx`.

---

## File Structure

- **Create** `crates/cairn-node/src/localstate.rs` — the whole slice's pure logic + DB seams + status helper.
- **Modify** `crates/cairn-node/src/seal.rs` — expose envelope primitives `pub(crate)` (no behaviour change to the signing-key path).
- **Modify** `crates/cairn-node/src/lib.rs` — register `pub mod localstate;`.
- **Modify** `crates/cairn-node/src/identity.rs` — add a `local_state: String` field to `Status` + assemble it.
- **Modify** `crates/cairn-node/src/main.rs` — `init`/`seal-key` establish the `.lsk`; new `establish-local-state-key` verb; `backup` writes the export; `restore` consumes it; `status` prints the line.
- **Create** `crates/cairn-node/tests/localstate.rs` — DB-gated export→restore round-trip + no-export degradation.

---

## Task 1: `LocalState` bundle type + CBOR additive roundtrip

**Files:**
- Create: `crates/cairn-node/src/localstate.rs`
- Modify: `crates/cairn-node/src/lib.rs:6` (add `pub mod localstate;` in alpha order, after `pub mod keystore;`)
- Test: inline `#[cfg(test)]` in `localstate.rs`

**Interfaces:**
- Produces: `struct LocalState`, `LocalState::empty() -> LocalState`, `to_cbor(&LocalState) -> Vec<u8>`, `from_cbor(&[u8]) -> Result<LocalState, LocalStateError>`, `enum LocalStateError`.

- [ ] **Step 1: Register the module**

In `crates/cairn-node/src/lib.rs`, add after the `pub mod keystore;` line:

```rust
pub mod localstate;
```

- [ ] **Step 2: Write the failing test (module + bundle + CBOR additive roundtrip)**

Create `crates/cairn-node/src/localstate.rs` with ONLY the test module first so it fails to compile (red):

```rust
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
        // field defaulted. We simulate "older" by serializing a struct with a subset of
        // fields via a serde_json->cbor shim: encode a map missing `drafts`.
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
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p cairn-node --lib localstate`
Expected: FAIL — `cannot find type LocalState` / `cannot find function to_cbor` (does not compile).

- [ ] **Step 4: Write the minimal implementation**

Insert ABOVE the `#[cfg(test)]` block in `crates/cairn-node/src/localstate.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(thiserror::Error, Debug)]
pub enum LocalStateError {
    /// The bytes are not a valid bundle / container / sidecar (bad magic or malformed body).
    #[error("decode: {0}")]
    Decode(String),
    /// A sealing/unsealing step failed (wrong secret, tamper, or entropy failure).
    #[error("seal: {0}")]
    Seal(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
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
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p cairn-node --lib localstate`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-node/src/localstate.rs crates/cairn-node/src/lib.rs
git commit -m "feat(node): LocalState export bundle type + additive CBOR (ADR-0026 slice D)"
```

---

## Task 2: Expose `seal.rs` primitives + the LSK sealing API

**Files:**
- Modify: `crates/cairn-node/src/seal.rs` — change six items from private to `pub(crate)`.
- Modify: `crates/cairn-node/src/localstate.rs` — add the LSK structs + sealing fns + tests.

**Interfaces:**
- Consumes (from `seal.rs`, made `pub(crate)`): `wrap_dek`, `try_unwrap`, `aead_encrypt`, `aead_decrypt`, `rand_bytes`; plus the already-`pub` `Wrap`, `ArgonParams`, `normalize_recovery_code`. (`derive_kek` stays private — it is used only inside `wrap_dek`/`try_unwrap`, which we now reuse directly rather than re-deriving KEKs.)
- Produces: `struct LskWraps`, `struct SealedLocalState`, `fn establish_lsk(op_pass, recovery_code) -> Result<LskWraps, LocalStateError>`, `fn seal_local_state(&LskWraps, op_pass, bundle: &[u8]) -> Result<SealedLocalState, LocalStateError>`, `fn unseal_local_state_op(&SealedLocalState, op_pass) -> Option<Vec<u8>>`, `fn unseal_local_state_rec(&SealedLocalState, recovery_code) -> Option<Vec<u8>>`.

- [ ] **Step 1: Make the `seal.rs` primitives crate-visible**

In `crates/cairn-node/src/seal.rs`, change these signatures (add `pub(crate)`; bodies unchanged) so `localstate.rs` reuses the SAME audited wrap/unwrap + AEAD logic (no duplicated crypto):

- `fn rand_bytes<const N: usize>() -> ...` → `pub(crate) fn rand_bytes<const N: usize>() -> ...`
- `fn aead_encrypt(...) -> ...` → `pub(crate) fn aead_encrypt(...) -> ...`
- `fn aead_decrypt(...) -> ...` → `pub(crate) fn aead_decrypt(...) -> ...`
- `fn wrap_dek(dek: &[u8; 32], secret: &str, salt: &[u8; 16], p: &ArgonParams) -> Result<Wrap, SealError>` → add `pub(crate)`
- `fn try_unwrap(w: &Wrap, secret: &str, salt: &[u8; 16], p: &ArgonParams) -> Option<[u8; 32]>` → add `pub(crate)`

`pub struct Wrap`, `pub struct ArgonParams`, `pub enum SealError`, and `pub fn normalize_recovery_code` are already public — leave them. `derive_kek` stays private. Do NOT touch `seal`/`unseal`/`SealedKey` (the signing-key path stays exactly as-is).

- [ ] **Step 2: Write the failing tests (LSK lifecycle)**

Append to the `#[cfg(test)] mod tests` in `crates/cairn-node/src/localstate.rs`:

```rust
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
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p cairn-node --lib localstate`
Expected: FAIL — `cannot find function establish_lsk` etc.

- [ ] **Step 4: Write the LSK sealing implementation**

Insert into `crates/cairn-node/src/localstate.rs` ABOVE the test module (after the `from_cbor` fn):

```rust
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p cairn-node --lib localstate`
Expected: PASS (all Task 1 + Task 2 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/cairn-node/src/seal.rs crates/cairn-node/src/localstate.rs
git commit -m "feat(node): long-lived LSK dual-wrap + local-state seal/unseal (ADR-0026 slice D)"
```

---

## Task 3: Container format, sidecar (de)serialize, path derivation, status helper

**Files:**
- Modify: `crates/cairn-node/src/localstate.rs` — add magics, serialize/parse, path fns, `describe_local_state`, + tests.

**Interfaces:**
- Produces: `fn serialize_container(&SealedLocalState) -> Vec<u8>`, `fn parse_container(&[u8]) -> Result<SealedLocalState, LocalStateError>`, `fn serialize_sidecar(&LskWraps) -> Vec<u8>`, `fn parse_sidecar(&[u8]) -> Result<LskWraps, LocalStateError>`, `fn localstate_path_for(&Path) -> PathBuf`, `fn lsk_sidecar_path_for(&Path) -> PathBuf`, `fn describe_local_state(lsk_present: bool, export_present: bool) -> String`.

- [ ] **Step 1: Write the failing tests**

Append to the test module in `crates/cairn-node/src/localstate.rs`:

```rust
    use std::path::Path;

    #[test]
    fn container_roundtrips_and_has_magic() {
        let wraps = establish_lsk(OP, REC).unwrap();
        let sealed = seal_local_state(&wraps, OP, b"x").unwrap();
        let bytes = serialize_container(&sealed);
        assert!(bytes.starts_with(b"CAIRNL1\n"), "export container must carry CAIRNL1 magic");
        let back = parse_container(&bytes).unwrap();
        assert_eq!(unseal_local_state_rec(&back, REC).as_deref(), Some(b"x".as_slice()));
    }

    #[test]
    fn sidecar_roundtrips_and_has_magic() {
        let wraps = establish_lsk(OP, REC).unwrap();
        let bytes = serialize_sidecar(&wraps);
        assert!(bytes.starts_with(b"CAIRNX1\n"), "lsk sidecar must carry CAIRNX1 magic");
        let back = parse_sidecar(&bytes).unwrap();
        // The recovered wraps still unseal an export sealed under the originals.
        let sealed = seal_local_state(&back, OP, b"y").unwrap();
        assert_eq!(unseal_local_state_op(&sealed, OP).as_deref(), Some(b"y".as_slice()));
    }

    #[test]
    fn parse_rejects_wrong_or_missing_magic() {
        assert!(parse_container(b"nope").is_err());
        assert!(parse_sidecar(b"nope").is_err());
        // A container's bytes are not a valid sidecar and vice-versa (distinct magics).
        let wraps = establish_lsk(OP, REC).unwrap();
        let container = serialize_container(&seal_local_state(&wraps, OP, b"z").unwrap());
        assert!(parse_sidecar(&container).is_err(), "a container must not parse as a sidecar");
    }

    #[test]
    fn paths_are_deterministic_siblings() {
        assert_eq!(localstate_path_for(Path::new("/mnt/backup/cairn.medium")),
                   Path::new("/mnt/backup/cairn.medium.localstate"));
        assert_eq!(lsk_sidecar_path_for(Path::new("/var/lib/cairn/node.key")),
                   Path::new("/var/lib/cairn/node.key.lsk"));
    }

    #[test]
    fn describe_local_state_is_honest_about_escrow_and_export() {
        assert!(describe_local_state(false, false).contains("no local-state escrow"));
        assert!(describe_local_state(true, false).contains("escrow set"));
        assert!(describe_local_state(true, false).contains("no export yet"));
        assert!(describe_local_state(true, true).contains("exported"));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p cairn-node --lib localstate`
Expected: FAIL — `cannot find function serialize_container` etc.

- [ ] **Step 3: Implement**

Add near the top of `crates/cairn-node/src/localstate.rs` (after the `use serde` line):

```rust
use std::path::{Path, PathBuf};

/// Magic for the `.lsk` sidecar (the dual-wrapped LSK). 8 bytes, like CAIRNK1/CAIRNB1.
const SIDECAR_MAGIC: &[u8] = b"CAIRNX1\n";
/// Magic for the export container (the sealed local-state bundle).
const CONTAINER_MAGIC: &[u8] = b"CAIRNL1\n";
```

Add these fns ABOVE the test module:

```rust
/// Serialize a sealed export to magic-prefixed CBOR for the `CAIRNL1` sibling file. Pure.
pub fn serialize_container(s: &SealedLocalState) -> Vec<u8> {
    let mut out = CONTAINER_MAGIC.to_vec();
    ciborium::into_writer(s, &mut out).expect("CBOR serialization of SealedLocalState cannot fail");
    out
}

/// Parse a `CAIRNL1` container. Errors (never panics) on bad magic / malformed body.
pub fn parse_container(bytes: &[u8]) -> Result<SealedLocalState, LocalStateError> {
    let body = bytes.strip_prefix(CONTAINER_MAGIC)
        .ok_or_else(|| LocalStateError::Decode("missing CAIRNL1 magic".into()))?;
    ciborium::from_reader(body).map_err(|e| LocalStateError::Decode(e.to_string()))
}

/// Serialize the LSK wraps to magic-prefixed CBOR for the `.lsk` sidecar. Pure.
pub fn serialize_sidecar(w: &LskWraps) -> Vec<u8> {
    let mut out = SIDECAR_MAGIC.to_vec();
    ciborium::into_writer(w, &mut out).expect("CBOR serialization of LskWraps cannot fail");
    out
}

/// Parse a `.lsk` sidecar. Errors on bad magic / malformed body.
pub fn parse_sidecar(bytes: &[u8]) -> Result<LskWraps, LocalStateError> {
    let body = bytes.strip_prefix(SIDECAR_MAGIC)
        .ok_or_else(|| LocalStateError::Decode("missing CAIRNX1 magic".into()))?;
    ciborium::from_reader(body).map_err(|e| LocalStateError::Decode(e.to_string()))
}

/// The export sibling for a backup medium: `<medium>.localstate` in the same directory,
/// so the operator carries ONE artifact off-site (ADR-0026 point 3 — "same artifact"). Pure.
pub fn localstate_path_for(medium: &Path) -> PathBuf {
    let mut name = medium.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".localstate");
    medium.with_file_name(name)
}

/// The `.lsk` sidecar for a key file: `<key>.lsk`, sibling of the signing key. Pure.
pub fn lsk_sidecar_path_for(key: &Path) -> PathBuf {
    let mut name = key.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".lsk");
    key.with_file_name(name)
}

/// The `status` local-state line. Pure (presence flags injected). Honest about BOTH the
/// day-one escrow (`.lsk` present) and whether an export has been written. Absent escrow is
/// the loud case — a node accruing real content without the channel would lose it on a dead disk.
pub fn describe_local_state(lsk_present: bool, export_present: bool) -> String {
    match (lsk_present, export_present) {
        (false, _) => "no local-state escrow — run `cairn-node establish-local-state-key`".to_string(),
        (true, false) => "escrow set (dual-recipient); no export yet — run `cairn-node backup`".to_string(),
        (true, true) => "escrow set (dual-recipient); exported alongside the last backup".to_string(),
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p cairn-node --lib localstate`
Expected: PASS (all tests so far).

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-node/src/localstate.rs
git commit -m "feat(node): CAIRNL1 container + CAIRNX1 sidecar + paths + status helper (ADR-0026 slice D)"
```

---

## Task 4: DB read/apply seams (empty today)

**Files:**
- Modify: `crates/cairn-node/src/localstate.rs` — add `read_local_state` + `apply_local_state`.
- Create: `crates/cairn-node/tests/localstate.rs` — the DB-gated empty read/apply test (Task 6 appends to this same file).

**Interfaces:**
- Produces: `async fn read_local_state(&tokio_postgres::Client) -> anyhow::Result<LocalState>`, `async fn apply_local_state(&tokio_postgres::Client, &LocalState) -> anyhow::Result<()>`.

- [ ] **Step 1: Write the failing DB-gated test**

DB-gated tests live in `tests/` and gate on the `CAIRN_TEST_PG` env var (the exact pattern used by `crates/cairn-node/tests/restore.rs`: a `cs()` helper, `db::test_serial_guard(&base)` which takes the conn STRING and returns a guard `Client`, then `db::connect_and_load_schema(&base)` + `db::reset_node_federation_tables`). Create `crates/cairn-node/tests/localstate.rs`:

```rust
//! ADR-0026 slice D — integration tests for the sealed local-state export.
//! DB-gated tests need CAIRN_TEST_PG (local PG with cairn_pgx installed); offline tests
//! always run.

use cairn_node::db;
use cairn_node::localstate::{apply_local_state, read_local_state};

fn cs() -> Option<String> { std::env::var("CAIRN_TEST_PG").ok() }

#[tokio::test]
async fn read_local_state_is_empty_at_the_federation_tier() {
    let Some(base) = cs() else { eprintln!("skipped: set CAIRN_TEST_PG"); return };
    let _guard = db::test_serial_guard(&base).await.unwrap();
    let conn = db::connect_and_load_schema(&base).await.unwrap();
    db::reset_node_federation_tables(&conn).await.ok();

    let ls = read_local_state(&conn).await.expect("read must succeed");
    assert!(ls.is_empty(), "no clinical surface yet => the bundle is empty");
    // Applying an empty bundle is a clean noop (the seam the clinical tier extends).
    apply_local_state(&conn, &ls).await.expect("applying an empty bundle is a noop");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p cairn-node --test localstate`
Expected: FAIL — `cannot find function read_local_state` / `apply_local_state` (does not compile).

- [ ] **Step 3: Implement the seams**

Add to `crates/cairn-node/src/localstate.rs` ABOVE the test module:

```rust
/// Read the node's exportable local state from the DB. At the federation-node tier there
/// is no clinical surface — no DEK store, no draft store, no config table — so this returns
/// the EMPTY bundle. THIS IS THE SEAM the clinical tier extends: it will read the keystore's
/// node-default + sealed-episode DEKs (minus erased — ADR-0026 point 6), node config, and
/// the draft/scratchpad store into the typed slots. The signing key is never read here
/// (point 4). Async + DB-handle so the future shape needs no signature change.
pub async fn read_local_state(_db: &tokio_postgres::Client) -> anyhow::Result<LocalState> {
    Ok(LocalState::empty())
}

/// Apply a restored local-state bundle into a fresh node. At this tier the bundle is empty,
/// so this is a validated noop — it asserts the bundle carries no content it cannot yet
/// honour, rather than silently dropping it. THIS IS THE SEAM the clinical tier extends: it
/// will load DEKs into the keystore, restore config, and rehydrate drafts. A non-empty
/// bundle here means a newer node wrote content an older restorer cannot apply — fail loud.
pub async fn apply_local_state(_db: &tokio_postgres::Client, ls: &LocalState) -> anyhow::Result<()> {
    if !ls.is_empty() {
        anyhow::bail!(
            "restored local-state bundle carries content this node version cannot apply \
             (the clinical-tier apply seam is not built yet); refusing to silently drop it"
        );
    }
    Ok(())
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p cairn-node --test localstate` (DB-gated test runs if `CAIRN_TEST_PG` is set, else prints "skipped" and passes).
Run: `cargo test -p cairn-node --lib localstate` (the pure unit tests remain green).
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/cairn-node/src/localstate.rs crates/cairn-node/tests/localstate.rs
git commit -m "feat(node): read/apply local-state DB seams (empty at federation tier) (ADR-0026 slice D)"
```

---

## Task 5: CLI — establish `.lsk` at provisioning + new `establish-local-state-key` verb + status line

**Files:**
- Modify: `crates/cairn-node/src/identity.rs` — add `local_state: String` to `Status` + assemble it.
- Modify: `crates/cairn-node/src/main.rs` — establish `.lsk` in `init`/`seal-key`; new `EstablishLocalStateKey` verb; print the status line.

**Interfaces:**
- Consumes: `localstate::{establish_lsk, serialize_sidecar, lsk_sidecar_path_for, localstate_path_for, parse_sidecar, describe_local_state}`, `fsio::atomic_write`.
- Produces: a `local_state` status field; a `establish-local-state-key` CLI subcommand.

- [ ] **Step 1: Add the `local_state` field + assembly to `Status` (with a unit test)**

In `crates/cairn-node/src/identity.rs`, add to the `Status` struct (after the `supersedes` field, line ~212):

```rust
    /// Local-state export posture (ADR-0026 slice D): whether the day-one local-state
    /// escrow (`<key>.lsk`) exists and whether an export sibling sits alongside the last
    /// backup medium. Node-local, never an event. Honest-degrades to the "no escrow"
    /// warning when absent.
    pub local_state: String,
```

In the `status` fn, before building the returned `Status { ... }`, add:

```rust
    // Local-state export posture (ADR-0026 slice D). The escrow is the `.lsk` sidecar of
    // the key; the export is the `<medium>.localstate` sibling of the LAST backup medium
    // (its path is recorded in the backup-health sidecar we already read above).
    let lsk_present = crate::localstate::parse_sidecar(
        &std::fs::read(crate::localstate::lsk_sidecar_path_for(key_path)).unwrap_or_default()
    ).is_ok();
    let export_present = health.as_ref().is_some_and(|h| {
        let medium = std::path::Path::new(&h.medium_path);
        crate::localstate::localstate_path_for(medium).exists()
    });
    let local_state = crate::localstate::describe_local_state(lsk_present, export_present);
```

Then add `local_state,` to the returned struct literal. (`health` is already in scope from the backup-health read at line ~292.)

Add a unit test to `identity.rs`'s test module (or create one if absent) asserting the field is wired — but since `status` is DB-gated, instead rely on the pure `describe_local_state` test in Task 3. No new test needed here beyond compilation; the integration test in Task 6 exercises it.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p cairn-node`
Expected: SUCCESS (the `Status` initializer now includes `local_state`).

- [ ] **Step 3: Establish the `.lsk` in `init` (sealed path) + print confirmation**

In `crates/cairn-node/src/main.rs`, in the `Cmd::Init` arm's sealed `else` branch, AFTER `generate_sealed` returns and BEFORE `provision`, add the LSK establishment. Replace:

```rust
                print_recovery_code(&code);
                cairn_node::keystore::generate_sealed(&cli.key, &op, &code)?
            };
```

with:

```rust
                print_recovery_code(&code);
                let kp = cairn_node::keystore::generate_sealed(&cli.key, &op, &code)?;
                // Establish the day-one local-state escrow (ADR-0026 slice D): a long-lived
                // local-state DEK dual-wrapped under the SAME two secrets. Must happen here,
                // while both are in hand — it cannot be retrofitted onto state accrued later.
                establish_local_state_escrow(&cli.key, &op, &code)?;
                kp
            };
```

Add this helper near the other free fns at the top of `main.rs` (after `print_recovery_code`):

```rust
/// Write the `.lsk` sidecar (the day-one local-state escrow, ADR-0026 slice D). Mints +
/// dual-wraps a long-lived local-state DEK and atomically writes it 0600 beside the key.
/// Errors loudly if the sidecar already exists (no silent overwrite of an escrow).
fn establish_local_state_escrow(key_path: &std::path::Path, op_pass: &str, recovery_code: &str)
    -> anyhow::Result<()> {
    use cairn_node::localstate::{establish_lsk, lsk_sidecar_path_for, serialize_sidecar};
    let sidecar = lsk_sidecar_path_for(key_path);
    if sidecar.exists() {
        anyhow::bail!("local-state escrow already exists at {}", sidecar.display());
    }
    let wraps = establish_lsk(op_pass, recovery_code)?;
    cairn_node::fsio::atomic_write(&sidecar, &serialize_sidecar(&wraps), Some(0o600))?;
    eprintln!("local-state escrow established at {}", sidecar.display());
    Ok(())
}
```

- [ ] **Step 4: Establish the `.lsk` in `seal-key` too**

In the `Cmd::SealKey` arm, after `seal_existing(...)?;` and before the `println!`, add:

```rust
            establish_local_state_escrow(&cli.key, &op, &code)?;
```

- [ ] **Step 5: Add the `establish-local-state-key` verb (for pre-slice-D nodes)**

In the `Cmd` enum, after `SealKey { ... }`, add:

```rust
    /// Establish the local-state escrow (`.lsk`) for a node provisioned before slice D.
    /// Prompts for the op passphrase AND the recovery code (both needed once). Errors if
    /// an escrow already exists.
    EstablishLocalStateKey {
        #[arg(long, env = "CAIRN_KEY_PASSPHRASE")] passphrase: Option<String>,
    },
```

In `main`, add the arm (after the `Cmd::SealKey` arm):

```rust
        Cmd::EstablishLocalStateKey { passphrase } => {
            let op = resolve_passphrase(passphrase)?;
            // The recovery code is the OFF-NODE secret; the node never stored it, so the
            // operator must type the one shown at `init`/`seal-key`.
            let code = Zeroizing::new(
                rpassword::prompt_password("recovery code (from init/seal-key): ")?);
            if code.is_empty() {
                anyhow::bail!("no recovery code provided");
            }
            establish_local_state_escrow(&cli.key, &op, &code)?;
            println!("local-state escrow established.");
        }
```

- [ ] **Step 6: Print the status line**

In the `Cmd::Status` arm, after the `last_backup` print and before the `supersedes` block, add:

```rust
            println!("local_state   {}", st.local_state);
```

- [ ] **Step 7: Build + run the existing status/keystore tests**

Run: `cargo test -p cairn-node --test status --test keystore_seal`
Expected: PASS (no regressions; existing tests still green).

Run: `cargo build -p cairn-node`
Expected: SUCCESS.

- [ ] **Step 8: Commit**

```bash
git add crates/cairn-node/src/identity.rs crates/cairn-node/src/main.rs
git commit -m "feat(node): establish .lsk at provisioning + establish-local-state-key + status line (ADR-0026 slice D)"
```

---

## Task 6: `backup` writes the export; `restore` consumes it; integration round-trip

**Files:**
- Modify: `crates/cairn-node/src/main.rs` — `Cmd::Backup` writes the `CAIRNL1` sibling; `Cmd::Restore` consumes it + the new node establishes its own `.lsk`.
- Modify: `crates/cairn-node/tests/localstate.rs` — append offline export→restore round-trip + sidecar atomic-write tests (file created in Task 4).

**Interfaces:**
- Consumes: `localstate::{read_local_state, to_cbor, parse_sidecar, seal_local_state, serialize_container, lsk_sidecar_path_for, localstate_path_for, parse_container, unseal_local_state_rec, from_cbor, apply_local_state, establish_lsk, serialize_sidecar}`, the existing `restore`/`backup`/`identity` fns.

- [ ] **Step 1: `Cmd::Backup` writes the export sibling**

First, add a passphrase arg to the `Backup` command so the op-pass (which unwraps the LSK) comes from `--passphrase`/`CAIRN_KEY_PASSPHRASE` like `init`/`restore` — never an unattended prompt-hang. In the `Cmd` enum, change the `Backup` variant to:

```rust
    Backup {
        /// Path of the backup medium to write (e.g. a mounted encrypted volume).
        #[arg(long)]
        to: PathBuf,
        /// Operational passphrase to seal the local-state export (else CAIRN_KEY_PASSPHRASE,
        /// else prompt). Only used when a local-state escrow (`.lsk`) exists.
        #[arg(long, env = "CAIRN_KEY_PASSPHRASE")]
        passphrase: Option<String>,
    },
```

Then change the arm header `Cmd::Backup { to } => {` to `Cmd::Backup { to, passphrase } => {`. AFTER the existing `backup_to` call and its two `println!`s, add:

```rust
            // ADR-0026 slice D: co-locate the sealed local-state export beside the medium,
            // IF the local-state escrow exists. Degrades honestly (warn, never fail the
            // event backup) when the escrow is absent — the events are the load-bearing copy.
            let sidecar = cairn_node::localstate::lsk_sidecar_path_for(&cli.key);
            match std::fs::read(&sidecar).ok()
                .and_then(|b| cairn_node::localstate::parse_sidecar(&b).ok()) {
                Some(wraps) => {
                    let op = resolve_passphrase(passphrase)?; // op-pass unwraps the LSK
                    let bundle = cairn_node::localstate::read_local_state(&db).await?;
                    let sealed = cairn_node::localstate::seal_local_state(
                        &wraps, &op, &cairn_node::localstate::to_cbor(&bundle))?;
                    let export_path = cairn_node::localstate::localstate_path_for(&to);
                    cairn_node::fsio::atomic_write(
                        &export_path, &cairn_node::localstate::serialize_container(&sealed), Some(0o600))?;
                    println!("local-state exported to {}", export_path.display());
                }
                None => eprintln!(
                    "WARNING: no local-state escrow ({} absent) — backed up events only; \
                     run `cairn-node establish-local-state-key` to enable the sealed export",
                    sidecar.display()),
            }
```

- [ ] **Step 2: `Cmd::Restore` consumes the export sibling**

In the `Cmd::Restore` arm, AFTER step 5 (`finalize_identity`) and BEFORE the closing `println!`s, add the local-state restore + the new node's own escrow. Insert:

```rust
            // ADR-0026 slice D: if a sealed local-state export sits beside the medium,
            // unseal it with the OLD recovery code and apply it (empty/noop today), then
            // give the NEW node its OWN local-state escrow. Absent export => skip (the node
            // restores from events alone — honest degradation).
            let export_path = cairn_node::localstate::localstate_path_for(&from);
            if let Ok(bytes) = std::fs::read(&export_path) {
                let sealed = cairn_node::localstate::parse_container(&bytes)?;
                eprintln!("Local-state export found. Enter the OLD node's recovery code to unseal it:");
                let old_code = Zeroizing::new(
                    rpassword::prompt_password("old recovery code: ")?);
                let plaintext = cairn_node::localstate::unseal_local_state_rec(&sealed, &old_code)
                    .ok_or_else(|| anyhow::anyhow!(
                        "could not unseal the local-state export with that recovery code"))?;
                let bundle = cairn_node::localstate::from_cbor(&plaintext)?;
                cairn_node::localstate::apply_local_state(&db, &bundle).await?;
                println!("local-state restored from {}", export_path.display());
            }
            // The NEW node mints its OWN local-state escrow under its NEW secrets (the new
            // `code`/`op` minted in step 4). Skipped for the insecure-plaintext test path
            // (no recovery escrow exists there), mirroring the signing-key seal.
            if !insecure_plaintext {
                // `op` and `code` were consumed in step 4's else-branch; re-resolve op from
                // env/flag and reuse the just-shown `code` is not possible here. Instead,
                // establish the escrow INSIDE step 4 (see Step 3 below).
            }
```

NOTE: `op`/`code` are scoped inside step 4's `else` block, so the new node's `.lsk` must be established THERE, not here — Step 3 fixes this. This `if !insecure_plaintext` placeholder block is removed in Step 3.

- [ ] **Step 3: Establish the new node's `.lsk` inside restore step 4**

In the `Cmd::Restore` arm's step 4 sealed `else` branch, mirror `init`: replace

```rust
                print_recovery_code(&code);
                cairn_node::keystore::generate_sealed(&cli.key, &op, &code)?
            };
```

with

```rust
                print_recovery_code(&code);
                let kp = cairn_node::keystore::generate_sealed(&cli.key, &op, &code)?;
                // The restored node gets its OWN day-one local-state escrow under its NEW
                // secrets (ADR-0026 slice D) — the old `.lsk` was on the dead disk.
                establish_local_state_escrow(&cli.key, &op, &code)?;
                kp
            };
```

Then DELETE the `if !insecure_plaintext { ... }` placeholder block added in Step 2 (the escrow is now handled here).

- [ ] **Step 4: Append the offline round-trip tests to the integration file**

Append to `crates/cairn-node/tests/localstate.rs` (created in Task 4). Extend the imports: replace the `use cairn_node::localstate::{apply_local_state, read_local_state};` line with the fuller set below, and add `use tempfile::tempdir;`:

```rust
use cairn_node::localstate::{
    apply_local_state, establish_lsk, from_cbor, localstate_path_for, lsk_sidecar_path_for,
    parse_container, read_local_state, seal_local_state, serialize_container, serialize_sidecar,
    to_cbor, unseal_local_state_rec, LocalState,
};
use tempfile::tempdir;
```

Then append these tests:

```rust
#[test]
fn export_then_restore_roundtrips_an_empty_bundle_offline() {
    // Pure/offline slice of the round-trip (no DB): seal an empty bundle under an LSK,
    // write the CAIRNL1 sibling, then unseal it via the recovery code and apply-check it.
    let dir = tempdir().unwrap();
    let medium = dir.path().join("cairn.medium");
    let op = "op-pass";
    let code = "AB12C-D34EF";

    let wraps = establish_lsk(op, code).unwrap();
    let bundle = to_cbor(&LocalState::empty());
    let sealed = seal_local_state(&wraps, op, &bundle).unwrap();
    let export_path = localstate_path_for(&medium);
    std::fs::write(&export_path, serialize_container(&sealed)).unwrap();

    // Restore side: read the sibling, unseal with the OLD recovery code, decode, check empty.
    let bytes = std::fs::read(&export_path).unwrap();
    let parsed = parse_container(&bytes).unwrap();
    let plaintext = unseal_local_state_rec(&parsed, code).expect("recovery code must unseal");
    let restored = from_cbor(&plaintext).unwrap();
    assert!(restored.is_empty(), "an empty bundle restores empty");
}

#[test]
fn sidecar_written_atomically_is_readable() {
    // The `.lsk` escrow the CLI writes must parse back (guards the serialize/atomic-write pair).
    let dir = tempdir().unwrap();
    let key = dir.path().join("node.key");
    let wraps = establish_lsk("op", "REC-CODE").unwrap();
    cairn_node::fsio::atomic_write(
        &lsk_sidecar_path_for(&key), &serialize_sidecar(&wraps), Some(0o600)).unwrap();
    let back = std::fs::read(lsk_sidecar_path_for(&key)).unwrap();
    assert!(cairn_node::localstate::parse_sidecar(&back).is_ok());
}
```

- [ ] **Step 5: Run the integration test to verify it passes**

Run: `cargo test -p cairn-node --test localstate`
Expected: PASS (both tests; they are offline/pure so no DB needed).

- [ ] **Step 6: Full build + clippy + whole suite**

Run: `cargo build -p cairn-node && cargo clippy -p cairn-node --all-targets -- -D warnings`
Expected: SUCCESS, no warnings.

Run: `cargo test -p cairn-node`
Expected: PASS (DB-gated tests run if PG present, else skip; pure tests all green).

- [ ] **Step 7: Commit**

```bash
git add crates/cairn-node/src/main.rs crates/cairn-node/tests/localstate.rs
git commit -m "feat(node): backup writes + restore consumes the sealed local-state export (ADR-0026 slice D)"
```

---

## Self-Review Notes (for the executor)

- **Spec coverage:** Task 1 = bundle + additive CBOR; Task 2 = dual-recipient LSK lifecycle (Approach 1); Task 3 = CAIRNL1/CAIRNX1 + paths + status; Task 4 = empty DB seams (no schema change); Task 5 = day-one `.lsk` at `init`/`seal-key` + migration verb + status line; Task 6 = backup export + restore consume + the new node's own escrow + tests. All four brainstorm decisions are realized.
- **Signing key excluded** (point 4): `LocalState` has no key field; documented. **Point 5** (escrow needs nothing at backup time): backup uses only the op-pass (env-available) to unwrap the long-lived LSK — verified by the wraps-stable test.
- **Type consistency:** `LskWraps`/`SealedLocalState` field names (`wraps`, `payload_nonce`, `payload_ct`, `salt_op`, `salt_rec`, `wrap_op`, `wrap_rec`) are used identically across Tasks 2/3/6. Magics `CAIRNX1\n` (sidecar) / `CAIRNL1\n` (container) are distinct and asserted not to cross-parse.
- **Open executor check:** confirm the exact DB test-helper names in `crates/cairn-node/src/db.rs` (`test_connect`/`test_serial_guard`) before writing the Task 4 DB-gated test; match the sibling pattern. If `resolve_passphrase(None)` in Task 6 Step 1 would prompt in an unattended backup, that is acceptable (backup is operator-run); for a fully unattended `run`-driven backup later, the op-pass comes from `CAIRN_KEY_PASSPHRASE` (clap env), so no prompt occurs.
