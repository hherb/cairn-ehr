# ADR-0018 — Federation revocation: counterparty enforcement, cascade, and the anchor as a position of power

- **Status:** Accepted
- **Date:** 2026-06-16
- **Refines:** ADR-0017

## Context

[ADR-0017](0017-federation-admission-sovereignty-peering-and-trust-anchors.md) established federation
admission (the sovereignty floor, mutual peering, pluggable trust anchors, the custodian contract) and noted
that *unpeering is a `revoke` overlay reusing the [§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody)
actor algebra + contamination cascade.* Pressure-testing that claim against a real adversarial case — **a
credentialed clinic whose operator is struck off, with a range of synchronising subsidiaries** — showed the
base mechanism holds but that several revocation properties are load-bearing enough to state explicitly, and
that one (the anchor's power to exclude) reaches principle level. Where authority can be *granted* it can be
*revoked*; getting the revocation semantics exactly right is what keeps admission honest.

The forces the case surfaced:

- **A bad node cannot be trusted to honor its own revocation** — it can be patched to ignore it. Enforcement
  cannot live on the revoked party.
- **Identity is free; credentials are not.** Revoking a node *key* is theatre if the principal mints a fresh
  key and re-enrols. The whack-a-mole evasion must be foreclosed.
- **Subsidiaries multiply the blast.** A bad actor controlling many nodes means revocation must *cascade* over
  whatever records the controlling relationship, without over-reaching onto innocent co-tenants.
- **The same mechanism that excludes a bad actor can be weaponised.** A captured national registry could
  mass-revoke a dissident clinic, or its signing key could be stolen — admission control's dark mirror.
- **Clawback is not Cairn's job.** Recovering data a bad actor already synced is a matter for authorities
  (subpoena, warrant); having the option to *stop the flow immediately* is Cairn's.

## Decision

Revocation **dissolves into the existing primitives** — **no new founding principle**; it is the
[§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody)/[§5.7](../identity.md#57-identity-event-algebra-closed-set-all-append-only-syncable-auditable)
actor-event algebra and the contamination cascade run backwards. Canonical home:
[security §7.7](../security.md#77-federation-admission-peering-trust-anchors-and-the-custodian-contract).
Seven properties:

1. **Enforced by the counterparties, never by the revoked node.** The trust set is mutual; each honest peer,
   on learning a credential is revoked, independently refuses further sync. *"Sync stops now"* means **every
   honest peer drops the edge**, not that the bad node cooperates. The enforceable boundary is the honest set
   — the same shape as the [§7.1](../security.md#71-erasure-the-severity-ladder) honest-erasure ceiling (you
   compel your own side, never the hostile one).

2. **Forward-looking distrust, not retroactive erasure.** `revoke` = *distrust events authored after the
   compromise/revocation time*, never *cannot-verify-old* ([§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody)).
   Events authored while credentialed stay in the record (append-only), carrying a de-credentialed-author
   trust marker ([§5.10](../identity.md#510-authorship-and-responsibility-state-the-consumer-side)); events
   the revoked party tries to author or push *after* T are refused by honest peers. Clinically correct: a
   patient actually treated there keeps a true record of what happened.

3. **Cascade over the issuance/affiliation graph — revoke the principal, not the key.** Revoking a node key
   alone invites re-enrolment, so revocation targets the **principal** (the struck-off operator / controlling
   entity):
   - **By issuance chain** (automatic): a credential verifies against an anchor *through a chain*; revoke an
     intermediate issuer and every credential beneath it fails — standard chain revocation, already implied by
     the anchor model. One revocation, whole subtree dark.
   - **By controlling-entity** (additive): each credential/enrolment carries a **controlling-entity /
     `on_behalf_of` attribute** — the [§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody)
     mandatory responsible-human backstop generalised to an organisation (the [ADR-0007](0007-authorship-and-accountability.md)
     responsibility chain applied to node credentials). Revocation is then a contamination-cascade selection:
     *"revoke every credential whose controlling entity is X"* — structurally identical to a model recall. It
     works only insofar as the anchor's issuance policy *recorded* the affiliation (policy populates the field;
     Cairn supplies it and the cascade).
   - **Issuance must check principal status** — a registry refuses (re-)issuance to a principal whose status is
     revoked, closing whack-a-mole. (In the self-CA world whack-a-mole is moot: a self-issued credential is
     honored only by the bad actor's own nodes.)

4. **Two distinct edge-removals — do not conflate them.** **Anchor revocation** (a credential invalidated *by
   its issuer*: bidirectional within that anchor's trust domain, cascades, every honest peer drops the edge —
   the struck-off-operator case) versus **voluntary unpeering** (a node's *sovereign choice* to stop federating
   with a peer: unilateral, local to that edge, no anchor involved, the other side may still legitimately trust
   it). Both are `revoke`-shaped overlays; they differ in *authority* and *scope*, and the implementation must
   not treat a unilateral unpeer as a global distrust signal or vice-versa.

5. **A trust anchor is a position of power; Cairn minimises its blast radius and makes it auditable, but
   cannot and must not prevent legitimate exclusion** (the anti-capture principle turned inward on admission).
   The same revocation that cleanly cuts off a bad actor is, in a captured registry's hands, a kill-switch and
   a surveillance surface. Cairn's containment, all already present:
   - **the sovereignty floor** — a cut-off node never dies; it keeps all local data, keeps operating, and can
     peer pairwise or form an alternative federation (capture of the *mesh* ≠ capture of the *node*);
   - **multi-anchor by default** — a node honors several anchors, so one captured anchor cannot isolate it.
     **Never mandate a single anchor**: a deployment that wires every node to one registry has *built* the
     kill-switch (the footgun to call out);
   - **audited, signed revocation** — a captured registry mass-revoking leaves a non-repudiable signed trail;
     abuse becomes *evidence*, unlike a silent proprietary kill-switch;
   - **the availability floor** — revocation never blocks local read of already-held data;
   - **no Cairn-owned root** — Cairn itself can never be the kill-switch.

   The honest limit: a deployment's chosen authority *can* exclude a node from *that authority's mesh* — that
   is the legitimate point of admission control, and the defence against its abuse is governance plus the
   ability to form alternative federations, never a mechanism that forbids anchors from revoking.

6. **Partition-honest: best-effort-immediate, guaranteed-eventual, enforced at every honest reconnection.** A
   partitioned subsidiary learns of revocation only on reconnect, and two of the bad actor's own islanded
   nodes can keep syncing with each other until then — Cairn can no more prevent that than stop two stolen
   paper charts being photocopied. What it *guarantees*: every honest node bridging that island refuses on
   next contact, so the island gets **no new honest data** after the bridge learns; the mesh heals as the
   signed revocation propagates (active registry push on the fast path, peer-to-peer gossip on the partition
   path). **Fail-open/closed knob:** local reading of already-held clinical data **never fails closed** (never
   brick a clinic — availability + paper-parity), while *ongoing sync with, and new peering to, a credentialed
   peer* **may** be gated on a policy-set revocation-freshness window. Availability of data already present is
   sacred; propagation of new data to a maybe-revoked peer is a policy choice.

7. **Granularity: one credential per accountable principal.** The cascade selects by *principal/credential*,
   never by *physical box*, so an innocent co-tenant with their own credential is untouched; sharing one
   credential means sharing its fate (paper-parity: practising under another's licence means falling with it).
   This is deployment guidance Cairn enables, not a mechanism.

**Trigger vs. mechanism (principle 9).** Cairn knows nothing of medical boards. The board pulls registration →
the registry, *by policy*, revokes the Cairn credential → the mechanism cascades. Cairn ships revoke + cascade
+ propagation + the controlling-entity field; the board→registry tie, the freshness window, and what counts as
grounds are policy.

**Blast radius ([§9](../language-substrate.md)).** Unchanged from [ADR-0017](0017-federation-admission-sovereignty-peering-and-trust-anchors.md):
credential/signature verification, the peering-edge gate, anchor/chain evaluation, and revocation checking are
safety-critical (a defect keeps an excluded node connected, leaking data across the boundary); the
issuance/revocation UI and affiliation bookkeeping are fit-for-purpose. The one safety-critical seam is
*verified-and-non-revoked credential → admitted peer*.

## Consequences

- **Easier:** the struck-off operator's whole controlled set goes dark on one principal-scoped revocation
  (chain + controlling-entity), enforced by honest counterparties, with no reliance on the bad actor and no
  ambiguity between exclusion and a peer's own unpeering; reinstatement on appeal is just a new signed overlay.
- **Harder / new surface:** the controlling-entity attribute is new (small, additive, only as good as the
  registry's bookkeeping); the freshness fail-open/closed window is new policy surface; and a captured-anchor
  kill-switch is an irreducible risk that must be documented and mitigated by multi-anchor defaults, never
  pretended away.
- **The bet:** that counterparty enforcement + principal-scoped cascade + an honestly-stale signed revocation
  feed cut a bad actor out fast enough in practice, and that multi-anchor survivability keeps a captured anchor
  from being a true kill-switch. We would know it is wrong if revocation cannot propagate fast enough across a
  badly-partitioned fleet to be relied upon, or if real deployments converge on single-anchor topologies
  despite the warning.
- **Clawback stays out of scope (honest ceiling).** Cairn stops the flow and can crypto-shred keys it controls
  ([§7.1](../security.md#71-erasure-the-severity-ladder)); plaintext a bad actor already holds is an
  authorities' matter. *Deletion is best-effort and declared, never guaranteed* ([principle 4](../index.md#founding-principles-the-lens-for-every-decision)).
