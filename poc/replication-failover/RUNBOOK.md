# Live demo runbook — replication & failover

Hand-this-to-yourself steps for the official's visit. Total run time ≈ 3–4 min.
Rehearse once with `walkthrough --auto` beforehand.

## Before they arrive

```bash
cd poc/replication-failover
bin/setup.sh        # only if the clusters don't exist yet (safe to re-run)
bin/reset.sh        # pristine, empty, in-sync — do this right before the demo
```

Use **two terminal windows side by side**:

* **Left = the picture.** Make it big.
  ```bash
  uv run cairn-demo dashboard
  ```
  It refreshes itself; you never touch it during the demo. Quit with `Ctrl-C`.

* **Right = the actions.** You type the commands below here.

> Tip: the dashboard uses the full window — make the left terminal at least
> ~110 columns wide so the two node panels sit side by side.

---

## The script (what to say + what to type)

The headline is **both clinics keep working during the outage, and the records
merge themselves with no winner and no loss.**

**1. "Here are two separate machines — two clinics. Both online, in sync."**
Point at the dashboard: two green panels, banner says *IN SYNC*.

**2. "A patient arrives at clinic A and is registered. The clinics sync."**
```bash
uv run cairn-demo patient add "Jane Doe" --dob 1980-04-12 --sex F --node A
uv run cairn-demo sync
```
Jane appears on **both** panels. Banner green: *IN SYNC*.

**3. "Now the link between them goes down." — pull the plug.**
```bash
bin/node.sh A stop
```
Node A's panel goes red: **⏻ OFFLINE**. Banner: *PARTITIONED*.

**4. "Neither clinic stops. BOTH keep treating the patient." (the key moment)**
```bash
# clinic B is reachable from here:
uv run cairn-demo note add Jane "Clinic B during the outage: gave analgesia." --node B
```
Node A is dark, but it is **not dead** — its database is still running and would
accept writes from its own staff (we show that for real in the two-machine
setup). The point to say out loud: *no clinician anywhere is blocked or waiting.*

**5. "The link comes back." — plug back in.**
```bash
bin/node.sh A start
```
Node A turns green again — but it's **stale**: it has Jane, not B's new note.
Banner: *DIVERGED*.

**6. "They reconcile — automatically, with no data loss."**
```bash
uv run cairn-demo sync
```
**B's note appears on A.** Banner green: *IN SYNC*. Nothing was overwritten.

> The whole proof in one line: *a machine dropped off, care continued, and when
> it came back the records merged themselves — automatically, losslessly.*

---

## The strongest version: both sides write while split

If you can drive two terminals (or use the two-machine setup), this is the most
convincing because there is visibly **no "winner" and no lost note**:

```bash
bin/node.sh A stop                                                    # link down
uv run cairn-demo note add Jane "Clinic B: started antibiotics." --node B
bin/node.sh A start                                                   # A back, still split-era data
# (on the two-machine setup, clinic A's staff also wrote during the split)
uv run cairn-demo sync
```
After the sync, **both notes are on both nodes, in the identical order
everywhere** — guaranteed by the Hybrid Logical Clock, not by luck or by picking
a winner.

> Rehearse the full bidirectional story hands-free any time:
> `uv run cairn-demo walkthrough` (add `--auto` for no pauses). It scripts and
> narrates exactly this.

## If they ask "can't you just edit the record?"
Corrections are *new overlay events*; the original is never erased (Cairn
principle #2). The database itself rejects edits — demonstrate it:
```bash
"$(bin/pg-env.sh >/dev/null 2>&1)"  # (binaries) then, against node A's 'cairn' db:
#   UPDATE event_log SET payload='{}';   -> ERROR: event_log is append-only …
#   DELETE FROM event_log;               -> ERROR: event_log is append-only …
```

---

## Auto-heal variant (hands-free reconnect)

If you'd rather not type `sync` at step 7, start a watcher beforehand in a third
terminal:
```bash
uv run cairn-demo sync --watch
```
Then step 7 happens by itself the moment Node A returns — the record heals on
its own. (For a controlled demo, manual `sync` is easier to narrate.)

---

## If something goes sideways

| Symptom | Fix |
|---|---|
| A node won't start | `bin/node.sh A start` again; check `~/.cairn-replication-demo/nodeA.log`. |
| Weird/leftover state | `bin/reset.sh` (empties both, keeps clusters). |
| Truly broken | `bin/teardown.sh --yes` then `bin/setup.sh`. |
| "command not found: cairn-demo" | Prefix with `uv run`, and run from `poc/replication-failover`. |
| Dashboard layout cramped | Widen the terminal to ≥110 columns. |

Your production servers (PG16 on :5432, PG18 on :5532) are never involved.
