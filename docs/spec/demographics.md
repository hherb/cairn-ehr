# 4. Demographics — Assertion Stream Model

Demographics are matching evidence as much as they are display data. Overwriting them (LWW storage) destroys evidence (maiden names, old phone numbers, prior transliterations). Therefore:

## 4.1 Demographic assertions
Each change is an immutable **assertion event**: *source S asserts at HLC t that field F of patient P has value V, with provenance class C.* Displayed demographics are a projection. Sync is set union, conflict-free.

**Provenance ladder:** document-verified > patient-stated > third-party-stated > clinician-observed > imported/unknown > inferred. Capturing provenance must cost the registrar one tap.

**Comparator-profile tag (matcher interpretation provenance).** An assertion also carries an optional **comparator-profile** — the naming/date/address convention under which the value should be *interpreted* by the matcher (`namespace@content-hash`, content-addressed so it is globally meaningful with no central registry). It is additive provenance (no day-one reserve; absence = unknown → the matcher degrades to human review), **defaults silently from the registering node's locale with a registrar-visible override** (the relocation and visitor cases — a tourist injured in the Top End must not be silently tagged with the local Indigenous convention, nor vice-versa), and is **per-assertion** (one patient may carry differently-tagged names). This is what lets the right comparator *travel with the data* while the comparator code travels the distribution plane — see [identity §5.13](identity.md#513-locale-pluggable-comparators-the-matcher-extension-point), [ADR-0014](decisions/0014-locale-pluggable-matcher-comparators.md).

## 4.2 Per-field projection policy
| Field | Nature | Projection rule | Conflict across linked records means |
|---|---|---|---|
| Names | Multi-valued set (legal, maiden, alias, transliteration) | All retained; display = highest-provenance recent legal name | Weak evidence |
| DOB | Stable, precision-aware: `(value, precision, basis)` | Provenance beats recency; verified value locks vs. lower provenance | **Strong evidence against link** |
| Sex / gender | Three fields: sex-at-birth, administrative sex, gender identity | Sex-at-birth provenance-locked; gender identity patient-stated authoritative, recency wins | Sex-at-birth conflict: strong evidence against link |
| Identifiers (national ID, insurance, program IDs) | Multi-valued set keyed by issuing system | Set union, never LWW | Same-system different-value = **very strong evidence against link** |
| Phone | Volatile | Recency (HLC) wins; history retained | Nearly meaningless |
| Address | Multi-valued, volatile, `use`-scoped (§4.3) | Per `use`: displayed current = highest-provenance most-recent non-superseded assertion; full history retained; supersession is an explicit assertion, never an overwrite | Weak evidence (culture-aware via the profile comparator; tight `geo` or exact structured match is mild positive evidence, weighted by `accuracy_m`) |
| Deceased status | Safety-asymmetric | Sets easily, never auto-clears; reversal = explicit human event | Strong evidence against link |
| Photo | Optional; powerful in low-ID settings | Append-only gallery, newest displayed | Human-reviewable evidence |

Notes:
- DOB precision is first-class ("age about 40, recorded 2026-06"). Default 01-01 birthdays are down-weighted by the matcher (overrepresented in low-resource registries).
- Conflicting "corrections" at equal provenance during a partition are **not** auto-resolved: project prior stable value, flag for human review. Rule: *recency resolves volatile fields; humans resolve identity-bearing fields.*

## 4.3 Address: the three-facet value

Demographic shape varies by nation, legislation, and culture; the **infrastructure must carry any address representation** while the **UI localises presentation** ([ADR-0032](decisions/0032-culture-neutral-address-representation.md), [principle 12](index.md#founding-principles-the-lens-for-every-decision)). An address is a §4.1 assertion whose value has **three facets, only the first mandatory**:

- **`display`** *(mandatory)* — the complete human-readable address, the [principle 11](index.md#founding-principles-the-lens-for-every-decision) legibility twin; always sufficient alone. It is **derived** from the structured parts by the profile's formatter when they are present (so it cannot drift), **authored** verbatim when they are absent, and **materialised into the signed event at authoring time** so a node lacking the profile still has correct display text (re-derivation/verification needs the profile).
- **`geo`** *(optional)* — a first-class geolocation `(lat, lon, accuracy_m, basis)`, precision-aware ([principle 4](index.md#founding-principles-the-lens-for-every-decision) in space): `accuracy_m` is the honest uncertainty radius (GPS ±10 m, village centroid ±2 km), `basis` its provenance (`device_gps` / `map_pin` / `geocoded_from_text` / `region_centroid` / `declared`). The culture-neutral universal locator, frequently the only viable address in informal-settlement, refugee, disaster, and remote contexts. Geocoding is advisory and off the wire — turning text into a point is a *new* `geocoded_from_text` assertion, never a mutation.
- **`structured`** *(optional)* — an open ordered bag of named `parts` plus a content-addressed locale **`profile`** (`namespace@hash`). The profile defines the parts, their order, the formatter, advisory validators, and the [§5.13](identity.md#513-locale-pluggable-comparators-the-matcher-extension-point) matcher comparator — **one locale bundle carries all of these**. The profile *reference* rides the assertion as data; the profile *code* travels the [§7.6](security.md#76-the-software-distribution-plane-signed-releases-and-extension-load) distribution plane.

**No canonical part names; values opaque to the core.** `parts` keys are interpreted by the profile, never by Cairn core; **country is a `part`, not a privileged column** (stateless / disputed-territory / cross-border cases). **Validation is advisory** — per-profile validators flag for human review, never reject ([principle 4](index.md#founding-principles-the-lens-for-every-decision)); the in-DB floor enforces only the culture-neutral structural invariants (`display` non-empty; `structured` ⇒ `profile` present; `parts` are text) and never holds a profile ([principle 12](index.md#founding-principles-the-lens-for-every-decision)).

**Multi-valued, `use`-scoped.** A patient holds a *set* of addresses; each assertion carries an optional `use` from a **recommended-but-open** vocabulary (`residential`, `postal`, `temporary`, `work`, …) — recommended so the common case interoperates, open so it cannot become capture. Multiple scripts/transliterations are multiple assertions, each with its own `profile`+`display`.

**Honest degradation.** A node lacking a record's profile still shows `display` and uses `geo`; `parts` render as opaque labelled strings (never reinterpreted under a substitute profile); matching degrades to human review. Cross-facet consistency (`display == formatter(parts)`) is an *advisory verification* by profile-holding nodes, not a floor gate — keeping the floor culture-neutral ([ADR-0032](decisions/0032-culture-neutral-address-representation.md)). Confidentiality (refuge addresses, geo especially) is [ADR-0006](decisions/0006-visibility-scope-replication-and-the-safety-projection.md) key-custody + safety-projection; a clinic that requires structured entry enforces it as soft UI policy ([ADR-0021](decisions/0021-layering-the-node-api-and-ui-pluralism.md)), never on the wire.
