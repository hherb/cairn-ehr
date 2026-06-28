# ADR-0036 — Demographic name display: recency-first within the legal tier

- **Status:** Accepted
- **Date:** 2026-06-28
- **Refines:** [§4.2](../demographics.md#42-per-field-projection-policy), [ADR-0014](0014-locale-pluggable-matcher-comparators.md)

## Context

[§4.2](../demographics.md#42-per-field-projection-policy) originally described the names display
rule as *"display = highest-provenance recent legal name"* — provenance-first, the same ordering as
DOB. This is correct for DOB: a date of birth is a stable biological fact, and a document-verified
DOB should lock against a later patient-stated one because the document is more reliable evidence of
the same unchanging fact.

Names are a **fundamentally different kind of field.** A name is not a stable fact to be recovered
with increasing precision — it is a **legitimately changing identifier** that a patient may update
for entirely valid reasons: marriage, divorce, gender transition, cultural reclamation, or simply
preferring to go by a different legal name. In any of these cases, a *more recent* patient-stated
legal name reflects reality more accurately than an *older* document-verified one.

The provenance-first rule applied to names produces two distinct failure modes:

1. **Stale married-name lock.** A patient divorces, legally reverts to their birth name, and states
   it at registration. A document-verified assertion of the old married name from a previous visit
   outranks the patient-stated assertion and remains as the displayed name — the patient is
   addressed by a name they have rejected.

2. **Deadname lock.** A transgender patient changes their legal name and presents their current
   legal identity. An older document-verified assertion of their previous name outranks the new
   patient-stated one. The patient is deadnamed — a dignity failure and a safety failure: care
   providers who call a patient by the wrong name signal that the chart cannot be trusted, and can
   deter future care-seeking. Paper-parity ([principle 3](../index.md#founding-principles-the-lens-for-every-decision))
   is unambiguous here: on a paper chart you call the patient by the name they give you today.

The deadname case also illustrates why the failure is not merely cosmetic. A patient who is
systematically addressed by the wrong name at a clinic may avoid care for conditions that require
that clinic, or may give up correcting the record. The displayed name is a clinical safety signal.

This divergence from DOB is **intentional by design**, not an inconsistency. DOB and names have
different semantics under change: a corrected DOB claims the original was wrong (provenance is
evidence of the *same* underlying fact); a new legal name does not claim the old name was wrong —
it claims the patient's **identity has changed** (provenance is evidence of a *different* moment in
a legitimately moving field). The display rule must reflect this semantic difference.

## Decision

The names display-winner rule is **recency-first within the legal-use tier**, with
provenance/origin breaking ties among same-HLC assertions; when no legal name exists, it **falls
back to the most-recent name of any `use`** (the unidentified-patient, alias-only, or
transliteration-only cases).

Concretely:

1. **Primary tier:** among all retained name assertions with `use = legal`, the one with the
   highest HLC wall-clock timestamp is the display winner; provenance and origin break ties among
   exact-same-HLC entries.
2. **Fallback tier:** if no legal-use name exists, the most-recent name of any `use` is displayed.
3. **All names are retained** in the assertion log regardless of which wins display. An older
   document-verified name is not displaced from the record — it remains as evidence, available for
   matching ([§5.2](../identity.md#52-matching-pipeline-safety-asymmetric-false-merge-worse-than-false-split))
   and for audit. Retention is what the append-only model ([principle 1](../index.md#founding-principles-the-lens-for-every-decision))
   guarantees; display is a projection, not a deletion.
4. **Provenance still feeds the matcher.** The [§4.1](../demographics.md#41-demographic-assertions)
   provenance ladder and the [ADR-0014](0014-locale-pluggable-matcher-comparators.md) comparator-profile
   machinery apply to the *full retained set* when the matcher runs — a document-verified name is
   stronger matching evidence than a patient-stated one, even when it does not win display.
5. **The displayed name is the legal-preferred reference point.** Surfacing a patient's
   preferred or chosen name (a "known-as" or "a.k.a." alongside the legal name, common for patients
   who go by a chosen name not yet on their legal documents) is **UI soft-policy above the floor**
   ([ADR-0021](0021-layering-the-node-api-and-ui-pluralism.md), [principle 12](../index.md#founding-principles-the-lens-for-every-decision)).
   The floor projects a display-winner; the UI reads the same retained set and may surface an
   additional preferred/chosen name. This is not a floor concern — the floor has no "preferred" tier
   yet, and soft-policy is the right location for nuanced presentation choices that legitimately vary
   by clinic and jurisdiction.

**Do not revert to provenance-first for names.** If a future session encounters the rule and is
tempted to align it with DOB for consistency, read this ADR first. The asymmetry is load-bearing:
DOB is a stable fact (provenance = reliability of evidence for the same fact); a name is a
legitimately moving identifier (provenance = reliability of a past assertion, not of the current
truth). Making them symmetric would reintroduce the deadname failure mode.

## Consequences

- **Easier:** a patient's current legal name always appears in their chart as the display name, even
  when only patient-stated, because they changed their name after their last document-verified
  registration. Clinicians calling a patient by name use the name the patient has told them to use —
  paper-parity in the most direct sense.
- **Evidence preserved:** old names are retained in `event_log` and in the `patient_name` retained-set
  projection, visible to the matcher and to any auditor. The display rule change is purely a
  projection policy — nothing is erased.
- **Matcher unaffected:** the §5.2 matching pipeline reads the full retained set with provenance
  weights. A document-verified name remains strong matching evidence whether or not it wins display.
- **UI "a.k.a." seam is deliberate:** the floor cannot know what constitutes a "preferred" name in
  every clinic context. Clinics that wish to display both a legal name and a chosen name surface the
  chosen `use` from the same retained set as a secondary line — a soft-policy UI decision sitting
  correctly above the floor.
- **The bet:** recency-first means a single patient-stated assertion late in the record can displace
  an older document-verified one. The downside risk is a clerical error (wrong name entered) winning
  display. Mitigation: the old name is retained and visible; a corrected assertion supersedes the
  error; the threshold for a dignity or safety failure from a stale *document-verified* name far
  exceeds the threshold for a dignity or safety failure from a temporary *clerical error* name — the
  latter is caught and corrected, the former silently persists.
- **How we'd know the bet fails:** systematic reports of patients being addressed by wrong names
  where the error is a recent clerical input that displaced a correct document-verified name (not the
  reverse). This pattern would motivate a UI-layer review flag (flag for human review when a
  lower-provenance assertion displaces a higher-provenance one — still soft policy, never a floor
  gate). The floor itself does not change: the record retains both; the projection winner changes.
