# Design — The first federating Cairn node

**Date:** 2026-06-21
**Status:** Approved (brainstorming complete; ready for implementation planning)
**Topic:** Establish the first real Cairn node — node identity + node-to-node federation
(peering), with **no EHR/clinical functionality**. This is the moment the project graduates
from spec/spike into the production implementation tree.

---

## 1. Goal and scope

Build the smallest thing that is *genuinely* a Cairn node that can **federate**: provision a
node with its own signing identity, pair it with another node over an out-of-band fingerprint
confirmation, and let the two nodes exchange their identity and peering events by the existing
set-union sync — with admission enforced in the database. No patient data, no clinical events.

The only payload that flows in this slice is the **federation machinery itself** (`node.enrolled`,
`peer.added`, `peer.revoked`). That is deliberate: it exercises ADR-0017's one safety-critical
seam — *verified credential → admitted peer* — end-to-end with zero clinical surface.

### In scope
- A new top-level Cargo workspace (the production tree), with the walking-skeleton crates promoted
  up as its foundation.
- A new `cairn-node` binary: a control CLI **and** a sync daemon.
- Node provisioning (`init`): mint keypair → sealed keystore → self-signed genesis enrollment.
- The node peering algebra as additive events behind `submit_event`, with a trigger-maintained
  trust-set projection.
- Direct-pairwise pairing ceremony (out-of-band fingerprint confirmation), symmetric, deny-all by
  default.
- Built-in mTLS transport pinned to the trust set (the node owns its transport; WireGuard optional
  underneath).
- In-DB (pgrx) admission gate: an event enters the log only if it verifies **and** its signer is an
  active peer.

### Explicit non-goals (this slice)
- **No EHR / clinical events** of any kind.
- **No trust anchors beyond direct-pairwise** — no practice-issuing-key, no registry-as-node-role
  (ADR-0017's other two anchor modes layer on later without rework).
- **No key rotation** (`rotate-key`) and **no disaster recovery** (`supersede`) — both *reserved*
  in the shape, not built.
- **No full ADR-0026 sealed-state / recovery-escrow** — minimal real keystore now, DR is a named
  honest stub.
- **No registry, no MPI, no discovery tier** (ADR-0016) — those presuppose federation, which this
  slice establishes.

---

## 2. Decisions taken (and why)

| # | Decision | Rationale |
|---|----------|-----------|
| D1 | **New top-level workspace**; promote `cairn-event`, `cairn-sync`, `cairn_pgx`, `db/` out of `poc/walking-skeleton`; `poc/` freezes as historical spikes. | This is the start of the real implementation; the skeleton was always "the seed of the real thing," not throwaway. |
| D2 | **A fresh `cairn-node` crate** (service), depending on the promoted crates — not extending the spike harness. | Separates node lifecycle from spike harnesses; `cairn-node` is the first product binary. |
| D3 | **Symmetric peering, labelled.** One mutual trust edge (ADR-0017). "Upstream/downstream" is an optional **role label + sync-scope hint** (ADR-0004) on that edge, never an asymmetric trust primitive. | Faithful to ADR-0017; avoids inventing a directional trust model the ADR doesn't have. |
| D4 | **Direct-pairwise trust only** for v1 (out-of-band fingerprint/QR/short-code; no external authority). | The two-node private-practice floor; the smallest real slice. Other anchors are pure additions later. |
| D5 | **In-DB admission floor from day one** (pgrx + `submit_event` + a remote-apply gate). | Matches ADR-0021 "floor in the DB"; the federation seam is exactly the kind of defect that leaks data across the boundary, so it belongs in the trusted base. |
| D6 | **Trust set + node enrollment are append-only *events* in Postgres**, not a config file. The **private signing key** lives off-log in a sealed keystore (ADR-0026). | Peering becomes auditable, reversible-by-overlay, and syncs by set-union like everything else (ADR-0017 §2, ADR-0001). |
| D7 | **`node_id` = content-address of the genesis `node.enrolled` event — genesis-stable.** Key rotation (future) is a signed chain from genesis (`rotate-key`); recovery is `supersede` (new id). | The §7.5 algebra already separates `rotate-key` (stable id) from `supersede` (new id); principle 2 makes the id immortal and the key a rotatable overlay. Key-tracking would collapse the two ops, re-pair the whole federation on every rotation, and fragment provenance. |
| D8 | **`cairn-node` owns its transport: built-in mTLS, cert-pinned to the trust set.** The node's Ed25519 key is also its TLS identity (self-signed cert, no CA). WireGuard is optional belt-and-braces underneath, never a dependency. | Lets a single clinic's workstations pair and sync with zero infrastructure; depending on WireGuard would block low-friction trusted-LAN setup. The trust set becomes *both* the sync-admission gate and the transport-auth gate. |
| D9 | **Keystore: minimal real now, DR a named honest stub.** OS-permissioned, optionally passphrase-wrapped Ed25519 key file. Recovery/escrow (ADR-0026) surfaced as a declared gap, not silently absent. | Full DR is a separate ADR-sized build; honesty about the gap preserves the acknowledged-uncertainty discipline (principle 4). |

---

## 3. Architecture

### 3.1 Workspace layout (after graduation)

```
/Cargo.toml                  # NEW root workspace
/crates/
  cairn-event/               # promoted: canonical bytes, COSE_Sign1/Ed25519, multihash, BLAKE3
  cairn-sync/                # promoted: set-union ship/apply engine (no merge logic — ADR-0001)
  cairn-node/                # NEW: node lifecycle — provision, pair, peers, transport, serve
/extensions/
  cairn_pgx/                 # promoted: in-DB verify gate + NEW federation admission functions
/db/                         # promoted migrations + NEW node-identity / peering migrations
/poc/                        # FROZEN historical spikes; Python harnesses repoint at new binary path
```

The promotion is a move (single source of truth), not a copy. `poc/walking-skeleton`'s Python
harnesses (`bet_a.py`, `bench_b.py`, `spike_0002.py`) keep working by repointing their
`--bin`/SQL paths; their pass/fail results are historical and do not need re-running.

### 3.2 Substrate split (§9 blast-radius)

| Piece | Substrate | Blast radius |
|---|---|---|
| Genesis enrollment validation, peering-event validation, `node_id` derivation, trust-set projection, **admission gate**, **mTLS cert-pin check** | in-DB (pgrx/SQL) + Rust trusted base | **Safety-critical** — a defect admits an unauthorized node or leaks across the federation boundary |
| Pairing UX (bundle display/scan, fingerprint prompt), `peers`/`status` output, CLI ergonomics | `cairn-node` (Rust, fit-for-purpose) | Fit-for-purpose — a defect yields a bad *proposal* a human reviews at the fingerprint step |

The one safety-critical seam is **verified credential → admitted peer**, the federation analogue of
the §7.5 enrolment seam and the Spike 0002 `submit_event` floor.

### 3.3 Event algebra (additive — new validators behind `submit_event`)

The §7.5 actor-event algebra applied to nodes. Three event types in v1:

| Event type | Author | Body (essential fields) |
|---|---|---|
| `node.enrolled` | self (genesis) | signing pubkey, display name, created HLC. Self-signed at provisioning. **`node_id` is *derived* — the content-address of this event's canonical signed bytes — not a body field** (an event cannot contain its own hash). |
| `peer.added` | this node | peer `node_id`, peer pubkey, confirmed fingerprint, `anchor: "direct-pairwise"`, optional `role` label (`upstream`/`downstream`/`peer`), optional default sync-scope hint |
| `peer.revoked` | this node | `target_event_id` of the `peer.added` it overlays (never erases) |

`rotate-key` and `supersede` are **reserved** (named in the algebra, not implemented). Peering is
**mutual**: each node independently records its own `peer.added` about the other. The edge "exists"
when both sides have recorded it; until then it is honestly **pending** (one-sided trust is safe —
each side decides admission independently, deny-all default).

### 3.4 Trust-set projection

A trigger-maintained projection `trust_peer` folds the peering events into the current trust set:

```
trust_peer(peer_node_id, peer_pubkey, fingerprint, status{active|revoked},
           role, scope_hint, first_seen_hlc, last_event_hlc)
```

`peer.added` → active row; `peer.revoked` → status=revoked (row retained, never deleted). This
projection is the single source consulted by **both** the in-DB admission gate (§3.6) and the
transport cert-pin check (§3.5).

### 3.5 Transport: built-in mTLS pinned to the trust set

- The node's **Ed25519 signing key is its TLS identity** — a self-signed Ed25519 cert (rustls), no
  CA, matching the no-anchor / direct-pairwise posture.
- A node accepts a TLS session **only from a peer whose presented cert key is `active` in
  `trust_peer`**. An unpaired node cannot open a session → deny-all at the connection layer.
- The out-of-band fingerprint confirmed at pairing covers that same key, so confirming it
  simultaneously pins the transport.
- Trusted LAN → built-in mTLS suffices, zero infra. Hostile WAN → drop WireGuard underneath,
  unchanged. (This drops the skeleton's "NoTls-on-purpose, WireGuard-is-the-encryption" assumption.)

### 3.6 Admission gate (the safety-critical seam, in-DB)

Two write paths, both gated:
- **Local authoring** (`submit_event`, Spike 0002 pattern): the node authors its own
  `node.enrolled` / `peer.added` / `peer.revoked`. Validators registered behind the single door;
  raw `INSERT` blocked by the grant floor.
- **Remote apply** (sync): an inbound event is admitted to the log **iff** `cairn_verify` passes
  **AND** its `signer_key_id` resolves to an `active` row in `trust_peer`. Unpeered signer →
  reject; revoked peer → reject; each with a **legible** reason. (v1 admits events authored by a
  **directly-peered** node only; transitive/multi-hop trust is out of scope and noted.)

---

## 4. Ceremonies (the `cairn-node` surface)

| Command | Effect |
|---|---|
| `cairn-node init` | Mint Ed25519 keypair → sealed keystore; author + self-sign the genesis `node.enrolled` and append it; `node_id` falls out as its content-address. Result: a live node, **zero peers, zero data** — the sovereignty floor (asks no one's permission). |
| `cairn-node identity` | Show `node_id`, pubkey, **short fingerprint**, listen address. |
| `cairn-node pair-offer` | Emit a signed pairing bundle `{node_id, pubkey, address, short-fingerprint, nonce, HLC}` as text + QR. |
| `cairn-node pair-accept <bundle>` | Display the peer's fingerprint for **human out-of-band confirmation**; on confirm, append `peer.added` (optionally `--role upstream\|downstream\|peer`). Run on **both** nodes (symmetric). |
| `cairn-node peers` | List the trust set: active / revoked / pending, role, last-seen. |
| `cairn-node unpeer <node>` | Append `peer.revoked`. |
| `cairn-node serve` / `run` | The mTLS-gated sync daemon (reuses the `cairn-sync` engine); `run` loops on a timer and survives link drops. |
| `cairn-node status` | Honest-assembly state: peers, convergence fingerprint vs each peer, **keystore health**, declared DR-stub gap. |

The pairing flow (direct-pairwise, symmetric):
1. `pair-offer` on A → bundle (operator carries it to B: paste / QR scan — out-of-band).
2. `pair-accept <A-bundle>` on B → B confirms A's fingerprint → B records `peer.added(A)`; B emits its own bundle back.
3. `pair-accept <B-bundle>` on A → A confirms B's fingerprint → A records `peer.added(B)`.
4. Both trust sets now contain the other; mTLS-pinned sync begins; the peering events converge.

No cloud round-trip at any step. The bundle exchange is operator-carried, so it needs no secured
channel to bootstrap — the fingerprint is the root of trust.

---

## 5. Error handling and honest-assembly

- **Fingerprint mismatch** at `pair-accept` → refuse to pair, loudly (the MITM antidote).
- **Partition** → the node keeps running solo, reads and writes locally, never blocks (availability
  floor / sovereignty floor).
- **Sealed keystore unreadable** → the node can read and verify but **cannot author/sign**; this is
  surfaced as honest degradation in `status`, not a silent failure.
- **Revoked peer** → admission rejects its new events with a legible reason; the revoke itself is an
  audited overlay event.
- **Half-paired (pending) edge** → surfaced as `pending` in `peers`, not an error.

---

## 6. Testing (TDD)

- **Unit (Rust):** pairing-bundle sign/verify, short-fingerprint derivation, `node_id`
  content-address from genesis, `trust_peer` folding (`added` → `revoked` → revoked), mTLS
  cert-key ↔ trust-set pin check.
- **Integration (two local Postgres DBs):** provision both → pair with fingerprint confirm →
  `node.enrolled` + `peer.added` converge by set-union → `unpeer` → post-revoke events rejected →
  an **unpaired third node is rejected** at both transport and admission.
- **In-DB floor (pgrx), federation hostile cases** (Spike-0002-style, fail-closed with legible
  reasons): raw-`INSERT` bypass blocked; event from an **unpeered** signer rejected; event from a
  **revoked** peer rejected; TLS session from an unpinned cert refused.

---

## 7. Traceability

| Artifact | This design's use |
|---|---|
| ADR-0017 (federation admission, sovereignty floor, peering, trust anchors) | The governing ADR; this is its first implementation, scoped to the direct-pairwise anchor. |
| ADR-0011 / §7.5 (actor registry, actor-event algebra, key custody) | A node is a `device`-kind actor; peering reuses the closed algebra (`enroll`/`peer`/`revoke`, `rotate-key`/`supersede` reserved). |
| ADR-0021 / §9.5–§9.6 (layering, floor-in-DB, validated submit surface) | Admission and peering enforced unbypassably in the DB; raw access safe by construction. |
| ADR-0001 (fat Postgres, thin daemon) | Trust set + projection live in Postgres; `cairn-node` carries no merge logic. |
| ADR-0015 (serialization, signatures, content-addressing) | Genesis `node_id` is a content-address; events are signed verbatim bytes. |
| ADR-0026 (node durability & DR) | Sealed keystore is the off-log key home; full DR/escrow is the declared honest stub. |
| ADR-0004 (sync-scope = prefetch hint) | The optional upstream/downstream role carries a scope hint, never an authority. |
| Spike 0002 (advisory-actor write contract) | The `submit_event` door + grant floor + pgrx verify gate this design extends with federation validators and the remote-apply admission gate. |

---

## 8. The bet (what would tell us this is wrong)

That direct-pairwise mutual peering over the actor algebra, with admission and transport both pinned
to a single in-DB trust set, is a clean foundation that the other two anchor modes (practice key,
registry) extend **additively**. We would know it is wrong if: the two-node pairing proves too heavy
in real use (ADR-0017's own failure signal); or if folding transport into the node forces a coupling
that the in-DB admission gate cannot express; or if genesis-stable `node_id` makes the eventual
`rotate-key` continuity proof harder to keep reviewer-legible than a re-pair would have been.
