# ADR-0021 — Layering, the node API, and UI pluralism: uniform core, plural edges

- **Status:** Accepted
- **Date:** 2026-06-17

## Context

The infrastructure stack was settled — fat Postgres + a thin Rust daemon
([ADR-0001](0001-fat-postgres-thin-daemon.md)), the defect-blast-radius selection rule and
*"the integration boundary is the database boundary"* ([§9.1](../language-substrate.md#91-selection-rule-by-defect-blast-radius)/[§9.3](../language-substrate.md#93-integration-boundary)).
But the spec said nothing about the **application/API layer above that core**, nor about how a
**plurality of user interfaces** can exist without threatening the single guarantee the whole mission
rests on: **any Cairn node must interoperate with any other, regardless of which UI or policy a
deployment runs.**

The shape the user proposed: an infrastructure core (fat Postgres + Rust/PL-pgSQL); **hard policy** in
a thin middle layer (Rust); **soft policy** in the UI; UIs reaching the system through an **API** —
"either the thin policy layer or the database directly." Diversity of UIs must be facilitated (small
teams or individuals building bespoke front-ends quickly and safely) **without ever compromising the
compatibility of the infrastructural core.** The steward's own baseline is a single opinionated
"best-of-breed" UI (tuned for the author's needs and, probably, most Australian GPs and hospital
doctors) — explicitly *one* UI among many, not a privileged one.

Two observations make the decision tractable:

- **Most of the compatibility guarantee is already secured, and is UI/policy-independent by
  construction.** What makes two nodes interoperable is the signed, append-only **event core**: the
  serialization/signature/content-addressing format
  ([ADR-0015](0015-event-serialization-signatures-and-content-addressing.md)), set-union sync + HLC
  ordering ([ADR-0001](0001-fat-postgres-thin-daemon.md), [§6](../sync.md)), the identity and actor
  algebras ([identity §5.7](../identity.md#57-identity-event-algebra-closed-set-all-append-only-syncable-auditable),
  [data-model §3.12](../data-model.md#312-actor-identity-in-the-registry)), additive-only schema
  evolution + the legibility twin ([ADR-0012](0012-schema-evolution-event-format-and-legibility-across-time.md)),
  and federation peering/revocation ([ADR-0017](0017-federation-admission-sovereignty-peering-and-trust-anchors.md)).
  None of these know what a UI is. So the task is **not to build compatibility — it is to *name* that
  core as the sole inter-node contract and forbid everything above it from sitting on the inter-node
  path.**
- **The one genuine tension is the bypass hole:** if a UI may talk to the database directly, then hard
  policy enforced *only* in a Rust middle layer is walk-around-able. Resolving that is the crux.

## Decision

Establish a four-layer model with the **compatibility boundary running below the application layer**,
and resolve the bypass tension by putting the floor in the database. Canonical home:
[language-substrate §9.5](../language-substrate.md#95-layering-the-node-api-and-ui-pluralism-uniform-core-plural-edges).
**No new event stream; one new founding principle (12).**

1. **The compatibility contract is the wire/event core (L0), and nothing above it may sit on the
   inter-node path.** The enumerated core above *is* the contract between nodes; policy, API, and UI are
   forbidden from the inter-node path by construction (the only thing that crosses a node boundary is
   signed events over the sync protocol). This is already true — this ADR *declares* it load-bearing and
   protects it, rather than inventing it.

2. **Four layers; the boundary is below L2.**
   - **L0 — wire/event core** (the compatibility contract; an invariant set, not a runtime). **Uniform
     across every node, non-negotiable.**
   - **L1 — node core / enforcement floor:** fat Postgres + in-DB/pgrx safety logic
     ([§9.4](../language-substrate.md#94-merge-projection-boundary-fat-postgres-thin-rust-daemon)) —
     event validation, signature/content-address verification, the algebras, projections, access-control
     (RLS), append-only enforcement. **Uniform; the unbypassable floor.**
   - **L2 — policy / application:** a thin Rust service — the stable node **API**, deployment **hard
     policy** that is cleaner in Rust than SQL, and fit-for-purpose orchestration. **Plural.**
   - **L3 — UI:** many UIs — soft policy, workflow, presentation. **Plural.** The reference UI is one
     citizen.

   L0/L1 are shared by every node; L2/L3 vary freely per deployment, vendor, or lone developer.

3. **The enforcement floor is in the database — which dissolves the bypass tension.** This is the
   [§9.4](../language-substrate.md#94-merge-projection-boundary-fat-postgres-thin-rust-daemon) move
   ("unbypassable, next to the data") extended to the *external client* boundary:
   - **(a)** Every safety/compatibility-critical invariant is enforced *in the DB* (the validated write
     path + RLS + constraints), never only in L2. A client talking raw SQL still cannot write a
     malformed/unsigned event, mis-attribute authorship, bypass access control, or break the algebra —
     the DB rejects it. **Direct DB access is therefore safe by construction** for the core invariants:
     the client is never trusted, and the trust boundary sits *below* the API.
   - **(b)** UIs never `INSERT` into event tables; they call a small set of **validated submit
     functions** (the [§9.4](../language-substrate.md#94-merge-projection-boundary-fat-postgres-thin-rust-daemon)
     grant model extended — the UI role gets `EXECUTE` on submit-functions + `SELECT` on projection
     views, *not* raw `INSERT`). Even "DB directly" goes through the validated write path.
   - **(c)** "Via the API" vs "DB directly" is a **privilege gradient, not a contradiction.** The more
     trusted/co-located a component, the lower it may bind; the floor is identical at every level. L2 is
     **ergonomics + deployment hard policy**, never the *sole* line of defence. Residual hard policy that
     is genuinely cleaner in Rust is enforced by **role-gating raw access** so the API becomes the only
     write path for *untrusted* clients — but the safety floor never depends on the API being in the loop.

4. **Hard policy vs soft policy — the [principle 9](../index.md#founding-principles-the-lens-for-every-decision)
   mechanism/policy split, located.** *Hard policy* is enforced and a deployment must not be able to
   bypass it from the UI (e.g. "un-vouched suppressing AI output must be attested before effect"
   [identity §5.10](../identity.md#510-authorship-and-responsibility-state-the-consumer-side); a
   retention-rung floor; two-person break-glass): it lives **anchored in the DB**, or in L2 with raw
   access role-gated. *Soft policy* is presentation, workflow, salience ranking, prefetch heuristics,
   optional confirmations, layout: it lives in **L3**, fit-for-purpose, swappable with zero blast radius.
   The dividing line is [§9.1](../language-substrate.md#91-selection-rule-by-defect-blast-radius)'s defect
   blast radius, applied to policy.

5. **The native API is an additive, versioned, capability-described contract — the anti-drift
   guarantee.** Small teams must be able to build bespoke UIs quickly *without risking compatibility or
   drift*. They can, because a UI is a pure **producer/consumer of events over a contract it cannot
   alter**, so it can produce content wrong for its clinic but never a wire-incompatible event:
   - The UI **never owns serialization or signing** — it submits through the validated path and the
     *node* canonicalises and signs ([ADR-0015](0015-event-serialization-signatures-and-content-addressing.md)).
     Two different UIs on two nodes always emit byte-compatible events.
   - The API evolves **additively only** — the [ADR-0012](0012-schema-evolution-event-format-and-legibility-across-time.md)
     schema discipline applied to the API surface (never remove or repurpose; deprecate by overlay). A
     UI built against API v1 keeps working — **[principle 11](../index.md#founding-principles-the-lens-for-every-decision)
     (legibility across time) applied to the contract.**
   - The API is **capability-described** — a node advertises its API version + optional capabilities, so
     a UI **degrades gracefully** against an older or newer node (the
     [data-model §3.13](../data-model.md#313-schema-evolution-event-format-and-the-legibility-twin)
     `min(...)` ladder, applied to the API).
   - A **published conformance suite** makes "any node talks to any node" *executable* — a small team
     verifies compatibility before shipping. The contract *properties* are fixed here; the wire
     **transport** (REST/JSON, gRPC, SQL-over-the-wire, …) is a later fit-for-purpose choice, the way
     [§9](../language-substrate.md) fixes the rule and not the tech.

6. **The native API is not the FHIR façade — two surfaces, two jobs.** FHIR stays a boundary skin for
   external/legacy interop ([data-model §3.4](../data-model.md#34-interoperability)); the **native** API
   is richer (events, projections, the identity algebra, possession/write-context, notifications-as-
   projections — none of which FHIR represents) and is the contract Cairn-native UIs are built on.

7. **The reference UI is built only on the public API — anti-capture turned inward.** The steward's
   opinionated "best-of-breed" UI gets **no private back door**: it consumes the exact contract every
   third-party UI consumes, so it can never accidentally privilege itself or become a chokepoint. It is
   *one* citizen of L3 — deliberately opinionated for Australian GP/hospital workflow, never the only way
   in. [Principle 9](../index.md#founding-principles-the-lens-for-every-decision)/vendor-independence enforced
   against the project itself.

8. **Founding principle 12 — uniform core, plural edges.** The integration contract is the signed event
   core, *below* UI and policy; the safety/compatibility floor is enforced unbypassably in the database;
   above it, UIs and policy may proliferate without ever threatening node interoperability. Many
   front-ends, one record.

## Consequences

- **Easier.** The anti-capture mission **survives UI diversity**: a thousand bespoke front-ends cannot
  fork the record, because none of them owns the wire contract. Small teams build safely (the
  conformance suite + the can't-emit-an-incompatible-event guarantee). The steward can ship an
  opinionated reference UI **without** becoming a vendor chokepoint. The "DB directly vs via the API"
  question stops being a security worry and becomes a deployment ergonomics choice.
- **Harder / new trusted surface.** The **validated submit-function surface + RLS + the role/grant
  model** are now part of the [§9](../language-substrate.md) trusted base for *external* clients too —
  they *are* the floor for direct-DB callers. Getting that submit surface **complete** matters: every
  legitimate write must be expressible through it, or UIs are pushed toward raw access and the bypass
  re-opens. The **API version/capability negotiation** and the **conformance suite** are new artifacts
  to build and maintain.
- **The bet.** That the in-DB floor + validated write path is complete and cheap enough that direct DB
  access never *needs* to be a bypass, and that **additive-only API evolution** holds for decades the way
  additive-only schema evolution must. We would know the bet is wrong if real UIs routinely need writes
  the submit surface cannot express (forcing raw table access and re-opening the hole), or if the API
  cannot stay additive without breaking-change churn.
- **Policy-neutral (principle 9) and vendor-independent (principle 7).** The layering is *mechanism*;
  which UIs run, which soft policies apply, and which hard-policy rungs a deployment enforces are its
  choice. **No new event stream; one new founding principle (12).**
