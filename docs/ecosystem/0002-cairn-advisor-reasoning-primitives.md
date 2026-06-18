# Reasoning-Primitive Catalog — Clinical Evidence Advisory Service

> **Status:** founding design doc / draft 1 · 2026-06-18
> **Working name:** `cairn-advisor` *(provisional — a stone-motif sibling to Cairn/Kastellan
> such as `lodestone`, the historical guiding stone, would fit; name is swappable)*
> **Lineage:** distilled from `bmlibrarian` (research workbench), standing on `bmlib`
> (LLM/ingestion/quality/transparency primitives), reusing `biasbuster`'s LoRA pipeline.
> **This document is simultaneously the system design, the fine-tuning spec, and the
> acceptance criteria.** If a section can't be turned into an eval, it isn't done.

---

## 1. What this is

A narrowly-scoped service that answers a recurring family of clinical evidence
questions with **graded, cited, balanced, auditable reports**. It does *one* job:

> de-identified clinical question (+ coded context) → grounded report whose every
> claim carries a verified citation, a certainty grade, the contradicting evidence,
> and a visible reasoning trace.

It is **not** a literature-research workbench (that stays `bmlibrarian`), not a
chatbot, and not a point-of-care typeahead. It is the evidence-appraisal engine
behind a clinician's "give me a balanced briefing" request.

### Core insight (the reason this is buildable at 20–30B)

The reasoning a clinician needs here is overwhelmingly **procedural, not insight**.
Evidence appraisal is codified — GRADE, RoB2, ROBINS-I, the CONSORT non-inferiority
extension, NNH for harms. We are not teaching a model to *be* a clinician; we are
teaching it to **faithfully execute published appraisal procedures inside an explicit
clinical frame**. Procedures are exactly what mid-size fine-tunes capture (confirmed
empirically by `biasbuster` for the risk-of-bias step), and the "constrained domain"
caveat dissolves because **each appraisal step is itself a constrained domain.**

This buys three things at once:

1. **Feasibility** — procedural reasoning is tunable at 20–30B.
2. **Auditability** — the model shows its working ("downgraded for indirectness:
   population was X, not the asked Y; quote: '…'"). A clinician trusts *that*, never
   a bare score. The reasoning trace is the deliverable, as much as the answer — and
   it is the regulatory safe harbor (clinician can independently verify the basis).
3. **Maintainability** — when a report is wrong, the failing *step* is localizable;
   you retrain one adapter, not a monolithic prompt. `bmlibrarian` could not do this.

---

## 2. Scope & non-goals

**In scope**

- The recurring question archetypes (§6).
- Producing the structured `AdvisoryReport` contract (§7).
- Per-primitive reasoning that conforms to a named published standard.
- Honest **declination** when a question is outside calibrated competence.

**Out of scope / non-goals (deliberate)**

- **No PHI.** The service never receives names, MRNs, dates, or free-text narrative.
  De-identification is owned upstream by Kastellan (§3). Defense-in-depth only here:
  treat input as sensitive, never log raw questions.
- **No offline/edge survivability.** This is a service-tier component on capable
  hardware. A disconnected node loses the advisor and that is acceptable — it lives
  in Cairn's "enhanced when connected" tier, not "works through any outage." Cairn's
  record keeps working without it.
- **No write path to the chart.** Advisory-only. The clinician disposes; Cairn
  records the disposition as a signed append-only event.
- **No multi-tenant clinician-facing auth/ACL.** Single trusted client (Kastellan).
  Authenticate the client; do not rebuild localmail's per-user ACL.
- **Not a systematic-review tool.** The heavyweight SR machinery (PRISMA 27-item,
  full counterfactual sweeps over the whole literature) belongs to the **cold path**
  (corpus curation, §8), not the per-question hot path.

---

## 3. Position in the ecology

Pathway: **`cairn-advisor` → Kastellan ↔ Cairn.**

| Component | Owns |
|---|---|
| **Cairn (EHR)** | The record (FHIR, append-only, offline-first). Translates patient context → a coded question; renders the returned report; records the clinician's disposition. |
| **Kastellan (broker)** | The trust/egress boundary. De-identifies the outgoing question, makes the **local-30B-vs-cloud-frontier routing a per-question risk decision** (CASSANDRA), enforces constitutional bounds, returns the vetted report. The advisor's *only* client. |
| **`cairn-advisor`** | Pure evidence appraisal. Receives a de-identified coded question, returns the `AdvisoryReport`. Knows nothing of patients, clinicians, or egress policy. |

Consequences of this boundary:
- The advisor's contract collapses to a small purpose-built schema (§5). **FHIR stays
  on the Cairn↔Kastellan side**; the advisor does not parse FHIR.
- Prefer **coded/structured context over free-text** across the Kastellan boundary —
  it minimizes the re-identification surface (mosaic effect) and is what the PICO
  frame consumes anyway.
- The advisor can be configured to route synthesis to a frontier model *because*
  Kastellan guarantees scrubbed input — but that decision is Kastellan's, per
  question, not a static flag here.

---

## 4. How the reasoning is structured

Three structural moves compensate for everything a 20–30B model lacks on its own:

1. **PICO as context scaffold (§5).** Every primitive operates *relative to an
   explicit PICO frame*. The model is never asked "is this relevant?" in a vacuum —
   it is asked "does this support claim C **for this P, I, C, O**?" This injects the
   clinical context the small model cannot supply itself, and it is the direct fix
   for the citation-extraction failure ("no understanding of clinical context").

2. **A library of constrained, individually-tunable primitives (§6).** Each executes
   one codified procedure, has its own input/output contract, its own eval harness,
   and its own LoRA adapter.

3. **Archetype routing + recipes (§6.9).** A router classifies the question into one
   of a small set of archetypes; each archetype is a *recipe* over the shared
   primitives. The router must be able to return `declined` / `low_confidence` rather
   than confabulate — graceful incompetence is a safety requirement, the clinical
   analog of localmail's honest `rewrite_skipped`.

---

## 5. The PICO frame (shared context object)

Every primitive takes a `PicoFrame`. It is produced once by P1 and threaded through.

```
PicoFrame
  population     : coded comorbidities/findings + age band + sex + setting
                   (NO free-text narrative; codes + bounded enums only)
  intervention   : the agent/procedure/strategy in question (+ dose/regimen/route if material)
  comparator     : what it is weighed against — a real comparator, or
                   "placebo" / "usual care" / "none" for existence & harm questions
  outcome        : outcome(s) of interest, each typed: efficacy | harm | surrogate
  archetype      : existence | superiority | non_inferiority | harm | best_practice
  question_text  : the original de-identified question (audit + retrieval seed)
  applicability  : the population/setting the answer must generalize TO
                   (kept distinct from study populations — drives "indirectness")
```

`applicability` is deliberately separate from `population`: it is the *target* the
GRADE directness judgment is made against. Most "wrong citation" errors are really
applicability errors, so it is a first-class field.

---

## 6. The primitive catalog

Each primitive below specifies: **purpose · input → output · the codified procedure ·
eval rubric · fine-tune notes.** Outputs always carry a `rationale` (the per-step "why")
and, where a judgment is made, the **supporting quote(s)** from source text.

### P1 — Frame & route
- **Purpose:** question → `PicoFrame` + archetype; or honest declination.
- **In → Out:** `question_text (+coded context)` → `PicoFrame` *or*
  `{competence: declined|low_confidence, reason}`.
- **Procedure:** extract P/I/C/O (reuse `bmlibrarian` PICOAgent design); classify
  archetype; check that a calibrated recipe exists for (archetype × specialty);
  decline if not.
- **Eval:** PICO-slot extraction F1 vs clinician-labeled gold; archetype accuracy;
  **calibrated abstention** — % of out-of-scope questions correctly declined
  (false-confidence rate is the safety metric).
- **Fine-tune:** small adapter; teacher-distilled PICO traces + curated decline cases.

### P2 — Retrieve & screen (PICO-conditioned eligibility)
- **Purpose:** pull candidate studies and decide each one's eligibility *against the frame*.
- **In → Out:** `PicoFrame` → `[{source_id, eligible: yes|partial|no, reason,
  pico_match:{P,I,C,O}}]`.
- **Procedure:** hybrid retrieval (adopt localmail's stack wholesale: tsvector +
  pgvector `halfvec` + HNSW + EmbeddingGemma/arctic) over the curated corpus; then a
  per-candidate eligibility judgment keyed on PICO match. This is where
  directness/applicability screening *begins* (population/intervention/comparator/
  outcome match), not after synthesis.
- **Eval:** recall@K vs a clinician-built relevant set (localmail's recall-eval
  pattern); eligibility precision/recall; **population-mismatch catch rate**.
- **Fine-tune:** eligibility-judgment adapter; the retrieval layer is not tuned (config).

### P3 — Appraise risk of bias (per study) — *biasbuster anchor*
- **Purpose:** domain-by-domain RoB for each eligible study.
- **In → Out:** `study` → `{tool: RoB2|ROBINS-I, domains:[{name, judgment, quote}],
  overall: low|some_concerns|high}`.
- **Procedure:** RoB2 for RCTs, ROBINS-I for non-randomized studies; each domain
  judgment must cite the sentence that grounds it.
- **Eval:** per-domain agreement (Cohen's κ) vs clinician labels; overall-RoB
  agreement. **This primitive already has a working dataset/pipeline in `biasbuster`** —
  slot its labels in; template traces on the published RoB2/ROBINS-I signaling questions.
- **Fine-tune:** the proven case. Reuse `biasbuster` LoRA artifacts as the baseline adapter.

### P4 — Appraise GRADE domains (per outcome, body of evidence)
- **Purpose:** turn a set of studies into a certainty grade per outcome.
- **In → Out:** `[studies for an outcome] + PicoFrame` → `{certainty:
  high|moderate|low|very_low, domains:[{name: risk_of_bias|inconsistency|indirectness|
  imprecision|publication_bias, judgment, rationale}], upgrades:[large_effect|
  dose_response|confounding_toward_null]}`.
- **Procedure:** GRADE. `risk_of_bias` rolls up P3; `indirectness` is judged against
  `PicoFrame.applicability`; `imprecision` from CI vs decision threshold.
- **Eval:** certainty agreement vs clinician GRADE; per-domain rationale faithfulness
  (does the cited rationale actually justify the downgrade?).
- **Fine-tune:** per-domain adapters (each GRADE domain is its own constrained skill).

### P5 — Verify citation support (PICO-conditioned) — *second pain point*
- **Purpose:** gate every claim→citation link.
- **In → Out:** `{claim, candidate_passage, PicoFrame}` → `{support: supports|
  partial|does_not|contradicts, reason, pico_match_notes}`.
- **Procedure:** judge support **conditioned on PICO** — a passage about a different
  population, endpoint, or dose is `partial` or `does_not`, never `supports`. Only
  `supports` (and `contradicts`, routed to P7) ship in the report.
- **Eval:** support-judgment accuracy vs clinician labels; **false-support rate** on
  adversarial near-miss passages (right drug, wrong population/endpoint) — this is the
  metric that distinguishes a trustworthy advisor from a plausible one.
- **Fine-tune:** the second high-value tune; curate hard negatives (near-misses) heavily.

### P6 — Appraise harms (safety)
- **Purpose:** harms are *not* benefit-with-the-sign-flipped; appraise separately.
- **In → Out:** `PicoFrame(outcome:harm)` → `{absolute_risk, NNH, certainty,
  evidence_types:[rct|observational|pharmacovigilance], sources[]}`.
- **Procedure:** weight observational + pharmacovigilance evidence appropriately
  (harms are often only observable there); report absolute risk and NNH, not just
  relative; certainty graded independently of efficacy.
- **Eval:** NNH/absolute-risk extraction accuracy; correct down-weighting of
  signal-vs-background; separation of efficacy-certainty from harm-certainty.
- **Fine-tune:** harms-appraisal adapter; `bmlib.transparency` (ClinicalTrials.gov)
  feeds registered-harm-outcome checks.

### P7 — Counterfactual / contradictory-evidence sweep
- **Purpose:** structurally enforce balance.
- **In → Out:** `PicoFrame + current claim set` → `{contradicting:[{source_id,
  passage, support:contradicts}], null_or_negative:[…], searched: <query trace>}`.
- **Procedure:** actively retrieve null/negative/contradicting evidence (reuse
  `bmlibrarian` CounterfactualAgent design); appraise it through P3–P5 like any other.
- **Eval:** the synthesis step **fails** if P7 returned empty *without* a justified
  "searched X, none found" trace. Measured as: % of reports with a non-vacuous balance
  section; recall of known contradicting trials on a planted gold set.
- **Fine-tune:** query-generation adapter for adversarial/null-result retrieval.

### P8 — Synthesize (the report)
- **Purpose:** compose the `AdvisoryReport` and enforce the guardrails.
- **In → Out:** all of the above → `AdvisoryReport` (§7).
- **Procedure & guardrails (hard-coded, not left to the model):**
  - **Absence of evidence ≠ evidence of equivalence** — existence/NI archetypes must
    distinguish "no evidence found" from "evidence of no effect / equivalence."
  - **Efficacy non-inferiority ≠ safety non-inferiority** — separate the conclusions.
  - **Population validity** — the bottom line must state the population the evidence
    actually supports vs the asked `applicability`.
  - For `non_inferiority`: surface the **margin, assay sensitivity, ITT vs per-protocol**.
- **Eval:** the whole-report rubric (§9 acceptance) — these guardrails are checklist items.
- **Fine-tune:** synthesis prose adapter, but constrained by structured inputs; the
  guardrails are code, not model behavior.

### 6.9 Archetype → primitive recipes

All archetypes run P1, P2, P5, P8. The rest compose:

| Archetype | Adds / emphasis |
|---|---|
| **existence** (`is there evidence for X?`) | P3+P4 body-of-evidence summary; P8 guards absence≠equivalence |
| **superiority** (`is X superior to Y?`) | P3+P4 (effect size, CI, certainty, direction); P7 mandatory |
| **non_inferiority** (`is X non-inferior to Y?`) | P3+P4 + the NI guardrails (margin/assay sensitivity/ITT-PP); P6 (NI on efficacy ≠ on harms); P7 mandatory |
| **harm** (`is there harm in X?`) | P6 leads; observational weighting; P7 for null/negative harm signals |
| **best_practice** (`how to manage X?`) | guideline-aware synthesis; P4 + **strength-of-recommendation**; recency/conflict reconciliation across guidelines |

---

## 7. Output contract (`AdvisoryReport`)

What Kastellan receives and Cairn renders. Every field is auditable; the
`reasoning_trace` and `provenance` make a report **reproducible**.

```
AdvisoryReport
  question, pico_frame, archetype
  bottom_line          : one balanced paragraph, carrying an explicit certainty word
  competence           : in_scope | low_confidence | declined  (+ reason)
  claims[]             : { statement, direction, certainty(GRADE),
                           citations[], counter_citations[] }
  citations[]          : { source_id, passage, support: supports|contradicts,
                           pico_match_notes }
  evidence_quality[]   : per-outcome GRADE table + per-domain rationale
  harms                : { absolute_risk, nnh, certainty, sources[] }   # when relevant
  bias_funding         : { rob_summary, funding_flags, registration/outcome-switching flags }
  balance              : counterfactual findings — or explicit "none found; searched: …"
  certainty_overall
  strength_of_recommendation                                          # best_practice only
  uncertainty          : limits, applicability caveats, population-validity notes
  reasoning_trace[]    : ordered per-primitive steps {primitive, input, output, rationale}
  provenance           : { base_model, adapter_versions{}, corpus_snapshot_id, timestamp }
```

`provenance` pins base model + per-primitive adapter versions + corpus snapshot, so any
report can be regenerated identically for audit/medico-legal review.

---

## 8. Model & fine-tuning strategy

- **One 30B-class base + per-primitive LoRA adapters**, hot-swapped per step. This is
  exactly what `biasbuster`'s pipeline already emits, and it is single-GPU-node sane
  (one base resident, adapters swapped). Re-spike the base on current 27–32B incl. MoE
  (lower active-param cost for similar quality); model is swappable via `bmlib`'s
  `provider:model` abstraction.
- **Reasoning distillation, PHI-free by construction:** a frontier teacher (e.g.
  Anthropic) generates candidate procedure traces **over published literature** →
  **clinician curates/corrects** → the 30B student learns the corrected procedure.
  The teacher only ever sees published papers, so the egress concern does not touch
  training at all.
- **Template traces on published standards** (GRADE handbook, RoB2, ROBINS-I,
  CONSORT-NI) — teach conformance to accepted reasoning, not invented reasoning.
- **Per-primitive eval gates.** A primitive ships only when its harness clears a
  clinician-set bar (mirrors localmail's recall/MRR gates). Whole-report rubric (§9)
  gates the composition.

### Cold path (corpus curation) vs hot path (per question)
The expensive appraisal (grading every study, full counterfactual sweeps) runs **ahead
of time** to build a curated, pre-graded corpus. The per-question hot path then mostly
*retrieves pre-appraised evidence* + does the question-specific synthesis. This is what
keeps a referenced report inside a "clinician waits for a briefing" latency budget and
is the reuse home for `bmlibrarian`'s heavyweight machinery.

---

## 9. Acceptance — the walking skeleton

Build **one archetype end-to-end first**: `non_inferiority` (it exercises every
primitive and is the most dangerous genre, so it forces the guardrails early).

- **Gold set:** N real comparative questions, clinician-authored, with reference answers
  (the `multilingual_queries.json` pattern). The "patient wants A instead of our
  preferred B — non-inferior? risks?" case is gold entry #1.
- **Whole-report rubric (pass/fail per item):** citations verified (P5) · counterfactual
  present (P7) · harms separated from efficacy (P6) · NI margin + population stated (P8) ·
  funding/RoB flagged (P3 + `bmlib.transparency`) · uncertainty calibrated · correct
  declination on out-of-scope variants.
- **Per-primitive gates** (§6) must clear before the report rubric is meaningful.

When that vertical slice passes, the remaining archetypes are recipe changes over the
same primitives, not new systems.

---

## 10. Open questions / risks (honest)

1. **Data curation is the real cost and the moat.** Per-primitive reasoning-trace
   datasets with expert correction are labor — but it is *your* expertise, reusable
   across the ecology. `biasbuster` proves one primitive; scaling to ~8 is "more of the
   same," a known quantity, not a research gamble.
2. **Specialty applicability.** Appraisal *procedures* generalize across specialties;
   *applicability* judgments don't. Start with procedure-level adapters (broad); let the
   `low_confidence` flag + human-in-the-loop carry specialty nuance until there's data to tune it.
3. **Long-tail coverage.** Questions that cross or fall outside archetypes must route to
   honest declination, not confabulation. The false-confidence rate (P1) is the headline
   safety metric.
4. **Base-model re-spike.** Confirm the 20B→30B/MoE choice empirically before committing
   the fine-tune budget.
5. **Local-vs-cloud routing** is a Kastellan/CASSANDRA per-question risk decision, not a
   config flag here — design the contract so the advisor is indifferent to which it ran on
   (provenance records it).
6. **Corpus currency & reproducibility.** Snapshot the corpus; pin its id in `provenance`
   so reports are regenerable.

---

## 11. First commits (suggested)

1. This doc → `cairn-advisor` repo (cairn-ehr org), `docs/design/`.
2. The `PicoFrame` + `AdvisoryReport` schemas as typed contracts (the Kastellan API surface).
3. The `non_inferiority` gold set skeleton + the whole-report rubric as an eval harness stub.
4. P5 (citation support) + P3 (RoB, from `biasbuster`) as the first two adapters — the two
   pain points, each with its own harness.
