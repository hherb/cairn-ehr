# ADR-0014 — Locale-pluggable matcher comparators: content-addressed profiles that travel with the data

- **Status:** Accepted
- **Date:** 2026-06-16

## Context

Former open question §11.7 — *define the matcher's comparator extension point (comparator API, weight
configuration, per-deployment evaluation harness)* — is the last of the original §11 items. The matching
pipeline ([§5.2](../identity.md#52-matching-pipeline-safety-asymmetric-false-merge-worse-than-false-split))
already states the requirement — *"phonetic encodings, name structures, DOB precision handling, address
semantics are deployment configuration, not hardcoded"* — and the matcher is **advisory and external**
(Python; [§9.4](../language-substrate.md#94-merge-projection-boundary-fat-postgres-thin-rust-daemon)): it
only ever *proposes* link candidates, which become ordinary `assert`/`link` events through the closed
identity algebra ([§5.7](../identity.md#57-identity-event-algebra-closed-set-all-append-only-syncable-auditable)).

This makes §11.7 **structurally low-stakes** in a way §11.6/§11.4 were not — it touches nothing
irreversible: **no envelope reserve, no day-one commitment, no new event stream.** The blast radius is
already contained twice over: (1) a comparator feeds only *additive advisory evidence* into a conservative,
human-backstopped, coherence-checked decision ([§5.2](../identity.md#52-matching-pipeline-safety-asymmetric-false-merge-worse-than-false-split)),
and (2) *"unmerge is always possible and clean"* ([§5.1](../identity.md#51-linkage-layer-never-merge-always-link)),
so even a wrong auto-link is reversible with no data loss. The architecture already absorbed *"the matcher
will sometimes be wrong."*

Two forces, surfaced in case-mining with a clinician working across the Australian Top End, Kimberley, and
the east/south coasts, make it richer than a pure configuration question:

- **The right comparator is a property of the *data's* cultural origin, which travels with the patient — but
  comparator *code* cannot travel the clinical plane.** Indigenous Top End / Cape York naming and
  birthdate-uncertainty norms are *the rule* there and completely different from Melbourne's; people
  relocate, and **forcing one region's comparators onto another region's records on a merge is
  catastrophic** (a foreign comparator that over-trusts weak evidence manufactures false merges — the
  asymmetric error). Yet a comparator is executable code, which must never sync over the clinical mesh
  ([principle 8](../index.md#founding-principles-the-lens-for-every-decision), [ADR-0012](0012-schema-evolution-event-format-and-legibility-across-time.md)),
  and a **central registry is undesirable** (a point of capture, against the mission).
- **The silent false split is the one error the live matcher cannot see.** Generous surfacing of *suspected*
  matches passes paper-parity (paper identity-matching is equally cumbersome), but a duplicate the matcher
  *confidently rejected* is never shown to anyone — fragmented history, hidden allergies on the other chart.
  Inline metrics cannot measure confident-rejects (humans only adjudicate what the matcher surfaced).

The deeper framing the case sharpened: **hardcoding one culture's name/date/address model is cultural
capture** — the same anti-capture instinct behind vendor-independence ([principle 7](../index.md#founding-principles-the-lens-for-every-decision)),
applied to the demographic model. A matcher that assumes given+family order, Soundex, and a reliable
Gregorian DOB *fails* the Kimberley clinic, the refugee camp, and the Indonesian mononym. Locale-pluggable,
locally-evaluable comparators are **paper-parity for the registrar in any culture.**

## Decision

§11.7 **dissolves** into existing primitives composed — **no new founding principle, no envelope reserve,
one small additive data-model field.** Canonical home: [identity §5.13](../identity.md#513-locale-pluggable-comparators-the-matcher-extension-point);
the assertion-level profile tag is [demographics §4.1](../demographics.md#41-demographic-assertions); the
matcher's advisory/registered-actor status is [§5.2](../identity.md#52-matching-pipeline-safety-asymmetric-false-merge-worse-than-false-split)
/ [ADR-0011](0011-actor-registry-version-pinning-and-key-custody.md).

1. **Pluggable, never hardcoded — anti-capture applied to the demographic model.** Cairn ships the comparator
   *mechanism*, never a privileged culture's model. Phonetic encodings, name structures, nickname/diminutive
   and transliteration lexicons, DOB-precision handling, and address semantics are all plugins selected by
   deployment, not baked into the core (principle 7 / [principle 9](../index.md#founding-principles-the-lens-for-every-decision)).

2. **The comparator API contract.** A comparator is a pure, **field-typed** function
   `compare(value_a, value_b, context) → graded agreement`, returning not a boolean but an **agreement level**
   (exact / nickname- or transliteration-equivalent / phonetic / edit-distance / none), because
   Fellegi–Sunter weighs each level differently. Three contract properties are principle-bearing:
   - **Uncertainty-aware ([§3.7](../data-model.md#37-acknowledged-uncertainty-uncertainty-capable-value-types)):
     no-data is never disagreement.** A missing/unknown field contributes *zero* evidence, never a penalty —
     fabricating "disagree" from an absent DOB is the §3.7 sin and actively misleads the matcher (the
     generalization of §4.2's down-weighting of default `01-01` birthdays). Precision-tagged values yield
     *partial* agreement (year-only DOB, estimated age).
   - **Provenance-aware ([§4.2](../demographics.md#42-per-field-projection-policy)).** Agreement/disagreement
     weight scales with provenance — a *verified*-DOB clash is strong evidence against a link; an
     *imported/unknown*-DOB clash is weak.
   - **Operates over the multi-valued name *history set*, not the current display value
     ([§4.2](../demographics.md#42-per-field-projection-policy)).** Maiden/married switching, changed family
     names, and discarded aliases match because the append-only name set retained them (match if *any*
     historical name agrees). Name comparison is **token-based, role-tolerant, and order-tolerant** —
     given-name order, given/family swaps, and hyphenated-surname order compare as bags of role-tagged
     tokens, not positionally.

3. **Comparator *identity* travels with the data; comparator *code* travels the distribution plane; a
   missing comparator degrades honestly to human.** The split that resolves "comparators must travel" without
   syncing code and without a central registry:
   - A **comparator-profile tag** rides each demographic assertion as declarative, non-executable
     **provenance** (*"this value was asserted under naming-convention profile `namespace@content-hash`"*) —
     ordinary append-only data on the clinical plane. It **defaults silently from the registering node's
     locale, with a registrar-visible override** (a one-tap convention selector) for the relocation and
     **visitor** cases (a tourist injured in Cape York must not be silently tagged with the local Indigenous
     convention, and vice-versa). It is **per-assertion**, so one patient may carry an `anglo` name and a
     `capeyork` name, each tagged.
   - The comparator **code/weights** travel the [distribution plane](../security.md#76-the-software-distribution-plane-signed-releases-and-extension-load)
     (signed, per-node, verified before load, sneakernet-capable), never the clinical mesh.
   - **Content-addressed identity = no central registry.** The tag is `namespace@content-hash` — the
     human-readable namespace for selection/display, the hash for unambiguous global identity and integrity
     (the [ADR-0013](0013-attachments-content-addressed-lazy-blob-tier.md) content-addressing payoff applied
     to comparator identity: two nodes referring to the same hash mean the *same* comparator with no
     coordinator, and the tag still resolves against any mirror or sneakernet copy).
   - **Honest degradation, never the wrong comparator** — the [§3.13](../data-model.md#313-schema-evolution-event-format-and-the-legibility-twin)
     legibility-ladder / [§6.2](../sync.md#62-consistency-model) honest-assembly pattern applied to *matching*:
     when a node lacks a record's tagged comparator, or matches *across* two different profiles, it does
     **not** force its local comparator — it surfaces the pair to a human. **Safety-preserving by
     construction:** uncertainty about *which* comparator applies can only ever *withhold* an auto-link (push
     to human), never manufacture one, so it sits on the safe side of the false-merge ≫ false-split asymmetry
     automatically. The "foreign comparator forced on a relocated record" catastrophe becomes structurally
     unreachable.

4. **Weight configuration is the locale parameter set; the matcher is a registered actor.** The m/u
   probabilities per field per agreement-level *are* the deployment's locale tuning (a surname match means
   far more in a high-diversity population). Cairn ships defaults plus a way to *learn* them from local data.
   A comparator+weight bundle is the matcher's version-pinned **standing configuration** under
   [ADR-0011](0011-actor-registry-version-pinning-and-key-custody.md) (the §3.12 inference-config analogue),
   so *"which links did matcher-config v3 propose?"* is recall-traceable and a bad rollout is recalled via the
   [§5.5](../identity.md#55-reattribution-one-primitive-tiered-workflows) contamination-cascade primitive,
   exactly like an AI model recall.

5. **The evaluation harness and the duplicate-sweep miss-detector.** The human-adjudication outcomes
   accumulating in the reconciliation queue (confirmed/rejected links, disputes) are labeled evaluation data
   produced by normal operation; the harness computes precision/recall **with the false-merge rate as its own
   safety-asymmetric metric** and gates config changes behind it. The confident-reject blind spot is closed
   by a **periodic, low-priority, aggressive (low-threshold) background re-match sweep at the hub tier**
   (fractal topology — the hub has the compute and sees the population; the Pi does live single-record
   matching). The sweep **never auto-acts**; it emits a ranked *possible-duplicate* worklist for a records
   officer, runs **preemptibly at lowest priority and can never starve clinical work** (the
   [ADR-0013](0013-attachments-content-addressed-lazy-blob-tier.md) byte-tier discipline), and is additive /
   advisory (safe un-owned, [ADR-0010](0010-additive-vs-suppressing-classification.md)). Its **yield is the
   miss-rate and drift metric** the inline view cannot produce (the [ADR-0010](0010-additive-vs-suppressing-classification.md)
   atrophy-signal pattern). Two existing legs complete it: **opportunistic re-match on every new assertion**
   ([§5.2](../identity.md#52-matching-pipeline-safety-asymmetric-false-merge-worse-than-false-split)/[§5.4](../identity.md#54-unidentified-registration-john-doe-baked-into-the-root) —
   a confident-reject flips as a shared phone/ID/refined-DOB lands; monotonic refinement,
   [§3.7](../data-model.md#37-acknowledged-uncertainty-uncertainty-capable-value-types)), and a cheap
   **point-of-care "this might be a duplicate — search & link" affordance** (paper-parity *gain*: the patient
   who says "I have another file here" is evidence the matcher never had; finding the other folder on paper is
   harder).

6. **The safety floor pluggability may not relax.** Regardless of which comparators/weights are plugged in:
   auto-link still requires a **conservative threshold**, the wide middle band still goes to humans, and the
   coherence check ([§5.2](../identity.md#52-matching-pipeline-safety-asymmetric-false-merge-worse-than-false-split))
   still demotes contradictory links. A small closed set of **hard vetoes** (same-system identifier mismatch;
   *verified* DOB clash; *verified* sex-at-birth clash; deceased-status conflict — [§4.2](../demographics.md#42-per-field-projection-policy))
   **forces a human decision** — *never an auto-link, and never an auto-reject* (an auto-reject is itself a
   silent false split). *"Err on the side of caution and prompt the user."*

7. **Distribution is federated, not central — GitHub doubles as the registry.** Cairn-official, vetted,
   signed comparator packs in a Cairn repository plus community packs in theirs; signed and content-addressed,
   so trust is in the signature/hash, not the host. Git is mirrorable and sneakernet-cloneable, so GitHub is
   convenience, **never a dependency** — no single point of capture (mission-aligned).

8. **Blast radius ([§9](../language-substrate.md)).** **Fit-for-purpose** (Python, advisory): every
   comparator, the weight-learning, the evaluation harness, and the duplicate sweep — a defect yields a bad
   *proposal* a human reviews, never a silent record corruption. **Safety-critical** (in-DB): the conservative
   auto-link threshold, the hard-veto set, the coherence check, and the proposal → identity-algebra apply
   **seam** (the one path where an advisory proposal becomes an authoritative event) — the recurring
   seam motif.

## Consequences

- **Easier:** the matcher fits any culture without core changes; a relocated patient is matched safely because
  the comparator-profile travels as data and a node that lacks it degrades to human rather than guessing wrong;
  comparator identity is global with no central registry (content hash + git); the confident-reject blind spot
  becomes a measurable, standing, advisory background signal; and recall/rollout of a comparator config is free
  off the actor registry + contamination cascade.
- **Harder / new surface:** the comparator-profile tag is new (small, additive) provenance on assertions and a
  registration UI affordance (silent default + override); the hub duplicate-sweep is a new background job
  (preemptible, blocked/indexed for tractability); cross-profile matching needs an explicit "different
  conventions → human" path; and weight-learning per deployment is real ML/ops work (mitigated by shipped
  defaults).
- **The bet:** that an advisory, content-addressed, honestly-degrading comparator framework — backstopped by a
  conservative threshold, hard vetoes-to-human, the coherence check, and §5.1 reversibility — keeps the
  catastrophic false-merge rate near zero across wildly different populations while letting each tune its own
  locale, and that the low-priority hub sweep surfaces the silent false splits without ever compromising care.
  We would know it is wrong if cross-profile matching floods humans to the point of alert-fatigue
  ([§5.12](../identity.md#512-the-notification-economy-salience-responsibility-routing-and-the-acknowledgment-floor)),
  or if the sweep proves intractable at population scale even with blocking.
- **Policy-neutral ([principle 9](../index.md#founding-principles-the-lens-for-every-decision)):** Cairn ships
  the API, the harness, the sweep, the conservative-threshold-and-veto floor, and a starter comparator set;
  *which* comparators and weights a deployment runs, *which* hard vetoes it configures (above the floor), the
  sweep cadence, and whether registration auto-defaults or forces an explicit convention choice, are policy.
