# Two-machine runbook — pull the cable for real

This is the setup where the failover is *physical*: two laptops, and you yank
the cable. Each machine is a full, independent node. When you pull the cable,
**both screens** react — each says "my partner is gone, but I'm still working" —
and when you reconnect, they heal. That is what makes a tech-naive functionary
"get it".

> Already verified: the networked code path (LAN binding, password auth, sync
> over the network, partition detection, heal-on-reconnect) was tested
> end-to-end. What only you can do is the literal cable-pull on real hardware —
> rehearse it once with the steps below.

---

## 0. What you need

* **Two computers** (two Macs, or a Mac + a Linux box), each with **PostgreSQL ≥ 18**
  and **`uv`**, and this repo checked out.
* **A network between them.** Two equally good options:
  * **Best for the demo — a direct Ethernet cable** between the two machines
    (plus USB-C/Thunderbolt-Ethernet adapters if needed). No switch, no Wi-Fi,
    nothing to depend on. Pulling *this* cable is the demo.
  * **A shared switch / the venue LAN.** Works too; you can pull the cable from
    either machine to the switch.
* If you use a direct cable, the Macs will self-assign `169.254.x.x` addresses,
  or you can set static IPs (e.g. `10.0.0.1` and `10.0.0.2`) in
  System Settings → Network → the Ethernet adapter → Details → TCP/IP →
  "Configure IPv4: Manually". Static IPs are more predictable on stage.

> **Power cable instead of network cable?** Also fine, and even more dramatic —
> just pull the power on a machine with no battery, or hold the power button.
> The architecture treats "host unreachable" and "host powered off" identically.
> The network cable is gentler (both machines keep running and accepting local
> writes, so you can show *both* sides working during the split). Pick whichever
> reads better to your audience.

---

## 1. One-time setup (do this the day before)

On **each** machine, from `poc/replication-failover/`:

```bash
cp demo.env.example demo.env
```

Edit `demo.env`. The two files are mirror images. Example for a direct cable
with static IPs `10.0.0.1` (A) and `10.0.0.2` (B):

**Machine A's `demo.env`:**
```
CAIRN_SELF_NAME=A
CAIRN_SELF_PORT=55432
CAIRN_PEER_NAME=B
CAIRN_PEER_HOST=10.0.0.2        # B's address
CAIRN_PEER_PORT=55432
CAIRN_DB_PASSWORD=pick-one-shared-secret   # SAME on both machines
```

**Machine B's `demo.env`:**
```
CAIRN_SELF_NAME=B
CAIRN_SELF_PORT=55432
CAIRN_PEER_NAME=A
CAIRN_PEER_HOST=10.0.0.1        # A's address
CAIRN_PEER_PORT=55432
CAIRN_DB_PASSWORD=pick-one-shared-secret   # SAME on both machines
```

Don't know a machine's address? Run `bin/netcheck.sh` on it — it prints them.

Then, on **each** machine:
```bash
bin/setup-node.sh        # creates + starts this machine's node, networked
```

Finally, confirm they can see each other. On **each** machine:
```bash
bin/netcheck.sh
```
You want `✅ peer PostgreSQL is reachable` and `✅ authenticated connection to
peer works`. If not, the script lists the things to check (cable, firewall,
matching password).

> **Firewall:** if a machine's firewall is on, allow incoming connections for
> `postgres` (macOS will usually prompt the first time; or
> System Settings → Network → Firewall → Options).

---

## 2. Just before the demo

On **each** machine, get a clean slate:
```bash
uv run cairn-demo sync      # make sure they start identical
# (optional) bin/setup-node.sh --force   # nuke & recreate if state got messy
```

Set up screens — ideally **both laptops facing the audience**, each running its
own live dashboard:
```bash
uv run cairn-demo dashboard
```
Each dashboard shows its own node as **"◀ you are here"** and the peer by
address. Both green, banner *IN SYNC*.

---

## 3. The demo (≈ 3 minutes)

1. **"Two separate computers — two clinics. Watch both screens."**
   Both dashboards green, *IN SYNC*.

2. **Register a patient on Machine A.** In a second terminal on A:
   ```bash
   uv run cairn-demo patient add "Jane Doe" --dob 1980-04-12 --sex F
   uv run cairn-demo sync
   ```
   Jane appears on **both** screens.

3. **PULL THE CABLE.** Physically unplug the Ethernet cable (or power).
   Within ~2 seconds **both dashboards** flip: each shows its peer **OFFLINE**
   and the banner goes **PARTITIONED**. Each still shows its own data, green.

4. **Both clinics keep working during the outage.** Add a note on each machine
   *in its own terminal* — this is the killer point, no one is blocked:
   ```bash
   # on Machine A:
   uv run cairn-demo note add Jane "Seen at clinic A during the outage."
   # on Machine B:
   uv run cairn-demo note add Jane "Seen at clinic B during the outage."
   ```
   Each note appears only on its own screen. Neither machine lost service.

5. **PLUG THE CABLE BACK IN.** The peer panels go green again — but each screen
   is still missing the *other* machine's note (banner: *DIVERGED*).

6. **Heal.** On either machine:
   ```bash
   uv run cairn-demo sync
   ```
   **Both notes now appear on both screens, in the same order.** Banner *IN
   SYNC*. Nothing was lost, nothing was overwritten, no one had to choose a
   "winner".

> The one sentence to land: *"Each clinic kept working alone, and when the link
> came back the records merged themselves — automatically, with no data loss."*

### Hands-free heal (optional)
Instead of typing `sync` at step 6, run this on one machine beforehand:
```bash
uv run cairn-demo sync --watch
```
Then reconnecting the cable heals the records on its own.

---

## 4. Troubleshooting on the day

| Symptom | Fix |
|---|---|
| `netcheck` can't reach peer | Cable seated? Right IP in `CAIRN_PEER_HOST`? Peer node started? Firewall? |
| "authenticated connection failed" | `CAIRN_DB_PASSWORD` must be **identical** on both machines. Re-run `bin/setup-node.sh` after fixing. |
| Dashboard sluggish while unplugged | Normal — it waits ~2s for the dead peer per refresh, then shows OFFLINE. |
| State is messy | `bin/setup-node.sh --force` on both, then `uv run cairn-demo sync`. |
| Addresses changed (DHCP) | Re-check with `bin/netcheck.sh`, update `CAIRN_PEER_HOST`. Static IPs avoid this. |

Throwaway clusters live in `~/.cairn-replication-demo/node` on each machine.
No production database is involved.
