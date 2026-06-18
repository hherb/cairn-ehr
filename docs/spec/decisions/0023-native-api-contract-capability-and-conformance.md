# ADR-0023 — The native API contract: capability description and executable conformance

- **Status:** Accepted
- **Date:** 2026-06-17
- **Refines:** [ADR-0021](0021-layering-the-node-api-and-ui-pluralism.md)

## Context

[ADR-0021](0021-layering-the-node-api-and-ui-pluralism.md) fixed the native API's *properties* — additive,
versioned, capability-described, with a published **conformance suite** making *"any node talks to any
node"* executable — and deliberately left the wire **transport** a later fit-for-purpose choice.
[ADR-0022](0022-validated-submit-surface-the-write-path.md) then specified the *write* surface. This ADR
specifies the two pieces ADR-0021 named but left unspecified, and which together are the practical
**anti-drift tool** a small team needs: **(1)** how a node *describes* its capabilities so a UI adapts and
degrades gracefully, and **(2)** how conformance to the contract is made *executable* so a developer
verifies compatibility before shipping rather than discovering drift in the field.

Two framing realizations make the design fall out of existing canon:

- **API compatibility is the same problem as schema evolution** ([ADR-0012](0012-schema-evolution-event-format-and-legibility-across-time.md)):
  a fleet of offline nodes carries **permanent, unbounded version skew**, and a UI may meet a node
  *newer* or *older* than itself. A monotonic "API v1/v2/v3" number is the wrong primitive — it
  *linearizes* what is really a **set of independently-present capabilities** (a node understands these
  event-type/schema-version validators and these optional features, in no total order). The right
  primitive is **additive-only capability evolution + the [§3.13](../data-model.md#313-schema-evolution-event-format-and-the-legibility-twin)
  `min()` degradation ladder**, applied to the API.
- **Anti-capture forbids a Cairn-owned conformance gatekeeper** ([principle 7](../index.md#founding-principles-the-lens-for-every-decision)).
  If "conformant" were a certificate the steward issues or withholds, that certificate *is* the capture
  surface. Conformance must therefore be **self-runnable and self-verifiable**, published like the
  [ADR-0014](0014-locale-pluggable-matcher-comparators.md) comparator registry (signed, content-addressed,
  mirrorable, sneakernet-cloneable; trust in signature/hash, not host).

## Decision

Specify the native API as a **capability set over a mandatory baseline**, described by a served
self-describing projection, with conformance as an **open, executable, self-verifiable suite**. Canonical
home: [language-substrate §9.7](../language-substrate.md#97-the-native-api-contract-capability-description-and-conformance).
**No new founding principle** — this operationalizes [principle 12](../index.md#founding-principles-the-lens-for-every-decision)
and applies [principle 11](../index.md#founding-principles-the-lens-for-every-decision) to the contract.

1. **Capability flags + a mandatory baseline, not version gates.** A node advertises a *set* of
   capabilities — the `(event_type, schema_version)` validators it serves
   ([ADR-0022](0022-validated-submit-surface-the-write-path.md)) and its optional features — over a small
   **mandatory core** every conformant node has. Feature growth is **additive capability flags, never
   version bumps that strand old UIs** ([ADR-0012](0012-schema-evolution-event-format-and-legibility-across-time.md)
   discipline applied to the API). A coarse baseline marker names the mandatory floor; everything above it
   is set-based, partially ordered, never a linear gate.

2. **The capability descriptor is a served, self-describing projection of local-node-properties.** It is
   **not new state**: a node's capabilities are a function of its installed schema versions, loaded
   validators/extensions, and active config — all already *local node properties*
   ([ADR-0012](0012-schema-evolution-event-format-and-legibility-across-time.md)). The descriptor merely
   *exposes that set* to clients. It is itself **additively evolvable and legible across time** (a 2030 UI
   reads a 2026 node's descriptor and vice versa) and **transport-independent** — it describes *operations
   and capabilities*, not REST endpoints, so the same contract binds to REST, gRPC, or in-process
   (ADR-0021's *"properties fixed, transport later"*).

3. **Negotiation is stateless description + client-side graceful degradation, not a handshake.** The node
   *serves* its descriptor (cacheable); the client *adapts*. There is no stateful round-trip that could
   fail during a partition ([principle 5](../index.md#founding-principles-the-lens-for-every-decision),
   availability). A UI lights up optional capabilities when present and **degrades to the floor when
   absent** — the [§3.13](../data-model.md#313-schema-evolution-event-format-and-the-legibility-twin)/[§5.9](../identity.md#59-sensitivity-grade-the-safety-projection-and-break-glass-visibility-scope)
   `min(what the UI needs, what the node offers)` ladder, now on the API. **Degradation may reduce
   experience, never correctness or safety:** because the mandatory core *is* the floor, every UI gets at
   least it on every conformant node, so safety-bearing surfaces (the [§5.9](../identity.md#59-sensitivity-grade-the-safety-projection-and-break-glass-visibility-scope)
   safety projection, honest assembly-state) are always present even when an advanced capability is not.

4. **The conformance suite is the executable contract — two faces.**
   - **Wire/node conformance:** does this node correctly participate in **L0** — emit canonical signed
     events ([ADR-0015](0015-event-serialization-signatures-and-content-addressing.md)), apply set-union,
     honor the identity/actor algebras ([§5.7](../identity.md#57-identity-event-algebra-closed-set-all-append-only-syncable-auditable)/[§3.12](../data-model.md#312-actor-identity-in-the-registry)),
     evolve additive-only. This *is* the *"any node talks to any node"* guarantee made **checkable**, and a
     federation may require it at admission ([ADR-0017](0017-federation-admission-sovereignty-peering-and-trust-anchors.md)) —
     the **technical** gate, distinct from the trust/credential gate.
   - **API conformance:** does this node's L2 API honor the published contract **for the capabilities it
     advertises** — what lets a small team trust *"any conformant node will serve my UI."* It is
     **capability-partitioned** (conformant = passes the core tests **+** the tests for every capability it
     claims) and **additively versioned** (a new capability adds tests; tests are **never removed**, the
     same discipline as additive schema evolution — a dropped test would silently strand the guarantee).

5. **Conformance is self-runnable and self-verifiable — no Cairn-owned gatekeeper.** The suite is published
   like the spec and the [ADR-0014](0014-locale-pluggable-matcher-comparators.md) registry: open, signed,
   content-addressed, mirrorable, sneakernet-cloneable. A node *proves* conformance by running the suite
   and publishing a **signed result**, not by obtaining a certificate the steward could grant or withhold.
   This is **anti-capture turned inward a second time** — ADR-0021 denied the steward's own UI a private
   API; this denies the steward a conformance chokepoint. GitHub is convenience, never a dependency.

6. **The suite is the spec's executable form — legibility across time for the contract.** A small team
   cannot read twenty-plus ADRs, but it can *run the suite*; the suite operationalizes the contract the way
   the legibility twin operationalizes an event ([principle 11](../index.md#founding-principles-the-lens-for-every-decision)).
   The descriptor and the suite are **two views of one capability set** — what a node *claims* (descriptor)
   and what it can *prove* (suite).

## Consequences

- **Easier.** A bespoke-UI developer gets a **runnable definition of "will this work everywhere"**: code
  against the mandatory core + the served descriptor, run the suite, ship with confidence. Drift becomes
  **mechanically detectable**, not discovered in the field — the anti-drift promise of
  [principle 12](../index.md#founding-principles-the-lens-for-every-decision) made concrete. Federation
  admission ([ADR-0017](0017-federation-admission-sovereignty-peering-and-trust-anchors.md)) gains an
  objective **technical** conformance gate distinct from its trust gate. The steward cannot become a
  conformance chokepoint.
- **Harder / new artifacts.** The **mandatory-core definition**, the **capability taxonomy**, and the
  **suite itself** are new things to build and to *maintain additively* (the suite may never drop a test,
  mirroring additive schema evolution). The descriptor's own additive-legibility must hold across decades.
- **The bet.** That the **mandatory core can be kept small enough for a Pi-class node to fully conform yet
  rich enough that "conformant" is a meaningful promise to a UI** — the same tension ADR-0001 bet on for
  in-DB cost, now for the contract. We would know it is wrong if real UIs routinely need capabilities the
  core cannot include *but cannot degrade without* (forcing a core bump that strands nodes), or if the
  capability set fragments so far that "conformant" stops meaning *"my UI works here."*
- **Policy-neutral (principle 9), anti-capture (principle 7).** Mechanism, not policy; self-verifiable, no
  central authority. **No new founding principle; no new event stream** — refines
  [ADR-0021](0021-layering-the-node-api-and-ui-pluralism.md).
