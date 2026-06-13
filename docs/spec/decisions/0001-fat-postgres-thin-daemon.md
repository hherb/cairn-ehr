# ADR-0001 — Postgres-intelligence cluster: fat Postgres, thin Rust daemon

- **Status:** Accepted
- **Date:** 2026-06-13
- **Supersedes:** —

## Context

Three open questions in the spec were entangled and were attacked as a single decision, because
they all turn on one axis: **how much clinical intelligence is enforced by Postgres itself vs. an
orchestrating application core.**

- **§11.1** — build a custom sync backbone vs. adapt an existing multi-master tool (pgactive /
  SymmetricDS).
- **§11.2** — storage model: FHIR-native JSONB vs. normalized relational with a FHIR façade.
- **§11.11** — the merge/projection boundary: which logic lives in PostgreSQL (constraints /
  PL-pgSQL, unbypassable) vs. an orchestrating Rust core.

Two facts established during the deciding session disambiguated the axis:

1. **Tablets are thin clients, not autonomous edge nodes.** The smallest node that must survive a
   full partition alone is a **Pi-class full PostgreSQL ≥18**. Therefore PL/pgSQL, constraints,
   projection tables, and logical decoding are available on *every computing node* — the "must also
   run on PGlite/SQLite" portability constraint that would have forced logic out of the database is
   gone. The only remaining cost of in-database logic is *performance on Pi-class hardware*. (Revised
   [topology §2](../topology.md).)
2. **FHIR is a skin, not a skeleton.** FHIR is useful at the integration boundary but is a bloated
   committee artifact; it has no claim to be the internal model. Cairn is envisioned as a
   national-scale system, so its internal model is the *canonical* one.

The pivot that collapses most of the difficulty: because the clinical log is **append-only and
immutable**, syncing the source of truth is INSERT-only, idempotent (UUIDv7 PK), scoped
**set-union** — there are *no row-level clinical conflicts to resolve*. All genuinely hard "merge"
logic is confined to *derived* state (chart projection, golden-identity graph, the
[§3.3](../data-model.md#33-mutable-non-demographic-state) mutable lists), which is rebuildable and
never synced.

Alternatives considered and rejected:

- **Thin Postgres, fat Rust core** (Postgres stores the log + structural constraints only; all
  projection/identity/merge logic in Rust). Better unit-testability, but invariants on *derived*
  state become **bypassable** by any other writer (the Python matcher, a future tool, a DBA at the
  console), and the audited surface grows in the application layer.
- **Adopt pgactive / SymmetricDS + FHIR-native JSONB** (Postgres as a dumb store). These tools
  exist to resolve *row-level* conflicts — a problem Cairn designed away — and their default
  policies (last-writer-wins, etc.) can *violate* invariants (LWW on a demographic = silent data
  loss, forbidden by [§4](../demographics.md)). It is also a hard third-party dependency (mission
  risk) and adopts the FHIR-native storage we reject.

## Decision

Adopt **"Fat Postgres, thin Rust daemon"** across all three questions:

- **Storage (§11.2 → [data-model §3.5](../data-model.md#35-event-storage-model-hybrid-envelope)):**
  a **hybrid event envelope** — typed/normalized columns where invariants, identity, sync, and
  matching bind (UUIDv7 PK, patient UUID, HLC, author/device, signature, closed `event_type` enum,
  scope keys); **Cairn-native JSONB** for clinical bodies; demographic-assertion fields are typed
  columns. **FHIR is a façade view/export, never the storage model.**
- **Merge boundary (§11.11 → [language-substrate §9.4](../language-substrate.md#94-merge-projection-boundary-fat-postgres-thin-rust-daemon)):**
  structural invariants + the identity event algebra + **all projections** live **in Postgres**
  (trigger-maintained incremental tables, `AFTER INSERT` only). The **Rust daemon ships and applies
  events but carries no merge logic.** The probabilistic matcher stays **Python and advisory** — it
  proposes candidates; the database decides. A **per-projection Rust escape hatch** relocates a
  specific projection only on measured need.
- **Sync backbone (§11.1 → [sync §6.1](../sync.md#61-mechanism)):** **build** a thin custom Rust
  service on Postgres **logical decoding**; **borrow** pgactive/SymmetricDS patterns (decoding
  plumbing, store-and-forward) but **do not depend** on them.

## Consequences

**Easier / gained:**
- Invariants are **unbypassable** (DB constraints) and **reviewer-legible** (logic next to data) —
  the two properties [§9](../language-substrate.md) prizes most — and the audited surface is the
  smallest possible.
- Sync is literally scoped INSERT set-union with idempotent apply; unmerge stays clean (split the
  connected component, nothing rewritten).
- No hard third-party multi-master dependency; the vendor-independence mission is honored.
- In-database logic runs identically on every node down to the Pi.

**Harder / the bet:**
- PL/pgSQL + trigger-maintained projections must stay **cheap on Pi-class hardware** to keep chart
  reads local and fast (the [§1.2](../vision.md#12-the-paper-parity-test-normative) paper-parity
  floor). **This is the load-bearing assumption.**
- PL/pgSQL is less unit-testable than Rust; the escape hatch exists for projections where this or
  performance bites.

**How we'd know the bet fails (named go/no-go spike):**
- The **first implementation spike** is a Raspberry-Pi-5 benchmark harness: synthetic event volumes
  for (i) a solo practice and (ii) a busy ED/department, measuring per-INSERT projection-maintenance
  latency and chart-read latency. **Threshold:** a chart read must beat "grab the paper chart."
  Failure triggers the per-projection Rust escape hatch — beginning with the identity
  connected-component (the likeliest hot/gnarly projection).
