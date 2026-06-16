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
| Phone, address | Volatile | Recency (HLC) wins; history retained | Nearly meaningless |
| Deceased status | Safety-asymmetric | Sets easily, never auto-clears; reversal = explicit human event | Strong evidence against link |
| Photo | Optional; powerful in low-ID settings | Append-only gallery, newest displayed | Human-reviewable evidence |

Notes:
- DOB precision is first-class ("age about 40, recorded 2026-06"). Default 01-01 birthdays are down-weighted by the matcher (overrepresented in low-resource registries).
- Conflicting "corrections" at equal provenance during a partition are **not** auto-resolved: project prior stable value, flag for human review. Rule: *recency resolves volatile fields; humans resolve identity-bearing fields.*
