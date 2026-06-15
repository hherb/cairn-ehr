# ADR-0013 — Attachments: content-addressed blobs, the lazy byte tier, and reference-eager replication

- **Status:** Accepted
- **Date:** 2026-06-15

## Context

Former open question §11.6 — *inline vs. content-addressed blob store with lazy sync* — is the sharper
of the two remaining original §11 items. **Attachments** are the binary clinical artifacts that are not
naturally Cairn-native JSONB bodies ([§3.5](../data-model.md#35-event-storage-model-hybrid-envelope)):
DICOM imaging (gigabyte-scale), scanned legacy paper (PDF/TIFF — the import case), clinical photography
(wounds, dermatology, retina), waveforms (ECG/EEG/fetal CTG), dictation audio (the AI-scribe source),
endoscopy/ultrasound video, externally-signed referral PDFs, genomic data.

The forces, sharpened by the offline-first / fractal / append-only commitments:

- **Size and bandwidth dominate everything.** A blob is orders of magnitude larger than an event body and
  can reach gigabytes. This is the entire reason it cannot live inline in the event: syncing the *reference*
  must never drag the *bytes* with it. The motivating real-world failure (Kimberley region, *Communicare*):
  a nightly bulk imaging sync ground the whole system to a halt, so that during emergencies **no** patient
  information could be retrieved at all — a blob-contention failure that became a clinical-**availability**
  failure, and that recurred even in the degenerate single-server / thin-client topology where no node
  replication was involved.
- **Opacity.** Attachments are binary and frequently uncoded; the [§3.13](../data-model.md#313-schema-evolution-event-format-and-the-legibility-twin)
  mandatory plaintext twin cannot be "mechanically derived" from pixels the way it is from a JSONB body.
- **Immutability *and* erasure, at once.** A blob is the most immutable content there is (a CT's bytes never
  change) yet also the highest-stigma, highest-volume content to *erase* (wound photos, scanned psychiatric
  or abuse records). Both append-only ([§3.1](../data-model.md#31-append-only-clinical-event-log-source-of-truth))
  and crypto-shredding ([§3.8](../data-model.md#38-erasure-and-key-custody)) must hold for them.
- **The envelope is the least-reversible thing in Cairn.** As with §11.4, part of the answer constrains the
  signed event and is therefore **can't-retrofit** — it must be reserved before the first production event,
  exactly as `t_effective` ([§3.6](../data-model.md#36-bitemporal-event-time-recording-time-vs-effective-time))
  and the encryption-capable body slot ([§3.5](../data-model.md#35-event-storage-model-hybrid-envelope)) were.

## Decision

Attachments are **the founding principles applied to large binary content.** They reuse existing primitives
almost entirely; the single genuinely new commitment is a day-one envelope reserve, and the single new
concept is a third degradation axis. Canonical homes: the attachment-reference shape and rendition set
[data-model §3.14](../data-model.md#314-attachments-content-addressed-blobs-and-the-rendition-set); the lazy
byte tier and reference-eager replication [sync §6.6](../sync.md#66-attachments-the-lazy-byte-tier); erasure
inherits [§3.8](../data-model.md#38-erasure-and-key-custody) / [§7.1](../security.md#71-erasure-the-severity-ladder);
lossless passthrough and the legibility ladder inherit [§3.13](../data-model.md#313-schema-evolution-event-format-and-the-legibility-twin).

1. **Content-addressed by reference — principle 1 applied to binaries.** A blob is named by a
   **self-describing content digest**; the hash *is* its identity, exactly as the signature is the event
   body's. Same bytes → same address → idempotent set-union with zero merge, so two nodes that independently
   receive the same scan converge by construction. The blob needs **no separate signature**: the event body
   names it *by digest* and the event signature covers that digest, chaining integrity from the signed event
   into the bytes. Tampering is detectable, and a blob **self-verifies against any source**.

2. **Reference-eager, byte-lazy.** The attachment *reference* is part of the signed clinical event and
   replicates on the **eager** sync plane; the *bytes* live in a **lazy** by-reference tier
   ([sync §6.6](../sync.md#66-attachments-the-lazy-byte-tier)). A node therefore **always knows an attachment
   exists** before its bytes arrive, and can demand them on legitimate need
   ([§6.4](../sync.md#64-scope-is-a-prefetch-hint-not-an-authority)) — foreground, overriding the background
   *"attachments last"* priority. Paper-parity is *exceeded*: a not-yet-retrieved blob shows through
   honest-assembly ([§6.2](../sync.md#62-consistency-model)) as *"referenced here — not yet retrieved"*,
   where paper leaves a missing film invisibly absent.

3. **The lazy tier never starves clinical sync (the availability floor).** Priority *ordering* is necessary
   but insufficient — an in-flight gigabyte still head-of-line-blocks the channel, which *is* the Communicare
   failure. Blob transfer is therefore **chunked, preemptible, and on a separate transfer budget**, so
   clinical events always interleave *between* chunks. **Blob transfer must never reduce clinical-data
   availability** — availability-over-consistency ([principle 5](../index.md#founding-principles-the-lens-for-every-decision))
   and paper-parity ([principle 3](../index.md#founding-principles-the-lens-for-every-decision)) applied to
   the transport itself. The floor holds even in the degenerate single-server / thin-client topology.

4. **Byte-replication is opt-in and separately scoped.** [§6.4](../sync.md#64-scope-is-a-prefetch-hint-not-an-authority)'s
   prefetch-hint applied to the byte tier — but the **blob prefetch predicate is a separate, much narrower
   thing** than the event-scope predicate. **References replicate everywhere; bytes replicate by election.**
   A resource-starved node defaults to **references-only, zero bytes prefetched, fetch-on-demand** from a
   holder; *"store all PACS blobs"* is merely an over-broad blob predicate, and the default is its opposite.
   Because a blob self-verifies (point 1), on-demand fetch is a **multi-source, chunked, resumable,
   content-verified swarm pull** from any holder — sibling on the LAN, parent, or the device carried with the
   patient — with **zero trust in the source** (trust is in the digest named by the signed event).
   *"Carry the film with the patient"* becomes byte-exact and partition-tolerant (the
   [§6.1](../sync.md#61-mechanism) sneakernet, generalized to binaries).

5. **One day-one envelope reserve — the attachment-reference shape.** Locked at write time and covered by
   the event signature, expressing from day one: (a) a **self-describing content digest** (algorithm + value,
   multihash-style — because the hash function *will* eventually break, and a future algorithm is an
   **additive** migration ([§3.13](../data-model.md#313-schema-evolution-event-format-and-the-legibility-twin))
   while the original event's digest stays fixed under its original algorithm); (b) a **seal indicator /
   DEK-wrap reference** (without it, an attachment could never be crypto-shredded later — back to
   *"can't delete from an append-only log"*); (c) **clear-text descriptor metadata** (media type, byte
   length, modality / human descriptor) so a sealed *or* pending *or* unparseable blob still renders down the
   ladder and feeds the safety projection; (d) a **rendition set** (point 6); (e) a **small-blob inline path**
   — below a node-tuned threshold a tiny blob (a signature glyph) is embedded in the body and rides the eager
   plane, above it the blob is referenced; both forms expressible from day one. Everything else — store
   layout, dedup, GC, transfer protocol — is mutable infrastructure.

6. **The rendition set is the legibility twin for binaries.** One logical attachment is **N content-addressed
   renditions** (raw gigabytes + kilobyte preview + extracted report text), each with its own sync priority:
   the lightweight rendition rides along, the raw is fetched on demand. This resolves the apparent
   [§3.13](../data-model.md#313-schema-evolution-event-format-and-the-legibility-twin) tension — *you do not
   twin the pixels.* The mandatory plaintext twin is derived from the event's coded/descriptor fields
   (*"Chest CT with contrast, 2026-06-15, reported: no PE"*), and the lightweight rendition is the blob's
   twin — the same unification §3.13 already made between the legibility and confidentiality ladders.

7. **A third degradation axis — retrievability.** §3.13's effective rendering `min(parseable, cleared)` gains
   a retrievability axis: a blob is **present / pending / shredded**. So
   **effective rendering = `min(retrievable, parseable, cleared)`**. A pending CT, a sealed CT, and an
   unparseable-format CT all degrade down the *same* ladder to the same honest floor (*"imaging attachment,
   type X, 412 MB, not available on this node"*). **Coarseness varies; existence never disappears** — the
   [§5.9](../identity.md#59-sensitivity-grade-the-safety-projection-and-break-glass-visibility-scope) / §3.13
   safety-floor invariant, generalized once more.

8. **Erasure and lossless passthrough inherit, unchanged.** A blob is **encryption-capable by construction**,
   mirroring the §3.5 body slot: *plaintext* (content-addressed by plaintext hash, dedup within a trust
   domain, whole-storage-encrypted at rest) *or* **sealed under a per-blob DEK** (content-addressed by
   ciphertext hash, crypto-shreddable). The [§3.8](../data-model.md#38-erasure-and-key-custody) /
   [§7.1](../security.md#71-erasure-the-severity-ladder) erasure ladder applies with **no new mechanism**, and
   GC ≠ erasure (a garbage-collected blob is re-fetchable; a crypto-shredded one is keyless noise). **No
   convergent encryption for sealed blobs** — deriving the key from the plaintext would leak *"someone holds
   this exact file"* (a confirmation attack); confidentiality outranks dedup. And
   [§3.13](../data-model.md#313-schema-evolution-event-format-and-the-legibility-twin) lossless-passthrough
   applies to bytes: a blob is **never transcoded in place** (re-encoding breaks embedded signatures and
   changes the hash); a derived preview is a *new* rendition *added*, never a replacement. Original bytes are
   custody-immortal.

9. **DICOM / WADO / IHE-XDS is a façade, never the storage model** — the §3.4 FHIR posture applied to imaging
   and document exchange. Cairn stores content-addressed (optionally sealed) blobs + signed event references,
   and *exports* the standard on demand at the boundary; the standard never dictates the store.

10. **No new founding principle.** Content-addressing is principle 1 applied to binaries; the lazy tier is
    availability + paper-parity applied to the transport; seal/shred is §3.8; the rendition-twin and the
    retrievability axis are principle 11. Same trajectory as §11.8 / §11.9 / §11.10.

11. **Blast radius ([§9](../language-substrate.md)).** **Safety-critical** (in-DB/Rust): the digest binding in
    the signed event, the seal / DEK-wrap and crypto-shred of blob keys, and the **content-verification on
    fetch** (a wrong-hash blob must never be served as the named one). **Fit-for-purpose:** the blob-store
    layout, transfer protocol, dedup, GC, rendition derivation, and every viewer. The one safety-critical
    **seam** is the fetch-verify path (bytes-in → hash-check → serve) — the content-addressing analogue of the
    [§3.13](../data-model.md#313-schema-evolution-event-format-and-the-legibility-twin) write-time twin seam.

## Consequences

- **Easier:** text sync is never blocked by imaging (the Communicare failure designed out); a resource-starved
  node holds *references* for the whole record but *bytes* only for what it elects, fetching the rest on
  demand; content-addressing gives swarm fetch, resumable transfer, and sneakernet for free, with no trust in
  the source; erasure, lossless passthrough, and the legibility ladder are all inherited rather than rebuilt;
  and the rendition set yields the §3.13 plaintext twin for binaries at no extra conceptual cost.
- **Harder / new surface:** a content-addressed blob store, a transfer protocol with genuine **resource
  isolation** (chunked, preemptible, separately budgeted — not just a priority queue), the blob prefetch
  predicate as a *separate* piece of configuration, refcount-based GC distinct from erasure, and
  **algorithm-agility** on the digest (a future hash is additive, the old one immortal).
- **The bet:** that reference-eager / byte-lazy transport with resource isolation keeps clinical data
  available under *any* blob load, and that the day-one reference shape (point 5) is complete enough never to
  need an envelope change. We would know it is wrong if a real attachment workflow needs a reference field we
  did not reserve, or if content-verification on fetch proves too costly for large blobs on Pi-class hardware
  (mitigation: chunk-level Merkle verification, already implied by chunked transfer).
- **Policy-neutral ([principle 9](../index.md#founding-principles-the-lens-for-every-decision)):** Cairn ships
  the content-addressed store, the lazy tier, the prefetch mechanism, and the seal/shred capability; *which*
  blobs a node prefetches versus fetches on demand, retention/GC thresholds, the inline threshold, and whether
  a deployment runs per-blob sealing at all, are policy.
