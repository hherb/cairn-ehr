//! Identity linkage assertion builders (spec §5.1/§5.7 — matcher piece C1). Pure:
//! explicit inputs, no I/O, no DB. The safety-critical structural floor and the
//! connected-component projection live in the database (db/018); these functions
//! only shape and render the event a node will sign. Mirrors `demographics.rs`.

use serde_json::{json, Value};

/// One §5.7 link/unlink assertion between two immortal patient UUIDs. `subject_a`
/// and `subject_b` are the two UUIDs whose linkage is asserted; the event_type
/// (link vs unlink) — not the payload — carries the direction. The in-DB floor
/// (db/018) rejects a self-link (a == b) and an empty provenance.
pub struct LinkAssertion<'a> {
    pub subject_a: &'a str,  // §5.7 — one immortal subject UUID (string form)
    pub subject_b: &'a str,  // §5.7 — the other immortal subject UUID
    pub provenance: &'a str, // §4.1 provenance ladder — required-present, value-open
    pub confidence: Option<&'a str>, // acknowledged uncertainty (principle 4); omitted when None
}

/// Shared payload shape for link and unlink (identical; the event_type distinguishes
/// them). `confidence` is omitted entirely when absent — never serialized as null —
/// so the in-DB floor's key-presence checks see exactly what the author asserted.
fn assertion_body(a: &LinkAssertion) -> Value {
    let mut p = json!({
        "subject_a": a.subject_a,
        "subject_b": a.subject_b,
        "provenance": a.provenance,
    });
    if let Some(c) = a.confidence {
        p.as_object_mut()
            .expect("json! built an object")
            .insert("confidence".into(), json!(c));
    }
    p
}

/// Build the `identity.link.asserted` payload (the value of `EventBody.payload`).
pub fn link_assertion_body(a: &LinkAssertion) -> Value {
    assertion_body(a)
}

/// Build the `identity.unlink.asserted` payload — same shape as a link.
pub fn unlink_assertion_body(a: &LinkAssertion) -> Value {
    assertion_body(a)
}

/// Render the §4.5-style legibility twin for a link: profile-independent plaintext.
pub fn render_link_twin(a: &LinkAssertion) -> String {
    format!("link: {} ↔ {} ({})", a.subject_a, a.subject_b, a.provenance)
}

/// Render the §4.5-style legibility twin for an unlink.
pub fn render_unlink_twin(a: &LinkAssertion) -> String {
    format!("unlink: {} ↔ {} ({})", a.subject_a, a.subject_b, a.provenance)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LinkAssertion<'static> {
        LinkAssertion {
            subject_a: "aaaaaaaa-0000-0000-0000-000000000001",
            subject_b: "bbbbbbbb-0000-0000-0000-000000000002",
            provenance: "matcher:cfg@hash",
            confidence: None,
        }
    }

    #[test]
    fn body_has_subjects_and_provenance() {
        let b = link_assertion_body(&sample());
        assert_eq!(b["subject_a"], "aaaaaaaa-0000-0000-0000-000000000001");
        assert_eq!(b["subject_b"], "bbbbbbbb-0000-0000-0000-000000000002");
        assert_eq!(b["provenance"], "matcher:cfg@hash");
    }

    #[test]
    fn confidence_omitted_when_absent_never_null() {
        let b = link_assertion_body(&sample());
        assert!(
            b.get("confidence").is_none(),
            "confidence must be omitted entirely when absent, never serialized as null"
        );
    }

    #[test]
    fn confidence_present_when_given() {
        let a = LinkAssertion { confidence: Some("0.91"), ..sample() };
        let b = link_assertion_body(&a);
        assert_eq!(b["confidence"], "0.91");
    }

    #[test]
    fn link_and_unlink_bodies_are_identical() {
        assert_eq!(link_assertion_body(&sample()), unlink_assertion_body(&sample()));
    }

    #[test]
    fn twins_distinguish_link_from_unlink() {
        assert!(render_link_twin(&sample()).starts_with("link: "));
        assert!(render_unlink_twin(&sample()).starts_with("unlink: "));
        assert!(render_link_twin(&sample()).contains("matcher:cfg@hash"));
    }
}
