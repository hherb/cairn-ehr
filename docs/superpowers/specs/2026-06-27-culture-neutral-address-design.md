# Design — Culture-neutral address representation (demographics §4)

**Date:** 2026-06-27 · **Status:** design approved, pre-implementation · **Scope:**
demographics §4 address model only (a reusable pattern, not a generalised field
framework — see Scope).

## Problem

Demographic data shape varies enormously by nation, legislation, and culture — name
models, address structure, and identifier systems all differ. An international,
anti-capture EHR must let the **infrastructure carry any demographic representation**
while the **UI localises presentation** to the operator's context. The same record must
work for a clinician in a Sudanese refugee camp and one in a metropolitan hospital, and
neither may emit something the other cannot read.

A prior-art review of a mature single-jurisdiction relational EHR schema showed the
characteristic failure: address structure (street/town/postcode) and a fixed town
lookup were welded into the schema, so supporting another jurisdiction would mean schema
migration. Cairn's demographics §4 already models **names** richly (multi-valued set +
transliteration + per-assertion comparator profile) and **identifiers** as a keyed set,
but treats **address** thinly — "volatile; recency wins; nearly meaningless for
matching." That is a *matching* statement, not a *representation* one: §4 has no
culture-neutral structured-address model and no geolocation fallback. This design closes
that gap.

## Requirements (agreed)

The **infrastructure must guarantee, for every address, regardless of culture:**
1. **Human-readable display** — always renders correctly no matter how sparse or exotic
   the structure (principle 11 legibility twin).
2. **Geographic proximity** — an optional but first-class geolocation for routing,
   catchment, outbreak mapping; works even with no street address.
3. **Full structured/postal parts** — machine-readable components for postal formatting
   and integration, but as culture-tagged **data**, never fixed columns.

Explicitly **not** a hard infrastructure requirement: a coarse administrative-region
query spine (region aggregation is best-effort over the structured parts when present).

## Scope

Address only. The pattern (content-addressed culture-profile + mandatory legibility twin
+ optional geolocation) is deliberately reusable, but this spec does **not** generalise
it into a universal field framework, and does **not** revisit the existing §4.2 name
model. Identifier representation (gap B) and tying the legibility twin to all demographic
assertions (gap C) remain separate future threads.

## Approaches considered

- **A — Three facets, display is the derived floor (chosen).** Only `display` is
  mandatory; `geo` and `structured` are optional first-class enrichments; the structured
  parts are interpreted by a content-addressed locale profile reusing the ADR-0014
  mechanism. Most faithful to "infrastructure carries any representation; UI presents,"
  principle-4-clean, and extends a mechanism §4 already has.
- **B — Everything is structured; freeform is a degenerate profile.** Single invariant
  (display always derived from parts), but forces ceremony onto sparse/geo-only
  addresses. Rejected as over-imposing.
- **C — Canonical superset + extensions (FHIR-shaped).** Best out-of-the-box interop but
  re-imposes a Western-ish canonical shape (the capture we are escaping). Rejected for
  the core; FHIR mapping belongs in the interop façade (§9.7), not the wire model.

## Design

### 1. The address assertion value

Asserted through the existing §4.1 mechanism (*source S asserts at HLC t that field
`address` of patient P has this value, provenance class C*). The new part is the value
shape — three facets, only the first mandatory:

```
AddressValue {
  display    : text          -- MANDATORY. The legibility twin (principle 11): the
                             -- complete human-readable address. Always sufficient alone.
  geo        : Geolocation?  -- OPTIONAL. Universal locator (§4).
  structured : Structured?   -- OPTIONAL. Culture-tagged machine-readable parts.
}

Structured {
  profile : profile-ref      -- content-addressed locale bundle "namespace@hash"
                             -- (the ADR-0014 mechanism).
  parts   : { key: value }   -- open ordered bag of named components; key/value semantics
                             -- defined by the profile, NOT by the wire schema.
}
```

Load-bearing invariant: **`display` alone is a complete, valid address.** Everything else
is enrichment. Consequences: no canonical part names in the wire model (zero
per-jurisdiction migration — the principle-11 / ADR-0012 win); `parts` values are opaque
text to the core (no floor `CHECK` constraints); address history is intact (append-only).

### 2. How `display` is produced (principle-11 compliance)

Two production modes, with a drift-prevention rule:
- **Derived** (when `structured` present): `display` **must** equal `formatter(parts)`
  from the profile. Not independently authored — so it cannot drift from the parts.
- **Authored** (when `structured` absent): the human-entered text *is* the address;
  `display` carries it verbatim (derivation degenerates to identity).
- **Geo-only**: `display` derived by a built-in culture-neutral coordinate formatter.

Details:
- `display` is **materialised into the signed event at authoring time**, so a receiving
  node lacking the profile still has correct display text; re-derivation/verification
  needs the profile. (Same shape Cairn already uses for the legibility twin.)
- Drift is **detectable and verifiable**: any profile-holding node recomputes
  `formatter(parts)` and confirms it matches the signed `display`.
- **Multiple scripts/transliterations are multiple assertions**, each with its own
  `profile` + `display` (reusing §4.2 multi-valued-set + ADR-0014 per-assertion tagging).
  One `AddressValue` carries exactly one `display`.

### 3. The address-profile (content-addressed locale bundle)

The `profile` ref travels per-assertion as signed **data**; the bundle it names travels
the **distribution plane** (ADR-0012 code plane), exactly as ADR-0014 does for
comparators. One locale bundle per culture carries everything:

```
AddressProfile bundle (content-addressed) {
  grammar    : named parts, order, expected vs optional
  formatter  : parts -> display text (the §2 derivation)
  validators : per-part ADVISORY format checks (e.g. postcode shape)
  comparator : the ADR-0014 matcher normalisation/comparison for this culture
}
```

So address grammar/formatter and the ADR-0014 comparator are the **same bundle**, not two
parallel systems.

- **Validation is advisory, never a floor constraint.** This is the deliberate answer to
  the prior-art `CHECK`-constraint trade-off: moving structure to data trades away
  DB-guaranteed format validation, recovered as per-profile advisory validators that
  **flag for human review, never reject** (principle 4). The in-DB **floor** enforces
  only culture-neutral structural invariants: `display` non-empty; `structured` ⇒
  `profile` present; `parts` are text. The floor never holds a profile (principle 12).
- **Profile assignment** follows ADR-0014: defaults silently from the registering node's
  locale, registrar-overridable, per-assertion (the visitor/relocation case). No central
  registry; content-addressing + the signed distribution registry make it globally
  meaningful with zero coordination.
- **Honest degradation when a node lacks the profile:** `display`+`geo` still work;
  `parts` shown as opaque labelled strings, never reinterpreted under a substitute
  profile; matching degrades to human review. Never forces the wrong structure.
- **Known limitation (deliberate):** the floor does not verify `display ==
  formatter(parts)` (that needs the profile), so a buggy/hostile authoring node could
  sign an inconsistent `display`. This is **detectable** (profile-holding nodes recompute
  and flag) and is not silent record corruption (both facets are signed, immutable,
  attributable) — surfaced for human review, consistent with "flag, never auto-resolve."
  Cross-facet consistency is an advisory verification, not a floor gate.

### 4. The geolocation facet

```
Geolocation {
  lat, lon   : decimal degrees (WGS84 default; datum named if other)
  accuracy_m : radius of uncertainty in metres   -- principle 4, in space
  basis      : how obtained (device_gps | map_pin | geocoded_from_text |
               region_centroid | declared)       -- provenance for the point
}
```

Principle 4 applied to space — the spatial twin of DOB's `(value, precision, basis)`. The
culture-neutral universal locator, frequently the only viable address in informal
settlements, refugee camps, disaster response, remote/nomadic contexts. Boundaries:
- **Geocoding is not a wire operation.** Turning text into coordinates produces a *new*
  geo assertion (`basis = geocoded_from_text`, append-only), never a silent mutation;
  geocoders are advisory plugins (§9).
- **Point + radius only; polygons deferred** (YAGNI; a later profile-carried extension if
  a real need appears).
- **Measurement uncertainty ≠ deliberate obfuscation.** `accuracy_m` honestly represents
  how well the location is known; deliberately coarsening (e.g. a refuge) is a
  projection/policy act (ADR-0006), distinct and declared, never disguised as low
  accuracy.

### 5. Fitting §4: stream, projection, matching, confidentiality

- **Assertion stream:** append-only, set-union, history retained. "Moved out" is an
  explicit superseding/ended assertion with a `t_effective`, not a boolean flag — `when`
  and `why` are recoverable.
- **Multi-valued with an optional `use` tag** from a small **recommended-but-open**
  vocabulary (`residential`, `postal`, `temporary`, `work`, …) — recommended so the
  common case interoperates, open so it cannot become capture.
- **Country is a `part`, not a privileged column** (stateless / disputed-territory /
  cross-border cases; no coarse-region spine was required). Profiles conventionally
  include it, so it is reliably present without being privileged.
- **Refined §4.2 address projection row:** *Address — multi-valued, volatile,
  `use`-scoped. Per `use`: displayed current = highest-provenance most-recent
  non-superseded assertion; full history retained; supersession is an explicit assertion,
  never an overwrite. Conflicts across linked records: weak identity evidence.*
- **Matching role:** weak/advisory, culture-aware (comparison via the profile's
  comparator), with tight geo or exact structured match as *mild positive* evidence
  weighted by `accuracy_m`; never a hard veto or strong link; no-profile → human review.
- **Confidentiality (ADR-0006):** address facets (geo especially) can be acutely
  sensitive; a sealed address emits a de-identified existence/severity projection without
  disclosure; break-glass is audited key-use. Mechanism only; policy above the line.

### 6. Layering (principle 12 / ADR-0021)

`display` is the only wire-mandatory facet, keeping the core permissive and
interoperable. A clinic that needs structured addresses enforces that as **soft policy in
its UI** — a bespoke UI may demand structure for its own context but can **never** emit a
wire-incompatible address. Many front-ends, one record.

## Worked spectrum (the acceptance test)

| Setting | `display` | `geo` | `structured` |
|---|---|---|---|
| Refugee camp | "Tent block C, Sector 4, Kakuma" (authored) | centroid, ±500 m | — |
| Metro hospital (Tokyo) | local script, derived | map-pin, ±10 m | `jp@hash`: prefecture/city/chōme/banchi |
| Rural (no street #) | "Ballyvaughan, Co. Clare" derived | — | `ie@hash`: townland/county |
| No fixed abode | "No fixed address" (explicit, ≠ not-asked) | — | — |

Same three-facet value throughout; different optional facets populate; neither clinician
can emit what the other cannot read.

## Implementation (documentation/spec change set — no code; clinical tier not built)

1. **New `docs/spec/demographics.md` §4.3** — the `AddressValue` model (three facets,
   twin-derivation rule, profile, geo, `use`, country-as-part, layering note).
2. **Refine the §4.2 address projection row** (Section 5 wording).
3. **`docs/spec/identity.md` §5.13 cross-ref** — the comparator bundle also carries the
   address grammar/formatter (one locale bundle).
4. **New ADR-0032 "Culture-neutral address representation"** — the *why* (a
   can't-easily-retrofit, day-one value shape, cf. ADR-0013's attachment-reference
   shape); references ADR-0014/0012/0006 and principles 4/11/12. ADRs are immutable, so a
   new ADR, not an edit to 0014.
5. **`docs/spec/index.md`** spec-version bump (0.32 → 0.33).

## Out of scope / future threads

- Gap **B** — identifier representation (validation-as-advisory; provider/professional
  identifiers scoped to person × organisation; confirm home vs §7.5 actor registry).
- Gap **C** — tie the principle-11 legibility twin to all demographic assertions.
- Coarse administrative-region query spine (not required; best-effort over parts).
- Address boundary polygons.
