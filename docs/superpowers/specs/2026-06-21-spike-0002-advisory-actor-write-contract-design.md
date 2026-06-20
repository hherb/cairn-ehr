# Spike 0002 — Advisory-Actor Write Contract: implementation design

- **Date:** 2026-06-21
- **Status:** Design approved; ready for an implementation plan.
- **Spec it implements:** [`docs/spikes/0002-advisory-actor-write-contract.md`](../../spikes/0002-advisory-actor-write-contract.md)
  (the *what* and the C1–C6 pass/fail). This document is the *how*.
- **Builds on:** the Spike 0001 walking skeleton in [`poc/walking-skeleton`](../../../poc/walking-skeleton)
  — the signed COSE_Sign1 envelope (`db/001_envelope.sql`), the `cairn-event` crate
  (canonical CBOR + Ed25519 sign/verify + multihash), the thin `cairn-sync` daemon, the
  trigger-maintained projection (`db/002_projection.sql`), and the content-addressed blob
  tier (`db/003_blobs.sql`).

> [!NOTE]
> Build-prep, not architecture. Nothing here changes the numbered spec or the ADR log.
> Passing this spike is the *trigger* to write two ADRs (the parked ADR-0011 skill-epoch
> refinement and an advisory-actor integration-contract ADR); those are written **after**
> the spike, citing its results.

---

## 1. The bet, restated

Convert "kastellan/localmail *fit*" (the [ecosystem 0001](../../ecosystem/0001-agent-and-messaging-plugins-kastellan-localmail.md)
*reasoning*) into a *demonstration*: an advisory agent authors a clinical advisory into Cairn
**through the validated submit surface**, never around it, as an **additive, un-attested,
provenance-anchored, recallable** event — **and the in-DB safety floor rejects everything a
buggy or hostile agent must not be allowed to do, even with direct DB access.**

This is a **design-validity** bet (like Spike 0001's Bet A), not a performance bet. A "the
floor can't express this" result is design feedback that goes back to ADR-0022, not a defect
to paper over.

## 2. Key decision — the in-DB verify floor is real (pgrx), not stubbed

The spike's sharpest claim (C5) is *"floor-in-DB, direct DB access safe by construction."*
Four of the five hostile attacks in spike §4 are enforceable in **pure PL/pgSQL + the grant
model**. The fifth — an **unsigned/malformed** event — needs **Ed25519 verification**, which
core PostgreSQL + `pgcrypto` **cannot do** (no EdDSA). So the signature check is made truly
in-DB via a **`pgrx` extension** — exactly "the Spike 0001 pgrx verifier" the spike references
and the [ADR-0002](../../spec/decisions/0002-in-database-rust-pgrx-escape-hatch.md) production
move. This is reusable seed code, not throwaway.

**Environment:** PostgreSQL 16.13 (Postgres.app) + Rust 1.96 on this box; `cargo-pgrx` not yet
installed. **First checkpoint of the build is getting `cairn_verify` callable in-DB** — so a
toolchain fight against the Postgres.app layout surfaces early. Documented fallback if pgrx
fights Postgres.app: a Homebrew/PGDG PG16 dev install with headers. (The fallback is an
*environment* swap, not a design change.)

## 3. Two supporting decisions

- **Attestation token = an Ed25519-signed token over the event's content-address**, verified
  in-DB by the *same* verifier against the attester's key in the actor registry. A human
  "vouches" by producing one; the agent stand-in has none, so its advisory is un-vouched **by
  construction** (C1). Matches [ADR-0008](../../spec/decisions/0008-armed-write-context-and-the-possession-gesture.md)
  ("the token, never the DB session, stops a direct-DB client forging authorship"). Rejected
  alternative: an HMAC/shared-secret token — lighter, but breaks the signature=origin model.
- **C6 (notification projection) is deferred.** Its absence does not block ratification, and it
  pulls in the separate [ADR-0009](../../spec/decisions/0009-notification-economy-salience-routing-and-the-acknowledgment-floor.md)
  consumer surface. This spike is C1–C5: the write contract + the floor.

## 4. Components (with the §9 blast-radius mapping)

Faithful to the [§9.1](../../spec/language-substrate.md) rule: safety-critical pieces in-DB /
Rust; the agent stand-in in Python.

### 4.1 `db/004_actors.sql` — the actor registry (safety → in-DB)

An append-only registry over the closed actor-event algebra
([ADR-0011](../../spec/decisions/0011-actor-registry-version-pinning-and-key-custody.md)):

- `actor_event(actor_event_id UUID PK, actor_id BYTEA, op TEXT, …, recorded_at)` with
  `op ∈ {enroll, supersede, revoke}`. **Append-only**, enforced by the same no-UPDATE/DELETE
  trigger pattern as `event_log` (principle 1).
- Pinned standing-config carried on `enroll`/`supersede`: `kind` (`human` | `agent` | `device`),
  `vendor, model, version, weights_ref, inference_config_ref, system_prompt_ref,
  tool_rag_config_ref, deploying_node`, **`skill_epoch`** (a content-address — the parked
  ADR-0011 refinement, exercised here), and `signing_key_id` (hex Ed25519 public key).
- `actor_current` — a projected view: the latest non-revoked identity per `actor_id`.
- **Actor-identity rule (the mechanism behind C4):** `actor_id` **is the content-address of the
  pinned-determinant set**. Bump *any* pinned field (incl. `skill_epoch`) → a different digest →
  a `supersede` mints a **new `actor_id`**. Computed in the pgrx crate (§4.3), so "identity =
  hash of what is pinned" is *enforced*, not asserted.

### 4.2 Contributor set + responsibility (extend `db/001_envelope.sql` + `EventBody`)

Replace Spike 0001's single-author stub ([§3.9](../../spec/data-model.md)) with a **set** of
`{actor_id, role, responsibility?}`:

- `role` ∈ the [ADR-0028](../../spec/decisions/0028-finalized-closed-contributor-role-enum.md)
  closed enum (bearing: `authored/ordered/attested/co-signed/witnessed/dictated`; contributory:
  `drafted/transcribed/graded/triaged/suggested`).
- `responsibility` is present **only** when backed by a valid attestation token.
- The agent authors with role `triaged` and **no** `responsibility` → "AI-generated /
  un-vouched" is emergent. **No `is_ai` boolean exists anywhere** (C1).

The Rust `EventBody.contributors` field already carries arbitrary JSON; the work is giving it a
typed shape and teaching `submit_event` to read it.

### 4.3 `cairn_pgx` — the pgrx extension (safety → Rust, the floor's teeth)

A thin extension that **wraps the existing `cairn-event` crate** so there is *one* verify
implementation, not two:

- `cairn_verify(signed_bytes bytea) returns bool` — COSE_Sign1 / Ed25519 verify. Rejects
  unsigned/malformed bytes → **C5.1, in-DB**.
- `cairn_actor_id(pinned jsonb) returns bytea` — content-address of the pinned-determinant set
  (C4). Canonicalizes the pinned set deterministically before hashing.
- `cairn_attestation_ok(token bytea, content_address bytea, attester_key bytea) returns bool`
  — verifies a signed attestation token binds the attester to this event's content-address.

### 4.4 `submit_event(...)` — the single write door (safety → in-DB, PL/pgSQL `SECURITY DEFINER`)

The in-DB convergence of the write-time seams ([ADR-0022](../../spec/decisions/0022-validated-submit-surface-the-write-path.md)),
run atomically, dispatching by `(event_type, schema_version)` to additively-registered
validators:

```
1. cairn_verify(signed_bytes)                                  → reject unsigned/malformed  (C5.1)
2. resolve signer_key_id against actor_current                 → must be enrolled, non-revoked
3. stamp envelope + HLC ceiling (reuse the 001 hlc_state path)
4. classify event_type as additive | suppressing               (a classification table)
5. if suppressing OR a responsibility is asserted:
      require a valid attestation token (cairn_attestation_ok)  → (C2 suppressing; C5.2 forged author; C5.3)
6. if the event is a salience/visibility overlay on ANOTHER author's event:
      owner-gate it                                             → (C5.5)
7. bind provenance: the source blob's content-address present in attachments  → (C3)
8. canonicalize + derive plaintext twin + idempotent append (ON CONFLICT DO NOTHING)
```

Every rejection raises a **legible reason** (a distinct `RAISE EXCEPTION` message), so a buggy
agent gets a clear error, never a silent corruption.

**The grant floor (mechanism behind C5.4):** `REVOKE INSERT/UPDATE/DELETE ON event_log` from the
agent's DB role; `GRANT EXECUTE ON submit_event(...)` + `SELECT` on projections only. A raw
`INSERT` from the agent role fails at the *privilege* layer — *direct DB access is safe by
construction* ([ADR-0021](../../spec/decisions/0021-layering-the-node-api-and-ui-pluralism.md)
floor-in-DB).

> **Who signs:** the agent holds its **own** Ed25519 identity (spike §3.1) and signs the event
> **client-side** (Python, §4.5); `submit_event` **verifies and appends, never signs** in this
> spike. This is faithful — signature proves the agent's origin. (In-DB signing for a *direct-DB*
> author is the separate ADR-0022 concern; out of scope here because the external agent is the
> signer.) Authoring ≠ applying: the existing `cairn-sync` apply path verifies peer signatures and
> idempotent-appends, never re-signs; this spike exercises the **authoring** door, apply unchanged.

### 4.5 `harness/agent_standin.py` — the agent stand-in (fit-for-purpose → Python, `uv`)

Loads its actor identity + skill-epoch, reads the provenance blob (a content-addressed blob via
the existing `003` byte tier standing in for a localmail-mirrored mail), computes a trivial
urgency score, **signs the event with its own Ed25519 key** (client-side, via a `uv`-managed
crypto lib producing the same COSE_Sign1 bytes `cairn-event` emits), and authors the advisory
**only** through `submit_event` — never raw `INSERT`. Per project convention, the Python env is
managed with **`uv`** (never venv/pip).

> Producing byte-identical COSE_Sign1 from Python is the one integration risk in the stand-in. If
> matching the `coset`/`ciborium` encoding from Python proves finicky, the fallback is a tiny
> `cairn-sync sign-stdin` helper the Python agent shells out to (Rust does the canonical encode +
> sign; Python still drives the contract). Either way `submit_event` is the only write door.

### 4.6 Recall — query + contamination overlay (safety → in-DB)

- A query "events authored by `actor_id` X under `skill_epoch` E" — returns exactly the spike's
  advisory (C4).
- A `revoke`/recall **overlay** event that marks affected events **without erasing** them (C4) —
  the contamination cascade in miniature, principle 2 (never erase, always overlay).

## 5. Test harness — `harness/spike_0002.py` (the C1–C5 table)

Stdlib-or-`uv`, self-contained `selftest` against a local DB, mirroring `bet_a.py`'s structure
and `--force` guard. It drives the agent stand-in and the five hostile attacks, prints the
C1–C5 pass/fail table, and exits 0 iff all PASS. Each floor rejection asserts a **legible
reason**, not merely that the row is absent.

| # | Claim | How the harness checks it |
|---|---|---|
| C1 | Additive authorship, un-attested | advisory reads back with contributor set `{agent, triaged}`, **no** responsibility; assert no `is_ai` column/flag exists |
| C2 | Additive accepted, suppressing-un-attested rejected | author an additive advisory (accepted) and an otherwise-identical suppressing event un-attested (rejected in-DB with a legible reason) |
| C3 | Provenance-anchored | advisory carries the source blob's content-address; re-verify the blob against it; survives a `sign → ship → apply` round-trip |
| C4 | Version-pinned + recallable | the recall query returns exactly this advisory; bumping any pinned determinant (incl. `skill_epoch`) mints a new `actor_id` via `supersede`; a `revoke` overlay marks affected events without erasing |
| C5 | Floor holds against a hostile agent | each of the five §4 attacks is rejected with a legible reason; the committed-event set is byte-identical before/after the attacks |

## 6. Build order (early-risk-first)

1. **`cairn_pgx` skeleton + `cairn_verify` callable in-DB** — the toolchain checkpoint. Prove a
   signed event verifies and a tampered one fails, *from SQL*, before building anything on top.
2. `db/004_actors.sql` + `cairn_actor_id` + the actor-identity rule (C4 mechanics).
3. Contributor-set shape on the envelope + `EventBody`.
4. `submit_event` with the seam pipeline + the grant floor.
5. The agent stand-in authoring one advisory through `submit_event`.
6. Recall query + contamination overlay.
7. `harness/spike_0002.py` C1–C5 table; iterate to green.

## 7. Exit criteria

- **C1–C5 PASS** → the trigger to write the two ADRs (parked ADR-0011 skill-epoch refinement;
  advisory-actor integration-contract ADR). Those are written *after*, citing these results.
- **Any FAIL is design feedback.** If the floor cannot express "reject suppressing-un-attested,"
  that is an ADR-0022 submit-surface completeness gap sent back to design. If recall cannot bound
  to a skill-epoch, the parked refinement shape is wrong and that ADR is not written as drafted.

## 8. Out of scope (inherited from spike §2)

Not kastellan itself (a Python stand-in mimics the *contract*); not localmail (a content-addressed
blob stands in for a mirrored mail); not the notification economy (C6, deferred); not CASSANDRA;
not a transport-security review (inherits the Spike 0001 WireGuard/NoTls assumption).
