# ADR-0019 — Author-scoped record export: the clinician's medico-legal copy

- **Status:** Accepted
- **Date:** 2026-06-16
- **Refines:** ADR-0007

## Context

A clinician's records are frequently their **sole defence** in litigation that may arrive *decades* later —
the canonical case (raised by the clinician-architect) is being sued twenty years on over an infant
encounter that may or may not have occurred as alleged, where only the contemporaneous record answers it. The
absolute risk of record loss at any one workplace is small, but it **compounds with the number of
workplaces** — the roaming locum / portfolio career accumulates exposure across many independent custodians,
each with its own (sometimes poor) preservation and culling practices. The clinician therefore has a
legitimate durability interest in **the records they themselves generated**, independent of any single
employer's custodianship.

Most jurisdictions permit a clinician to retain a private copy of their own records; some require such a copy
to be **encrypted under an authority-generated public key**; the details are policy and vary. What is *not*
optional is that the architecture must **facilitate the option** — and must do so without becoming a privacy
hole or an unaudited bulk-egress channel. This is paper-parity ([principle 3](../index.md#founding-principles-the-lens-for-every-decision):
a paper clinician keeps their own notes), data-sovereignty extended from the patient to the **author**, and
the [ADR-0005](0005-erasure-key-custody-and-crypto-shredding.md) honest-erasure ceiling already *assumed* this
mechanism exists (rung-2's *"clinician's cover migrates to their own retained sealed copy"*).

## Decision

Author-scoped export **dissolves into existing primitives** — a refinement of authorship
([ADR-0007](0007-authorship-and-accountability.md)) composing the [§7.1](../security.md#71-erasure-the-severity-ladder)
key-custody ladder, the signed-event/legibility-twin guarantees, and the audit stream. **No new founding
principle.** Canonical home: [security §7.8](../security.md#78-author-scoped-record-export-the-medico-legal-copy);
authorship model: [data-model §3.9](../data-model.md#39-authorship-and-accountability).

1. **A first-class, audited export of *what the clinician authored*.** *"Records I generated"* is a
   projection over the append-only log selected by **contributor identity** — well-defined because every event
   names its contributor set ([§3.9](../data-model.md#39-authorship-and-accountability)). Cairn provides the
   selection + packaging as a first-class operation.

2. **The bound is strictly the clinician's own authorship — regardless of results** (the clinician-architect's
   ruling). The export carries what they authored: **progress notes, pathology and imaging *requests*,
   referrals** — the **clinical reasoning and actioning**, which is what ordinarily suffices as a defence. It
   does **not** pull in others' content or the *results* those requests produced: a result is authored by the
   lab/radiologist, not the clinician, so it falls outside the scope. This is the load-bearing safety property
   — **author-scoped, never patient-scoped** — so the confidentiality blast radius is exactly the author's own
   contributor-set and nothing more. Where statute obliges the *clinic/practice* to retain results and the
   like, that is a **separable custodianship duty**: the clinician can enforce delivery or litigate the
   practice's failure to keep custody — out of Cairn's mechanism (Cairn records the chain;
   [§7.7](../security.md#77-federation-admission-peering-trust-anchors-and-the-custodian-contract) is where one
   custodian's obligations live).

3. **The exported artifact is self-verifying and legible across time — court-admissible by construction.**
   Because events are signed and signing publics are immortal ([§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody),
   [ADR-0015](0015-event-serialization-signatures-and-content-addressing.md)), a bundle exported today still
   **proves origin and integrity twenty years on** — tamper-evident without trusting whoever stored it. The
   mandatory **plaintext legibility twin** ([principle 11](../index.md#founding-principles-the-lens-for-every-decision),
   [data-model §3.13](../data-model.md#313-schema-evolution-event-format-and-the-legibility-twin)) keeps it
   human-readable regardless of how far the schema has since moved. The bundle is the **signed original bytes +
   twins**, with any *authored* attachment carried by content digest ([ADR-0013](0013-attachments-content-addressed-lazy-blob-tier.md);
   bytes opt-in) — exactly the *"my records are my defence"* property, obtained for free from the append-only
   design.

4. **Export is an audited, append-only event recording its blast radius** (the clinician-architect's explicit
   requirement). It appends `{ who exported, when, selection predicate, blast radius (event count / patient
   count / date span), seal mode }` to the audit stream — the same audited-egress family as break-glass
   key-*use* ([§7.1](../security.md#71-erasure-the-severity-ladder)/[§5.9](../identity.md#59-sensitivity-grade-the-safety-projection-and-break-glass-visibility-scope)).
   Bulk egress is never silent.

5. **Seal mode is the policy-neutral key-custody ladder, not a new mechanism.** The bundle may be
   **author-readable**, **sealed to an authority-generated public key** (the clinician holds a copy they
   cannot unilaterally open — for jurisdictions that require it), or **both**, reusing the [§3.5](../data-model.md#35-event-storage-model-hybrid-envelope)
   DEK-wrap-for-a-key-holder-hierarchy. *Which* is permitted is policy ([principle 9](../index.md#founding-principles-the-lens-for-every-decision));
   Cairn ships all rungs.

6. **It is the general mechanism behind ADR-0005 rung-2, and the erasure interaction is the intended honest
   ceiling.** A patient's later crypto-shred ([§7.1](../security.md#71-erasure-the-severity-ladder)) cannot
   reach a clinician's lawfully-held, author-scoped sealed copy outside the institution's boundary — which is
   exactly what the honest-erasure ceiling (*"…all copies in our existence"*) already declared, and is lawful
   (the author's own record of their own care). The author-scope bound minimises what persists: only the
   author's reasoning/actioning, never the whole chart.

**Blast radius ([§9](../language-substrate.md)).** The **predicate evaluation + seal + audit-emission** are
the one safety/privacy-critical seam (a bug that exports beyond the author's own contributor-set, or fails to
seal when policy requires, is a breach) → in-database/Rust trusted surface. The packaging/serialisation,
format conversion, and any viewer are fit-for-purpose.

## Consequences

- **Easier:** a clinician carries a durable, self-verifying, decades-legible copy of their own reasoning and
  actioning across a portfolio career, defusing the compounding record-loss risk; the export is bounded,
  audited, and seal-configurable per jurisdiction; and ADR-0005 rung-2's escrowed copy now has a concrete,
  general mechanism.
- **Harder / new surface:** a first-class export operation with a strict contributor-scoped predicate, a
  blast-radius-recording audit event, and a seal-mode selector — all small, all reusing existing parts; plus a
  UI that makes *"export everything I authored here"* a routine, low-friction action a departing clinician
  actually performs.
- **The bet:** that strict author-scoping is both a sufficient medico-legal defence (reasoning + actioning,
  per the clinician-architect) and a tight enough privacy bound to be broadly lawful, and that signed +
  twinned exports remain verifiable and legible across the decades the use-case demands. We would know it is
  wrong if jurisdictions commonly demanded the *results* context inside the author's own copy (they would then
  configure a broader, separately-justified export), or if the verifiability chain proved fragile across a
  20-year crypto-migration ([ADR-0015](0015-event-serialization-signatures-and-content-addressing.md)
  re-attestation-as-overlay is the hedge).
- **Policy-neutral ([principle 9](../index.md#founding-principles-the-lens-for-every-decision)):** Cairn ships
  the author-scoped selection, the audited export event, and the full seal ladder; *whether* a deployment
  permits author-readable copies, *requires* authority-key encryption, and *what* retention the practice
  itself owes are policy and jurisdiction.
