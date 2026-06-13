# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this repository is

Cairn is an **offline-first, vendor-independent electronic health record**, currently in the
**architecture / specification phase — there is no code, build system, or tests yet.** The
current artifacts are design documents and the reasoning behind them. Work here is design work:
clinical case-mining, stress-testing the data model, and writing/refining specification prose.
Do not invent build/test/run instructions; they don't exist until implementation begins.

## Document hierarchy (what wins when sources disagree)

1. **`docs/principles/`** — canonical statements of mission and governance. Highest authority.
   - `STEWARDSHIP-OF-THE-NAME.md` — the "name belongs to the mission" commitment.
   - The mission and founding principles also live in the root `README.md`.
2. **`docs/planning/ehr-sync-architecture-spec-v0.5.md`** — the canonical architecture spec.
   **Read its changelogs (top of file) before reopening any settled question** — they record
   *why* each decision was made. Each version supersedes the previous; work from the highest.
3. **`docs/HANDOVER.md`** — disposable working scaffolding, NOT a source of truth. It points at
   the canonical docs and captures decisions made in conversation but not yet written into them.
   **Regenerate it at the end of a working session.** If it disagrees with canonical docs, the
   canonical docs win.

When starting a session, read HANDOVER.md first for current state (open questions, decisions
pending write-up, time-sensitive items), then the canonical docs it points to.

## The three governing principles (the lens for every decision)

Check every new design choice against these three before anything else. They are load-bearing;
the entire architecture is downstream of them.

1. **Append-only + causal ordering.** All clinical content is immutable, signed events ordered
   by Hybrid Logical Clocks. Corrections are new events referencing originals. This makes sync a
   safe **set-union** operation plus a small, *explicitly enumerated* set of clinically-reasoned
   merge policies — never a dangerous merge. Proposed mutability is almost always wrong.
2. **Identity is a claim, never a fact.** **Never merge — always link; never erase — always
   overlay.** Patient UUIDs are immortal; identity is an append-only stream of link/unlink/
   reattribute/repudiate/identify/dispute events (the closed "identity event algebra," spec §5.7).
   Every identity error must be repairable by an auditable event with no data loss.
3. **Paper-parity (governing law).** No clinical workflow may be slower, harder, more cognitively
   demanding, or impossible versus its paper-record equivalent (sole exclusion: paper capabilities
   that are malfeasance — silent falsification, untraceable backdating). New workflows must name
   their paper counterpart and benchmark in time/steps/cognitive load. **Confirmation dialogs are
   explicitly NOT an acceptable safety mechanism** — they fail paper-parity; restore the physical
   affordance instead (e.g. possession semantics for wrong-chart prevention).

Two more architectural invariants worth holding: **availability over consistency** (a clinician
must always be able to read locally-relevant records and write new data during a partition; AP in
CAP terms) and **fractal topology** (one codebase at every tier — workstation→department→facility→
region→nation; a node's role is configuration, not a different product).

## When implementation begins: language/substrate selection rule

The spec deliberately does **not** fix languages per component (spec §9). It fixes the *rule*:
**choose by defect blast radius.**

- **Safety-critical** (a defect can silently corrupt the record, mis-merge patients, leak data,
  or crash an unattended node) → **Rust or in-database (SQL / PL-pgSQL / constraints)**, optimized
  above all for **reviewer-legibility**. Members: sync/merge engine, identity event algebra and
  projections, HLC ordering, coherence checks, audit-log integrity, access control. Keep this
  surface as small as possible.
- **Fit-for-purpose** (defect is caught immediately, advisory, or cosmetic) → optimize for
  iteration speed. Members: the probabilistic matcher (advisory only — Python/ML), FHIR façade,
  integration glue, UI backends.
- **The integration boundary is the database boundary.** Each component talks to its node's
  PostgreSQL (≥ 18); Postgres is the integration substrate. Avoid FFI coupling.

All components must be **AGPL-3.0-compatible**. The whole project is AGPL-3.0 — non-negotiable.

## Working conventions

- **The user is an EM physician** who codes mostly in Python and brings real ED/hospital failure
  modes from multiple health systems. Case-mining (testing whether existing primitives absorb a
  real clinical failure mode) is the most productive mode — the event-overlay primitives have so
  far absorbed every case without needing new architecture. Treat clinical realism as first-class.
- **The mission is the tie-breaker.** The project is explicitly anti-capture / anti-vendor-lock-in.
  When convenience conflicts with the mission (open standards, no proprietary dependency, no
  mandatory cloud, data sovereignty), the mission wins.
- **Don't re-litigate parked decisions** (e.g. legal entity/jurisdiction, formal trademark
  registration) without new reason — see HANDOVER.md "Parked" section.
- Spec §11 lists the open architecture questions; §11.1/§11.2/§11.11 are one entangled decision
  ("how much intelligence lives inside Postgres"), best attacked together.
