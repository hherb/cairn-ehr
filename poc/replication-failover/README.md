# Cairn — replication & failover proof-of-concept

A small, honest demonstration of the single property that matters most to a
health official evaluating an offline-first record system:

> **A machine can drop off the network (or lose power), care continues on
> another machine, and when the failed machine comes back the records reconcile
> automatically with nothing lost and nothing overwritten.**

This PoC does exactly that, using a faithful slice of Cairn's real architecture
rather than a special-purpose mock.

---

## What it actually shows

Two independent PostgreSQL 18 clusters — **Node A** and **Node B** — stand in
for two physically separate machines (two clinics, or a clinic and a regional
centre). Each holds:

* a **demographic record** (name, DOB, sex), and
* one or more **free-text clinical notes** — the "atomic component of a health
  record" for this demo.

The demo is the obvious, almost-too-simple thing functionaries trust:

1. Add a patient on Node A → synchronise → it appears on Node B.
2. **Pull the plug** (literally cut the link / stop a database).
3. **Both clinics keep working** — each records a new note for the same patient,
   independently. Neither is blocked, neither waits for the other.
4. **Reconnect** the two nodes.
5. **Synchronise.** Both notes are now on both nodes, **in the identical order**.
   No "winner" was chosen, nothing was overwritten, nothing was lost.

The headline is step 3+5 together: *care continued on both sides during the
outage, and the records merged themselves losslessly afterwards.* A live
side-by-side dashboard makes the divergence and the self-healing visible in real
time.

---

## Why this is Cairn, not a trick

The convergence is not generic database replication — it is Cairn's actual
resilience model, in miniature:

* **Append-only event log (governing principle #1).** Every write is an
  immutable event. The database *enforces* this — `UPDATE`/`DELETE` on the log
  raise an error. "Current state" (demographics, notes) is a **projection**
  computed over the log.
* **Hybrid Logical Clocks** give every event a deterministic total order across
  nodes even when their physical clocks disagree — so two nodes that wrote
  independently while partitioned order their events identically afterwards.
* **Sync is a set-union, never a merge.** Because every event has a globally
  unique id and nothing is ever edited, synchronising two nodes is "copy the
  events each side is missing". There is no last-write-wins, no field-level
  conflict, no data loss — *by construction*. Re-running sync is always safe.
* **Corrections are overlays (governing principle #2: never erase).**
  A demographic amendment is a new event; the original remains in the log.

This is why the user can stand behind the demo: it is a (small) honest
implementation of the architecture, not a staged animation.

> **Why not Postgres logical replication?** Native logical replication is great
> for primary→replica streaming, but a bidirectional, write-during-partition
> scenario is exactly where it forces conflict resolution — the thing Cairn's
> append-only design eliminates. Showing Cairn's own set-union sync is both more
> honest and more on-message. (Logical replication remains a fine option for
> read replicas; it is orthogonal to this demonstration.)

---

## Requirements

* macOS/Linux with **PostgreSQL ≥ 18** binaries available (the scripts
  auto-detect Postgres.app 18, Homebrew `postgresql@18`, or anything ≥ 18 on
  `PATH`; override with `PG_BIN=/path/to/bin`).
* [`uv`](https://docs.astral.sh/uv/) for the Python CLI.

The demo creates **its own throwaway clusters** on ports **55432/55433** with
data directories under `~/.cairn-replication-demo/`. It never touches any other
PostgreSQL server on the machine.

---

## Quick start

```bash
cd poc/replication-failover

bin/setup.sh                 # create + start both nodes, load the schema (once)
uv run cairn-demo status     # see both nodes, online and empty

# Rehearse the whole thing automatically:
uv run cairn-demo walkthrough --auto
```

For the **live demo**, see [`RUNBOOK.md`](RUNBOOK.md) — it has the exact
commands and the recommended two-terminal layout (dashboard on one side,
commands on the other).

### Two physical machines — the real cable-pull

The single-machine setup above runs both nodes on one host (great for
development and rehearsal). For the actual demonstration to an official, run one
node per machine and **physically pull the network or power cable** — far more
convincing. Each machine becomes a full, independent node; when the cable is
pulled, *both* screens show the partition, and reconnecting heals them.

This is configured entirely by a `demo.env` file (no code changes):

```bash
cp demo.env.example demo.env     # on EACH machine, then edit (peer IP + shared secret)
bin/setup-node.sh                # on EACH machine: create + start its networked node
bin/netcheck.sh                  # on EACH machine: confirm they can see each other
```

Full step-by-step (cabling, static IPs, firewall, the live script) is in
[`TWO-MACHINE-RUNBOOK.md`](TWO-MACHINE-RUNBOOK.md). The networked path
(LAN binding, password auth, sync-over-network, partition + heal) has been
tested end-to-end.

### The 60-second manual version

```bash
# terminal 1: the live picture
uv run cairn-demo dashboard

# terminal 2:
uv run cairn-demo patient add "Jane Doe" --dob 1980-04-12 --sex F
uv run cairn-demo sync                       # Jane now on both nodes
bin/node.sh A stop                           # pull the plug on A
uv run cairn-demo note add Jane "Seen in ED, chest pain, ECG normal." --node B
bin/node.sh A start                          # plug A back in (still stale)
uv run cairn-demo sync                       # the note appears on A — converged
```

---

## Commands

**Single-machine (both nodes on this host):**

| Command | What it does |
|---|---|
| `bin/setup.sh [--force]` | Create/start both clusters and load the schema. |
| `bin/node.sh A stop\|start\|restart\|status` | Pull the plug / plug back in a node. |
| `bin/reset.sh` | Wipe both nodes to a pristine, empty, in-sync state. |
| `bin/teardown.sh --yes` | Destroy the demo clusters entirely. |

**Two-machine (one node per host, configured via `demo.env`):**

| Command | What it does |
|---|---|
| `bin/setup-node.sh [--force]` | Create/start **this machine's** networked node + role. |
| `bin/netcheck.sh` | Print local addresses; test peer reachability + auth. |
| `bin/node-ctl.sh start\|stop\|restart\|status` | Control this machine's node (test/simulate power-off). |
| `uv run cairn-demo status` | One-shot view of both nodes + convergence. |
| `uv run cairn-demo dashboard` | Live side-by-side view (the centrepiece). |
| `uv run cairn-demo patient add NAME [--dob --sex] [--node A\|B]` | Register a patient. |
| `uv run cairn-demo patient list [--node A\|B]` | List patients on a node. |
| `uv run cairn-demo note add PATIENT TEXT [--node A\|B]` | Append a clinical note. |
| `uv run cairn-demo note list [--node A\|B]` | List notes on a node. |
| `uv run cairn-demo sync [--watch]` | Set-union sync; `--watch` auto-heals on reconnect. |
| `uv run cairn-demo walkthrough [--auto]` | Scripted, narrated end-to-end run. |

---

## Layout

```
poc/replication-failover/
├── README.md             ← you are here
├── RUNBOOK.md            ← exact steps for the single-machine live demo
├── TWO-MACHINE-RUNBOOK.md← exact steps for the physical cable-pull demo
├── NOTES-FROM-CLAUDE.md  ← judgement calls, caveats, what to verify
├── demo.env.example      ← template for the two-machine topology
├── schema.sql            ← append-only event log + projection views
├── pyproject.toml        ← uv project (psycopg, rich, typer)
├── bin/                  ← cluster lifecycle + "pull the plug" / networking scripts
├── src/cairn_demo/       ← config · hlc · db · events · projections · sync · cli
└── tests/                ← pure unit tests + live-cluster integration tests
```

This is a proof-of-concept, not production Cairn. See `NOTES-FROM-CLAUDE.md` for
the explicit list of what is simplified and what is faithful.
