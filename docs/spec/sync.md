# 6. Synchronisation Layer

## 6.1 Mechanism
- Transport-agnostic, resumable, delta-based protocol over HTTPS; optional store-and-forward via removable media ("sneakernet sync") for fully disconnected sites.
- **Build, don't adopt.** Built on **PostgreSQL logical decoding** as the change-capture primitive, with a **thin custom Rust sync service** implementing scoping, filtering, priority, and idempotent apply. The service **ships and applies events; it does not merge or resolve conflicts** — because the clinical log is append-only and immutable, syncing the source of truth is INSERT-only, idempotent (UUIDv7 PK), scoped **set-union**; there are no row-level clinical conflicts to resolve. The DB guarantees that "apply" is safe ([language-substrate §9.4](language-substrate.md#94-merge-projection-boundary-fat-postgres-thin-rust-daemon)); the daemon stays thin.

> [!NOTE]
> **pgactive / SymmetricDS are references, not dependencies.** They exist to resolve *row-level*
> conflicts (last-writer-wins and similar) — a problem Cairn designed away — and their default
> policies can *violate* invariants (LWW on a demographic = silent data loss, forbidden by
> [§4](demographics.md)). Borrow their patterns (logical-decoding plumbing, store-and-forward /
> sneakernet) without a hard runtime dependency, honoring the vendor-independence mission. See
> [ADR-0001](decisions/0001-fat-postgres-thin-daemon.md).

- Per [§9](language-substrate.md), this safety-critical service is Rust and/or in-database, never a dynamic language.
- **Sync scopes** are declarative subscription predicates, evaluated at the parent, versioned, auditable.
- Bandwidth discipline: compression, binary diffs, attachments synced lazily by reference with priority queues.
- **Upstream priority order:** new clinical events and audit events first; identity events (link/repudiate/reattribute) high priority; attachments last.

## 6.2 Consistency model
- Eventual consistency with causal ordering (HLC) within a patient record.
- Every projection displays a **freshness indicator** ("last synced with parent 4 h ago") — a first-class UI requirement.

## 6.3 Failure modes (designed-for)
| Failure | Behaviour |
|---|---|
| Internet down | Facility operates on facility server; queues outbound |
| Intranet down | Department server is local master for its scope |
| Department server down | Workstations operate standalone on mirrored scope |
| Node destroyed | Re-provision from parent; only unsync'd local events are at risk → aggressive upward sync priority |
