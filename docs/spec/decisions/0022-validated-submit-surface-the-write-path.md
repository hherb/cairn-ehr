# ADR-0022 — The validated submit surface: the node's write path

- **Status:** Accepted
- **Date:** 2026-06-17
- **Refines:** [ADR-0021](0021-layering-the-node-api-and-ui-pluralism.md), [ADR-0001](0001-fat-postgres-thin-daemon.md)

## Context

[ADR-0021](0021-layering-the-node-api-and-ui-pluralism.md) put the safety/compatibility floor in the
database and named the **validated submit surface** as the unbypassable entry point for all writes: UIs
and external clients never `INSERT` into event tables; they call validated submit functions that the DB
role-grant model confines them to. ADR-0021 then made that surface's **completeness** the load-bearing
bet — *every legitimate write must be expressible through it, or UIs are pushed to raw access and the
bypass re-opens* — but left the surface itself unspecified.

The hard part is a paradox in ADR-0021's own words: the surface must be **small** (the smallest possible
audited trusted base, [principle 8](../index.md#founding-principles-the-lens-for-every-decision)) yet
**complete** (cover every legitimate write). A function per clinical event type would be neither — it
would sprawl, and it would have to *grow every time a new event type is added*, violating additive-only
evolution ([ADR-0012](0012-schema-evolution-event-format-and-legibility-across-time.md)) and bloating the
audited surface. Two observations dissolve the paradox:

- **Cairn is append-only ([principle 1](../index.md#founding-principles-the-lens-for-every-decision)), so
  *almost every write is the same operation*** — append a validated, signed event. Clinical events,
  identity events ([§5.7](../identity.md#57-identity-event-algebra-closed-set-all-append-only-syncable-auditable)),
  actor events ([§3.12](../data-model.md#312-actor-identity-in-the-registry)), demographic assertions
  ([§4.1](../demographics.md#41-demographic-assertions)), the overlay streams (sensitivity, responsibility-tags,
  acknowledgments, trust/recall markers, the [ADR-0020](0020-active-write-thin-encounters-and-the-delete-vs-erase-distinction.md)
  visibility-suppression overlay), federation peering ([ADR-0017](0017-federation-admission-sovereignty-peering-and-trust-anchors.md)),
  and audited configuration acts ([ADR-0010](0010-additive-vs-suppressing-classification.md)) are *all*
  appends of a signed event. The only writes that are **not** appends are a small, enumerable set.
- **The write path is where a long list of write-time seams the spec has accumulated all converge** — the
  authorship stamp ([ADR-0008](0008-point-of-care-identity-possession-and-salvage.md)), clash detection
  ([ADR-0003](0003-bitemporal-time-and-acknowledged-uncertainty.md)), the seal-time safety projection
  ([§5.9](../identity.md#59-sensitivity-grade-the-safety-projection-and-break-glass-visibility-scope)), the
  suppressing owner-gate ([ADR-0010](0010-additive-vs-suppressing-classification.md)), the legibility-twin
  derivation ([ADR-0012](0012-schema-evolution-event-format-and-legibility-across-time.md)), and
  canonicalize+sign ([ADR-0011](0011-actor-registry-version-pinning-and-key-custody.md)/[ADR-0015](0015-event-serialization-signatures-and-content-addressing.md)).
  The submit surface is **not a new mechanism — it is the named convergence point of these seams.**

## Decision

Specify the submit surface as **one generic validated-append, plus a closed set of non-append
operations** — small by construction, complete by genericity. Canonical home:
[language-substrate §9.6](../language-substrate.md#96-the-validated-submit-surface-the-write-path).
**No new founding principle.**

1. **One generic validated-append function, not one per type.** `submit_event(envelope, body)` is generic
   in signature and **type-validated by dispatch** to a per-`(event_type, schema_version)` validator held
   in the trusted layer and registered **additively** ([ADR-0012](0012-schema-evolution-event-format-and-legibility-across-time.md)).
   A new event type adds a validator *behind the same door* — the surface does not grow. Closed-enum
   membership (`event_type`, the role enum) is still enforced. This single move is what makes the surface
   simultaneously small and complete.

2. **Atomic multi-event submission.** A clinical action routinely co-produces several events — the
   [ADR-0020](0020-active-write-thin-encounters-and-the-delete-vs-erase-distinction.md) order + note-line,
   or an event together with its overlay. `submit_event` accepts a **set** committed all-or-nothing (one
   transaction, one possession, one encounter scope), so co-produced events land together or not at all.

3. **The non-append surface is a small closed set**, each operation a narrow, specially-audited entry
   point because it is *not* a pure append:
   - **Erasure / key-custody redistribution** ([ADR-0005](0005-erasure-key-custody-and-crypto-shredding.md)) —
     crypto-shred / seal / escrow. It destroys or re-custodies a key rather than appending content, though
     it **emits an append-only erasure-declaration audit event**; the irreversible rung carries the
     [ADR-0020](0020-active-write-thin-encounters-and-the-delete-vs-erase-distinction.md) forced-rationale gate.
   - **Author-scoped export** ([ADR-0019](0019-author-scoped-record-export-the-medico-legal-copy.md)) —
     reads + packages signed bytes, emitting an append-only audit event recording blast radius.
   - **Blob byte-tier put** ([§6.6](../sync.md#66-attachments-the-lazy-byte-tier), [§3.14](../data-model.md#314-attachments-content-addressed-blobs-and-the-rendition-set)) —
     content-addressed bytes on the separate byte plane, **content-verified on store**. (The *reference* is
     an ordinary `submit_event`; only the *bytes* take this door.)

   Everything not in this closed set is `submit_event`. **Reads/projections are `SELECT`** on projection
   views (the [§9.5](../language-substrate.md#95-layering-the-node-api-and-ui-pluralism-uniform-core-plural-edges)
   grant), outside the surface; an *audited* read such as break-glass key-**use**
   ([§5.9](../identity.md#59-sensitivity-grade-the-safety-projection-and-break-glass-visibility-scope))
   emits its audit through the append path.

4. **Drafts are adjacent, not part of it.** Pre-commit drafts ([§3.10](../data-model.md#310-session-identity-event-authorship-and-draft-durability))
   are **local, mutable, never-synced** node state behind a separate validated (but not append-only) local
   API; commit turns a draft into a `submit_event` call. The wire contract is untouched by drafts, so they
   need not — and must not — sit on the immutable log.

5. **`submit_event` is the in-DB convergence of every write-time seam, executed atomically in order:**
   1. **Authenticate the author** — verify an **author-attestation token**
      ([ADR-0008](0008-point-of-care-identity-possession-and-salvage.md)), yielding the contributor set and
      the authorship-confidence grade (*attested / asserted / unattributed*). **The DB *session* is never
      trusted as the author** (`session.user ≠ event.author`, [§3.10](../data-model.md#310-session-identity-event-authorship-and-draft-durability)) —
      this is why a direct-DB client cannot forge authorship: it cannot forge the token.
   2. **Authorize** — access-control enforcement ([§7](../security.md)).
   3. **Validate the envelope** — required typed fields, scope-key shape, contributor-set well-formedness,
      closed-enum membership, and the [§3.6](../data-model.md#36-bitemporal-event-time-recording-time-vs-effective-time)
      Tier-1 ceiling `t_effective ≤ t_recorded`.
   4. **Validate the body** — dispatched per `(event_type, schema_version)` (ruling 1); demographic
      coherence ([§4.2](../demographics.md#42-per-field-projection-policy)/[§5.2](../identity.md#52-matching-pipeline-safety-asymmetric-false-merge-worse-than-false-split));
      [§3.6](../data-model.md#36-bitemporal-event-time-recording-time-vs-effective-time) Tier-2 clash →
      **flag, never resolve**.
   5. **Apply hard-policy gates** — the [ADR-0010](0010-additive-vs-suppressing-classification.md)
      suppressing owner-gate; the [§5.9](../identity.md#59-sensitivity-grade-the-safety-projection-and-break-glass-visibility-scope)
      sensitivity auto-grade + seal-time safety-projection derivation.
   6. **Canonicalize + twin + sign** — deterministic-CBOR canonical bytes, the **mandatory plaintext
      legibility twin** derived at write-time ([ADR-0012](0012-schema-evolution-event-format-and-legibility-across-time.md)),
      the signature ([ADR-0011](0011-actor-registry-version-pinning-and-key-custody.md)/[ADR-0015](0015-event-serialization-signatures-and-content-addressing.md)),
      and the content digest.
   7. **Idempotent append** — `INSERT … ON CONFLICT (uuid) DO NOTHING`; `AFTER INSERT` trigger projections
      ([§9.4](../language-substrate.md#94-merge-projection-boundary-fat-postgres-thin-rust-daemon)).
   8. **Return** the assigned UUID/HLC and honest assembly state.

6. **Signing must be reachable from the in-DB path — a direct consequence of ADR-0021's floor-in-DB.** If
   signing lived only in the L2 Rust layer, a direct-DB caller could not produce a signed event and the
   in-DB floor would be *incomplete*, contradicting [ADR-0021](0021-layering-the-node-api-and-ui-pluralism.md).
   Therefore the node's trusted base **includes the database process** (fat Postgres,
   [ADR-0001](0001-fat-postgres-thin-daemon.md)): signing is performed in-DB (pgrx + node-keystore access)
   by default, or delegated to a co-located trusted signer the in-DB submit invokes — but **never required
   to sit in L2**. The author-attestation token and the signer are the two pieces of trusted base the
   submit path binds.

7. **The authoring path and the apply path are distinct and must not be conflated.** `submit_event` is the
   **authoring** path (this node mints, validates, signs, appends). The **apply** path
   ([§6.1](../sync.md#61-mechanism)/[§9.4](../language-substrate.md#94-merge-projection-boundary-fat-postgres-thin-rust-daemon))
   ingests already-signed events from peers — it **verifies** signature/content-address and idempotently
   appends, but **never re-signs and never re-runs authorship/authz** (a foreign node's attestation
   stands). Both terminate in the same idempotent append; only authoring signs.

## Consequences

- **Easier.** The surface is provably **small** — one append door + a three-member non-append set — yet
  **complete**: genericity (ruling 1) covers every event type, and the append-only taxonomy enumerates the
  categories so the completeness claim is checkable. Completeness is *maintainable*: a new event type is an
  additive validator, never a new door. Concentrating every write-time seam at one in-DB function gives the
  smallest-possible audited surface ([principle 8](../index.md#founding-principles-the-lens-for-every-decision))
  and puts every seam the spec has named ([ADR-0003](0003-bitemporal-time-and-acknowledged-uncertainty.md)/[0005](0005-erasure-key-custody-and-crypto-shredding.md)/[0006](0006-visibility-scope-replication-and-the-safety-projection.md)/[0008](0008-point-of-care-identity-possession-and-salvage.md)/[0010](0010-additive-vs-suppressing-classification.md)/[0011](0011-actor-registry-version-pinning-and-key-custody.md)/[0012](0012-schema-evolution-event-format-and-legibility-across-time.md)/[0015](0015-event-serialization-signatures-and-content-addressing.md)/[0020](0020-active-write-thin-encounters-and-the-delete-vs-erase-distinction.md))
  in one reviewable place.
- **Harder / new trusted surface.** That one in-DB function becomes the **most safety-critical code in the
  system** (mis-sign, mis-attribute, mis-validate, or mis-gate → corruption or leak). It must be optimised
  for reviewer-legibility and is the prime pgrx candidate ([ADR-0002](0002-in-database-rust-pgrx-escape-hatch.md)).
  The **author-attestation-token verification** and the **in-DB signer / keystore access** are the two
  sub-seams to get right; the **validator-dispatch registry** must itself be additive-only and
  tamper-evident.
- **The bet (refining ADR-0021's).** That the *generic-append + closed-non-append-set* surface stays
  complete as the clinical model grows — that no future legitimate write is *neither* an append *nor* a
  member of the closed non-append set. We would know the bet is wrong if such an operation appears (forcing
  a new door and growing the surface) — which would itself be a useful signal that the data model has grown
  a non-append-shaped concept worth scrutinising before it is admitted.
- **Policy-neutral (principle 9).** The surface is *mechanism*; which writes a deployment permits, and the
  hard-policy gates it activates at step 5.5, are configuration. **No new event stream; no new founding
  principle** — this refines [ADR-0021](0021-layering-the-node-api-and-ui-pluralism.md) and
  [ADR-0001](0001-fat-postgres-thin-daemon.md).
