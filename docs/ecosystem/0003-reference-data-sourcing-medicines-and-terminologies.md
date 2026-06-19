# Ecosystem evaluation — external reference-data sourcing (medicines, and disease/injury terminologies)

**Date:** 2026-06-19
**Status:** Evaluation, **in progress**. Spec unchanged; no ADR minted. Captures sourcing research for the
**reference-data service tier** — a *separable* service consumed by a Cairn node, not part of the wire core.
Medicines sourcing (§1–§7) is complete; disease/injury concept identifiers (§8) is the open companion thread.
**Subjects:** open/government drug-reference feeds (WHO INN, RxNorm, DailyMed/openFDA, TGA ARTG, PBS) and
disease/injury classifications (ICD-10/11, ICPC, alternatives) — evaluated for **license compatibility with a
freely-redistributable AGPL-3.0 node**.

> [!NOTE]
> This is an **ecosystem evaluation** — not architecture and not a decision. It records *what reference-data
> sources exist, on what licence, and what we concluded about fit*. The reference-data service is an **optional
> external tier**: a bare Cairn node records medication as `INN-anchored substance UUID + dose + amount + units +
> formulation enum` and disease as a stable concept code, and is a complete EHR **without** any bundled
> commercial drug or terminology database. Nothing here sits on the inter-node path, so a licence-encumbered
> source can never contaminate interoperability or the safety floor — it simply doesn't attach. That posture is
> exactly what founding principle 12 (*uniform core, plural edges*) and the language-substrate rule
> ([§9](../spec/language-substrate.md)) require of an advisory, fit-for-purpose tier.

---

## 1. Framing — why the architecture makes this tractable

The decisive design choice (the user's, ratified in conversation 2026-06-19) is that **the EHR does not
foreign-key into an integrated proprietary product database.** A medication is recorded as:

- a **substance identity** anchored on the **WHO INN** (International Nonproprietary Name) as the stable concept
  UUID — *not* a free-text brand name that is spelled differently across sources and drifts over time;
- plus **dose, amount, units**, and a **formulation enum** (tablet / ointment / solution / …) stored explicitly.

This matters because **the proprietary parts of the drug-data world are precisely the integrated, value-added
product databases** (First Databank, Micromedex, Multum, AMH, DrugBank-commercial). By never depending on one
for core clinical recording, Cairn needs only *reference and decision-support layers*, and those can be
assembled from public-domain government feeds. The reference-data service is therefore a **two-tier** problem:

- **Tier 1 — generic / substance-level**, jurisdiction-independent (the INN-anchored substance dictionary, plus
  classification and decision-support reference).
- **Tier 2 — packaging / product / country-specific**, starting with Australia.

The single hardest gap — unchanged in 20 years since the user's drugref.org work — is **drug–drug interactions**
(§4). The mission tie-breaker throughout: **license-and-redistribution terms win over convenience.** A source
that is free-to-*use* but `NonCommercial` or `NoDerivatives` is **not** freely redistributable and cannot be
bundled into the AGPL artifact; at best it is an operator-supplied, node-local, separately-licensed plug-in.

> [!IMPORTANT]
> **The recurring trap is the licence, not the price.** Four flavours of "free" are *not* AGPL-bundleable:
> `NonCommercial` (CC BY-NC — DrugBank, WHO EML), `NoDerivatives`, member/affiliate-gated (SNOMED CT and all its
> national extensions incl. AMT), and registration-walled-but-otherwise-open (UK dm+d via TRUD). Each is flagged
> 🚩 below.

---

## 2. Tier 1 — generic / substance-level (jurisdiction-independent)

| Source | Body | Licence | AGPL-bundleable? | Verdict |
|---|---|---|---|---|
| **WHO INN** | WHO INN Programme | **Public domain** ("placed in the public domain … used without any restriction whatsoever") | ✅ Yes | **✅ Anchor.** Names are free; only friction is *access* — no clean bulk file (registration-gated INN Hub API, or parse the biannual public-domain PDF lists). An engineering problem, not a legal one. Biannual updates. |
| **RxNorm — Current Prescribable Content** | US NLM | **Public domain** (only `SAB=RXNORM` + `SAB=MTHSPL`) | ✅ Yes | **✅ Primary normalization layer.** Downloadable **without** a UMLS login. Ingredient / clinical-drug / dose-form / strength. |
| RxNorm — *full* release / RxNav-in-a-Box | US NLM | Public-domain core **+ UMLS-restricted SABs** (First DataBank, Micromedex, Multum, VA) | 🚩 **No** | ⚠️ **Avoid for redistribution.** The offline-friendly RxNav-in-a-Box is UMLS-gated. Build your own offline store from the prescribable subset instead. |
| **DailyMed / SPL** | US NLM / FDA | **Public domain** | ✅ Yes | **✅** Active ingredient, strength, form, route, NDC; LOINC-coded label sections. Also the DDI-mining substrate (§4). |
| **openFDA** | US FDA | **CC0 1.0** | ✅ Yes | **✅** NDC directory, drug labels, FAERS. Narrow caveat: a few privately-submitted copyrightable items are marked. |
| **Drugs@FDA** | US FDA | Public domain | ✅ Yes | ✅ Approved products, ingredients, strengths. |
| **PubChem** | NCBI | Public domain | ✅ Yes | ✅ Chemistry layer (structures, InChI, identifiers). Per-depositor caveat for sub-collections. |
| **ChEMBL** | EMBL-EBI | **CC BY-SA 3.0** | ⚠️ **Caveat** | Usable, but **ShareAlike is copyleft** — keep it an isolated, attributed data layer; don't mix into a combined work. Bioactivity-oriented; low priority for prescribing. |
| **WHO ATC/DDD** | WHO CC, Oslo (atcddd.fhi.no) | NC **+ no-derivatives** + attribution | 🚩 **No** | 🚩 **€200 fee + forbids commercial redistribution + forbids modification.** Operator-supplied / separately-licensed only. Community scraper `fabkury/atcd` outputs CC BY-NC-SA, confirming it is not free. |
| **WHO Essential Medicines List** | WHO | **CC BY-NC-SA 3.0 IGO** | 🚩 **No** (NC) | Free to *access* (eEML export at list.essentialmeds.org), but NC blocks bundling. Factual "on-list y/n" flag *may* be reconstructable from non-copyrightable facts — needs legal review. |
| **DrugBank (full)** | OMx / U. Alberta | **CC BY-NC 4.0** / paid commercial | 🚩 **No** | 🚩 **NonCommercial — excluded.** *Only* the tiny **DrugBank Open Data** ID-mapping subset (CC0) is usable. |
| **KEGG DRUG** | Kanehisa Labs | Academic-only / paid commercial | 🚩 **No** | 🚩 Excluded. |

---

## 3. Tier 2 — packaging / product (Australia first)

| Source | Body | Licence | AGPL-bundleable? | Role |
|---|---|---|---|---|
| **PBS API / CSV** | Dept. of Health | **CC BY 3.0 AU** | ✅ Yes | **✅ Primary packaging backbone** — item code, form & strength, pack size/quantity, manner of administration, restrictions, ATC linkage, pricing. See §3.1 for the 2026 format cutover. |
| **TGA ARTG** | TGA | **CC BY** (3.0 AU → 4.0 on newer releases) | ✅ Yes | **✅ Primary product registry** — sponsor, trade name, ARTG ID, active ingredients, dosage form, status. |
| **AMT (in SNOMED CT-AU)** | ADHA / NCTS | SNOMED National/Affiliate licence | 🚩 **No** | ⚠️ **Free to use *inside* Australia (free registration), NOT internationally redistributable.** SNOMED affiliate licensing charges fees in non-member territories and forbids sub-licensee redistribution. Treat as a **per-node, per-jurisdiction-licensed plug-in** fetched over the distribution plane — never bundled. Maps cleanly onto the [ADR-0014](../spec/decisions/0014-locale-pluggable-matcher-comparators.md) content-addressed-component posture. |
| NPS MedicineWise | — | — | — | ☠️ **Defunct** (ceased 31 Dec 2022). Ignore. |
| AMH; Therapeutic Guidelines (eTG) | proprietary | All rights reserved | 🚩 **No** | 🚩 Paid/proprietary subscription products. Exclude. |

### 3.1 PBS feed — viable, but the format just shifted (time-sensitive)

The user fed drugref from PBS data in EasyGP; the old pain was a PBS schedule format that "kept shifting like
quicksand," eventually settling on a mostly-standards-compliant XML that nearly always had easy-to-fix parsing
errors. **As of 2026 that avenue is still viable but the XML is now retired** — the format shifted one final time:

- **PBS XML and PBS Text files: discontinued 1 May 2026.** Gone. Any ingest pointed at the XML broke this May.
- **PBS Offline: discontinued 1 March 2026.** Vendor Schedule distribution ceased October 2024.
- **Replacement: PBS API v3 + monthly "PBS API CSV files"** (every API endpoint/table exported as CSV,
  published monthly). The CSV bundle is the **direct modern equivalent of the old bulk XML dump**, and the
  structured CSV/JSON should largely end the parsing-error pain.

Practical specifics for rebuilding the ingest:

- **Licence: CC BY 3.0 Australia** (confirmed on the data.gov.au PBS datasets — Item Report, ATC Report,
  Patient Category Report). Redistributable, commercial OK, attribution only — **AGPL-compatible.** ✅
- **Public API is free, no login** (default subscription key) but **brutally rate-limited: 1 request / 20 s,
  shared across *all* users globally.** Per-item API enumeration of the full schedule is therefore a non-starter
  — **use the monthly CSV bundle** for bulk load; the dept's own developer docs recommend weekly snapshots.
- **Free registration** via the PBS Data API Portal (`data-api-portal.health.gov.au`) yields a subscription
  key, higher limits, and — important for a dispensing system — **embargo access to *future* schedules**, so
  next month's data can be staged before it goes live. The public API exposes only current + trailing 12 months.
- **~14 tables**, base URL `https://data-api.health.gov.au/pbs/api/v3`: `items`, `item-overview`, `prescribers`,
  `schedules`, `atc-codes`, `organisations`, `restrictions`, `criteria`, `copayments`, `fees`, `programs`,
  `summary-of-changes`, … The **`summary-of-changes`** endpoint is new vs. the EasyGP era — it gives the monthly
  delta directly, so full-dump diffing is no longer required.

**Conclusion:** PBS + TGA ARTG (both CC BY) give the entire Australian product/packaging layer with no licence
fees and full redistribution rights — the EasyGP→drugref pattern, minus the brittle XML parser. Because the
format *just* shifted again, the ingest should treat the PBS schema as a **versioned external dependency**
(founding principle 11, *legibility across time*, applied to an upstream feed), not as a fixed column set.

---

## 4. The drug–drug interaction gap (still the hard part)

**No comprehensive, clinically-validated, openly-redistributable, AGPL-compatible DDI database exists.** Every
clinical-grade source (Stockley's, Lexicomp, Micromedex, First Databank, DrugBank) is paywalled. The defensible
open composite, in descending licence-safety:

1. **ONC high-priority DDI list** — expert-consensus set (Phansalkar *et al.*, *JAMIA* 2012; + CredibleMeds QT
   drugs) in open-access literature. **Clean and authoritative but minimal — a high-severity safety floor, not a
   full checker.** Note: NLM's RxNav Interaction API that served it was **permanently retired 2 Jan 2024**;
   re-encode from the papers.
2. **Mine DailyMed SPL interaction sections** (public domain) with NLP. **ONSIDES** (Tatonetti lab) is
   **MIT-licensed** and proves the pattern for adverse events — reuse as precedent. Engineering- and
   clinical-validation-heavy, but the strongest from-scratch open foundation.
3. **DDInter 2.0** (Central South University) — ~302k DDI records *with severity + mechanism + management*. The
   single best candidate for structured open DDI — **but its exact download-page licence needs a human (browser)
   confirm** (aggregators tag it CC0; that may describe metadata only; the site 403s automated fetch).
4. **TWOSIDES / OFFSIDES, Hetionet** — research-only FAERS signals; ambiguous/likely-NC; **never primary
   alerting.**

The honest conclusion: **the curation is the moat, and it is the part no one gives away.** This is exactly where
Cairn could revive the spirit of the hand-curated drugref DDI set — but as an institutionally-owned,
**append-only overlay** (principle 1), not a volunteer wiki (see §5).

---

## 5. Sustainability — why drugref.org soft-died, and the lesson

drugref.org (the user's ~20-year-old wiki, seeded by the Mercy Ships foundation, with a hand-curated DDI
database) soft-died from **structural volunteer attrition + grant-cycle mismatch** (funders pay for novelty,
never maintenance). **But it survived in Canada because OSCAR's `drugref2` replaced volunteer data-authoring
with an automated feed off Health Canada's open Drug Product Database (DPD).** That is the template:

> **Don't crowd-source the clinical facts. Aggregate them from institutionally-funded government feeds, and
> reserve scarce human curation for the thin, high-value layer machines can't supply (DDI severity, clinical
> judgment).**

Sustainable funding models, most-durable first: **(1) government public-good feeds** (RxNorm, openFDA, DPD,
dm+d, PBS, TGA — standing budgets, not grants); **(2) consortium/member dues** (SNOMED — durable funding,
*conditional* openness); **(3) foundation/nonprofit subscription** (TAIR → Phoenix Bioinformatics became
self-sustaining in a year); **(4) commercial-services-around-open-core.** Avoid CIEL-style single-maintainer
key-person dependency.

---

## 6. Recommended sourcing stack

- **Substance anchor (Tier 1, global):** **WHO INN** (public domain) as the UUID, **+ RxNorm Current
  Prescribable Content** (public domain) for normalization/clinical-drug/dose-form, **+ DailyMed/SPL + openFDA**
  (public domain) for ingredients/labels. All four bundle freely under AGPL.
- **Australian product/packaging (Tier 2):** **PBS API/CSV + TGA ARTG** (both CC BY) as the freely-shippable
  layer. **AMT/SNOMED CT-AU** as a per-node NCTS-licensed plug-in, never bundled.
- **Other jurisdictions (same shape):** open regulatory registry bundled (US FDA/NDC, Canada DPD, UK dm+d via
  free TRUD account); national SNOMED extension as a licensed plug-in.
- **DDI:** ONC high-priority floor (re-encoded) → SPL-mined layer (ONSIDES-style, MIT) → DDInter *if licence
  confirms* → Cairn's own curated append-only overlay as the durable value-add.
- **Hard excludes:** WHO ATC/DDD, WHO EML (NC), DrugBank-full, KEGG, AMH, eTG.

---

## 7. Verify-before-relying (could not auto-fetch; all 403'd)

1. **DDInter** exact download-page licence (the linchpin for structured open DDI).
2. **Canada DPD** and **EU Article 57** per-dataset licence *tags* (expected OGL-Canada / CC-BY; confirm).
3. Literal **CC BY version** on current TGA/PBS downloads and the **SNOMED CT-AU National Licence**
   redistribution clause text from NCTS.

---

## 8. Disease & injury concept identifiers — companion thread (in progress)

*Same requirement as medicines: solid, stable concept identifiers, never drifting free-text names, on the
decision-making pathway. Research underway (ICD-10/11, ICPC family, open alternatives). To be written up here.*

---

## Sources

**Tier 1:** WHO INN (who.int/teams/health-product-and-policy-standards/inn) · RxNorm Current Prescribable
Content (nlm.nih.gov/research/umls/rxnorm/docs/prescribe.html) + Terms of Service + UMLS License Agreement ·
DailyMed SPL Resources · openFDA License (open.fda.gov/license) · ChEMBL licensing (chembl.github.io) · WHO
ATC/DDD copyright (atcddd.fhi.no/copyright_disclaimer/) · WHO EML (list.essentialmeds.org) · DrugBank Terms of
Use (go.drugbank.com/legal/terms_of_use) · KEGG Legal (kegg.jp/kegg/legal.html).
**Tier 2 / PBS:** PBS New API & API CSV files news (pbs.gov.au/info/news/2024/12/...) · PBS Download
(pbs.gov.au/info/browse/download) · data.pbs.gov.au documents 91327/91602/90834 · Accessing PBS embargo data
(hpp.health.gov.au) · data.gov.au PBS Item Report (CC BY 3.0 AU) · TGA datasets (tga.gov.au/resources/datasets)
· SNOMED licensing (snomed.org/get-snomed, snomed.org/licensing).
**DDI:** Phansalkar 2012 (PMC3422823) · NLM RxNav Interaction API retirement · ONSIDES (github.com/tatonetti-lab/onsides,
MIT) · DDInter 2.0 (NAR 2025) · nsides.io.
**Sustainability:** OSCAR drugref2 (oscaremr.atlassian.net) · Health Canada DPD (open.canada.ca) · TAIR/Phoenix
(PMC4795935) · OpenMRS CIEL.
