//! ADR-0026 slice B — backup-as-cold-peer (the export + self-verify half) and backup
//! health.
//!
//! WHY THIS EXISTS: the spec designed *intentional* key-death in great detail
//! (crypto-shred) but left *accidental* data-death — a node's disk simply dying —
//! undesigned. For the genuinely-solo clinic (no parent to re-provision from) replication
//! provides zero durability, so a backup is the only safety net. ADR-0026's insight is
//! that a backup is just another replication peer: the medium holds a NORMAL Cairn event
//! set (nothing backup-specific about it), and restore is set-union apply through the
//! existing verify-on-apply path. This module is the EXPORT side of that — it reads the
//! signed `node_event` set and writes it to a local medium — plus the medium's
//! **self-verification** and the **backup-health** surfacing (point 7: a node running
//! without a net must say so).
//!
//! SCOPE (slice B): export + self-verify + health. The medium's events are already
//! signed, so confidentiality at rest is the operator's encrypted-volume choice (we add
//! nothing backup-specific). The APPLY/restore-into-a-DB half is slice C: it is coupled
//! with the new-identity `supersede` ceremony (a restored node mints a fresh key — the
//! signing key is deliberately never backed up) and needs its own self-trusting restore
//! door, because the live `apply_remote_node_event` gate is the PEER-admission path and
//! would deny a node rehydrating its OWN history. Keeping that out of this slice keeps
//! the safety-critical surface here tiny: pure serialization + the existing signature
//! invariant, no new in-DB door.
//!
//! The self-verification reuses `cairn_event::verify_self_described`: a tampered or
//! bit-rotted medium event fails the SAME Ed25519/COSE signature check that catches a
//! malicious peer, so the backup is "self-verifying and tamper-evident by construction"
//! (ADR-0026 point 2) with no separate "is the backup intact?" mechanism.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Magic header identifying a Cairn backup medium (versioned; bump on a format change).
/// Distinct from the keystore's `CAIRNK1` so the two artifacts can never be confused.
const MEDIUM_MAGIC: &[u8] = b"CAIRNB1\n";

/// Upper bound on a single framed event on the medium. A signed node-event envelope is
/// a few hundred bytes; 8 MiB is generous headroom while capping a corrupt length prefix
/// so a bit-flip can never force a multi-GiB allocation during parse. Mirrors the wire
/// frame cap in `sync.rs`.
const MAX_EVENT_BYTES: usize = 8 * 1024 * 1024;

#[derive(thiserror::Error, Debug)]
pub enum BackupError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// The medium bytes are not a valid backup container (bad magic / truncated frame).
    #[error("decode: {0}")]
    Decode(String),
}

// ---------------------------------------------------------------------------
// Medium container format (pure — no I/O, trivially unit-testable).
//
// Layout: MAGIC ++ repeated [u32 big-endian length][signed_bytes]. No event count is
// stored: frames are read until a clean end-of-buffer, exactly like a peer stream's EOF.
// ---------------------------------------------------------------------------

/// Serialize a set of signed events into a backup-medium byte image. Pure.
/// The order is the caller's (we preserve insertion/`seq` order for legibility, but the
/// set is order-independent on restore — convergence is set-union by content-address).
pub fn serialize_medium(events: &[Vec<u8>]) -> Vec<u8> {
    let total: usize = events.iter().map(|e| 4 + e.len()).sum();
    let mut out = Vec::with_capacity(MEDIUM_MAGIC.len() + total);
    out.extend_from_slice(MEDIUM_MAGIC);
    for e in events {
        // The `as u32` length prefix mirrors `MAX_EVENT_BYTES`/`parse_medium`. Real signed
        // node-events are a few hundred bytes and the wire layer caps them well under 4 GiB,
        // so this is unreachable today — but assert the invariant so a future change that
        // lifts the upstream cap can never silently truncate a length prefix here.
        debug_assert!(
            e.len() <= MAX_EVENT_BYTES,
            "event of {} bytes exceeds the {MAX_EVENT_BYTES}-byte medium frame cap",
            e.len()
        );
        out.extend_from_slice(&(e.len() as u32).to_be_bytes());
        out.extend_from_slice(e);
    }
    out
}

/// Serialize an event set into a medium image and self-verify it in one step, returning the
/// verified bytes — or an error if the set fails its own signature check.
///
/// This runs BEFORE the image is written over the live medium (see [`backup_to`]). That
/// ordering is the load-bearing half of the "never over-claim" guarantee: a re-backup
/// commonly targets a fixed path, so if a serialization/signing regression produced a set
/// that fails verification we must reject it here — *before* the atomic rename — rather than
/// overwrite the previous good medium with an unrestorable one and only then notice.
pub fn serialize_and_verify(events: &[Vec<u8>]) -> Result<Vec<u8>, BackupError> {
    let image = serialize_medium(events);
    let report = verify_medium_bytes(&image)?;
    if !report.all_intact() {
        return Err(BackupError::Decode(format!(
            "refusing to write a medium that fails its own self-verification \
             ({} of {} events intact, first bad at index {:?})",
            report.intact, report.total, report.first_bad
        )));
    }
    Ok(image)
}

/// Parse a backup-medium byte image back into its signed events. Pure.
///
/// Errors (never panics) on a missing magic header or a truncated frame — a partially
/// written or bit-rotted medium is reported, not silently accepted. This is the
/// structural integrity check; the cryptographic one is [`verify_events`].
pub fn parse_medium(bytes: &[u8]) -> Result<Vec<Vec<u8>>, BackupError> {
    let mut rest = bytes
        .strip_prefix(MEDIUM_MAGIC)
        .ok_or_else(|| BackupError::Decode("missing CAIRNB1 magic header".into()))?;
    let mut events = Vec::new();
    while !rest.is_empty() {
        if rest.len() < 4 {
            return Err(BackupError::Decode(format!(
                "truncated medium: {} trailing byte(s) without a complete length prefix",
                rest.len()
            )));
        }
        let len = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
        if len > MAX_EVENT_BYTES {
            return Err(BackupError::Decode(format!(
                "medium frame length {len} exceeds {MAX_EVENT_BYTES}-byte cap (corrupt)"
            )));
        }
        let body_start = 4;
        let body_end = body_start + len;
        if rest.len() < body_end {
            return Err(BackupError::Decode(format!(
                "truncated medium: frame claims {len} bytes, only {} remain",
                rest.len() - body_start
            )));
        }
        events.push(rest[body_start..body_end].to_vec());
        rest = &rest[body_end..];
    }
    Ok(events)
}

// ---------------------------------------------------------------------------
// Self-verification (reuses the existing signature invariant — no DB, no new gate).
// ---------------------------------------------------------------------------

/// What a verification pass found. `intact` events verified their signature; `first_bad`
/// is the index of the first event that did NOT (so a caller can point at it). A medium
/// is sound iff every event is intact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    pub total: usize,
    pub intact: usize,
    pub first_bad: Option<usize>,
}

impl VerifyReport {
    /// Every event verified — the backup is restorable as-is.
    pub fn all_intact(&self) -> bool {
        self.first_bad.is_none() && self.intact == self.total
    }
}

/// Verify ONE signed event the way restore will: its self-described Ed25519 key must sign
/// the COSE body, and the body's claimed signer must match that key (forged-authorship
/// guard). A single flipped byte breaks the signature → `false`. Deterministic, no I/O.
pub fn verify_event(signed: &[u8]) -> bool {
    cairn_event::verify_self_described(signed).is_ok()
}

/// Verify every event in a parsed set. Deterministic; no DB and no external key needed —
/// a node can verify a backup medium with just the `cairn-node` binary.
pub fn verify_events(events: &[Vec<u8>]) -> VerifyReport {
    let mut intact = 0;
    let mut first_bad = None;
    for (i, e) in events.iter().enumerate() {
        if verify_event(e) {
            intact += 1;
        } else if first_bad.is_none() {
            first_bad = Some(i);
        }
    }
    VerifyReport { total: events.len(), intact, first_bad }
}

/// Parse a medium image and verify every event in one step. The two failure modes are
/// distinct: a `Decode` error means the container is structurally broken (can't even be
/// read as a Cairn event set); an `Ok(report)` with `!all_intact()` means it parsed but
/// carries a tampered/corrupt event.
pub fn verify_medium_bytes(bytes: &[u8]) -> Result<VerifyReport, BackupError> {
    Ok(verify_events(&parse_medium(bytes)?))
}

// ---------------------------------------------------------------------------
// Backup health (node-local operational state — NOT a clinical event, never signed,
// never replicated). Lives in a local sidecar JSON, not the DB: see the slice-B design
// note (smaller safety-critical surface — no SECURITY DEFINER door — and it fails SAFE,
// degrading to "never / running without a net" when absent or unreadable).
// ---------------------------------------------------------------------------

/// A record of the last successful backup. Written only AFTER the medium is durable and
/// self-verified, so it can never over-claim a backup the node does not actually hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupHealth {
    pub version: u8,
    /// Unix seconds at which the backup completed (operational wall-clock, not the HLC).
    pub last_backup_unix: i64,
    /// Where the medium was written (for the operator to locate it).
    pub medium_path: String,
    /// How many signed events the medium holds.
    pub event_count: u64,
    /// Size of the medium image in bytes.
    pub medium_bytes: u64,
}

/// The sidecar path for backup health: a sibling of the key file named
/// `backup-status.json`. Node-local, discoverable from what `status` already has (the
/// key path). Pure.
pub fn health_path_for(key_path: &Path) -> PathBuf {
    key_path.with_file_name("backup-status.json")
}

/// Render a coarse "time ago" for a `status` line. Pure. Negative input (a clock that
/// went backwards between backup and read) is reported as "just now" rather than a
/// nonsense negative age — honest degradation, never a confusing display.
pub fn humanize_ago(secs: i64) -> String {
    if secs <= 0 {
        return "just now".to_string();
    }
    let s = secs as u64;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86_400)
    }
}

/// The `status` backup-health line. Pure (time injected). Absent health → the honest
/// "running without a net" warning; present → freshness + size + location.
pub fn describe_health(now_unix: i64, health: &Option<BackupHealth>) -> String {
    match health {
        None => "never — running without a net".to_string(),
        Some(h) => format!(
            "{} ago ({} events, {} bytes -> {})",
            humanize_ago(now_unix - h.last_backup_unix),
            h.event_count,
            h.medium_bytes,
            h.medium_path,
        ),
    }
}

/// Read the backup-health sidecar. Returns `None` on ANY error (absent, unreadable,
/// malformed) — the fail-safe reading, so `status` degrades to "never / running without
/// a net" rather than asserting a freshness it cannot vouch for. A lying or missing
/// sidecar can only UNDER-claim; it can never cause data loss, because the load-bearing
/// guarantee lives in the self-verifying medium, not here.
pub fn read_health(path: &Path) -> Option<BackupHealth> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Atomically write the backup-health sidecar (owner-only). Atomic so a torn write can
/// never corrupt the freshness reading; the flip from old→new is a single rename.
pub fn write_health(path: &Path, health: &BackupHealth) -> Result<(), BackupError> {
    let json = serde_json::to_vec_pretty(health)
        .map_err(|e| BackupError::Decode(format!("serializing backup health: {e}")))?;
    crate::fsio::atomic_write(path, &json, Some(0o600))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// DB / IO glue (thin — the only DB-touching part of this module).
// ---------------------------------------------------------------------------

/// What one backup did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupReport {
    pub event_count: usize,
    pub medium_bytes: usize,
}

/// Read this node's signed `node_event` set, in local `seq` order. A plain `SELECT` —
/// any role with read access works (the runtime `cairn_node` role has `GRANT SELECT ON
/// node_event`); no signing key and no validated door are needed to back up.
pub async fn read_event_set(db: &tokio_postgres::Client) -> anyhow::Result<Vec<Vec<u8>>> {
    use anyhow::Context;
    let rows = db
        .query("SELECT signed_bytes FROM node_event ORDER BY seq", &[])
        .await
        .context("reading node_event set for backup")?;
    Ok(rows.iter().map(|r| r.get::<_, Vec<u8>>(0)).collect())
}

/// Back up the node's event set to `medium_path`, then record health at `health_path`.
///
/// Ordering is deliberately fail-safe and verify-BEFORE-write:
///   1. serialize the medium and self-verify the image IN MEMORY — if the set fails its
///      own signature check we BAIL before touching disk, so the previous good medium at
///      `medium_path` is left completely untouched (a re-backup over a fixed path can
///      never destroy a restorable medium with an unrestorable one);
///   2. write the verified image atomically (a crash here never destroys the previous good
///      medium either — the rename either lands whole or not at all);
///   3. re-read and self-verify the on-disk bytes — a defence-in-depth tripwire for an fs
///      bug between write and rename; still BAIL WITHOUT touching health if it fails;
///   4. only then update the health sidecar.
///
/// A crash between (3) and (4) leaves health UNDER-reporting (older / "never"), which is
/// the correct direction for a safety-net indicator — it must never over-claim.
///
/// `now_unix` is injected (operational wall-clock) so the function stays deterministic
/// and testable; the CLI passes `SystemTime::now()`.
pub async fn backup_to(
    db: &tokio_postgres::Client,
    medium_path: &Path,
    health_path: &Path,
    now_unix: i64,
) -> anyhow::Result<BackupReport> {
    use anyhow::Context;
    let events = read_event_set(db).await?;

    // Verify the image BEFORE it can overwrite the live medium: a set that fails its own
    // signature check is rejected here, with the previous good medium still intact on disk.
    let medium = serialize_and_verify(&events)
        .context("self-verifying the backup image before writing (previous medium untouched)")?;

    crate::fsio::atomic_write(medium_path, &medium, Some(0o600))
        .with_context(|| format!("writing backup medium to {}", medium_path.display()))?;

    // Read-after-write: the on-disk bytes must still parse AND verify (catches an fs/rename
    // bug), or we refuse to advance health (never tell the operator a broken backup is good).
    let readback = std::fs::read(medium_path)
        .with_context(|| format!("re-reading backup medium {}", medium_path.display()))?;
    let report = verify_medium_bytes(&readback)
        .with_context(|| format!("verifying freshly-written medium {}", medium_path.display()))?;
    if !report.all_intact() {
        anyhow::bail!(
            "backup medium failed self-verification after write ({} of {} events intact, \
             first bad at index {:?}); health NOT advanced",
            report.intact,
            report.total,
            report.first_bad
        );
    }

    let health = BackupHealth {
        version: 1,
        last_backup_unix: now_unix,
        medium_path: medium_path.display().to_string(),
        event_count: events.len() as u64,
        medium_bytes: medium.len() as u64,
    };
    write_health(health_path, &health).context("writing backup-health sidecar")?;

    Ok(BackupReport { event_count: events.len(), medium_bytes: medium.len() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_event::{sign, EventBody, Hlc, SigningKey};
    use tempfile::tempdir;

    /// Mint a real, validly-signed event — no DB needed. This is the same signed-envelope
    /// shape `cairn_event` produces everywhere, so `verify_self_described` exercises the
    /// genuine signature path a tampered medium would fail.
    fn synth_event(sk: &SigningKey, n: u64) -> Vec<u8> {
        let kid = hex::encode(sk.verifying_key().to_bytes());
        let body = EventBody {
            event_id: format!("event-{n}"),
            patient_id: "00000000-0000-0000-0000-000000000000".into(),
            event_type: "note.added".into(),
            schema_version: "0".into(),
            hlc: Hlc { wall: n as i64, counter: 0, node_origin: "test".into() },
            t_effective: None,
            signer_key_id: kid,
            contributors: serde_json::json!([]),
            payload: serde_json::json!({ "n": n }),
            attachments: vec![],
        };
        sign(&body, sk).unwrap().signed_bytes
    }

    fn sk() -> SigningKey {
        cairn_event::generate_key().unwrap().0
    }

    #[test]
    fn medium_roundtrips_an_event_set() {
        let k = sk();
        let events: Vec<Vec<u8>> = (0..5).map(|n| synth_event(&k, n)).collect();
        let image = serialize_medium(&events);
        assert!(image.starts_with(MEDIUM_MAGIC), "medium must carry the CAIRNB1 magic");
        assert_eq!(parse_medium(&image).unwrap(), events, "parse must recover the exact set");
    }

    #[test]
    fn empty_medium_roundtrips() {
        // A node backing up before genesis (no events) is unusual but valid; it must not
        // error — an empty set serializes to just the magic and parses back to empty.
        let image = serialize_medium(&[]);
        assert_eq!(image, MEDIUM_MAGIC);
        assert!(parse_medium(&image).unwrap().is_empty());
    }

    #[test]
    fn parse_rejects_missing_magic() {
        assert!(matches!(parse_medium(b"not a medium"), Err(BackupError::Decode(_))));
        assert!(parse_medium(&[]).is_err(), "an empty file has no magic and is not a medium");
    }

    #[test]
    fn parse_rejects_a_truncated_frame() {
        let k = sk();
        let mut image = serialize_medium(&[synth_event(&k, 1)]);
        // Lop off the last byte: the final frame now claims more bytes than remain.
        image.pop();
        assert!(matches!(parse_medium(&image), Err(BackupError::Decode(_))));
    }

    #[test]
    fn parse_rejects_a_dangling_length_prefix() {
        // Magic followed by 2 stray bytes — not enough for even a 4-byte length prefix.
        let mut image = MEDIUM_MAGIC.to_vec();
        image.extend_from_slice(&[0u8, 0u8]);
        assert!(matches!(parse_medium(&image), Err(BackupError::Decode(_))));
    }

    #[test]
    fn verify_passes_for_an_intact_set_and_pinpoints_a_tampered_event() {
        let k = sk();
        let mut events: Vec<Vec<u8>> = (0..4).map(|n| synth_event(&k, n)).collect();
        assert!(verify_events(&events).all_intact(), "an untouched set must fully verify");

        // Flip a byte in the body of event index 2: its Ed25519 signature must now fail.
        let mid = events[2].len() / 2;
        events[2][mid] ^= 0x01;
        let report = verify_events(&events);
        assert!(!report.all_intact(), "a tampered event must break verification");
        assert_eq!(report.first_bad, Some(2), "the report must point at the tampered event");
        assert_eq!(report.intact, 3, "the other three events still verify");
    }

    #[test]
    fn verify_medium_bytes_catches_tamper_through_the_container() {
        let k = sk();
        let events: Vec<Vec<u8>> = (0..3).map(|n| synth_event(&k, n)).collect();
        let mut image = serialize_medium(&events);
        assert!(verify_medium_bytes(&image).unwrap().all_intact());
        // Corrupt a byte somewhere inside the framed body region (after the magic).
        let idx = MEDIUM_MAGIC.len() + 12;
        image[idx] ^= 0xff;
        // It still parses structurally (length prefixes intact) but fails verification.
        let report = verify_medium_bytes(&image).unwrap();
        assert!(!report.all_intact(), "a content bit-flip must fail the signature check");
    }

    #[test]
    fn serialize_and_verify_returns_a_verified_image_for_an_intact_set() {
        let k = sk();
        let events: Vec<Vec<u8>> = (0..3).map(|n| synth_event(&k, n)).collect();
        let image = serialize_and_verify(&events).expect("an intact set must serialize + verify");
        assert!(image.starts_with(MEDIUM_MAGIC));
        assert_eq!(parse_medium(&image).unwrap(), events, "the returned bytes are the medium image");
    }

    #[test]
    fn serialize_and_verify_refuses_a_set_with_a_tampered_event() {
        // The load-bearing guarantee behind verify-BEFORE-write: a set that fails its OWN
        // signature check is rejected before any bytes reach disk, so a re-backup over a
        // fixed medium path can never overwrite the previous good medium with an
        // unrestorable one.
        let k = sk();
        let mut events: Vec<Vec<u8>> = (0..3).map(|n| synth_event(&k, n)).collect();
        let mid = events[1].len() / 2;
        events[1][mid] ^= 0xff;
        assert!(
            matches!(serialize_and_verify(&events), Err(BackupError::Decode(_))),
            "a set that fails self-verification must be refused, not serialized to a medium"
        );
    }

    #[test]
    fn humanize_ago_buckets_are_coarse_and_safe() {
        assert_eq!(humanize_ago(-5), "just now", "a backwards clock must not show a negative age");
        assert_eq!(humanize_ago(0), "just now");
        assert_eq!(humanize_ago(42), "42s");
        assert_eq!(humanize_ago(120), "2m");
        assert_eq!(humanize_ago(7200), "2h");
        assert_eq!(humanize_ago(172_800), "2d");
    }

    #[test]
    fn describe_health_warns_when_absent_and_summarizes_when_present() {
        assert_eq!(describe_health(1000, &None), "never — running without a net");
        let h = BackupHealth {
            version: 1,
            last_backup_unix: 1000,
            medium_path: "/mnt/backup/cairn.medium".into(),
            event_count: 7,
            medium_bytes: 2048,
        };
        let line = describe_health(1000 + 3600, &Some(h));
        assert!(line.starts_with("1h ago"), "freshness first: got {line:?}");
        assert!(line.contains("7 events"));
        assert!(line.contains("/mnt/backup/cairn.medium"));
    }

    #[test]
    fn health_sidecar_roundtrips_and_absent_reads_as_none() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("backup-status.json");
        assert_eq!(read_health(&p), None, "a missing sidecar reads as None (fail-safe)");
        let h = BackupHealth {
            version: 1,
            last_backup_unix: 12_345,
            medium_path: "/mnt/x".into(),
            event_count: 3,
            medium_bytes: 999,
        };
        write_health(&p, &h).unwrap();
        assert_eq!(read_health(&p), Some(h), "a written sidecar reads back exactly");
    }

    #[test]
    fn malformed_sidecar_reads_as_none_not_a_crash() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("backup-status.json");
        std::fs::write(&p, b"{ this is not valid json").unwrap();
        assert_eq!(read_health(&p), None, "a corrupt sidecar must fail safe to None");
    }

    #[test]
    fn health_path_is_a_sibling_of_the_key() {
        let p = health_path_for(Path::new("/var/lib/cairn/node.key"));
        assert_eq!(p, Path::new("/var/lib/cairn/backup-status.json"));
    }
}
