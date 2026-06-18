# 9. Language & Substrate Selection Principle

The spec deliberately does **not** fix implementation languages per component. It fixes the *rule* by which they are selected, so the choice is auditable and survives changing tooling.

## 9.1 Selection rule — by defect blast radius
**The cost of a defect dictates how much the language/substrate must prevent defects at compile time or by construction.**

- **Safety-critical bucket** (a defect can silently corrupt the record, mis-merge patients, leak data, or crash an unattended node): implement in **Rust or in-database (SQL / PL-pgSQL / constraints)**. These make whole error classes unrepresentable — memory safety, exhaustive sum-type matching, no runtime metaprogramming, or database-enforced invariants that no buggy caller can bypass. Members: the sync/merge engine, the identity event algebra and projections ([§5.7](identity.md#57-identity-event-algebra-closed-set-all-append-only-syncable-auditable)), HLC ordering, coherence checks, audit-log integrity, access-control enforcement.
- **Fit-for-purpose bucket** (a defect is caught immediately, is advisory, or is cosmetic): optimize for iteration speed and ecosystem. Members: probabilistic matcher / record linkage (advisory — proposes candidates, humans/policy decide; Python's ML ecosystem is decisive here), FHIR façade and integration glue, tooling, UI backends.

In-database is a **first-class member of the safety bucket, not a footnote**: for some merge/projection logic, a constraint or PL/pgSQL routine next to the data is safer and more auditable than any application-layer code in any language, because the invariant is enforced unconditionally and cannot be bypassed.

## 9.2 Primary quality metric — reviewer-legibility
With AI-assisted development, the binding constraint shifts from *authorship fluency* to *specification + review*: comparable results are achievable with far smaller competent teams, and individual per-language coding skill matters much less between design spec and final review. Therefore:
- The artifacts that gate quality are the **specification** and the **review**, not the typing.
- Safety-critical layers are optimized for **auditability / reviewer-legibility**, even over authorship speed. Rust ("the types document the invariants") and in-database ("the logic sits next to the data it governs") both score high on this axis.
- Concentrating safety-critical logic in a small, well-bounded set of restrictive-language components **shrinks the audited surface** — the part needing the most rigorous review is also the smallest. This directly serves the small-team reality.

## 9.3 Integration boundary
Polyglot is expected ("horses for courses"). To avoid fragile coupling, **the language boundary is the database boundary**: each component talks to its node's PostgreSQL; Postgres is the integration substrate. (E.g. the Python matcher writes link-candidate events; the Rust/in-database core consumes them — loose coupling, no FFI.) The same boundary, extended *outward* to external clients and a plurality of UIs, is [§9.5](#95-layering-the-node-api-and-ui-pluralism-uniform-core-plural-edges).

## 9.4 Merge / projection boundary — fat Postgres, thin Rust daemon
> Resolves former open question §11.11 — see [ADR-0001](decisions/0001-fat-postgres-thin-daemon.md).

The [§9.1](#91-selection-rule-by-defect-blast-radius) safety bucket is divided concretely. Because every autonomous node runs full PostgreSQL ([§2](topology.md)), in-database logic runs *everywhere* — so safety-critical merge/projection logic is placed where it is **unbypassable and reviewer-legible**: next to the data.

**In Postgres** (constraints / PL-pgSQL / triggers — unbypassable, on every node incl. Pi):
- **Append-only enforcement:** the application role is granted INSERT/SELECT only on event tables; a trigger raises on any UPDATE/DELETE as defense-in-depth.
- **Idempotent apply:** `INSERT … ON CONFLICT (uuid) DO NOTHING` makes sync a clean set-union; a PK collision whose content hash differs is routed to the repair/quarantine queue, never silently merged ([data-model §3.2](data-model.md#32-identity-time) backstop).
- **Structural invariants:** HLC monotonicity / causal-ordering checks, the closed `event_type` enum, FK integrity.
- **The identity event algebra** (deterministic application of link / unlink / reattribute / identify / repudiate / dispute, [§5.7](identity.md#57-identity-event-algebra-closed-set-all-append-only-syncable-auditable)) and **all projections**: the chart projection (union of member-UUID event streams), the [§3.3](data-model.md#33-mutable-non-demographic-state) mutable-list unions, the [§4.2](demographics.md#42-per-field-projection-policy) demographic projection, the golden-identity connected-component ([§5.1](identity.md#51-linkage-layer-never-merge-always-link)), the coherence check ([§5.2](identity.md#52-matching-pipeline-safety-asymmetric-false-merge-worse-than-false-split), demotes to *under-review* on a [§4.2](demographics.md#42-per-field-projection-policy) conflict), the chart trust states ([§5.7](identity.md#57-identity-event-algebra-closed-set-all-append-only-syncable-auditable)), and the reattribution overlay ([§5.5](identity.md#55-reattribution-one-primitive-tiered-workflows)).
- **Projections are trigger-maintained incremental tables** (`AFTER INSERT` only — the INSERT-only log means no update/delete maintenance path), **not** periodic `REFRESH MATERIALIZED VIEW`; this keeps per-write cost low on Pi-class hardware.

**In Rust** (the thin sync daemon, [§6.1](sync.md#61-mechanism) — *ships and applies, never decides*): logical-decoding consumer, scope-predicate evaluation, the [§6.1](sync.md#61-mechanism) priority queue, resumable mTLS/HTTPS transport and store-and-forward, idempotent apply. **No merge logic.**

**In Python** (advisory only, [§5.2](identity.md#52-matching-pipeline-safety-asymmetric-false-merge-worse-than-false-split) / [§10](technology.md)): the probabilistic matcher *proposes* link candidates by writing candidate events; it never decides. Authoritative application and projection are in-DB. The Python↔in-DB seam is exactly the [§9.3](#93-integration-boundary) database boundary.

> [!WARNING]
> **The load-bearing bet:** that in-DB projections stay cheap enough on Pi-class hardware to keep
> chart reads local and fast (the [§1.2](vision.md#12-the-paper-parity-test-normative) paper-parity
> floor). A Pi serves only a handful of workstations with little concurrency, so the risk is
> single-operation latency on a weak CPU + SD/USB storage, not throughput. The first implementation
> spike is a Pi benchmark that validates or falsifies it.

**Escape hatch — an in-database escalation ladder, never leaving Postgres** (see [ADR-0002](decisions/0002-in-database-rust-pgrx-escape-hatch.md)):

1. **PL/pgSQL** — the default; most legible for set-oriented projection logic; no build step.
2. **Rust via pgrx (in-database)** — when a function is hot or algorithmically complex (the identity connected-component is the prime candidate). Compiled-Rust speed and type-safety while the function **stays a Postgres function** — next to the data, unbypassable, invoked by the same triggers, inside the [§9.3](#93-integration-boundary) database boundary. So "Rust" and "in-database" are one bucket, not two.
3. **External Rust** — only if logic genuinely cannot be a database function (not expected for projections).

The decision is per-projection and criteria-gated (Pi single-op latency, reviewer-legibility, bypassability), not an upfront blanket split. The thin sync daemon still carries no merge logic.

## 9.5 Layering, the node API, and UI pluralism (uniform core, plural edges)
> See [ADR-0021](decisions/0021-layering-the-node-api-and-ui-pluralism.md). Extends the [§9.3](#93-integration-boundary) integration boundary *outward* to external clients and UIs, and is the home of **founding principle 12**. No new event stream.

The whole mission rests on one guarantee — **any Cairn node must interoperate with any other, regardless of which UI or policy a deployment runs** — and that guarantee must survive a *plurality* of front-ends (the design goal: small teams or individuals building bespoke UIs quickly and safely, with the steward's own opinionated "best-of-breed" UI just *one* citizen among them). The resolution is to recognise that the inter-node contract is the **signed event core, below UI and policy**, and to enforce the floor where no client can bypass it.

**Four layers, with the compatibility boundary below the application layer:**

| | Layer | Holds | Uniform / plural |
|---|---|---|---|
| **L0** | Wire/event core — *the compatibility contract* | the signed-event format ([ADR-0015](decisions/0015-event-serialization-signatures-and-content-addressing.md)), set-union sync + HLC ([§6](sync.md)), the identity/actor algebras ([§5.7](identity.md#57-identity-event-algebra-closed-set-all-append-only-syncable-auditable)/[§3.12](data-model.md#312-actor-identity-in-the-registry)), additive-only evolution + the legibility twin ([ADR-0012](decisions/0012-schema-evolution-event-format-and-legibility-across-time.md)), federation peering ([ADR-0017](decisions/0017-federation-admission-sovereignty-peering-and-trust-anchors.md)) | **uniform** (non-negotiable) |
| **L1** | Node core — *the enforcement floor* | fat Postgres + in-DB/pgrx safety logic ([§9.4](#94-merge-projection-boundary-fat-postgres-thin-rust-daemon)): validation, signature/content-address checks, the algebras, projections, access-control (RLS), append-only | **uniform** (unbypassable) |
| **L2** | Policy / application | thin Rust: the stable node **API**, deployment **hard policy** cleaner in Rust than SQL, fit-for-purpose orchestration | plural |
| **L3** | UI | many UIs — soft policy, workflow, presentation; the reference UI is one citizen | plural |

- **The contract is L0, and nothing above it sits on the inter-node path.** The only thing that crosses a node boundary is signed events over the sync protocol; API, policy and UI are forbidden from that path by construction. Compatibility is therefore a property of the **core**, not of the application.
- **The floor is in the database — so direct DB access is safe, and the bypass tension dissolves.** Every safety/compatibility-critical invariant is enforced *in the DB* (the validated write path + RLS + constraints), never only in L2: a client talking raw SQL still cannot write a malformed/unsigned event, mis-attribute authorship, bypass access control, or break the algebra. UIs never `INSERT` into event tables — they call a small set of **validated submit functions** (the [§9.4](#94-merge-projection-boundary-fat-postgres-thin-rust-daemon) grant model extended: the UI role gets `EXECUTE` on submit-functions + `SELECT` on projection views, *not* raw `INSERT`). "Via the API" vs "DB directly" is thus a **privilege gradient, not a contradiction** — the more trusted/co-located a component, the lower it may bind; the floor is identical at every level. **L2 is ergonomics + deployment hard policy, never the sole wall**; residual Rust-only hard policy is enforced by role-gating raw access for *untrusted* clients.
- **Hard vs soft policy = [§9.1](#91-selection-rule-by-defect-blast-radius) blast radius applied to policy.** *Hard policy* (a deployment must not bypass it from the UI — an attestation requirement, a retention floor, two-person break-glass) lives **anchored in the DB**, or in L2 with raw access role-gated. *Soft policy* (presentation, workflow, salience, prefetch heuristics, optional confirmations, layout) lives in **L3**, swappable with zero blast radius. This is [principle 9](index.md#founding-principles-the-lens-for-every-decision) (mechanism vs policy), located on the stack.
- **The native API is additive, versioned, capability-described — the anti-drift guarantee.** A UI is a pure producer/consumer of events over a contract it cannot alter, so it can produce content wrong for its clinic but **never a wire-incompatible event**: the UI never owns serialization/signing (the *node* canonicalises + signs, [ADR-0015](decisions/0015-event-serialization-signatures-and-content-addressing.md)); the API evolves **additively only** (the [ADR-0012](decisions/0012-schema-evolution-event-format-and-legibility-across-time.md) discipline applied to the API — [principle 11](index.md#founding-principles-the-lens-for-every-decision) on the contract); it is **capability-described** so a UI degrades gracefully against an older/newer node (the [§3.13](data-model.md#313-schema-evolution-event-format-and-the-legibility-twin) `min(...)` ladder); and a published **conformance suite** makes "any node talks to any node" executable for a small team before it ships. The contract *properties* are fixed; the wire **transport** (REST/gRPC/SQL-over-the-wire/…) is a later fit-for-purpose choice ([§9](#9-language-substrate-selection-principle) fixes the rule, not the tech).
- **Native API ≠ the FHIR façade.** FHIR is a boundary skin for external/legacy interop ([§3.4](data-model.md#34-interoperability)); the native API is richer (events, projections, the identity algebra, possession/write-context, notifications-as-projections) and is what Cairn-native UIs bind to. Two surfaces, two jobs.
- **The reference UI is built only on the public API — anti-capture turned inward.** The steward's opinionated UI gets no private back door; it consumes the exact contract every third-party UI consumes, so it can never privilege itself or become a chokepoint.

> [!WARNING]
> The **validated submit-function surface + RLS + the role/grant model** are now part of the [§9.1](#91-selection-rule-by-defect-blast-radius) trusted base for *external* clients too — they **are** the floor for direct-DB callers. The one thing that must be complete is that surface: every legitimate write must be expressible through it, or UIs are pushed to raw access and the bypass re-opens. The submit-function → validated-append seam is the safety-critical path (the recurring seam motif).
