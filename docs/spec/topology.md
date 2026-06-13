# 2. Topology

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
- **The smallest *autonomous* node is a Pi-class full PostgreSQL ≥18** node (workstation / mini-PC / solar Pi). "Autonomous" = able to survive a full partition alone: read locally-relevant charts and write new clinical data with no upstream reachable.
- **Tablets / carts / phones are thin clients**, not autonomous edge stores: they attach to a nearby autonomous node (department server, workstation, or clinic Pi) which holds the database and computes projections. An embedded store (PGlite/SQLite) may back a thin client for transient buffering, but a thin client is **not** expected to survive a partition by itself.

> [!NOTE]
> Every *computing* node being full Postgres is what makes the in-database merge/projection design
> viable everywhere, Pi included. See [ADR-0001](decisions/0001-fat-postgres-thin-daemon.md) and
> [language-substrate §9.4](language-substrate.md#94-merge-projection-boundary-fat-postgres-thin-rust-daemon).
