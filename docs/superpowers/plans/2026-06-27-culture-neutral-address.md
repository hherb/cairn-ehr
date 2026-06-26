# Culture-neutral Address Representation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the approved culture-neutral address design into the canonical spec — a new ADR-0032, a new demographics §4.3 address model, a refined §4.2 projection row, an identity §5.13 cross-reference, and a spec-version bump.

**Architecture:** Documentation/spec change set only — there is **no code** (the clinical tier is not built yet). The "why" goes in an immutable ADR; the "what" goes in the spec aspect files. Source of truth for content is the design doc `docs/superpowers/specs/2026-06-27-culture-neutral-address-design.md`.

**Tech Stack:** Markdown (GitHub/Obsidian callout syntax), MkDocs Material build.

## Global Constraints

- **The design doc is the content source of truth:** `docs/superpowers/specs/2026-06-27-culture-neutral-address-design.md`. Do not invent new design; transcribe and adapt.
- **ADRs are immutable** — a new decision is a new ADR, never an edit to an existing one. ADR-0032 is the next free number (highest existing is 0031).
- **The spec carries no in-file changelogs and no filename version suffixes;** git is the line history. Spec version lives only in `docs/spec/index.md`.
- **Author callouts in GitHub/Obsidian syntax** (`> [!NOTE]`) so they render on GitHub and as Material admonitions.
- **Cross-references use relative paths** with section anchors (e.g. `[§4.2](demographics.md#42-per-field-projection-policy)`), matching the style already in `identity.md`/`demographics.md`.
- **Never commit the generated `site/`** (gitignored).
- **Product neutrality:** do not name any prior/real EHR product in committed docs; describe prior art generically.
- **Build/verify command** (the doc "test"):
  ```
  uv run --with mkdocs-material --with mkdocs-callouts --with mkdocs-redirects -- mkdocs build
  ```
  Expected: exits 0; no `WARNING` about unresolved links for the files we touched.
- **Work happens on branch `culture-neutral-address-design`** (already created; the design doc is committed there).

---

### Task 1: ADR-0032 — the immutable "why", + decisions index row

**Files:**
- Create: `docs/spec/decisions/0032-culture-neutral-address-representation.md`
- Modify: `docs/spec/decisions/README.md` (append one row to the ADR table, before the `## Template` heading)

**Interfaces:**
- Produces: the ADR path `decisions/0032-culture-neutral-address-representation.md` and anchor, referenced by Tasks 2–4.
- Consumes: nothing (foundational).

- [ ] **Step 1: Verify the ADR number is free**

Run: `ls docs/spec/decisions/ | grep -E '^0032' || echo FREE`
Expected: `FREE`

- [ ] **Step 2: Create the ADR file with this exact content**

Create `docs/spec/decisions/0032-culture-neutral-address-representation.md`:

```markdown
# ADR-0032 — Culture-neutral address representation: the three-facet address value

- **Status:** Accepted
- **Date:** 2026-06-27
- **Refines:** [ADR-0014](0014-locale-pluggable-matcher-comparators.md)

## Context

Demographic data shape varies enormously by nation, legislation, and culture — name models, address structure, and identifier systems all differ. Cairn is international and anti-capture: the **infrastructure must carry any demographic representation** while the **UI localises presentation** to the operator's context. The same record must work for a clinician in a refugee camp and one in a metropolitan hospital, and neither may emit something the other cannot read ([principle 12](../index.md#founding-principles-the-lens-for-every-decision), uniform core / plural edges).

The characteristic failure of single-jurisdiction designs is to **weld one culture's address structure into the schema** — street/town/postcode columns, a fixed town lookup — so supporting another jurisdiction means a schema migration. That is the same mistake as encoding national identifier *types* as columns: it forces a lockstep, per-jurisdiction change and violates additive-only evolution ([ADR-0012](0012-schema-evolution-event-format-and-legibility-across-time.md) / [principle 11](../index.md#founding-principles-the-lens-for-every-decision)).

Demographics [§4](../demographics.md) already models **names** richly (multi-valued set + transliteration + per-assertion comparator profile, [ADR-0014](0014-locale-pluggable-matcher-comparators.md)) and **identifiers** as a keyed set, but treated **address** thinly — "volatile; recency wins; nearly meaningless for matching." That is a *matching* statement, not a *representation* one. The address **value shape** is a **can't-retrofit, day-one decision** (like the [ADR-0013](0013-attachments-content-addressed-lazy-blob-tier.md) attachment-reference shape): once records carry a shape, changing it is a migration across the whole record. So it is fixed now.

## Decision

An address is asserted through the existing [§4.1](../demographics.md#41-demographic-assertions) mechanism; the **value** is a three-facet structure, only the first mandatory:

1. **`display`** (mandatory) — the human-readable address, the [principle 11](../index.md#founding-principles-the-lens-for-every-decision) legibility twin. Always sufficient on its own. **Derived** from the structured parts via the profile's formatter when structured parts are present (so it cannot drift); **authored** verbatim when they are absent; materialised into the signed event at authoring time so a node lacking the profile still has correct display text.
2. **`geo`** (optional) — a first-class geolocation `(lat, lon, accuracy_m, basis)`, precision-aware ([principle 4](../index.md#founding-principles-the-lens-for-every-decision) applied to space). The culture-neutral universal locator, often the only viable address in informal-settlement / refugee / disaster / remote contexts.
3. **`structured`** (optional) — an open ordered bag of named `parts` plus a **content-addressed locale `profile`** (`namespace@hash`) that defines the parts, their order, the formatter, advisory validators, and the [ADR-0014](0014-locale-pluggable-matcher-comparators.md) matcher comparator. The profile *reference* travels per-assertion as signed data; the profile *code* travels the [security §7.6](../security.md#76-the-software-distribution-plane-signed-releases-and-extension-load) distribution plane. **One locale bundle per culture carries comparator + grammar + formatter + validators** — not two parallel systems.

There are **no canonical part names in the wire model** — `parts` keys are interpreted by the profile, never by Cairn core; **`parts` values are opaque text to the core**. Country is a `part`, not a privileged column (stateless / disputed-territory / cross-border cases). Validation is **advisory per profile, flagging for human review, never rejecting** ([principle 4](../index.md#founding-principles-the-lens-for-every-decision)); the in-DB **floor** enforces only the culture-neutral structural invariants (`display` non-empty; `structured` ⇒ `profile` present; `parts` are text) and never holds a profile ([principle 12](../index.md#founding-principles-the-lens-for-every-decision)). Multiple scripts/transliterations are multiple assertions, each with its own `profile`+`display`. A clinic that needs structured addresses enforces it as **soft policy in its UI** ([ADR-0021](0021-layering-the-node-api-and-ui-pluralism.md)) — a bespoke UI can demand structure but can never emit a wire-incompatible address.

Profile assignment follows [ADR-0014](0014-locale-pluggable-matcher-comparators.md) verbatim: defaults silently from the registering node's locale, registrar-overridable, per-assertion. When a node lacks a record's profile it **degrades honestly** — `display` and `geo` still work, `parts` show as opaque labelled strings (never reinterpreted under a substitute profile), matching goes to human review.

## Consequences

- **Easier:** any jurisdiction is supported with **no schema migration** (parts + profile are data); the refugee-camp and low-infrastructure cases work via `geo` and/or freeform `display`; address history is intact (append-only; "moved out" is an explicit superseding assertion, not a flag); FHIR address mapping stays in the interop façade ([§9.7](../language-substrate.md)), out of the wire model.
- **Harder / the bet:** moving structure to data **trades away DB-guaranteed format validation and uniqueness** (recovered as advisory per-profile validators). Cross-facet consistency (`display == formatter(parts)`) is **not floor-gated** — to keep the floor culture-neutral — so a buggy/hostile authoring node could sign an inconsistent `display`; this is **detectable** (any profile-holding node recomputes and flags it) and is not silent record corruption (both facets signed, immutable, attributable), surfaced for human review per "flag, never auto-resolve." We are betting (as in [ADR-0014](0014-locale-pluggable-matcher-comparators.md)) that content-addressed profiles distribute reliably off the clinical plane.
- **How we'd know the bet fails:** addresses arrive that no available profile can format; clinicians are forced into a wrong structure (should be impossible — `display`/freeform is always available); or display-vs-parts drift goes undetected at scale (watch the advisory consistency-flag yield).
```

- [ ] **Step 3: Append the ADR row to the decisions index**

In `docs/spec/decisions/README.md`, the ADR table ends with the `0031` row immediately followed by a blank line and `## Template`. Add this row directly after the `0031` row (and before the blank line / `## Template`):

```markdown
| [0032](0032-culture-neutral-address-representation.md) | Culture-neutral address representation: the three-facet address value | Accepted (refines 0014) | 2026-06-27 |
```

- [ ] **Step 4: Build and verify links resolve**

Run: `uv run --with mkdocs-material --with mkdocs-callouts --with mkdocs-redirects -- mkdocs build 2>&1 | grep -iE 'warning|error' || echo CLEAN`
Expected: `CLEAN` (no warnings/errors referencing `0032` or broken anchors).

- [ ] **Step 5: Commit**

```bash
git add docs/spec/decisions/0032-culture-neutral-address-representation.md docs/spec/decisions/README.md
git commit -m "spec(adr): ADR-0032 culture-neutral address representation

Three-facet address value (mandatory display legibility twin + optional
geolocation + optional culture-tagged structured parts via a content-addressed
locale profile reusing ADR-0014). The immutable 'why'; refines ADR-0014.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: demographics §4.3 (the address model) + refined §4.2 row

**Files:**
- Modify: `docs/spec/demographics.md` (split the `Phone, address` row in the §4.2 table; append a new `## 4.3` section at end of file)

**Interfaces:**
- Consumes: ADR-0032 path/anchor from Task 1.
- Produces: anchor `#43-address-the-three-facet-value` referenced by Task 3.

- [ ] **Step 1: Verify §4.3 is absent and the combined row is present**

Run: `grep -nE '## 4\.3|Phone, address' docs/spec/demographics.md`
Expected: one match — the `| Phone, address | Volatile | ...` row; **no** `## 4.3`.

- [ ] **Step 2: Split the §4.2 table row**

In `docs/spec/demographics.md` §4.2 table, replace this exact line:

```markdown
| Phone, address | Volatile | Recency (HLC) wins; history retained | Nearly meaningless |
```

with these two lines:

```markdown
| Phone | Volatile | Recency (HLC) wins; history retained | Nearly meaningless |
| Address | Multi-valued, volatile, `use`-scoped (§4.3) | Per `use`: displayed current = highest-provenance most-recent non-superseded assertion; full history retained; supersession is an explicit assertion, never an overwrite | Weak evidence (culture-aware via the profile comparator; tight `geo` or exact structured match is mild positive evidence, weighted by `accuracy_m`) |
```

- [ ] **Step 3: Append the §4.3 section at the end of the file**

Append to `docs/spec/demographics.md`:

```markdown

## 4.3 Address: the three-facet value

Demographic shape varies by nation, legislation, and culture; the **infrastructure must carry any address representation** while the **UI localises presentation** ([ADR-0032](decisions/0032-culture-neutral-address-representation.md), [principle 12](index.md#founding-principles-the-lens-for-every-decision)). An address is a §4.1 assertion whose value has **three facets, only the first mandatory**:

- **`display`** *(mandatory)* — the complete human-readable address, the [principle 11](index.md#founding-principles-the-lens-for-every-decision) legibility twin; always sufficient alone. It is **derived** from the structured parts by the profile's formatter when they are present (so it cannot drift), **authored** verbatim when they are absent, and **materialised into the signed event at authoring time** so a node lacking the profile still has correct display text (re-derivation/verification needs the profile).
- **`geo`** *(optional)* — a first-class geolocation `(lat, lon, accuracy_m, basis)`, precision-aware ([principle 4](index.md#founding-principles-the-lens-for-every-decision) in space): `accuracy_m` is the honest uncertainty radius (GPS ±10 m, village centroid ±2 km), `basis` its provenance (`device_gps` / `map_pin` / `geocoded_from_text` / `region_centroid` / `declared`). The culture-neutral universal locator, frequently the only viable address in informal-settlement, refugee, disaster, and remote contexts. Geocoding is advisory and off the wire — turning text into a point is a *new* `geocoded_from_text` assertion, never a mutation.
- **`structured`** *(optional)* — an open ordered bag of named `parts` plus a content-addressed locale **`profile`** (`namespace@hash`). The profile defines the parts, their order, the formatter, advisory validators, and the [§5.13](identity.md#513-locale-pluggable-comparators-the-matcher-extension-point) matcher comparator — **one locale bundle carries all of these**. The profile *reference* rides the assertion as data; the profile *code* travels the [§7.6](security.md#76-the-software-distribution-plane-signed-releases-and-extension-load) distribution plane.

**No canonical part names; values opaque to the core.** `parts` keys are interpreted by the profile, never by Cairn core; **country is a `part`, not a privileged column** (stateless / disputed-territory / cross-border cases). **Validation is advisory** — per-profile validators flag for human review, never reject ([principle 4](index.md#founding-principles-the-lens-for-every-decision)); the in-DB floor enforces only the culture-neutral structural invariants (`display` non-empty; `structured` ⇒ `profile` present; `parts` are text) and never holds a profile ([principle 12](index.md#founding-principles-the-lens-for-every-decision)).

**Multi-valued, `use`-scoped.** A patient holds a *set* of addresses; each assertion carries an optional `use` from a **recommended-but-open** vocabulary (`residential`, `postal`, `temporary`, `work`, …) — recommended so the common case interoperates, open so it cannot become capture. Multiple scripts/transliterations are multiple assertions, each with its own `profile`+`display`.

**Honest degradation.** A node lacking a record's profile still shows `display` and uses `geo`; `parts` render as opaque labelled strings (never reinterpreted under a substitute profile); matching degrades to human review. Cross-facet consistency (`display == formatter(parts)`) is an *advisory verification* by profile-holding nodes, not a floor gate — keeping the floor culture-neutral ([ADR-0032](decisions/0032-culture-neutral-address-representation.md)). Confidentiality (refuge addresses, geo especially) is [ADR-0006](decisions/0006-visibility-scope-replication-and-the-safety-projection.md) key-custody + safety-projection; a clinic that requires structured entry enforces it as soft UI policy ([ADR-0021](decisions/0021-layering-the-node-api-and-ui-pluralism.md)), never on the wire.
```

- [ ] **Step 4: Build and verify**

Run: `uv run --with mkdocs-material --with mkdocs-callouts --with mkdocs-redirects -- mkdocs build 2>&1 | grep -iE 'warning|error' || echo CLEAN`
Expected: `CLEAN`.

- [ ] **Step 5: Verify the new section and split row exist**

Run: `grep -nE '## 4\.3 Address|^\| Address \||^\| Phone \|' docs/spec/demographics.md`
Expected: three matches (the §4.3 heading, the new `Address` row, the new `Phone` row).

- [ ] **Step 6: Commit**

```bash
git add docs/spec/demographics.md
git commit -m "spec(demographics): §4.3 three-facet address model; split §4.2 phone/address row

New §4.3 (display legibility twin + optional geolocation + optional culture-tagged
structured parts via content-addressed locale profile) per ADR-0032; refine the
§4.2 projection row, splitting Phone from a culture-aware Address row.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: identity §5.13 cross-reference

**Files:**
- Modify: `docs/spec/identity.md` (§5.13, the "Weight configuration … registered actor" bullet — add a sentence noting the bundle also carries the address grammar/formatter)

**Interfaces:**
- Consumes: the §4.3 anchor from Task 2 and ADR-0032 from Task 1.
- Produces: nothing downstream.

- [ ] **Step 1: Locate the anchor bullet**

Run: `grep -n 'Weight configuration is the locale parameter set' docs/spec/identity.md`
Expected: one match (a `- **Weight configuration …**` bullet in §5.13).

- [ ] **Step 2: Append a cross-reference sentence to that bullet**

In `docs/spec/identity.md` §5.13, find the bullet beginning `- **Weight configuration is the locale parameter set; the matcher is a registered actor.**`. At the **end of that bullet's paragraph** (after the existing sentence ending `…via the [§5.5](#55-reattribution-one-primitive-tiered-workflows) contamination cascade.`), append:

```markdown
 The **same content-addressed locale bundle also carries the address grammar, formatter, and advisory validators** ([§4.3](demographics.md#43-address-the-three-facet-value), [ADR-0032](decisions/0032-culture-neutral-address-representation.md)) — a culture is defined once, not as separate comparator and address systems.
```

- [ ] **Step 3: Build and verify**

Run: `uv run --with mkdocs-material --with mkdocs-callouts --with mkdocs-redirects -- mkdocs build 2>&1 | grep -iE 'warning|error' || echo CLEAN`
Expected: `CLEAN`.

- [ ] **Step 4: Verify the cross-ref landed**

Run: `grep -n 'address grammar, formatter, and advisory validators' docs/spec/identity.md`
Expected: one match.

- [ ] **Step 5: Commit**

```bash
git add docs/spec/identity.md
git commit -m "spec(identity): §5.13 note the locale bundle also carries address grammar/formatter

Cross-reference the new §4.3 / ADR-0032: one content-addressed locale bundle
carries comparator + address grammar + formatter + validators.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: spec version bump + full build gate

**Files:**
- Modify: `docs/spec/index.md` (version line)

**Interfaces:**
- Consumes: nothing.
- Produces: the published spec version 0.33.

- [ ] **Step 1: Confirm the current version**

Run: `grep -n 'Spec version:' docs/spec/index.md`
Expected: one match reading `**Spec version:** 0.32 · …`.

- [ ] **Step 2: Bump 0.32 → 0.33**

In `docs/spec/index.md`, replace `**Spec version:** 0.32` with `**Spec version:** 0.33`.

- [ ] **Step 3: Full clean-build gate**

Run: `uv run --with mkdocs-material --with mkdocs-callouts --with mkdocs-redirects -- mkdocs build`
Expected: exits 0; scan output — **no** `WARNING`/`ERROR` lines referencing any file touched in Tasks 1–4.

- [ ] **Step 4: Verify the bump**

Run: `grep -n 'Spec version:' docs/spec/index.md`
Expected: `**Spec version:** 0.33 · …`.

- [ ] **Step 5: Commit**

```bash
git add docs/spec/index.md
git commit -m "spec: bump spec version 0.32 -> 0.33 (ADR-0032, demographics §4.3)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## After the plan (not tasks — for the session, post-execution)

- Regenerate `docs/HANDOVER.md` and prune `docs/ROADMAP.md` to reflect ADR-0032 / §4.3 (and fix the HANDOVER v0.31→v0.33 currency drift); add a Phase-4 ROADMAP note that the address model is specified.
- Push branch `culture-neutral-address-design` and open a PR to `main` describing the change set.
- Out of scope (future threads, do not start here): gap B (identifier representation), gap C (legibility twin tied to all demographic assertions), coarse-region query spine, address boundary polygons.
```
