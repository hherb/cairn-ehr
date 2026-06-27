# Design — Patient-identifier representation (demographics §4, gap B)

**Date:** 2026-06-27 · **Status:** design approved, pre-implementation · **Scope:**
demographics §4 **patient**-identifier *representation* only — plus a boundary paragraph
fixing professional/provider identifiers in the §7.5 actor registry. The provider-number
person×org relational model is explicitly **deferred**.

## Problem

Demographics §4.2 already settles the *projection policy* for patient identifiers
(national ID, insurance, program IDs): a multi-valued set **keyed by issuing system**,
**set union, never LWW**, with *same-system different-value = very strong evidence against
link* (a hard veto). What §4 has **not** specified is **representation**: what an issuing
system *is* as a globally-meaningful key, how a value is validated, and how the hard veto
behaves when a node lacks the issuing system's validator. This is the identifier analogue
of the gap ADR-0032 just closed for addresses — and the HANDOVER names it as demographics
**gap B**.

The divergence from the address case is sharp and load-bearing: **address matching is
advisory/weak; identifier matching carries a hard veto.** A naive port of the ADR-0032
address `profile` (one content-addressed `namespace@hash` that is both system identity and
validator) would mean two nodes on different validator *versions* of the same logical
system (different hash) fail to recognise "same system", mis-firing or silently weakening
the veto. And a node lacking the validator could read two formatting variants of one
number as a *mismatch* and wrongly demote a good link to *under-review* — a paper-parity
regression caused by a space character.

## Requirements (agreed)

For every patient identifier, regardless of issuing system or which nodes hold which
validators:

1. **The as-entered value is evidence and is never destroyed** (principle 1 / principle 11
   legibility analogue) — maiden insurance numbers, old transcriptions, mis-spellings all
   stay as matching evidence.
2. **"Same system" is determinable globally** without a central registry and **independently
   of validator version**, so the hard veto keys reliably across nodes and locales.
3. **Validation is advisory** (principle 4) — a bad check digit *flags*, never *rejects*; no
   required identifier field is satisfiable only by fabrication.
4. **The hard veto survives honest degradation** — a node never fires a hard veto (or demotes
   a link) on a basis it cannot trust; absence of a validator withholds the veto, never
   invents a mismatch.
5. **The in-DB floor stays culture-neutral** (principle 12) — it enforces only structural
   invariants, never holds a profile, never runs a checksum, never rejects on validation.

Explicitly **out of scope** (deferred): the provider-number person×org relational model;
validity/expiry semantics (already covered by the assertion stream + supersession).

## Terminology guard

ADR-0031 owns **"canonical identifier"** = the system's own UUIDv7 + multihash. A patient's
**external** identifier (NHS number, Medicare number) is a different thing. To avoid
collision this design says **"normalized form"** for the matching key, never "canonical",
and the ADR calls out the distinction explicitly.

## The value model (§4.4)

An identifier is a §4.1 assertion whose value has these facets:

- **`value`** *(mandatory)* — the as-entered identifier string. The evidence/legibility
  facet: always sufficient alone, **never destroyed or rewritten**. (Principle 1.)
- **`system`** *(mandatory)* — the **stable content-addressed namespace** the hard veto keys
  on (`nhs-number`, `medicare-au`). Globally meaningful with no central registry. May be an
  explicit **`unknown`** sentinel (a number copied off an unrecognised card — principle 4:
  still recordable; yields *weak* evidence only, never a veto).
- **`normalized`** *(optional, materialised at authoring when the profile is present)* — the
  profile's normalized form of `value` (separators stripped, case/grouping canonicalised).
  **This is the matching key.** Materialising it into the signed event at author time is the
  identifier analogue of ADR-0032 materialising `display`: a node lacking the profile *code*
  can still veto-match correctly on the normalized form instead of mis-firing on formatting
  noise.
- **`profile`** *(optional)* — `namespace@hash`, the **versioned validator bundle** (format +
  checksum + normalizer + matching comparator), riding the §7.6 distribution plane. Needed to
  *re-derive/verify* `normalized` and to run advisory validation. **Evolves independently of
  `system`** — this is the namespace/profile split that lets the veto key on stable identity
  while validators version freely.
- **`use`/`type`** *(optional)* — a **recommended-but-open** vocabulary (`national-id`,
  `insurance`, `program`, `mrn`): recommended so the common case interoperates, open so it
  cannot become capture.

## Matching & honest degradation (the safety-critical rule)

**Same-system mismatch veto.** Per demographics §4.2 / identity §5.2, *same `system`, different value* is
very strong evidence against link — a hard veto that **forces a human decision, never an
auto-link and never an auto-reject** (an auto-reject is itself a silent false split).
"Different value" means **different `normalized` forms**, not different `value` strings —
so `9434765919` and `943 476 5919` are the **same** identifier and raise **no** veto.

**Honest degradation — load-bearing.** A node decides "same vs different value" only on a
basis it can trust:

- Both assertions carry a materialised **`normalized`** form → it compares those. **Works
  even without the profile code.** Normal path; the reason we materialise.
- A `normalized` form is **absent** and this node **also lacks the profile** to derive one →
  it **may treat string-equal as a positive signal, but must NOT declare a same-system
  *mismatch* from string inequality** (the difference may be pure formatting). It **holds for
  human review** instead of firing the veto or demoting an existing link.

This is the identifier analogue of ADR-0032's *"never reinterpret `parts` under a substitute
profile"*: **a node never fires a hard veto on a basis it cannot trust.** Without it a
profile-less node flips a good link into *under-review* trust mode over a space character.

**`system: unknown`** never participates in the veto — weak positive evidence only.

## Validation & the in-DB floor

- **Advisory validation.** The profile's validator (checksum, length, format) **flags for
  human review, never rejects** (principle 4). A failed check is a flag on the assertion,
  surfaced to a human, never a write barrier.
- **Floor invariants only** (culture-neutral, principle 12): `value` non-empty text; `system`
  present (possibly `unknown`); `normalized` is text when present; **`normalized` materialised
  ⇒ `profile` named** (so its provenance is known and it is re-derivable). The floor never
  holds a profile, never runs a checksum, never rejects on validation.
- **Cross-facet verification is advisory.** A profile-holding node may re-derive `normalized`
  from `value` and flag drift (`normalized == normalizer(value)`), exactly as ADR-0032 treats
  `display == formatter(parts)` — advisory, never a floor gate.

## The professional-ID boundary (the conflation guard)

Patient identifiers answer *"who is this patient?"* and live in demographics §4 (subject =
the patient as a demographic record). **Professional/provider identifiers** — AHPRA/GMC/NPI
registration numbers, billing provider numbers — answer *"who is licensed / who may sign /
who bills?"* and belong to the **§7.5 actor registry** (subject = the clinician as an
*actor*). They are **never conflated**:

- A person who is *both* patient and clinician carries their national-ID/insurance in
  demographics **and** their registration number in the actor registry — two streams, two
  questions, two subjects. Conflating them would let a billing number act as a patient match
  key, or a patient ID act as a signing credential — both are corruption.
- A **provider number is relational** (the same clinician holds different numbers per
  practice/location → scoped to person×org), unlike a patient identifier which is a property
  of the person alone. That relational model is **explicitly deferred** (gap B follow-on);
  this change only draws the line and states the **non-conflation invariant**.

## Spec changes

- **New §4.4 "Identifiers: representation"** in `demographics.md` (§4.3 is address): the facet
  model, matching/degradation rule, advisory validation, floor invariants, the boundary
  paragraph.
- **Update the §4.2 table Identifiers row** to cross-ref §4.4 (projection policy unchanged;
  representation now specified).
- **Cross-ref from identity §5.2** (where the hard veto / coherence check live) to the §4.4
  degradation rule.
- **New ADR-0033** — patient-identifier representation: namespace/profile split + materialised
  normalized form + veto-survives-degradation. Noted as **refining ADR-0014** (reuses the
  locale/profile bundle + §7.6 distribution plane) and **adjacent to but distinct from
  ADR-0031** (external identifier ≠ the system's canonical UUID — called out explicitly).
- **Bump spec 0.33 → 0.34** in `index.md`.
- **HANDOVER/ROADMAP** currency: gap B's representation half closed; provider-number
  person×org model remains the named follow-on; gap C (demographic legibility twin) unchanged.

## Why no new founding principle

Like ADR-0032, this is an application of existing principles (1 evidence-preservation, 4
advisory-uncertainty, 11 legibility, 12 culture-neutral floor) and reuses ADR-0014's
profile/distribution-plane machinery. The one genuinely new mechanism — materialising the
`normalized` form so the hard veto survives a profile-less node — is a *safety-preserving*
refinement of an existing veto, not a new architectural axis. No new founding principle.

## Testability

The spec change is prose, but each rule is stated as a checkable invariant for when the
floor + matcher are implemented:
- Floor: rejects empty `value`; rejects `normalized` present without `profile`; accepts
  `system: unknown`; never rejects on a bad checksum.
- Matcher: byte-different but normalized-equal values raise no veto; normalized-different
  values raise the veto (human decision, not auto-reject); a profile-less node with no
  materialised `normalized` holds for review rather than declaring mismatch; `unknown` never
  vetoes.
