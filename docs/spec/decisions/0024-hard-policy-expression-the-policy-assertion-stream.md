# ADR-0024 — Hard policy expression: the policy-assertion stream and the effective-policy projection

- **Status:** Accepted
- **Date:** 2026-06-17
- **Refines:** [ADR-0021](0021-layering-the-node-api-and-ui-pluralism.md)

## Context

[ADR-0021](0021-layering-the-node-api-and-ui-pluralism.md) *located* hard policy — "anchored in the DB, or
in L2 with raw access role-gated" — and distinguished it from soft (UI) policy, but did not say **how a
deployment authors, changes, and audits its hard policy.** Meanwhile the spec has accumulated a long list of
**"expressible policy rungs,"** each closed with some variant of *"Cairn ships the rung; the deployment
decides ([principle 9](../index.md#founding-principles-the-lens-for-every-decision))"* but with no common
expression mechanism: the [identity §5.10](../identity.md#510-authorship-and-responsibility-state-the-consumer-side)
*"un-vouched suppressing AI output must be attested before effect"* rung, the
[ADR-0005](0005-erasure-key-custody-and-crypto-shredding.md) erasure-ladder rung selection, the
[ADR-0006](0006-visibility-scope-replication-and-the-safety-projection.md) sensitivity-grading combination,
[ADR-0009](0009-notification-economy-salience-routing-and-the-acknowledgment-floor.md) notification routing,
[ADR-0010](0010-additive-vs-suppressing-classification.md)'s un-owned-suppression permission, and the
*"who may enroll / upgrade / admit"* rungs of [§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody)/[§7.6](../security.md#76-the-software-distribution-plane-signed-releases-and-extension-load)/[§7.7](../security.md#77-federation-admission-peering-trust-anchors-and-the-custodian-contract).
This is the last [ADR-0021](0021-layering-the-node-api-and-ui-pluralism.md) follow-on (#3): give them all one
mechanism.

The danger is that the policy-expression mechanism *itself* becomes a capture or safety hole — policy changed
silently, by the wrong party, or expressed as arbitrary code injected into the trusted floor. Two
realizations make the design fall out of existing canon and avoid that:

- **Hard policy is exactly the kind of thing Cairn always models as an append-only signed stream + a
  projection** — it is *enforced*, safety/compliance-relevant, and must be auditable (who set it, when, under
  what authority). The same shape as sensitivity ([§5.9](../identity.md#59-sensitivity-grade-the-safety-projection-and-break-glass-visibility-scope)),
  responsibility-routing ([data-model §3.11](../data-model.md#311-notifications-as-projections-responsibility-routing-and-acknowledgment)),
  and the identity link graph ([§5.1](../identity.md#51-linkage-layer-never-merge-always-link)). It is **not a
  mutable config file.**
- **Policy must be declarative *selection over Cairn-shipped mechanism*, never arbitrary code.** Arbitrary
  executable policy would be the remote-code-execution surface [ADR-0012](0012-schema-evolution-event-format-and-legibility-across-time.md)
  forbids on the data plane, and a defect-blast-radius nightmare. Cairn ships a **closed set of mechanisms**;
  policy *selects and parameterizes* within it — the closed-set discipline recurring once more.

## Decision

Express hard policy as an **append-only, signed, scoped, graded policy-assertion stream with an
effective-policy projection**, declaratively selecting Cairn-shipped mechanism. Canonical home:
[security §7.9](../security.md#79-hard-policy-expression-projection-and-enforcement). **No new founding
principle** — this is the concrete mechanism *of* [principle 9](../index.md#founding-principles-the-lens-for-every-decision),
built from [principle 1](../index.md#founding-principles-the-lens-for-every-decision) (append-only) and
[principle 2](../index.md#founding-principles-the-lens-for-every-decision) (overlay, never mutate).

1. **Hard policy is an append-only policy-assertion stream; the effective policy is a projection.** Every
   policy act is a **signed, audited event** (`{ who, when, scope, authority, selection }`), never a mutated
   setting — *never erase, always overlay* ([principle 2](../index.md#founding-principles-the-lens-for-every-decision)).
   *"What was the retention floor on date X, and who set it?"* is therefore a first-class query, and the
   effective policy at any instant is a deterministic projection — the
   [§5.9](../identity.md#59-sensitivity-grade-the-safety-projection-and-break-glass-visibility-scope) /
   [§3.11](../data-model.md#311-notifications-as-projections-responsibility-routing-and-acknowledgment) /
   [§5.1](../identity.md#51-linkage-layer-never-merge-always-link) overlay shape applied to policy. This makes
   [ADR-0010](0010-additive-vs-suppressing-classification.md)'s *"explicit audited configuration act"*
   concrete and general.

2. **Policy is declarative selection/parameterization over a closed, Cairn-shipped mechanism set — never
   arbitrary code.** A policy event selects rungs and sets parameters within the shipped mechanisms (which
   erasure rungs are reachable; whether attestation is required for class X; the break-glass authorization
   level; notification routing defaults; which suppressing operations a responsible owner may permit). The
   **two-plane split** ([ADR-0012](0012-schema-evolution-event-format-and-legibility-across-time.md)) applies:
   the policy *selection* is **data on the event plane** (set-union-syncable, never executable), while the
   policy *evaluation code* — whether in-DB or a vetted L2/extension — travels the **distribution plane**
   ([§7.6](../security.md#76-the-software-distribution-plane-signed-releases-and-extension-load)). A deployment
   never injects enforcement code into the trusted floor.

3. **The "DB-anchored vs role-gated-L2" fork dissolves — same expression, the enforcement *locus* is a
   [§9.1](../language-substrate.md#91-selection-rule-by-defect-blast-radius) blast-radius call.** The
   effective-policy projection lives in the DB; the [§9.6](../language-substrate.md#96-the-validated-submit-surface-the-write-path)
   submit surface and RLS **read it**, so policy is enforced in-DB and unbypassable by default (ADR-0021's
   floor-in-DB). Policy whose *evaluation* is genuinely richer in Rust runs in L2 with raw DB access
   role-gated so untrusted clients cannot bypass it — but the *selection* is the same append-only event. This
   mirrors [§9.4](../language-substrate.md#94-merge-projection-boundary-fat-postgres-thin-rust-daemon)'s
   PL/pgSQL-default / pgrx-escape-hatch split.

4. **Authority-gated authoring, bootstrapped at provisioning.** A policy event is authored by a registered
   actor ([§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody)) holding
   policy authority. *Who* holds it is itself policy, bottoming out at the **root authority set during node
   provisioning** ([§7.6](../security.md#76-the-software-distribution-plane-signed-releases-and-extension-load)
   ceremony) — the same bootstrap as the distribution-plane steward key and the
   [§7.7](../security.md#77-federation-admission-peering-trust-anchors-and-the-custodian-contract) self-issued
   practice key. **Meta-policy** (*"changing the retention floor requires two-person authority"*) is
   expressible by the same mechanism turned on itself.

5. **Scoped and floor-composing (fractal topology + the sovereignty floor).** Policy carries a **scope**
   (node / facility / federation), [principle 6](../index.md#founding-principles-the-lens-for-every-decision).
   **Floor policy composes max-strict:** a federation anchor may impose a minimum that a bound node can
   ratchet *stricter* but **never weaker** — the safety-floor-never-relaxes pattern recurring
   ([§5.9](../identity.md#59-sensitivity-grade-the-safety-projection-and-break-glass-visibility-scope) highest
   standing grade, [ADR-0010](0010-additive-vs-suppressing-classification.md) one-way caution ratchet) —
   imposed through the [§7.7](../security.md#77-federation-admission-peering-trust-anchors-and-the-custodian-contract)
   admission/anchor, the policy analogue of the trust anchor. **Local (non-floor) policy is node-autonomous**
   (the [ADR-0017](0017-federation-admission-sovereignty-peering-and-trust-anchors.md) sovereignty floor: a
   solo node sets its own). Effective policy = *strictest applicable floor, then local choice* — a projection.

6. **Partition-honest, like every overlay stream.** Policy events sync set-union on the event plane; the
   effective projection is computed locally; a partitioned node enforces **last-known policy** and surfaces
   its staleness ([§6.2](../sync.md#62-consistency-model)). Local **reads never fail closed** on policy
   unavailability (the availability floor; the [ADR-0018](0018-federation-revocation-cascade-and-the-anchor-as-power.md)
   local-read-never-fails-closed knob); policy propagates eventually.

7. **This unifies the scattered "expressible policy rungs."** The [§5.10](../identity.md#510-authorship-and-responsibility-state-the-consumer-side)
   attestation rung, the [ADR-0005](0005-erasure-key-custody-and-crypto-shredding.md) erasure rungs,
   [ADR-0006](0006-visibility-scope-replication-and-the-safety-projection.md) sensitivity combination,
   [ADR-0009](0009-notification-economy-salience-routing-and-the-acknowledgment-floor.md) routing,
   [ADR-0010](0010-additive-vs-suppressing-classification.md) suppression permission, and the
   [§7.5](../security.md#75-the-actor-registry-enrollment-version-pinning-and-key-custody)/[§7.6](../security.md#76-the-software-distribution-plane-signed-releases-and-extension-load)/[§7.7](../security.md#77-federation-admission-peering-trust-anchors-and-the-custodian-contract)
   *who-may-X* rungs are all **instances of this one mechanism** — not a new subsystem, the consolidation of an
   existing scatter.

## Consequences

- **Easier.** One auditable, sync-safe, partition-safe mechanism serves *all* hard policy; a policy change is
  accountable by construction, **closing [ADR-0010](0010-additive-vs-suppressing-classification.md)'s
  conservation-of-responsibility loop** — the audited configuration act it pointed to is now a concrete
  policy event. The scattered rungs get a common home and a uniform audit story, and federations can impose
  compliance floors **without breaking node sovereignty** (ratchet stricter, never weaker).
- **Harder / new trusted surface.** The effective-policy projection and the
  [§9.6](../language-substrate.md#96-the-validated-submit-surface-the-write-path)/RLS gates that read it are
  safety-critical (a defect mis-enforces policy → a compliance breach *or* care blocked). The **policy-authority
  model and its provisioning bootstrap** are the sensitive seam — *who may change policy is who may weaken a
  safety floor* — so authoring must be authority-gated, audited, and floor-protected (a local actor can never
  author *below* a federation floor it is bound to). The shipped mechanism set must stay **closed and
  additive-only**.
- **The bet.** That hard policy across all real deployments fits **declarative selection over a closed shipped
  mechanism set** — that no deployment needs arbitrary enforcement logic inside the trusted floor. We would
  know it is wrong if real compliance demanded an enforcement rule that is neither in the shipped set nor
  expressible as a vetted distribution-plane extension — which would be a signal to add a *mechanism rung
  deliberately*, never to open a code hole on the data plane.
- **Policy-neutral by construction (principle 9)** — this *is* the mechanism of principle 9. **No new founding
  principle; no new event stream** (policy rides the existing overlay/event plane). Refines
  [ADR-0021](0021-layering-the-node-api-and-ui-pluralism.md) and closes its layering/API arc
  (0021 → 0022 → 0023 → 0024).
