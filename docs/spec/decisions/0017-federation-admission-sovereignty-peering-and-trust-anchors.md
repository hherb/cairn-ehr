# ADR-0017 — Federation admission: the sovereignty floor, mutual peering, pluggable trust anchors, and the custodian contract

- **Status:** Accepted
- **Date:** 2026-06-16

## Context

[ADR-0016](0016-record-discovery-and-the-replicated-essential-tier.md) surfaced a hard dependency: the
existence-disclosure surface of record discovery — and the lawfulness of replicating a nation's essential
set — is tolerable only because every node holding the data is a **contracted, accountable custodian**.
But *how* a node earns the right to exchange data, **without Cairn becoming the authority that grants it**,
was left open as the *Custodian & Federation Admission* spec. This ADR settles it.

The governing requirement (stated by the clinician-architect) spans a full spectrum that the infrastructure
must serve **with the least possible friction**:

- **A single node needs no one's permission.** It works out of the box, with zero data in the store, talking
  to nobody. Requiring registration, a certificate authority, or any third-party authority just to *run* a
  node would be both a paper-parity failure (a paper practice asks no one's permission to keep records) and a
  capture surface ([principle 7](../index.md#founding-principles-the-lens-for-every-decision)).
- **The moment two nodes want to talk, they must negotiate who may access what.** Federation is a *mutual*
  act between specific parties, not a status conferred from above.
- **A private practice must be able to build its own node network without obtaining authority from any third
  party** — *and* must be able to **set its own rules for who may join** that network. Self-sovereign trust.
- **A national health system will ideally run a registry server / system** that issues and governs node
  admission at scale.

So the same mechanism must stretch from a two-node private LAN with no external authority to a
registry-governed national mesh — and must not, at any point, require a Cairn-owned root of trust or a
mandatory cloud. This is the [principle 9](../index.md#founding-principles-the-lens-for-every-decision)
*mechanism-not-policy* discipline and the [principle 7](../index.md#founding-principles-the-lens-for-every-decision)
anti-capture mission applied to **trust between nodes**.

Two adjacent decisions constrain the shape and make most of the machinery already-built:

- **A node is already an actor.** The [§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody)
  actor registry ([ADR-0011](0011-actor-registry-version-pinning-and-key-custody.md)) is *general* —
  human / device / **node**. A node has a self-generated signing identity ([ADR-0015](0015-event-serialization-signatures-and-content-addressing.md)),
  enrols through an audited ceremony mirroring mTLS provisioning ([§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody)/[§7.6](../security.md#76-the-software-distribution-plane-signed-releases-and-extension-load)),
  and its lifecycle runs on the closed actor-event algebra (`enroll / supersede / revoke / suspend /
  rotate-key`). Federation admission is largely **that registry + ceremony applied to node-to-node
  relationships** — not a new subsystem.
- **Admission is not confidentiality.** [ADR-0006](0006-visibility-scope-replication-and-the-safety-projection.md)
  settled that *replication is never the confidentiality boundary*. Admission must therefore gate the **outer
  boundary** (is there a peering edge at all, between which accountable parties) and must **not** be
  re-purposed as the inner confidentiality control — that stays key-custody + visibility + the safety
  projection. Conflating the two would re-introduce the *"withhold the row to keep it secret"* anti-pattern.

## Decision

The spec **dissolves into existing primitives composed** — **no new founding principle**, one operational
corollary (the *sovereignty floor*). Canonical home:
[security §7.7](../security.md#77-federation-admission-peering-trust-anchors-and-the-custodian-contract);
node identity and the actor algebra are [§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody);
the onboarding ceremony reuses [§7.6](../security.md#76-the-software-distribution-plane-signed-releases-and-extension-load).

1. **The sovereignty floor (operational corollary, not a new principle).** A node needs **no permission to
   exist and operate alone.** It self-generates its identity, runs with an empty or full store, and reads and
   writes locally — answering to no external authority. **Permission is a property of inter-node
   *relationships*, never of a node's right to run.** This is a corollary of availability
   ([principle 5](../index.md#founding-principles-the-lens-for-every-decision)), paper-parity
   ([principle 3](../index.md#founding-principles-the-lens-for-every-decision)), and anti-capture
   ([principle 7](../index.md#founding-principles-the-lens-for-every-decision)). Default posture is
   **deny-all peering**: a fresh node federates with nobody until explicitly introduced.

2. **Federation is mutual, signed, append-only peering — not central admission.** Two nodes form an edge by a
   **mutual cryptographic introduction**: each appends a signed peering assertion adding the other to its
   **trust set**, recording *which peer, under which trust anchor, at what time, for what default scope*. This
   is the [identity §5.7](../identity.md#57-identity-event-algebra-closed-set-all-append-only-syncable-auditable)/[§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody)
   algebra applied to node relationships (peer / supersede / revoke), so it is auditable, reversible by
   overlay, and **needs no third party for the two-node case** (the private-practice LAN). Unpeering is a
   `revoke` overlay; surplus data already held degrades by the [ADR-0005](0005-erasure-key-custody-and-crypto-shredding.md)
   key-custody ladder, never by mutation.

3. **Pluggable, self-hostable trust anchors are the spectrum knob — fractal topology applied to trust.** A
   node decides *whom it will peer with* by verifying a peer's **credential** against a **trust anchor it is
   configured to honor**. The anchor is configuration, and Cairn ships **no privileged root**:
   - **No anchor / direct pairwise** — the credential is the peer's own key, accepted by a one-time
     out-of-band confirmation (fingerprint / QR / short code). The two-node private practice, zero external
     authority.
   - **The practice's own issuing key** — the practice **is its own authority**, issuing credentials to the
     nodes it owns and **setting its own join rules**. A self-sovereign network, still no third party.
   - **A regional / national registry** — a node configured to require that anchor peers only with nodes
     bearing a valid, non-revoked credential the registry issued. The registry is **a node role (configuration,
     [principle 6](../index.md#founding-principles-the-lens-for-every-decision)), shipped in the same codebase
     and self-hostable** — never a Cairn-owned or proprietary service. Trust is in the signature/anchor, not
     the host, so registries are mirrorable and sneakernet-distributable (the [ADR-0014](0014-locale-pluggable-matcher-comparators.md)
     content-addressed-registry posture).

   The **same verification mechanism** serves all three; only the configured anchor set differs. A node may
   honor several anchors (its own, plus a regional registry) at once.

4. **The custodian contract is signed, verifiable metadata bound to the credential; its legal force is
   jurisdiction.** A credential is a signed attestation by an honored anchor that *"node X is a custodian
   operating under contract terms T"* — carrying or referencing (by content-hash) the privacy/retention/
   disclosure terms [ADR-0016](0016-record-discovery-and-the-replicated-essential-tier.md) requires. Cairn
   ships the **credential format, the verification, and the revocation mechanism**; *what the contract says,
   what counts as "health-system participation," and whether it is legally enforceable* are policy and
   jurisdiction — Cairn records and verifies the chain, jurisdictions interpret it (the
   [ADR-0007](0007-authorship-and-accountability.md) proxy/liability posture). For the solo practice, the
   practice self-issues: it is both custodian and contract authority inside its own walls.

5. **Admission gates the outer boundary; the existing stack governs what flows across it.** Two separable
   layers (the recurring *"one word hides dials"* motif):
   - **Peering / authentication** (this ADR): does an edge exist, and is each node cryptographically who it
     claims — coarse, rare, set once per relationship.
   - **Authorization across the edge** (unchanged): given peering, what *flows* is the
     [ADR-0004](0004-dynamic-sync-scope-prefetch-not-authority.md) sync-scope prefetch hint; what *decrypts*
     is key-custody ([ADR-0005](0005-erasure-key-custody-and-crypto-shredding.md)); what is *visible* is
     visibility + the safety projection ([ADR-0006](0006-visibility-scope-replication-and-the-safety-projection.md)).
     Admission **does not re-implement intra-federation confidentiality** — *"peered"* never means *"may see
     everything."* The [ADR-0016](0016-record-discovery-and-the-replicated-essential-tier.md) essential-tier
     replication flows only across admitted edges, and its disclosure granularity (region, never a named
     clinic) is a property of the anchor's policy.

6. **Verification is offline-capable; revocation is a synced, honestly-stale feed.** A credential is
   self-describing and **verifiable during a partition** with no live-CA callout
   ([principle 5](../index.md#founding-principles-the-lens-for-every-decision)). Revocation reuses the
   [§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody) `revoke` overlay +
   contamination cascade, distributed as a **signed feed** that syncs like the audit stream; a node checks the
   freshest revocation it holds and **surfaces its staleness** (the [§6.2](../sync.md#62-consistency-model)
   honest-assembly rule) — it never blocks a safety-critical peering on an unreachable registry, and it never
   silently trusts a credential it knows was revoked.

7. **Least-friction onboarding reuses the possession gesture and the provisioning ceremony.** Pairing two
   nodes is a one-time, **high-distinctiveness, low-time** gesture (out-of-band fingerprint / QR / short
   code) — the [identity §5.11](../identity.md#511-point-of-care-identity-possession-fast-authentication-and-salvage)
   possession antidote-to-click-through applied to nodes, with **no mandatory cloud round-trip**. Joining a
   registry-governed mesh means presenting a registry-issued credential, provisioned at deployment through
   the audited ceremony already defined for mTLS enrolment and signed-release install
   ([§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody)/[§7.6](../security.md#76-the-software-distribution-plane-signed-releases-and-extension-load)).
   Transport encryption (mTLS / WireGuard) is the channel; admission authorizes *which node identities* may
   establish it.

8. **Blast radius ([§9](../language-substrate.md)).** **Safety-critical** (in-DB / Rust trusted base, beside
   the [§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody) registry):
   credential/signature verification, the peering-edge gate, trust-anchor evaluation, and revocation checking —
   a defect admits an unauthorized node and leaks data across the federation boundary. **Fit-for-purpose:** the
   registry's issuance UI, the contract-authoring tools, and onboarding wizards — a defect yields a bad
   *proposal* a human reviews, never a silent breach. The one safety-critical seam is *verified credential →
   admitted peer*, the federation analogue of the [§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody)
   enrolment seam and the [§7.6](../security.md#76-the-software-distribution-plane-signed-releases-and-extension-load)
   verified-release→load seam.

## Consequences

- **Easier:** the whole spectrum runs on one mechanism with minimal new code — a lone node just works; a
  private practice builds and governs its own network with no external authority; a national system runs a
  self-hostable registry as a node role; and most machinery (node identity, the actor algebra, the audited
  ceremony, revocation-by-cascade) is reused from [§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody)/[§7.6](../security.md#76-the-software-distribution-plane-signed-releases-and-extension-load).
- **Harder / new surface:** the credential format and the custodian-contract binding are new (small, signed,
  content-addressed); trust-anchor configuration and multi-anchor evaluation are new policy surface; the
  registry-as-node-role needs an issuance + revocation-feed implementation; and the pairing UX must be both
  low-friction and high-distinctiveness.
- **The bet:** that pluggable, self-hostable trust anchors over a mutual-peering algebra genuinely span the
  solo-practice-to-nation spectrum without ever needing a Cairn-owned root, and that keeping admission
  strictly separate from confidentiality ([ADR-0006](0006-visibility-scope-replication-and-the-safety-projection.md))
  avoids re-introducing replication-as-access-control. We would know it is wrong if real deployments find the
  two-node pairing too heavy, or if a national registry's revocation feed cannot stay fresh enough across the
  partitioned fleet to be trusted.
- **Policy-neutral ([principle 9](../index.md#founding-principles-the-lens-for-every-decision)):** Cairn ships
  the identity, the peering algebra, the credential/verification/revocation mechanism, and a self-hostable
  registry role; *who* may peer, *which* anchors a node honors, *what* the custodian contract requires, and
  *what* counts as health-system participation are entirely the deployment's to set.
