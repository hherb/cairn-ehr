# Design — Spike 0002 attestation success-path

- **Date:** 2026-06-22
- **Status:** Approved (brainstorming) — ready for an implementation plan
- **Type:** build-prep (test coverage + one CLI helper). **No spec or ADR change.**
- **Closes:** the one honest gap carried into
  [ADR-0030](../../spec/decisions/0030-advisory-actor-integration-contract.md) — the attestation
  **success** path was never exercised end-to-end (Spike 0002 only tested the rejection half).

## 1. Problem

Spike 0002 ([0002-advisory-actor-write-contract.md](../../spikes/0002-advisory-actor-write-contract.md))
proved that the in-DB floor *rejects* five hostile-agent attacks (C1–C5). But every attestation test hits
exactly one branch — **"no token presented"** ([db/005_submit.sql:82–83](../../../db/005_submit.sql)). The
branches that run when a token *is* present are untested:

- `005:85–87` — token valid **and** bound to this event's content-address (`cairn_attestation_ok`).
- `005:88–91` — the attester is an **enrolled human** actor.
- the whole `attest → submit_event → append` accept flow, end-to-end through the external (Python) actor.

So part of the ADR-0030 contract is currently asserted on faith. ADR-0030 records this explicitly as the
open thread. The new repository [coding house rules](../../../CLAUDE.md) (TDD; fix-or-file; thoroughness)
make closing it the natural next step.

**The machinery already exists** — `cairn_event::sign_attestation` / `verify_attestation`
([crates/cairn-event/src/lib.rs](../../../crates/cairn-event/src/lib.rs)), `cairn_attestation_ok`
([extensions/cairn_pgx/src/lib.rs](../../../extensions/cairn_pgx/src/lib.rs)), and `submit_event`'s
`p_attestation`/`p_attester_key` parameters with the accept branch. What is missing is (a) a way for the
**Python** stand-in to mint a token without a second crypto implementation, and (b) tests that actually
drive the accept branch and the valid-token-but-bad-binding rejections.

## 2. Scope

**In scope:**
1. A `cairn-sync attest-stdin` CLI helper (mirror of `sign-stdin`).
2. A durable Rust integration test driving `submit_event`'s accept branch in-DB.
3. Python harness positive/negative cases exercising the full external-actor end-to-end.
4. Documentation follow-through (spike doc, HANDOVER).

**Explicitly out of scope (do NOT build or test):** owner/authority semantics for cross-author overlays.
`005:96–104` documents this as **deliberately deferred** — *"any enrolled human could downgrade any
author's event"* — and calls it an ADR-level question, not a spike hack. A positive test therefore needs
only *some* enrolled human attester. Testing owner-semantics would be testing behaviour that does not yet
exist by design.

## 3. Background — the binding model (from the code)

- **Content address** = `0x1220 ‖ sha256(signed_wire_bytes)` — `event_address()` in `cairn-event`,
  recomputed as `v_ca` at `005:60`. Attestation tokens bind to **this** value.
- **`AttestationBody`** = `{ content_address_hex, attester_key_id, role }`, CBOR-encoded and wrapped in a
  COSE_Sign1 signed by the attester's Ed25519 key.
- **`verify_attestation(token, content_address, vk)`** returns true iff the COSE signature verifies under
  `vk` **and** `body.content_address_hex == hex(content_address)`. A token minted for event A cannot be
  replayed onto event B (different signed bytes → different address).
- **Signature vs attestation:** the event's *signature* proves the signer authored it; an *attestation
  token* proves an attester vouches a responsibility-bearing role on that specific event. Decoupled per
  ADR-0007/0008 — the token, never the DB session, is what confers responsibility.

### The attestation gate triggers (submit_event step 4, `005:79–92`)
The gate runs when **either**:
- `v_mode = 'suppressing'` (event types `salience.downgrade`, `visibility.suppress`), **or**
- `v_bears` — any contributor in the body carries a `responsibility` key.

When triggered, all three must hold or the submit is rejected with a legible error: token present →
token valid & bound to `v_ca` → attester is an enrolled `human` actor.

## 4. Deliverables

### 4.1 `attest-stdin` CLI — `crates/cairn-sync/src/main.rs`
An exact structural mirror of `cmd_sign_stdin` ([main.rs:233](../../../crates/cairn-sync/src/main.rs)):

- **Input (stdin):** JSON `AttestationBody` — `{ "content_address_hex": "...", "attester_key_id": "...",
  "role": "attested" }`.
- **Behaviour:** load the key at `--key`; call `cairn_event::sign_attestation(content_address,
  attester_key_id, role, &sk)`; print hex COSE_Sign1 to stdout.
- **Wiring:** one new arm `"attest-stdin" => cmd_attest_stdin(...)` in the `match cmd` dispatch
  (`main.rs:1184+`) and a usage line beside the `sign-stdin` line (`main.rs:1170`).
- **Deliberate dumb-signer property (documented in the doc-comment, mirroring `sign-stdin`):** the helper
  attests *whatever* `content_address_hex` it is given, including one that matches no real event. This is
  required — it is how the wrong-address adversarial case is constructed — and the **in-DB floor**
  (`cairn_attestation_ok`) is the thing that rejects a mis-bound token, never the CLI. A future "hardening"
  of the CLI that validated the address would break the adversarial tests; the comment says so.

### 4.2 Durable Rust integration test — `crates/cairn-node/tests/attestation.rs`
Follows the `admission.rs` pattern: `#[tokio::test]`, skip-if-unset on `CAIRN_TEST_PG`,
`db::test_serial_guard` (cluster-wide serialize), `db::connect_and_load_schema`, `TRUNCATE` for
re-runnability. Enrolls a human attester and an agent signer. Mints tokens **directly** via
`cairn_event::sign_attestation` (no CLI dependency at this layer). Cases:

| Case | Event | Token | Expect |
|---|---|---|---|
| **P1** | `note.added` (additive, no provenance/target gate) with a contributor carrying `responsibility` (isolates the `v_bears` accept) | valid human, bound to the event | **accepted** (returns event_id) |
| **P2** | `salience.downgrade` targeting a real prior event (the positive mirror of C2) | valid human, bound to the event | **accepted** |
| **N1** | event B | valid human token bound to event **A** | rejected — *"attestation token invalid or not bound to this event"* |
| **N2** | valid event | token with one byte flipped | rejected — same error |
| **N3** | valid suppressing event | valid token, but attester is an enrolled **agent** (kind ≠ human) | rejected — *"attester is not an enrolled human actor"* |

N3 exercises gate check #3 (`005:88–91`), which no current test reaches (C5 forges a human *author* via the
signer, not the *attester*).

### 4.3 Python harness — `spike_0002.py` + `agent_standin.py`
- Add an `attest(...)` helper to `agent_standin.py`: build the `AttestationBody` JSON, shell to
  `cairn-sync attest-stdin --key <human.key>`, return the hex token (mirrors the existing `_sign` helper).
- Add to the `selftest` matrix in `spike_0002.py`, after the C1–C5 block, the full external-actor
  end-to-end: **P2** (accept) + **N1** (wrong-address) + **N2** (tamper), using `expect_raises` on the exact
  legible errors for the negatives. Keep the C-series naming convention; these are the success complement to
  C2. The harness already enrolls a human attester and authors the C1 advisory used as P2's target.

### 4.4 Documentation follow-through
- `docs/spikes/0002-advisory-actor-write-contract.md`: update the status/honest-gap note — the attestation
  success-path is now exercised (CLI + Rust integration + harness end-to-end).
- `docs/HANDOVER.md`: move this item from "honest gap carried forward" to done.
- **ADR-0030 is not edited** (immutable). It correctly recorded the gap at ratification; the closure is
  recorded here and in the spike doc.

## 5. Testing strategy (TDD)

- **`attest-stdin`** gets a Rust **unit test written first**: feed a known `AttestationBody` JSON to the
  command's core, assert the hex output decodes and `verify_attestation` accepts it for the right
  key+address and rejects a wrong address — i.e. the CLI boundary produces a token the verifier honours.
  (Mirrors the existing `attestation_binds_key_and_content_address` unit test, through the CLI seam.)
- **Integration + harness** cases each target one identified `005` branch; **negative cases assert the exact
  error string** so a future wording change cannot silently pass a test.
- DB-gated tests **skip cleanly** when `CAIRN_TEST_PG` is unset (existing convention) and require a local PG
  with `cairn_pgx` installed (`cargo pgrx install` against PG16; see the `crates/cairn_pgx`/spike toolchain
  notes).

## 6. Risks / honest notes

- **Two-key flow is intrinsic.** The accept path needs the agent to sign the event *and* a human to attest
  its content-address — the tests build both keys. This is the contract working as designed, not incidental
  complexity.
- **`connect_and_load_schema` coverage.** The integration test assumes the loader applies `db/004–006`
  (actor registry, submit, recall), not only the node schema (`db/007`). The plan's first step verifies this
  and, if it loads only a subset, extends the test setup to load the advisory-write schema. (Surfaced now so
  it is not a mid-implementation surprise.)
- **No owner-gate.** Restated because it is the most likely reviewer question: the deferred owner-gate
  (`005:96–104`) means P1/P2 pass with *any* enrolled human attester. That is correct for this spike.
