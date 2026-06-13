# HANDOVER — Cairn

**Session date:** 2026-06-13
**Status of this file:** Working scaffolding, not a source of truth. Disposable — regenerate
at the end of each working session. If this file ever disagrees with the canonical documents,
the canonical documents win.

---

## Read these first (the durable state)

The real project state lives in three documents. This handover points at them; it does not
restate them. Intended repository layout:

- **`docs/planning/`** — all planning / architecture documents.
  - `ehr-sync-architecture-spec-v0.5.md` — current macroscopic architecture spec. The
    **changelogs inside it record *why* each decision was made** — read them before
    reopening any settled question.
- **`docs/principles/`** — all statements of project principle / governance.
  - `README.md` — mission, founding principles, eventual goal, project identity.
    *(README itself typically lives at repo root; a copy or canonical source may sit here.)*
  - `STEWARDSHIP-OF-THE-NAME.md` — the "name belongs to the mission" governance commitment.

Everything below is the stuff that lives *between* those documents and would otherwise be lost.

---

## Decided in conversation, NOT yet written into the documents

These are real decisions that still need to be reflected in the canonical files:

1. **Project name is "Cairn."** The spec (v0.5) title block and headers still use a generic
   name — update them to Cairn for consistency with the README and stewardship doc.
2. **Domains registered (Cloudflare):** `cairn-ehr.org` (canonical) and `cairn-ehr.com`
   (defensive, redirect → `.org`). Reflected in STEWARDSHIP-OF-THE-NAME.md.
3. **Status line:** README and spec both say "specification / architecture phase." Still
   accurate today — flip when implementation begins.
4. **Governance / CONTRIBUTING document** is identified as the next principles document to
   write, but does not exist yet. STEWARDSHIP-OF-THE-NAME.md is intended for inclusion in it.

---

## Time-sensitive (do soon, before squatters do)

- **Reserve the project namespaces defensively** while the name is fresh: GitHub organization,
  and package registries (PyPI / crates / npm). Same logic as the domains — parking them now
  is trivial; reclaiming them later is painful.

---

## Open questions / where we'd pick up

The spec's §11 lists twelve open questions. Two synthesis points from discussion that are
**not** captured in the spec body and should guide what to tackle next:

1. **The "how much intelligence lives inside Postgres" cluster.** Spec §11.1 (build vs. adapt
   the sync backbone), §11.2 (storage model: FHIR-native JSONB vs. normalized relational), and
   §11.11 (in-database vs. application-layer merge boundary) are **one entangled decision**, not
   three independent ones — they all turn on the same axis. Best attacked as a single session.
2. **Dynamic sync-scope handoff (§11.3)** — the patient transferred ED→ICU mid-partition; who
   owns scope reassignment during a partition. This is the last genuinely unsolved
   distributed-systems problem in the design, and it stands largely independent of the cluster
   above — so it's a clean standalone session.

**The recurring menu** when resuming (pick one):
- The Postgres-intelligence cluster (§11.1 / §11.2 / §11.11 together).
- The dynamic sync-scope handoff (§11.3).
- More clinical case-mining — the most productive mode so far: the user (an EM physician) brings
  real failure modes from practice, and we test whether the existing primitives absorb them.
  The event-overlay primitives (link / unlink / repudiate / reattribute / identify / dispute)
  have absorbed every case raised so far without new architecture; continuing to stress-test
  this is high-value.

---

## Parked (deliberately not decided yet — don't re-litigate without reason)

- **Stewarding legal entity & jurisdiction.** Options floated: German Stiftung /
  gemeinnütziger Verein, US 501(c)(3), or an umbrella (e.g. Software Freedom Conservancy or a
  health-specific foundation). Deferred until the project has momentum and funding/adoption
  geography is clearer. Interacts with the trademark question below.
- **Formal trademark / wordmark registration.** Principle is recorded now (stewardship doc);
  the legal instrument is deliberately deferred to avoid premature legal scaffolding — file it
  when there is enough substance to be worth capturing.

---

## Working context for whoever resumes

- The user is a senior Physician with an interest in ML / AI / health IT; codes
  mostly in Python. Brings real ED and hospital system experience from several nations and health systems — case-mining sessions are unusually productive.
- The project's founding motivation is explicitly **anti-capture / anti-vendor-lock-in**, rooted
  in the user's experience of government EHR committees being sabotaged by commercial interests.
  Decisions consistently favour the mission over convenience; treat that as the tie-breaker.
- Two governing principles run through everything and are the right lens for new decisions:
  **(a) append-only + causal ordering** so sync is set-union plus a small enumerated set of
  clinically-reasoned merge policies; **(b) identity is a claim, never a fact** — never merge,
  always link; never erase, always overlay. A third, added later: **(c) paper-parity** — no
  workflow may be slower / harder / more cognitively demanding than its paper equivalent
  (malfeasance excepted). When a new design choice arises, check it against these three first.
