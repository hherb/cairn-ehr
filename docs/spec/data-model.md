# 3. Data Model Principles

## 3.1 Append-only clinical event log (source of truth)
- All clinical content (notes, observations, orders, results, administrations, signatures, addenda) is written as **immutable, signed events**. Corrections are new events referencing the original — matching medico-legal documentation norms.
- Immutable events cannot conflict; merging divergent logs is **set union**. This eliminates the bulk of the multi-master problem by construction.
- Current state ("the chart") is a **projection** materialized per node — rebuildable, cacheable, never synced itself.

> [!NOTE]
> Because the log is append-only and immutable, syncing the source of truth is INSERT-only,
> idempotent (UUIDv7 PK), scoped **set union** — there are no row-level clinical conflicts to
> resolve. All genuinely hard "merge" logic is confined to *derived* state (projections), which is
> rebuildable and never synced. This is the pivot the whole sync/merge design turns on
> ([ADR-0001](decisions/0001-fat-postgres-thin-daemon.md)).

## 3.2 Identity & time
- **UUIDv7 primary keys everywhere** (native `uuidv7()` in PostgreSQL 18) — globally unique, offline-generable, time-ordered.
  - Collision risk is negligible mathematically (74 random bits/ms); the real vectors are engineering defects. Mitigations: server-side generation only (Postgres/PGlite `uuidv7()`), entropy-readiness gate at boot, identity regeneration in the node provisioning ceremony. Backstop: PK conflicts with mismatched content hashes are quarantined to a repair queue, never silently merged.
  - UUIDv7 leaks creation timestamps by construction → raw UUIDs are not exposed in patient-facing URLs/documents.
- **Hybrid Logical Clocks (HLC)** on every event — causal ordering tolerant of skewed wall clocks on off-grid hardware.

## 3.3 Mutable non-demographic state
| Data class | Merge policy |
|---|---|
| Allergies, alerts | **Union, never auto-delete.** Removal requires explicit reconciliation event. |
| Problem & medication lists | Union + flagged for clinician reconciliation on conflict |
| Scheduling / bed management | Authoritative-node ownership (the owning tier wins) |

(Demographics are not modeled as a mutable record — see [§4](demographics.md).)

## 3.4 Interoperability
- Internal schema is event-sourced relational; a **FHIR R4/R5 façade** provides import/export and interop. **FHIR is a façade — a boundary skin, never the storage model** (see [§3.5](#35-event-storage-model-hybrid-envelope)). Cairn's internal model is canonical (a national-scale system is the thing others integrate *against*); FHIR is generated on demand for exchange with external/legacy systems and is not allowed to dictate the schema.

## 3.5 Event storage model — hybrid envelope
> Resolves former open question §11.2 — see [ADR-0001](decisions/0001-fat-postgres-thin-daemon.md).

The clinical event log ([§3.1](#31-append-only-clinical-event-log-source-of-truth)) is stored as **append-only event tables with a hybrid shape**, splitting columns by *what must be machine-enforced or matched* vs. *what is opaque clinical content*:

- **Typed/normalized envelope columns** — everything the safety machinery, identity subsystem, sync layer, and matcher must read or constrain: `uuidv7` primary key ([§3.2](#32-identity-time)), `patient_uuid` (FK), the **HLC** as typed fields (physical timestamp, logical counter, node id; [§3.2](#32-identity-time)), author / device, signature, `event_type` (a **closed enum**), scope keys (facility / department / encounter), `created_at`. Invariants live here because constraints can reach these columns; JSONB they cannot reach unbypassably.
- **Cairn-native JSONB clinical body** — the actual clinical payload (note/observation/order/result/etc.). JSONB avoids re-modeling the sprawling clinical content as relational tables and keeps the FHIR façade cheap, **without** adopting FHIR's resource graph as the schema. The body's integrity is its **signature**, not a SQL constraint — appropriate, since clinical content is immutable and signed.
- **Demographic-assertion events are the exception: their fields are typed columns, not JSONB**, because the matcher ([§5.2](identity.md#52-matching-pipeline-safety-asymmetric-false-merge-worse-than-false-split)) and the coherence checks ([§4.2](demographics.md#42-per-field-projection-policy), [§5.2](identity.md#52-matching-pipeline-safety-asymmetric-false-merge-worse-than-false-split)) read them and the identity algebra enforces invariants on them.

Rule of thumb: *normalized/typed where invariants, identity, sync, or matching bind; JSONB for clinical bodies; FHIR only at the façade.*
