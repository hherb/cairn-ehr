# ADR-0000 — Pre-ADR changelog history (spec v0.1 → v0.6)

- **Status:** Imported (historical record)
- **Date:** 2026-06-13

## Context

Before this decision log existed, the architecture spec was a single file that carried its
rationale in **in-file changelogs at the top**, with the version encoded in the filename
(`…-spec-v0.6.md`). When the spec was split into one-file-per-aspect and this ADR log was adopted
([ADR-0001](0001-fat-postgres-thin-daemon.md)), those conventions were retired. *Nothing is
erased* — the original changelogs are preserved here verbatim as the pre-ADR record. New rationale
is captured as numbered ADRs from 0001 onward.

> Note: section numbers (§N) below refer to the single-file spec as it existed at the time. They now
> map to the per-aspect documents — see the [document map](../index.md#document-map).

## Imported changelogs (verbatim)

**Changelog v0.5 → v0.6**
- **Resolved the three entangled open questions §11.1 / §11.2 / §11.11** as one decision — *"Fat Postgres, thin Rust daemon."* They all turned on a single axis: how much clinical intelligence is enforced by Postgres itself vs. an orchestrating Rust core. Resolution distributed across §2, §3.5 (new), §6.1, §9.4 (new); §11 entries marked **resolved**.
- **§2 edge tier revised:** tablets are **thin clients**, not autonomous edge nodes. The smallest *autonomous* node (one that must survive a full partition alone) is a **Pi-class full PostgreSQL ≥18**. Consequence: PL/pgSQL, constraints, projection tables, and logical decoding are available on *every* computing node — the "must also run on PGlite/SQLite" portability constraint is removed, and in-database merge/projection logic becomes viable everywhere. PGlite/SQLite remain a *thin-client* surface only.
- **§3.5 (new) Storage model (§11.2 resolved):** **hybrid event envelope** — typed/normalized envelope columns where invariants, identity, sync, and matching bind; **Cairn-native JSONB** for clinical bodies; FHIR is a façade view/export, never the storage model.
- **§6.1 (§11.1 resolved):** **build** a thin custom Rust sync service on Postgres logical decoding; **borrow patterns from pgactive/SymmetricDS, do not depend on them** — their row-conflict machinery solves a problem Cairn designed away and can violate §4 anti-data-loss policies.
- **§9.4 (new) Merge boundary (§11.11 resolved):** structural invariants + the identity event algebra + all projections live **in Postgres** (unbypassable, next to the data, run on every node); the Rust daemon does transport/scope/priority/apply and **carries no merge logic**; the probabilistic matcher stays **Python and advisory**. Per-projection Rust escape hatch on measured Pi-performance need.

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
