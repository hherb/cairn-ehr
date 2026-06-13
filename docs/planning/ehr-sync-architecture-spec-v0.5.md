# Offline-Resilient Health Record System — Macroscopic Architecture Spec

**Status:** Draft v0.5 — replaces language fixation with a language-selection principle (was v0.4)
**License target:** AGPL-3.0 (all components must be AGPL-3.0-compatible)
**Core constraint:** Full clinical functionality must survive loss of internet *and* intranet, degrading gracefully down to a single workstation.

**Changelog v0.4 → v0.5**
- Added **§9 Language & Substrate Selection Principle**: choose implementation language by defect blast radius, not team habit; security/safety-sensitive logic → Rust or in-database; everything else → fit-for-purpose. Auditability/reviewer-legibility elevated to the primary quality metric (rationale: AI-assisted development shifts the binding constraint from authorship fluency to specification + review)
- Technology table (now §10) de-fixated from specific languages to role + candidate substrate, with the principle governing selection
- "In-database (SQL/PL-pgSQL)" promoted to a first-class member of the safety-critical bucket, not a footnote
- Open questions updated: per-component substrate selection + in-database merge-logic boundary

**Changelog v0.3 → v0.4**
- **Paper-Parity Principle** added as design goal 0 and normative §1.2: no clinical workflow may be slower, harder, or more cognitively demanding than its paper-era equivalent (malfeasance excluded); operationalized as a falsifiable benchmark test
- Armed write-context (§5.8.4, §11.9) reframed around possession semantics — paper's physical possession *was* the write context
- Performance budget consequence noted: parity binds architecture (local-first reads), not just UI (§1.2)

**Changelog v0.2 → v0.3**
- Reattribution generalized: event-granular primitive with three-tier adjudication workflow; misfiled documentation (wrong-chart entry) as the primary high-frequency use case (§5.5)
- Contamination cascade: reattribution triggers alert recomputation and viewed-by notifications; disclosure-scope query named as a feature (§5.5)
- Auto-escalation for events with executed real-world effects (§5.5)
- Normative wrong-chart *write* prevention requirements: armed write-context model, persistent patient identity on input surfaces (§5.8)
- Open questions updated (§10)

**Changelog v0.1 → v0.2**
- Demographics redesigned from "field-level LWW record" to append-only assertion stream with per-field projection policies (§4)
- Identity subsystem specified: linkage layer, matching pipeline, registration classes, identity event algebra, chart trust states (§5)
- John Doe / false-identity / pseudonymous care baked into the root model (§5.4–5.6)
- Open questions updated (§10)

---

## 1. Vision & Design Goals

0. **Paper-Parity Principle (governing law).** No clinical workflow may be slower, more difficult, more cognitively demanding — or impossible — compared to its paper-record equivalent. Clinician resistance to computerized records is rooted in exactly such regressions. See §1.2 for the normative form. Sole exclusions: capabilities of paper that constitute malfeasance (silent falsification, untraceable backdating, removable pages), and friction legally mandated regardless of medium — which must still cost no more than its paper equivalent.
1. **Availability over consistency.** During a network partition, a clinician must always be able to read the locally relevant record set and write new clinical data. We explicitly accept eventual consistency (AP in CAP terms) and design the data model so that this is clinically safe.
2. **Fractal topology.** The same software stack runs at every tier — national/regional hub, hospital, department, single practice, individual workstation. A node's role is configuration, not a different product.
3. **Context-scoped replication.** Each node holds the subset of records relevant to its local context ("sync scope").
4. **Resource-proportional deployment.** Runs on Raspberry-Pi-class hardware with intermittent power and 2G/3G-grade connectivity, and scales to a tertiary-care cluster — same codebase.
5. **Vendor independence.** No proprietary services, no mandatory cloud, no license keys. Commodity x86/ARM hardware, standard Linux, PostgreSQL ≥ 18.
6. **Identity is a claim, never a fact.** Patients are sometimes unidentifiable (unconscious) and sometimes deliberately misidentified. Prevention cannot be complete; therefore identity *repair* is a first-class, cheap, fast, forensically clean operation (§5).

### 1.2 The Paper-Parity Test (normative)
- **Falsifiable form:** every clinical workflow must name its paper-era equivalent and benchmark against it in **time, steps, and cognitive load**. A workflow that loses to paper is a design defect and is tracked as one.
- **Honest accounting:** the benchmark measures the *lived* workflow — shared-workstation authentication, system latency under load, and interruption/resumption included. Paper's baseline (grab chart from rack, write) included no login and no lag.
- **Architectural consequence:** parity failures caused by round-trip latency are architecture defects, not UI defects. Local-first reads/writes against the node's own database (§2, §6) are the structural answer; paper parity and offline-first are the same requirement seen from two angles.
- **Floor, not target:** the principle forbids regressions relative to paper; it never argues against digital gains (simultaneous multi-site access — paper's one unfixable flaw, legibility, search, decision support, §5.5 disclosure-scope queries).
- **Design heuristic:** when a digital workflow breeds errors paper didn't (e.g., the wrong-chart misfile), first ask which physical affordance of paper suppressed the error (possession: one chart in one hand) and restore its semantics, before adding confirmations or alerts.

Existing instances of the principle already in this spec: offline-first operation (paper never had downtime), break-glass access ("pulling the chart"), Tier-1 self-correction (the strike-through, which no hospital countersigns), the append-only event log (how a paper record legally behaves).

### Non-goals (for this phase)
- Billing/claims, imaging PACS, full HL7v2 interface engine — integration points, not core.
- Real-time collaborative editing (Google-Docs-style). We need conflict *safety*, not character-level merging.
- Biometric matching in the core (see §5.7 — supported as a pluggable identifier system only).

---

## 2. Topology

```
                 [ National / Regional Hub ]        (optional tier)
                          │
              ┌───────────┴───────────┐
        [ Hospital A ]          [ Practice B ]      (facility tier)
              │                       │
     ┌────────┴────────┐         [ Workstations ]   (full mirror of practice DB)
[ Dept: ED ]      [ Dept: ICU ]                     (department tier)
     │
[ Workstations / Carts / Tablets ]                  (edge tier)
```

- Hub-and-spoke per tier, hierarchical overall. Peer sync between siblings is a later extension; the event-log design keeps the door open.
- **Every node is write-capable** (multi-master, not read replicas).
- Edge nodes run full PostgreSQL (workstation/mini-PC) or an embedded store (PGlite/SQLite on tablets) — same sync protocol.

---

## 3. Data Model Principles

### 3.1 Append-only clinical event log (source of truth)
- All clinical content (notes, observations, orders, results, administrations, signatures, addenda) is written as **immutable, signed events**. Corrections are new events referencing the original — matching medico-legal documentation norms.
- Immutable events cannot conflict; merging divergent logs is **set union**. This eliminates the bulk of the multi-master problem by construction.
- Current state ("the chart") is a **projection** materialized per node — rebuildable, cacheable, never synced itself.

### 3.2 Identity & time
- **UUIDv7 primary keys everywhere** (native `uuidv7()` in PostgreSQL 18) — globally unique, offline-generable, time-ordered.
  - Collision risk is negligible mathematically (74 random bits/ms); the real vectors are engineering defects. Mitigations: server-side generation only (Postgres/PGlite `uuidv7()`), entropy-readiness gate at boot, identity regeneration in the node provisioning ceremony. Backstop: PK conflicts with mismatched content hashes are quarantined to a repair queue, never silently merged.
  - UUIDv7 leaks creation timestamps by construction → raw UUIDs are not exposed in patient-facing URLs/documents.
- **Hybrid Logical Clocks (HLC)** on every event — causal ordering tolerant of skewed wall clocks on off-grid hardware.

### 3.3 Mutable non-demographic state
| Data class | Merge policy |
|---|---|
| Allergies, alerts | **Union, never auto-delete.** Removal requires explicit reconciliation event. |
| Problem & medication lists | Union + flagged for clinician reconciliation on conflict |
| Scheduling / bed management | Authoritative-node ownership (the owning tier wins) |

(Demographics moved to §4 — they are no longer modeled as a mutable record.)

### 3.4 Interoperability
- Internal schema is event-sourced relational; a **FHIR R4/R5 façade** provides import/export and interop. FHIR is the interface, not necessarily the storage model (open question §11).

---

## 4. Demographics — Assertion Stream Model

Demographics are matching evidence as much as they are display data. Overwriting them (LWW storage) destroys evidence (maiden names, old phone numbers, prior transliterations). Therefore:

### 4.1 Demographic assertions
Each change is an immutable **assertion event**: *source S asserts at HLC t that field F of patient P has value V, with provenance class C.* Displayed demographics are a projection. Sync is set union, conflict-free.

**Provenance ladder:** document-verified > patient-stated > third-party-stated > clinician-observed > imported/unknown > inferred. Capturing provenance must cost the registrar one tap.

### 4.2 Per-field projection policy
| Field | Nature | Projection rule | Conflict across linked records means |
|---|---|---|---|
| Names | Multi-valued set (legal, maiden, alias, transliteration) | All retained; display = highest-provenance recent legal name | Weak evidence |
| DOB | Stable, precision-aware: `(value, precision, basis)` | Provenance beats recency; verified value locks vs. lower provenance | **Strong evidence against link** |
| Sex / gender | Three fields: sex-at-birth, administrative sex, gender identity | Sex-at-birth provenance-locked; gender identity patient-stated authoritative, recency wins | Sex-at-birth conflict: strong evidence against link |
| Identifiers (national ID, insurance, program IDs) | Multi-valued set keyed by issuing system | Set union, never LWW | Same-system different-value = **very strong evidence against link** |
| Phone, address | Volatile | Recency (HLC) wins; history retained | Nearly meaningless |
| Deceased status | Safety-asymmetric | Sets easily, never auto-clears; reversal = explicit human event | Strong evidence against link |
| Photo | Optional; powerful in low-ID settings | Append-only gallery, newest displayed | Human-reviewable evidence |

Notes:
- DOB precision is first-class ("age about 40, recorded 2026-06"). Default 01-01 birthdays are down-weighted by the matcher (overrepresented in low-resource registries).
- Conflicting "corrections" at equal provenance during a partition are **not** auto-resolved: project prior stable value, flag for human review. Rule: *recency resolves volatile fields; humans resolve identity-bearing fields.*

---

## 5. Identity Subsystem

### 5.1 Linkage layer — never merge, always link
- Patient UUIDs are immortal and immutable; clinical events reference their original UUID forever (sole exception: reattribution overlay, §5.5).
- Identity is an append-only stream of **link / unlink assertions** with provenance, HLC, confidence.
- The "person" (golden identity) is a projection: the connected component of the link graph. The unified chart unions the event streams of all member UUIDs.
- Consequences: merges sync trivially (events union); redundant links are idempotent; **unmerge is always possible and clean** (split the component; nothing was rewritten).

### 5.2 Matching pipeline (safety-asymmetric: false merge ≫ worse than false split)
- **Deterministic tier:** exact match on a strong identifier → auto-link with provenance.
- **Probabilistic tier:** Fellegi–Sunter-style scoring; conservative auto-link threshold; wide middle band raises a "possible duplicate" banner on both charts (surfacing safety content — allergies, active meds — without co-mingling) and queues human reconciliation.
- **Locale-pluggable comparators:** phonetic encodings, name structures, DOB precision handling, address semantics are deployment configuration, not hardcoded.
- **Where matching runs follows topology:** at registration within local scope (search-before-create); cross-facility at the lowest tier that sees both registrations (typically the hub); link events flow down through normal sync.
- **Coherence check (feedback loop):** the unified-chart projection continuously validates linked components against §4.2's conflict column. Contradictions (same-system identifier mismatch, verified-DOB clash, sex-at-birth clash) demote the link to human review and render the chart in *under-review* trust mode. Every new demographic assertion cheaply re-triggers local matching.

### 5.3 Registration classes
| Class | Use | Properties |
|---|---|---|
| **Standard** | Normal registration | Search-before-create enforced funnel |
| **Unidentified** | Unconscious/unknown patient ("John Doe") | §5.4 |
| **Pseudonymous (sanctioned)** | Legally permitted anonymous/protective care | §5.6 |

Registrations created during a partition are tagged and go to the **head of the upstream matching queue on reconnect** — post-partition reconciliation is a scheduled pipeline stage, not an error state.

### 5.4 Unidentified registration (John Doe) — baked into the root
- UUID minted immediately; care proceeds without delay.
- **System-generated callsign** (e.g. `Unknown-ED-<site>-<date>-A`), never plausible fake names; matcher excludes placeholder names from its feature space.
- Identity evidence captured as **clinician-observed assertions**: estimated age with basis, observed sex, photo, distinguishing marks, belongings, EMS pickup context — honest data, full matcher features.
- **Identity-pending is an active workflow state:** chart renders in *unconfirmed* trust mode ("no history available; allergies unknown"); matcher re-runs on every new evidence assertion.
- Resolution = **identification event** (who, method) + ordinary **link assertion** if a prior chart exists. On link during an active encounter, the system **pushes an alert**: "prior history now available — N allergies, M active medications — review now."
- Partition-safe by construction: registration and care are local; identification may occur at hub tier; the link event syncs down normally.

### 5.5 Reattribution — one primitive, tiered workflows

The **reattribution event** — "event set E belongs to UUID-B, not UUID-A" — is an immutable overlay all projections respect (digital strike-through: originals stay in place, excluded from the source chart's projection, visible in its chart-history view). It is **event-granular** (a single note, observation set, or order can move). Granularity lives in the primitive; *risk control lives in the workflow tier*:

| Tier | Use case | Conditions (enforced automatically) | Adjudication |
|---|---|---|---|
| **1 — Self-correction** | Misfiled documentation: clinician with multiple charts open saves into the wrong one (high-frequency, often ≥ weekly per clinician) | Author moves own event(s); within time window (same shift / 24 h, policy-config); destination patient in author's active care context (open/recent encounters) | None — one-click "move to correct patient," picker pre-filled with author's open charts. Full audit automatic. Friction target: < 10 seconds, or it competes with copy-paste-and-lose-provenance or with not fixing it at all |
| **2 — Supervised** | Not the author, window expired, or destination outside care relationship | Any Tier-1 condition unmet | One second sign-off (records officer / senior clinician) |
| **3 — Forensic** | Identity theft, disputes (§5.5b), adversarial cases | — | Two-person rule; adjudication queue; affected events render *under-review* on both charts until resolved |

**Auto-escalation:** any event with executed real-world effects (administered medication, performed procedure, transfused product) is barred from Tier 1 and escalates with an incident-workflow flag. Reattribution records documentation truth; it must never paper over a clinical incident.

**Contamination cascade (mandatory on reattribution arrival, local or via sync):**
- Recompute decision support / alerts on both source and destination charts.
- Notify every user who **viewed or acted on** the misfiled content during the exposure window ("a note you read on patient B at 14:32 has been moved to patient A"). Generated locally on each node as the event lands → partition-safe by construction.
- **Disclosure-scope query as a named feature:** exposure window + viewer list is a single query over the append-only audit log (GDPR/HIPAA breach-scoping in seconds, not weeks).

**(a) Fabricated persona (deliberate false identity):** confession → link assertion to real chart + **repudiation events** marking false assertions. Repudiated values leave the displayed projection but enter a **known-alias pool** retained by the matcher (aliases are reused). The fact of presentation under a false name is preserved (medico-legally required).

**(b) Identity theft (events on victim's chart):** Tier-3 reattribution of the affected encounter(s). **Dispute event** as the patient/victim-initiated front door ("I was never there in March"), feeding the review queue.

### 5.6 Pseudonymous (sanctioned) care
- Covers legally permitted anonymous STI testing, protective aliases (domestic violence), staff treated at their own facility.
- Deliberately unlinked; flagged internally; later linking is **patient-initiated and consent-gated**.
- **Link assertions may carry a visibility scope; linking must never silently broaden access.** A sequestered episode joins the person's connected component (enabling e.g. interaction checking) without its contents flooding every chart view. Identity linkage and consent scoping intersect at the link event — this is an architectural invariant, not an edge case.

### 5.7 Identity event algebra (closed set; all append-only, syncable, auditable)
| Event | Resolves | Adjudication |
|---|---|---|
| `assert` | Registration & demographic updates | Automatic |
| `link` / `unlink` | Duplicates, John Doe identification, confessions | Auto above threshold, else human |
| `identify` | Identity-pending → confirmed | Human; method recorded |
| `repudiate` | Known-false assertions → alias pool | Human |
| `reattribute` | Misfiled documentation; wrong-chart contamination; identity theft | Tiered: self-service (author, windowed) / one sign-off / two-person rule (§5.5) |
| `dispute` | Patient-initiated review | Triage to queue |

**Chart trust states (projection-side contract):** *confirmed* / *unconfirmed* (identity-pending) / *under-review* (coherence failure, open dispute, pending reattribution). The chart always tells the clinician how much to trust the identity behind it.

**Biometrics:** excluded from core (vendor/AGPL minefield; poor offline performance on constrained hardware). Accommodated as one more identifier system in the multi-valued set via a pluggable module. The core must work with names, dates, photos, and human judgment alone.

### 5.8 Registration & documentation workflow (normative)
1. **Search-before-create enforced funnel:** "new patient" unreachable until local-scope matching has run; candidates shown with photo/age/locale/last visit; the create button records that N near-matches were displayed.
2. **Partition-aware duplicate expectation** (see §5.3).
3. **Wrong-chart protection at point of care (read side):** demographic banner always shows photo + age + provenance-flagged identifiers; cheap "confirm patient" affordances emit verification assertions, raising provenance as a side effect of normal care.
4. **Wrong-chart protection at point of documentation (write side):** every input surface carries persistent patient identity (photo, name, age, per-patient color coding consistent across all open windows). Documentation is bound to an explicit **armed write-context** designed on **possession semantics** (paper precedent: you physically held one chart; the misfile is a disease of windowing, which abstracted possession away). One chart is "in hand" for writing at a time; picking it up is a single natural gesture; which chart is held is as unmissable as the color of a folder. Confirmation dialogs are explicitly *not* the mechanism (they fail the paper-parity test, §1.2). Cross-window paste of patient-bound content is flagged at paste time.

---

## 6. Synchronisation Layer

### 6.1 Mechanism
- Transport-agnostic, resumable, delta-based protocol over HTTPS; optional store-and-forward via removable media ("sneakernet sync") for fully disconnected sites.
- Built on **PostgreSQL logical replication / logical decoding** as the change-capture primitive, with an application-level sync service implementing scoping, filtering, conflict policy, and event-log semantics. Per §9, this safety-critical core is implemented in Rust and/or in-database, not a dynamic language. No hard dependency on third-party multi-master extensions (candidates to borrow from in §10).
- **Sync scopes** are declarative subscription predicates, evaluated at the parent, versioned, auditable.
- Bandwidth discipline: compression, binary diffs, attachments synced lazily by reference with priority queues.
- **Upstream priority order:** new clinical events and audit events first; identity events (link/repudiate/reattribute) high priority; attachments last.

### 6.2 Consistency model
- Eventual consistency with causal ordering (HLC) within a patient record.
- Every projection displays a **freshness indicator** ("last synced with parent 4 h ago") — a first-class UI requirement.

### 6.3 Failure modes (designed-for)
| Failure | Behaviour |
|---|---|
| Internet down | Facility operates on facility server; queues outbound |
| Intranet down | Department server is local master for its scope |
| Department server down | Workstations operate standalone on mirrored scope |
| Node destroyed | Re-provision from parent; only unsync'd local events are at risk → aggressive upward sync priority |

---

## 7. Security & Compliance (macroscopic)

- **Encryption at rest** mandatory below facility tier (LUKS + per-database encryption).
- **Offline authentication:** cached short-lived credentials/certificates per device and user; offline access automatically narrows; break-glass with mandatory retrospective audit.
- **Audit log is an event stream**, syncing upstream at highest priority.
- mTLS between nodes; enrollment via explicit trust/provisioning ceremony (also regenerates machine identity and PRNG seed — see §3.2).
- **Visibility scopes on link events** (§5.6): access-control and identity-linkage decisions are coupled by design.
- Compliance posture (GDPR/HIPAA/national law) is configuration; core guarantees (encryption, audit, access control) are universal.

---

## 8. Deployment Profiles

| Profile | Hardware floor | Stack |
|---|---|---|
| Solo practice | 1× mini-PC + workstations | Full Postgres each machine; practice node = parent |
| Rural clinic (off-grid) | Raspberry Pi 5 class, solar | Postgres on Pi; sneakernet/3G sync to district |
| Hospital department | 1 small server | Postgres + sync service, scoped mirror |
| Hospital core | HA Postgres pair | Patroni-style failover; parent for departments |
| Regional/national | Cluster | Aggregation, registries, cross-facility matching, master patient index |

Packaging: single container image / Debian package per node; configuration declares tier, parent, sync scope. Zero-DBA target for lower tiers.

---

## 9. Language & Substrate Selection Principle

The spec deliberately does **not** fix implementation languages per component. It fixes the *rule* by which they are selected, so the choice is auditable and survives changing tooling.

### 9.1 Selection rule — by defect blast radius
**The cost of a defect dictates how much the language/substrate must prevent defects at compile time or by construction.**

- **Safety-critical bucket** (a defect can silently corrupt the record, mis-merge patients, leak data, or crash an unattended node): implement in **Rust or in-database (SQL / PL-pgSQL / constraints)**. These make whole error classes unrepresentable — memory safety, exhaustive sum-type matching, no runtime metaprogramming, or database-enforced invariants that no buggy caller can bypass. Members: the sync/merge engine, the identity event algebra and projections (§5.7), HLC ordering, coherence checks, audit-log integrity, access-control enforcement.
- **Fit-for-purpose bucket** (a defect is caught immediately, is advisory, or is cosmetic): optimize for iteration speed and ecosystem. Members: probabilistic matcher / record linkage (advisory — proposes candidates, humans/policy decide; Python's ML ecosystem is decisive here), FHIR façade and integration glue, tooling, UI backends.

In-database is a **first-class member of the safety bucket, not a footnote**: for some merge/projection logic, a constraint or PL/pgSQL routine next to the data is safer and more auditable than any application-layer code in any language, because the invariant is enforced unconditionally and cannot be bypassed.

### 9.2 Primary quality metric — reviewer-legibility
With AI-assisted development, the binding constraint shifts from *authorship fluency* to *specification + review*: comparable results are achievable with far smaller competent teams, and individual per-language coding skill matters much less between design spec and final review. Therefore:
- The artifacts that gate quality are the **specification** and the **review**, not the typing.
- Safety-critical layers are optimized for **auditability / reviewer-legibility**, even over authorship speed. Rust ("the types document the invariants") and in-database ("the logic sits next to the data it governs") both score high on this axis.
- Concentrating safety-critical logic in a small, well-bounded set of restrictive-language components **shrinks the audited surface** — the part needing the most rigorous review is also the smallest. This directly serves the small-team reality.

### 9.3 Integration boundary
Polyglot is expected ("horses for courses"). To avoid fragile coupling, **the language boundary is the database boundary**: each component talks to its node's PostgreSQL; Postgres is the integration substrate. (E.g. the Python matcher writes link-candidate events; the Rust/in-database core consumes them — loose coupling, no FFI.)

---

## 10. Technology Candidates (all AGPL-3.0-compatible)

Selection governed by §9. "Substrate" reflects the §9.1 bucket; specific frameworks are illustrative, not fixed.

| Role | Candidate / reference | Substrate bucket | License | Note |
|---|---|---|---|---|
| Database | PostgreSQL ≥ 18 | — (foundation) | PostgreSQL (permissive) | uuidv7(), async I/O, logical replication |
| Change capture | Logical decoding (`pgoutput` / wal2json) | safety / in-database | PostgreSQL / BSD | Core primitive |
| Sync / merge engine | (custom) | **safety → Rust or in-database** | — | Unattended, concurrent, safety-critical, constrained |
| Identity algebra & projections | (custom) | **safety → Rust or in-database** | — | §5.7; exhaustive matching / DB constraints |
| Multi-master reference | pgactive | safety | Apache-2.0 | Borrow patterns; evaluate vs. custom |
| Heterogeneous sync reference | SymmetricDS | safety | GPL-3.0 | GPLv3↔AGPLv3 compatible |
| Edge/in-browser store | PGlite | — | Apache-2.0 | Postgres-in-WASM for tablets/web |
| Read-path sync reference | ElectricSQL | — | Apache-2.0 | Shape-based partial replication patterns |
| Record linkage / matcher | Splink + custom | **fit-for-purpose → Python** | MIT | Advisory; Fellegi–Sunter; ML ecosystem |
| FHIR façade / interop | HAPI FHIR / fhir.resources | fit-for-purpose | Apache-2.0 / BSD | Interface, not merge core |
| Integration glue / tooling / UI backends | (various) | fit-for-purpose | permissive | Iteration speed prioritized |

---

## 11. Open Questions (next brainstorming targets)

1. **Build vs. adapt the sync backbone:** custom logical-decoding service vs. pgactive/SymmetricDS core. Interacts with §11.11 (how much merge logic lives in-database thins the orchestrating daemon).
2. **Storage model:** FHIR-native JSONB vs. normalized relational with FHIR façade (Pi-class performance vs. interop friction).
3. **Dynamic sync scopes:** patient transferred ED→ICU mid-partition; who owns scope reassignment during a partition?
4. **Schema migrations across a fleet of offline nodes:** version-skew tolerance window; forward-compatible event formats.
5. **Tombstones & retention:** legal deletion (GDPR erasure) in an append-only, multi-copy system — interacts with repudiation/reattribution overlays.
6. **Attachment strategy:** inline vs. content-addressed blob store with lazy sync.
7. **Locale-pluggable matcher comparators:** define the extension point (comparator API, weight configuration, evaluation harness per deployment).
8. **Visibility-scope semantics on links:** how scoped links interact with sync scopes (does a sequestered episode replicate to a node at all?).
9. **Armed write-context interaction model:** concrete possession-semantics design (§5.8.4) that passes the paper-parity benchmark at ED pace — "picking up a chart" must cost ≤ its paper equivalent (~seconds, zero cognitive overhead) without degrading into reflexive click-through.
10. **Notification economy:** contamination-cascade and history-arrival alerts (§5.4, §5.5) are safety-critical but additive; define a priority taxonomy so they don't drown in routine noise.
11. **In-database vs. application-layer merge boundary:** which parts of merge/projection logic belong in PostgreSQL (constraints/PL-pgSQL, unbypassable) vs. an orchestrating Rust core (§9.1). Governs how thin the daemon can be.
12. **Authentication vs. paper-parity tension:** shared-workstation login is the largest parity violation in deployed EHRs (§1.2 vs. §7); adjudicate explicitly — fast/proximity sessions enabled by local-first state vs. security posture.

---

## 12. Guiding Principles (three sentences now)

> Make the clinical data model append-only and causally ordered, so that synchronisation becomes set union plus a small, explicitly enumerated list of clinically-reasoned merge policies.

> Treat patient identity as a claim under continuous evaluation — never merge, always link; never erase, always overlay — so that every identity error, accidental or deliberate, is repairable by an auditable event.

> Select each component's language and substrate by the blast radius of its defects, not by team habit — pushing safety-critical logic into Rust or the database where whole error classes become unrepresentable, and optimizing those layers for reviewer-legibility, because specification and review are now the binding constraint.
