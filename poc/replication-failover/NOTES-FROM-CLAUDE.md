# Notes from Claude — built while you slept (2026-06-15)

You asked for a PoC replication/failover demo for a visiting official in 3 days,
gave me full latitude and privileges, and went to sleep. Here is everything I
decided and why, so you can audit fast. **It is built, runs, and is verified
end-to-end** (12/12 tests pass; the full pull-the-plug scenario reconciles
correctly). It's reset to a clean, empty, in-sync state ready to demo.

## TL;DR — is it ready?
Yes. From `poc/replication-failover/`:
```bash
uv run cairn-demo dashboard        # left terminal
# then follow RUNBOOK.md in a right terminal
```
Rehearse with `uv run cairn-demo walkthrough --auto`.

## The big judgement call: Cairn-native sync, not Postgres logical replication
I built the demo on Cairn's *own* model — append-only event log, Hybrid Logical
Clock ordering, conflict-free **set-union** sync between two independent PG18
clusters — rather than native PostgreSQL logical replication.

Why:
- It's honest. You can tell the official "this is how Cairn actually works,"
  not "here's a generic DB feature we'll wrap later."
- The scenario you described (write on both sides during a partition, then
  reconcile) is precisely where logical replication forces conflict resolution
  — the thing the append-only design *eliminates*. Showing set-union makes the
  "nothing lost, nothing overwritten" claim true by construction, and I proved
  it with a bidirectional-partition test.
- If the official specifically expects "standard Postgres replication," I note
  in the README that logical replication is fine for read replicas and is
  orthogonal. You could add a second demo, but I'd advise against muddying the
  message.

If you disagree, the layering makes a logical-replication variant easy to bolt
on without touching the schema.

## Environment decisions
- **PG18.1** binaries from `/Applications/Postgres 2.app/.../18/bin` (matches the
  spec's Postgres ≥ 18 requirement). Auto-detected by `bin/pg-env.sh`.
- **Two throwaway clusters** on ports **55432 / 55433**, data + logs under
  **`~/.cairn-replication-demo/`** (NOT inside the git worktree — a worktree
  cleanup can't wipe a live demo).
- **Your production servers were never touched.** I confirmed PG16 (:5432) and
  PG18 (:5532) were still accepting connections after every step. I did not open,
  alter, or stop them — the demo only ever talks to 55432/55433.
- `initdb` used `--auth-local=trust --auth-host=trust`. Fine for a localhost
  throwaway; **not** a production auth posture (I called this out in `setup.sh`).

## What's faithful to Cairn vs simplified (be ready for questions)
Faithful:
- Append-only, immutable event log; `UPDATE`/`DELETE` rejected by a DB trigger
  (demonstrable, not just claimed).
- Globally-unique `event_id` → sync is set-union → idempotent, conflict-free.
- Hybrid Logical Clock giving a deterministic total order across nodes
  (unit-tested in `tests/test_hlc.py`).
- "Current truth" is a **projection** over the log; corrections are overlay
  events (`patient.amended`), original never erased.

Simplified (say so if asked — don't oversell):
- **No cryptographic signing** of events yet (real Cairn signs every event).
- Sync is a manual/`--watch` pull over plain psql connections, not the real
  daemon/transport, and not authenticated beyond `trust`.
- No identity event algebra, no bitemporal `t_recorded`/`t_effective` split, no
  encryption/crypto-shredding, no access control. This is a single-slice PoC.
- HLC is persisted per-node in a tiny table; fine for a demo, not the final
  design.

## Things I did NOT do (your call)
- **I did not commit or push.** Policy is "commit only when asked," and you
  didn't. Everything is uncommitted on branch `claude/trusting-germain-1fba1a`.
  When you're happy:
  ```bash
  git add poc/replication-failover
  git commit   # suggested: "feat(poc): offline-first replication & failover demo"
  ```
  `uv.lock`, logs, and `.venv/` are gitignored. The runtime clusters live
  outside the repo, so nothing stateful gets committed.
- I did not touch the spec docs or HANDOVER.md — this PoC is separate from the
  specification work and I didn't want to muddy the canonical docs overnight.

## One thing worth your eyes in the morning
The demo's persuasiveness is the *live dashboard* + the physical `node.sh A stop`.
I tested the renderer (`uv run cairn-demo status`) in both states and it looks
great, but I could not interactively drive the full-screen `Live` dashboard in a
non-TTY here. **Please run `uv run cairn-demo dashboard` once in a real terminal**
to confirm the auto-refresh feels right on your screen before the official sees
it. Everything underneath it is verified.

## Two-machine mode (added after you said "physical cable-pull")
You asked for two real machines so you can yank the network/power cable for a
tech-naive functionary. Done — it's a config switch, no code fork:

- Drop a `demo.env` on each machine (template: `demo.env.example`) naming this
  host's node + the peer's IP + a shared password. `bin/setup-node.sh` creates
  one networked node per machine; `bin/netcheck.sh` is the pre-flight; the live
  script is `TWO-MACHINE-RUNBOOK.md`.
- Each machine's dashboard marks its own node **"◀ you are here"** and shows the
  peer by address. Pull the cable → *both* screens flip to PARTITIONED, each
  keeps working; reconnect + `sync` → both heal. That two-screen reaction is the
  thing that makes a non-technical official "get it".
- **Tested end-to-end against this Mac's real LAN IP (192.168.1.31), not
  loopback:** LAN binding, **password auth genuinely enforced** (no-password and
  wrong-password both rejected; correct password connects from the LAN address),
  replication over the network, partition detection, and heal-on-reconnect all
  verified. The only thing I couldn't do from here is the literal cable-pull on a
  second physical box — rehearse that once.

Security note for the LAN: peer auth is `scram-sha-256` with a shared password
over `samenet` (the directly-attached subnet); `127.0.0.1` stays `trust` so the
local CLI never needs the password. Fine for an isolated demo LAN / direct
cable; still not a production posture (no TLS, shared secret in a file).

My recommendation: **a direct Ethernet cable between the two laptops with static
IPs** (e.g. 10.0.0.1/10.0.0.2). Nothing depends on venue Wi-Fi or DHCP, and the
cable you pull *is* the demo. Details in the two-machine runbook.

## Earlier open question — now resolved
(Was: one laptop or two machines? — you answered: two physical machines. Built
and tested per above.)
