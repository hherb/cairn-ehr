//! At-rest key sealing for cairn-node (ADR-0026 slice A).
//!
//! WHY THIS EXISTS: a node's Ed25519 signing key must survive on disk without being
//! readable by anyone who copies the file, and must be recoverable off-node after a
//! lost passphrase or a dead disk. This module is the small safety-critical surface
//! ADR-0026 names: pure functions (entropy aside) that seal a 32-byte seed under TWO
//! independent secrets — an operational passphrase (daily, unattended `run`) and a
//! one-time recovery code (paper escrow). A defect here is silent key loss or a
//! forged identity, so it is exhaustively unit-tested and kept reviewer-legible.

/// Crockford base32 alphabet (excludes I, L, O, U to avoid transcription errors).
const B32: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Encode bytes as Crockford base32 (no padding). Pure. Used to render the
/// 160-bit recovery code as a human-transcribable string.
pub fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for &b in bytes {
        buf = (buf << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32[((buf >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(B32[((buf << (5 - bits)) & 0x1f) as usize] as char);
    }
    out
}

/// Decode Crockford base32; `None` on any character outside the alphabet.
/// Input must already be normalized (uppercase, no separators).
pub fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let (mut buf, mut bits) = (0u32, 0u32);
    let mut out = Vec::new();
    for c in s.chars() {
        let idx = B32.iter().position(|&a| a as char == c)? as u32;
        buf = (buf << 5) | idx;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

/// Canonical form of a recovery code for KDF input: uppercase, keep only
/// alphabet characters (drops grouping dashes/spaces and lowercases). This lets a
/// human re-type the code with any spacing/case and still unseal.
pub fn normalize_recovery_code(s: &str) -> String {
    s.to_ascii_uppercase()
        .chars()
        // Guard on `is_ascii()` BEFORE the `*c as u8` cast: that cast truncates a
        // multi-byte codepoint to its low 8 bits (e.g. 'Ł' U+0141 -> 0x41 'A'),
        // which would otherwise smuggle non-alphabet input past the filter and
        // corrupt the KDF input. ASCII-only is the real contract here.
        .filter(|c| c.is_ascii() && B32.contains(&(*c as u8)))
        .collect()
}

/// Generate a fresh 160-bit recovery code, grouped in 5-char blocks for legibility,
/// e.g. `AB12C-D34EF-...`. Shown ONCE at provisioning; the off-node escrow.
pub fn generate_recovery_code() -> String {
    let mut raw = [0u8; 20];
    getrandom::fill(&mut raw).expect("entropy source unavailable");
    let flat = base32_encode(&raw);
    flat.as_bytes()
        .chunks(5)
        // `unwrap()` is safe: `flat` is built from B32, which is ASCII-only, so
        // every byte (and thus every 5-byte chunk) is valid UTF-8 by construction.
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_roundtrips_arbitrary_bytes() {
        for v in [vec![], vec![0u8], vec![0xff; 20], (0u8..=255).collect::<Vec<_>>()] {
            let enc = base32_encode(&v);
            assert_eq!(base32_decode(&enc).as_deref(), Some(v.as_slice()),
                       "roundtrip failed for {} bytes", v.len());
        }
    }

    #[test]
    fn base32_rejects_invalid_chars() {
        // 'I','L','O','U' are excluded from Crockford base32; a literal '!' is invalid.
        assert!(base32_decode("!!!!").is_none());
        // The Crockford-excluded letters are the real transcription-error case:
        // a human reading 'I'/'L'/'O'/'U' must NOT silently decode to something.
        assert!(base32_decode("IIII").is_none());
        assert!(base32_decode("LLLL").is_none());
        assert!(base32_decode("OOOO").is_none());
        assert!(base32_decode("UUUU").is_none());
    }

    #[test]
    fn normalize_strips_grouping_and_case() {
        assert_eq!(normalize_recovery_code("ab cde-fghjk"), "ABCDEFGHJK");
    }

    #[test]
    fn recovery_code_is_160_bit_and_unique() {
        let a = generate_recovery_code();
        let b = generate_recovery_code();
        assert_ne!(a, b, "two codes must differ (entropy smoke test)");
        // Decodes to exactly 20 bytes (160 bits).
        assert_eq!(base32_decode(&normalize_recovery_code(&a)).map(|v| v.len()), Some(20));
    }
}
