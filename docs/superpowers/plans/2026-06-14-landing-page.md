# Cairn landing page — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a self-contained static landing page for `cairn-ehr.org`, deployed to Cloudflare Pages, in the warm paper + stone visual identity, that makes the case → presents the mission → recruits → routes to the spec.

**Architecture:** One `web/index.html` + one `web/styles.css`, no framework and no build step. Self-hosted woff2 fonts, an inline/SVG stone-mark logo, a sprinkle of vanilla JS only for the mobile nav. Cloudflare Pages serves the `web/` directory directly with no build command.

**Tech Stack:** Semantic HTML5, hand-written CSS (custom properties, `clamp()`, CSS grid), self-hosted Inter (sans) + Source Serif 4 (serif accent) from Fontsource (SIL OFL), SVG logo. Verification via a local `python -m http.server` and browser screenshots (Playwright MCP / preview tooling).

**Spec:** `docs/superpowers/specs/2026-06-14-landing-page-design.md` — the source of truth for palette, structure, and acceptance criteria. Copy is paraphrased from the root `README.md` and `docs/spec/index.md`; never contradict them.

**Conventions for the executor:**
- All paths are relative to the repo root.
- Commit after each task with the message shown.
- "Serve & screenshot" means: run `uv run python -m http.server 8799 --directory web` (background), open `http://127.0.0.1:8799/`, capture a screenshot with the Playwright MCP (`browser_navigate` then `browser_take_screenshot`) or the preview tool, then stop the server. Confirm visually against the spec before moving on.

---

## File structure

| File | Responsibility |
|---|---|
| `web/index.html` | The entire page: head/meta, all `<section>`s, footer, tiny nav script |
| `web/styles.css` | All styling: palette tokens, type scale, layout, components, responsive, a11y |
| `web/assets/logo.svg` | The three-stone mark (header + hero) |
| `web/assets/favicon.svg` | Favicon (stone mark, square crop) |
| `web/assets/og-card.svg` | Source for the 1200×630 social-share image |
| `web/assets/og-card.png` | Rasterized social card referenced by OG/Twitter meta |
| `web/assets/fonts/*.woff2` | Self-hosted Inter (400/500/700) + Source Serif 4 (400, 400-italic) |
| `web/robots.txt` | Allow indexing |
| `web/sitemap.xml` | Single-URL sitemap |
| `web/_headers` | Cloudflare Pages caching + security headers |
| `web/README.md` | How to deploy to Cloudflare Pages; how to preview locally |

---

## Task 1: Scaffold `web/` with palette tokens and a rendering skeleton

**Files:**
- Create: `web/index.html`
- Create: `web/styles.css`

- [ ] **Step 1: Create the HTML skeleton**

`web/index.html`:

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Cairn — offline-first, vendor-independent electronic health record</title>
  <meta name="description" content="Cairn — an offline-first, vendor-independent electronic health record. The grid goes down. The chart stays up.">
  <meta name="theme-color" content="#F4F1E9">
  <link rel="canonical" href="https://cairn-ehr.org/">
  <link rel="icon" href="/assets/favicon.svg" type="image/svg+xml">
  <link rel="stylesheet" href="/styles.css">
</head>
<body>
  <a class="skip-link" href="#main">Skip to content</a>
  <!-- header inserted in Task 4 -->
  <main id="main">
    <!-- sections inserted in Tasks 5–11 -->
    <p style="padding:4rem;text-align:center">scaffold</p>
  </main>
  <!-- footer inserted in Task 11 -->
</body>
</html>
```

- [ ] **Step 2: Create the stylesheet base with palette + reset + type scale**

`web/styles.css`:

```css
:root {
  --paper: #F4F1E9;
  --paper-alt: #EFEBE1;
  --ink: #262A2C;
  --ink-soft: #4A5358;
  --ink-faint: #7A827F;
  --navy: #1E3A52;
  --navy-soft: #DCE4EA;
  --teal: #2F6E62;
  --teal-light: #BFE0D5;
  --stone: #8B9398;
  --hairline: rgba(30, 58, 82, 0.12);

  --maxw: 1080px;
  --font-sans: "Inter", system-ui, -apple-system, "Segoe UI", sans-serif;
  --font-serif: "Source Serif 4", Georgia, serif;
}

*, *::before, *::after { box-sizing: border-box; }
html { scroll-behavior: smooth; }
body {
  margin: 0;
  background: var(--paper);
  color: var(--ink);
  font-family: var(--font-sans);
  font-size: 18px;
  line-height: 1.6;
  -webkit-font-smoothing: antialiased;
  text-rendering: optimizeLegibility;
}
img, svg { display: block; max-width: 100%; }
a { color: var(--teal); text-decoration: none; }
a:hover { text-decoration: underline; }

.skip-link {
  position: absolute; left: -9999px; top: 0;
  background: var(--navy); color: #fff; padding: 10px 16px; border-radius: 6px; z-index: 100;
}
.skip-link:focus { left: 12px; top: 12px; }

.wrap { max-width: var(--maxw); margin: 0 auto; padding: 0 24px; }

:focus-visible { outline: 3px solid var(--teal); outline-offset: 2px; }

@media (prefers-reduced-motion: reduce) {
  html { scroll-behavior: auto; }
  * { transition: none !important; animation: none !important; }
}
```

- [ ] **Step 3: Serve & screenshot**

Run: `uv run python -m http.server 8799 --directory web` (background), navigate to `http://127.0.0.1:8799/`, screenshot.
Expected: warm off-white page, the word "scaffold" centered, skip-link appears on Tab.

- [ ] **Step 4: Commit**

```bash
git add web/index.html web/styles.css
git commit -m "feat(web): scaffold landing page with palette tokens"
```

---

## Task 2: Create the stone-mark logo and favicon

**Files:**
- Create: `web/assets/logo.svg`
- Create: `web/assets/favicon.svg`

- [ ] **Step 1: Create the full stone mark**

`web/assets/logo.svg` (refine curve/etch details to match `assets/logo_abb1.png`; this is a faithful starting point):

```svg
<svg viewBox="0 0 120 140" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="Cairn logo">
  <ellipse cx="60" cy="116" rx="52" ry="20" fill="#1E3A52"/>
  <path d="M22 116 H98 M34 110 l8 -7 6 5 9 -10 8 7 7 -5" stroke="#5C7186" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round" opacity="0.55"/>
  <ellipse cx="60" cy="80" rx="42" ry="17" fill="#8B9398"/>
  <path d="M28 80 H92 M40 75 l7 4 6 -7 7 5 6 -4" stroke="#F0EDE4" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round" opacity="0.5"/>
  <path d="M20 52 C20 24 100 24 100 52 C100 70 80 78 60 78 C40 78 20 70 20 52 Z" fill="#2F6E62"/>
  <path d="M34 48 l9 -9 6 6 8 -11 7 9 7 -6" stroke="#BFE0D5" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round" opacity="0.6"/>
</svg>
```

- [ ] **Step 2: Create the favicon (square crop of the mark)**

`web/assets/favicon.svg`:

```svg
<svg viewBox="0 0 120 120" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="Cairn">
  <rect width="120" height="120" rx="22" fill="#F4F1E9"/>
  <ellipse cx="60" cy="98" rx="44" ry="16" fill="#1E3A52"/>
  <ellipse cx="60" cy="68" rx="35" ry="14" fill="#8B9398"/>
  <path d="M26 44 C26 20 94 20 94 44 C94 60 77 67 60 67 C43 67 26 60 26 44 Z" fill="#2F6E62"/>
</svg>
```

- [ ] **Step 3: Serve & screenshot**

Open `http://127.0.0.1:8799/assets/logo.svg` and `/assets/favicon.svg`.
Expected: three stacked stones (teal dome / grey / navy) with faint etched trend lines; favicon legible at small size.

- [ ] **Step 4: Commit**

```bash
git add web/assets/logo.svg web/assets/favicon.svg
git commit -m "feat(web): add stone-mark logo and favicon"
```

---

## Task 3: Self-host fonts

**Files:**
- Create: `web/assets/fonts/inter-400.woff2`, `inter-500.woff2`, `inter-700.woff2`
- Create: `web/assets/fonts/source-serif-4-400.woff2`, `source-serif-4-400-italic.woff2`
- Modify: `web/styles.css` (prepend `@font-face` blocks)

- [ ] **Step 1: Download the woff2 files from Fontsource (SIL OFL)**

```bash
mkdir -p web/assets/fonts
base="https://cdn.jsdelivr.net/fontsource/fonts"
curl -fsSL "$base/inter@latest/latin-400-normal.woff2" -o web/assets/fonts/inter-400.woff2
curl -fsSL "$base/inter@latest/latin-500-normal.woff2" -o web/assets/fonts/inter-500.woff2
curl -fsSL "$base/inter@latest/latin-700-normal.woff2" -o web/assets/fonts/inter-700.woff2
curl -fsSL "$base/source-serif-4@latest/latin-400-normal.woff2" -o web/assets/fonts/source-serif-4-400.woff2
curl -fsSL "$base/source-serif-4@latest/latin-400-italic.woff2" -o web/assets/fonts/source-serif-4-400-italic.woff2
ls -la web/assets/fonts
```
Expected: five non-empty `.woff2` files. If any URL 404s, list available files at `https://www.jsdelivr.com/package/npm/@fontsource/inter` and adjust the filename.

- [ ] **Step 2: Add `@font-face` declarations at the very top of `web/styles.css`**

Prepend (before `:root`):

```css
@font-face { font-family: "Inter"; font-style: normal; font-weight: 400; font-display: swap; src: url("/assets/fonts/inter-400.woff2") format("woff2"); }
@font-face { font-family: "Inter"; font-style: normal; font-weight: 500; font-display: swap; src: url("/assets/fonts/inter-500.woff2") format("woff2"); }
@font-face { font-family: "Inter"; font-style: normal; font-weight: 700; font-display: swap; src: url("/assets/fonts/inter-700.woff2") format("woff2"); }
@font-face { font-family: "Source Serif 4"; font-style: normal; font-weight: 400; font-display: swap; src: url("/assets/fonts/source-serif-4-400.woff2") format("woff2"); }
@font-face { font-family: "Source Serif 4"; font-style: italic; font-weight: 400; font-display: swap; src: url("/assets/fonts/source-serif-4-400-italic.woff2") format("woff2"); }
```

- [ ] **Step 3: Serve & screenshot**

Reload `http://127.0.0.1:8799/`. In the browser network panel (Playwright `browser_network_requests`), confirm font requests are served from `127.0.0.1` only — no requests to `fonts.googleapis.com` or any third party.
Expected: "scaffold" text renders in Inter; zero third-party requests.

- [ ] **Step 4: Commit**

```bash
git add web/assets/fonts web/styles.css
git commit -m "feat(web): self-host Inter and Source Serif 4"
```

---

## Task 4: Header / nav

**Files:**
- Modify: `web/index.html` (replace the header comment)
- Modify: `web/styles.css` (append header styles + nav script lives in HTML)

- [ ] **Step 1: Insert the header markup** (replace `<!-- header inserted in Task 4 -->`)

```html
<header class="site-header">
  <div class="wrap header-inner">
    <a class="brand" href="/" aria-label="Cairn home">
      <img class="brand-mark" src="/assets/logo.svg" alt="" width="28" height="33">
      <span class="brand-word">CAIRN</span>
    </a>
    <button class="nav-toggle" aria-expanded="false" aria-controls="nav" aria-label="Menu">☰</button>
    <nav id="nav" class="site-nav" aria-label="Primary">
      <a href="#mission">Mission</a>
      <a href="#architecture">Architecture</a>
      <a href="#principles">Principles</a>
      <a href="https://docs.cairn-ehr.org">Docs</a>
      <a class="nav-cta" href="https://github.com/cairn-ehr/cairn">GitHub</a>
    </nav>
  </div>
</header>
```

> Note: confirm the real GitHub URL before launch; `https://github.com/cairn-ehr/cairn` is the assumed slug.

- [ ] **Step 2: Add the nav toggle script** before `</body>`:

```html
<script>
  const t = document.querySelector('.nav-toggle');
  const n = document.getElementById('nav');
  t.addEventListener('click', () => {
    const open = n.classList.toggle('open');
    t.setAttribute('aria-expanded', String(open));
  });
</script>
```

- [ ] **Step 3: Append header styles to `web/styles.css`**

```css
.site-header { position: sticky; top: 0; background: var(--paper); border-bottom: 1px solid var(--hairline); z-index: 50; }
.header-inner { display: flex; align-items: center; justify-content: space-between; height: 64px; }
.brand { display: flex; align-items: center; gap: 10px; color: var(--ink); }
.brand:hover { text-decoration: none; }
.brand-word { font-weight: 700; letter-spacing: 2px; font-size: 18px; }
.site-nav { display: flex; align-items: center; gap: 24px; }
.site-nav a { color: var(--ink-soft); font-size: 15px; }
.nav-cta { border: 1px solid rgba(30,58,82,0.3); border-radius: 6px; padding: 7px 13px; color: var(--navy) !important; }
.nav-toggle { display: none; background: none; border: 0; font-size: 22px; color: var(--ink); cursor: pointer; }

@media (max-width: 720px) {
  .nav-toggle { display: block; }
  .site-nav { display: none; position: absolute; top: 64px; left: 0; right: 0; flex-direction: column; gap: 0; background: var(--paper); border-bottom: 1px solid var(--hairline); padding: 8px 24px 16px; }
  .site-nav.open { display: flex; }
  .site-nav a { padding: 10px 0; }
}
```

- [ ] **Step 4: Serve & screenshot at desktop (1200px) and mobile (375px)**

Use `browser_resize` to 1200 then 375. Expected: desktop shows inline nav with outlined GitHub; mobile shows ☰ that toggles a stacked menu. Verify `aria-expanded` flips.

- [ ] **Step 5: Commit**

```bash
git add web/index.html web/styles.css
git commit -m "feat(web): header with responsive nav"
```

---

## Task 5: Hero

**Files:**
- Modify: `web/index.html` (add hero section as first child of `<main>`, remove the scaffold `<p>`)
- Modify: `web/styles.css`

- [ ] **Step 1: Insert hero markup** (replace the scaffold `<p>`)

```html
<section class="hero">
  <div class="wrap hero-inner">
    <img class="hero-mark" src="/assets/logo.svg" alt="" width="96" height="112">
    <h1>The grid goes down.<br>The chart stays up.</h1>
    <p class="lede">An offline-first, vendor-independent electronic health record. It keeps working through any outage, runs anywhere from a Raspberry&nbsp;Pi to a hospital cluster, and belongs to no vendor.</p>
    <div class="cta-row">
      <a class="btn btn-primary" href="https://docs.cairn-ehr.org">Read the specification</a>
      <a class="btn btn-secondary" href="https://github.com/cairn-ehr/cairn">View on GitHub</a>
    </div>
    <p class="status-line">Architecture &amp; specification phase &nbsp;·&nbsp; AGPL-3.0 &nbsp;·&nbsp; PostgreSQL&nbsp;≥&nbsp;18</p>
  </div>
</section>
```

- [ ] **Step 2: Append hero + shared button styles**

```css
.btn { display: inline-block; border-radius: 7px; padding: 12px 22px; font-size: 16px; font-weight: 500; }
.btn:hover { text-decoration: none; }
.btn-primary { background: var(--navy); color: #fff; }
.btn-primary:hover { background: #16314a; }
.btn-secondary { border: 1px solid rgba(30,58,82,0.35); color: var(--navy); }
.btn-secondary:hover { background: rgba(30,58,82,0.06); }

.hero { padding: clamp(48px, 9vw, 104px) 0; }
.hero-inner { text-align: center; }
.hero-mark { margin: 0 auto clamp(20px, 3vw, 32px); }
.hero h1 { font-size: clamp(34px, 6vw, 60px); line-height: 1.1; font-weight: 700; margin: 0 0 20px; letter-spacing: -0.5px; }
.lede { font-size: clamp(17px, 2.2vw, 21px); color: var(--ink-soft); max-width: 620px; margin: 0 auto 28px; }
.cta-row { display: flex; gap: 14px; justify-content: center; flex-wrap: wrap; margin-bottom: 22px; }
.status-line { font-size: 14px; color: var(--ink-faint); letter-spacing: 0.3px; }
```

- [ ] **Step 3: Serve & screenshot at 1200px and 375px**

Expected: enlarged mark, two-line headline, lede, two buttons (navy primary + outlined secondary), faint status line. Buttons wrap cleanly on mobile.

- [ ] **Step 4: Commit**

```bash
git add web/index.html web/styles.css
git commit -m "feat(web): hero section"
```

---

## Task 6: "Why this exists" navy band

**Files:**
- Modify: `web/index.html` (after hero)
- Modify: `web/styles.css`

- [ ] **Step 1: Insert markup**

```html
<section class="band-why" aria-labelledby="why-h">
  <div class="wrap">
    <p class="eyebrow eyebrow-light" id="why-h">Why this exists</p>
    <p class="why-statement">There is no vendor in the room. Nothing here is incentivised to keep the hard problems hard — so one thing drives every decision: <em>what actually happens at the point of care, including at 3&nbsp;a.m. when the network is down.</em></p>
  </div>
</section>
```

- [ ] **Step 2: Append styles**

```css
.eyebrow { font-size: 13px; letter-spacing: 2px; text-transform: uppercase; color: var(--teal); margin: 0 0 14px; }
.eyebrow-light { color: #7FA3BE; }
.band-why { background: var(--navy); color: var(--navy-soft); padding: clamp(40px, 7vw, 72px) 0; text-align: center; }
.why-statement { font-family: var(--font-serif); font-size: clamp(20px, 3vw, 28px); line-height: 1.5; max-width: 760px; margin: 0 auto; color: #EAF0F4; }
.why-statement em { font-style: normal; color: var(--teal-light); }
```

- [ ] **Step 3: Serve & screenshot**

Expected: full-bleed navy band, serif statement, the 3 a.m. clause highlighted in teal-light. Verify contrast (light text on navy passes AA — see Task 12).

- [ ] **Step 4: Commit**

```bash
git add web/index.html web/styles.css
git commit -m "feat(web): why-this-exists navy band"
```

---

## Task 7: The mission (four cards)

**Files:**
- Modify: `web/index.html` (after the navy band; this section carries `id="mission"`)
- Modify: `web/styles.css`

- [ ] **Step 1: Insert markup**

```html
<section class="section" id="mission" aria-labelledby="mission-h">
  <div class="wrap">
    <p class="eyebrow center" id="mission-h">The mission</p>
    <div class="card-grid">
      <article class="card">
        <h3>Keeps working through any outage</h3>
        <p>Read and write continues during a network partition; synchronization catches up when connectivity returns.</p>
      </article>
      <article class="card">
        <h3>Runs anywhere, for anyone</h3>
        <p>One codebase from a solar-powered clinic to a national network — scaled by configuration, not forks.</p>
      </article>
      <article class="card">
        <h3>Belongs to no one but its users</h3>
        <p>AGPL-3.0 throughout, commodity hardware, open standards. No proprietary dependency and no lock-in at any layer.</p>
      </article>
      <article class="card">
        <h3>Respects the clinician's time</h3>
        <p>No workflow may be slower, harder, or more error-prone than its paper-record equivalent.</p>
      </article>
    </div>
  </div>
</section>
```

- [ ] **Step 2: Append styles**

```css
.section { padding: clamp(48px, 8vw, 88px) 0; }
.eyebrow.center { text-align: center; }
.card-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 18px; margin-top: 28px; }
.card { background: #fff; border: 1px solid var(--hairline); border-radius: 12px; padding: 24px 26px; }
.card h3 { font-size: 19px; font-weight: 500; margin: 0 0 8px; color: var(--ink); }
.card p { font-size: 16px; color: var(--ink-soft); margin: 0; }
@media (max-width: 640px) { .card-grid { grid-template-columns: 1fr; } }
```

> The spec mentions teal outline icons per card. Icons are optional polish; if added, use small inline SVGs (no icon-font dependency) so the page stays self-contained. Skipping them is acceptable for v1.

- [ ] **Step 3: Serve & screenshot at 1200px and 375px**

Expected: 2×2 white cards on paper at desktop, single column on mobile.

- [ ] **Step 4: Commit**

```bash
git add web/index.html web/styles.css
git commit -m "feat(web): mission cards"
```

---

## Task 8: Founding principles (condensed) + Design at a glance

**Files:**
- Modify: `web/index.html` (two sections; the glance section carries `id="architecture"`, principles `id="principles"`)
- Modify: `web/styles.css`

- [ ] **Step 1: Insert founding-principles markup**

```html
<section class="section section-tint" id="principles" aria-labelledby="principles-h">
  <div class="wrap">
    <p class="eyebrow center" id="principles-h">Founding principles</p>
    <ol class="principles">
      <li><strong>Availability over consistency.</strong> A clinician can always read locally-relevant records and write new data during a partition.</li>
      <li><strong>Paper-parity is the governing law.</strong> No workflow may lose to its paper equivalent in time, steps, or cognitive load.</li>
      <li><strong>The clinical record is append-only.</strong> Immutable signed events; corrections reference originals; sync becomes a safe set-union.</li>
      <li><strong>Identity is a claim, never a fact.</strong> Never merge — always link; never erase — always overlay. Every error is repairable with a full audit trail.</li>
      <li><strong>One system, every scale.</strong> The same software runs from workstation to nation; a node's role is configuration, not a different product.</li>
      <li><strong>Vendor independence is non-negotiable.</strong> AGPL-3.0, open standards, commodity hardware. No mandatory cloud, no license keys.</li>
      <li><strong>Safety-critical logic is unbreakable and auditable.</strong> Built where whole classes of error are unrepresentable, optimized for reviewer-legibility.</li>
    </ol>
    <p class="center"><a href="https://docs.cairn-ehr.org/principles/STEWARDSHIP-OF-THE-NAME/">Read the principles in full →</a></p>
  </div>
</section>
```

- [ ] **Step 2: Insert design-at-a-glance markup**

```html
<section class="section" id="architecture" aria-labelledby="arch-h">
  <div class="wrap">
    <p class="eyebrow center" id="arch-h">Design at a glance</p>
    <dl class="glance">
      <div><dt>Resilience</dt><dd>Offline-first; every node is write-capable; syncs to its parent when able; degrades to a single standalone workstation.</dd></div>
      <div><dt>Synchronization</dt><dd>Append-only event log + causal ordering (hybrid logical clocks); merge becomes set-union plus a small set of clinically-reasoned policies.</dd></div>
      <div><dt>Identity</dt><dd>A linkage layer over immortal patient IDs; deterministic + probabilistic matching; link / unlink / reattribute / repudiate as auditable events.</dd></div>
      <div><dt>Topology</dt><dd>Fractal: workstation → department → facility → region → nation, one codebase.</dd></div>
      <div><dt>Foundation</dt><dd>PostgreSQL ≥ 18; commodity hardware down to Raspberry-Pi class; standard Linux.</dd></div>
      <div><dt>Interoperability</dt><dd>FHIR as the interface, not a lock-in.</dd></div>
      <div><dt>Licensing</dt><dd>AGPL-3.0 end to end.</dd></div>
    </dl>
  </div>
</section>
```

- [ ] **Step 3: Append styles**

```css
.section-tint { background: var(--paper-alt); }
.principles { max-width: 760px; margin: 28px auto 20px; padding-left: 22px; }
.principles li { margin-bottom: 14px; color: var(--ink-soft); }
.principles strong { color: var(--ink); font-weight: 500; }
.glance { max-width: 820px; margin: 28px auto 0; }
.glance > div { display: grid; grid-template-columns: 200px 1fr; gap: 16px; padding: 16px 0; border-top: 1px solid var(--hairline); }
.glance > div:last-child { border-bottom: 1px solid var(--hairline); }
.glance dt { font-weight: 500; color: var(--ink); }
.glance dd { margin: 0; color: var(--ink-soft); }
@media (max-width: 640px) { .glance > div { grid-template-columns: 1fr; gap: 4px; } }
```

- [ ] **Step 4: Serve & screenshot at 1200px and 375px**

Expected: principles as a numbered list on the tinted band; glance as a clean two-column definition list collapsing to one column on mobile.

- [ ] **Step 5: Commit**

```bash
git add web/index.html web/styles.css
git commit -m "feat(web): founding principles and design-at-a-glance"
```

---

## Task 9: The name (editorial moment)

**Files:**
- Modify: `web/index.html`
- Modify: `web/styles.css`

- [ ] **Step 1: Insert markup**

```html
<section class="section section-tint" aria-labelledby="name-h">
  <div class="wrap name-block">
    <p class="eyebrow center" id="name-h">The name</p>
    <p class="name-prose">A cairn is a hand-built stack of stones that marks the safe path — needing no power, no network, no infrastructure, standing alone in the wilderness and still doing its job. Cairns are built by accretion, each traveller adding a permanent stone; they are decentralized, raised by many hands across a landscape; and they are found in nearly every culture on earth. So is this system meant to be.</p>
  </div>
</section>
```

- [ ] **Step 2: Append styles**

```css
.name-block { text-align: center; }
.name-prose { font-family: var(--font-serif); font-size: clamp(18px, 2.4vw, 23px); line-height: 1.6; max-width: 720px; margin: 24px auto 0; color: var(--ink); }
```

- [ ] **Step 3: Serve & screenshot**

Expected: serif paragraph on the tinted band. (Note: this sits adjacent to Task 8's principles tint — verify the two tinted bands are separated by the white glance section so they don't merge visually; if they end up adjacent, alternate one to `--paper`.)

- [ ] **Step 4: Commit**

```bash
git add web/index.html web/styles.css
git commit -m "feat(web): the-name editorial section"
```

---

## Task 10: Contribute band

**Files:**
- Modify: `web/index.html`
- Modify: `web/styles.css`

- [ ] **Step 1: Insert markup**

```html
<section class="band-contribute" aria-labelledby="contribute-h">
  <div class="wrap">
    <h2 id="contribute-h">Built by accretion. Raised by many hands.</h2>
    <p>For the people who have to use these systems and the people who have to keep them running. Clinical realism is valued as highly as code — a well-described failure mode from the front line is a genuine contribution.</p>
    <a class="btn btn-teal" href="https://github.com/cairn-ehr/cairn">Get involved</a>
  </div>
</section>
```

- [ ] **Step 2: Append styles**

```css
.band-contribute { background: var(--paper-alt); border-top: 1px solid var(--hairline); padding: clamp(48px, 8vw, 80px) 0; text-align: center; }
.band-contribute h2 { font-size: clamp(24px, 3.5vw, 34px); font-weight: 500; margin: 0 0 12px; }
.band-contribute p { font-size: 17px; color: var(--ink-soft); max-width: 560px; margin: 0 auto 24px; }
.btn-teal { background: var(--teal); color: #fff; }
.btn-teal:hover { background: #25564d; }
```

- [ ] **Step 3: Serve & screenshot**

Expected: tinted recruit band, headline, invitation, teal "Get involved" button.

- [ ] **Step 4: Commit**

```bash
git add web/index.html web/styles.css
git commit -m "feat(web): contribute band"
```

---

## Task 11: Footer

**Files:**
- Modify: `web/index.html` (replace the footer comment, after `</main>`)
- Modify: `web/styles.css`

- [ ] **Step 1: Insert markup**

```html
<footer class="site-footer">
  <div class="wrap footer-inner">
    <span>© Cairn · AGPL-3.0</span>
    <nav class="footer-links" aria-label="Footer">
      <a href="https://docs.cairn-ehr.org">Specification</a>
      <a href="https://github.com/cairn-ehr/cairn">GitHub</a>
      <a href="https://docs.cairn-ehr.org/principles/STEWARDSHIP-OF-THE-NAME/">The name is stewarded for the mission</a>
    </nav>
  </div>
</footer>
```

- [ ] **Step 2: Append styles**

```css
.site-footer { border-top: 1px solid var(--hairline); padding: 22px 0; }
.footer-inner { display: flex; align-items: center; justify-content: space-between; gap: 16px; flex-wrap: wrap; font-size: 14px; color: var(--ink-faint); }
.footer-links { display: flex; gap: 20px; flex-wrap: wrap; }
.footer-links a { color: var(--ink-faint); }
```

- [ ] **Step 3: Serve & screenshot (full page, 1200px and 375px)**

Expected: quiet footer; full page reads top-to-bottom as: header → hero → navy band → mission → principles → glance → name → contribute → footer.

- [ ] **Step 4: Commit**

```bash
git add web/index.html web/styles.css
git commit -m "feat(web): footer"
```

---

## Task 12: Accessibility & responsiveness pass

**Files:**
- Modify: `web/index.html` / `web/styles.css` (fixes as needed)

- [ ] **Step 1: Heading order & landmarks**

Confirm exactly one `<h1>` (the hero), that the contribute headline is `<h2>`, card titles `<h3>`, and each `<section>` has `aria-labelledby` pointing at a visible label. Where a section's label is an `.eyebrow`, that is acceptable as the accessible name. Fix any skipped levels.

- [ ] **Step 2: Contrast audit (WCAG AA)**

Check these pairs with a contrast tool (e.g. `browser_evaluate` running a luminance ratio, or any checker):
- `--ink` on `--paper` (body)
- `--ink-soft` on `--paper` and on `--paper-alt`
- `--ink-faint` on `--paper` (must reach 4.5:1 for normal text; if it fails, darken `--ink-faint` until it passes)
- `--navy-soft` / `#EAF0F4` on `--navy`
- white on `--navy`, white on `--teal`
Record results. Adjust any failing token and re-verify.

- [ ] **Step 3: Keyboard & no-JS**

Tab through the page: skip-link → brand → nav links → CTAs → footer links, all with visible focus rings. Then disable JavaScript (`browser` context) and reload: all content present, all links work, mobile nav still reachable (on mobile with JS off the nav can default to visible — verify it is not permanently hidden). Fix if the menu is unreachable without JS.

- [ ] **Step 4: Responsive sweep**

Screenshot at 320px, 375px, 768px, 1024px, 1280px. Confirm no horizontal scroll, no overflow, readable type at every width.

- [ ] **Step 5: Commit**

```bash
git add web/index.html web/styles.css
git commit -m "fix(web): accessibility and responsive pass"
```

---

## Task 13: SEO, social card, and Cloudflare config files

**Files:**
- Modify: `web/index.html` (add OG/Twitter meta)
- Create: `web/assets/og-card.svg`, `web/assets/og-card.png`
- Create: `web/robots.txt`, `web/sitemap.xml`, `web/_headers`

- [ ] **Step 1: Create the social card source** `web/assets/og-card.svg` (1200×630, paper bg, mark + tagline)

```svg
<svg viewBox="0 0 1200 630" xmlns="http://www.w3.org/2000/svg">
  <rect width="1200" height="630" fill="#F4F1E9"/>
  <g transform="translate(540,150) scale(1.0)">
    <ellipse cx="60" cy="116" rx="52" ry="20" fill="#1E3A52"/>
    <ellipse cx="60" cy="80" rx="42" ry="17" fill="#8B9398"/>
    <path d="M20 52 C20 24 100 24 100 52 C100 70 80 78 60 78 C40 78 20 70 20 52 Z" fill="#2F6E62"/>
  </g>
  <text x="600" y="430" text-anchor="middle" font-family="Inter, sans-serif" font-size="52" font-weight="700" fill="#262A2C">The grid goes down. The chart stays up.</text>
  <text x="600" y="490" text-anchor="middle" font-family="Inter, sans-serif" font-size="26" fill="#4A5358">Offline-first, vendor-independent electronic health record</text>
</svg>
```

- [ ] **Step 2: Rasterize to PNG**

```bash
# Prefer rsvg-convert; fall back to ImageMagick / sips as available.
rsvg-convert -w 1200 -h 630 web/assets/og-card.svg -o web/assets/og-card.png \
  || magick web/assets/og-card.svg web/assets/og-card.png \
  || qlmanage -t -s 1200 -o web/assets web/assets/og-card.svg
ls -la web/assets/og-card.png
```
Expected: a ~1200×630 PNG exists. If none of the converters are installed, note it and keep the SVG only, pointing OG tags at the PNG path to be generated at deploy.

- [ ] **Step 3: Add meta tags** inside `<head>` of `web/index.html` (after the description meta):

```html
  <meta property="og:type" content="website">
  <meta property="og:title" content="Cairn — the grid goes down, the chart stays up">
  <meta property="og:description" content="An offline-first, vendor-independent electronic health record.">
  <meta property="og:url" content="https://cairn-ehr.org/">
  <meta property="og:image" content="https://cairn-ehr.org/assets/og-card.png">
  <meta name="twitter:card" content="summary_large_image">
  <meta name="twitter:title" content="Cairn — the grid goes down, the chart stays up">
  <meta name="twitter:description" content="An offline-first, vendor-independent electronic health record.">
  <meta name="twitter:image" content="https://cairn-ehr.org/assets/og-card.png">
```

- [ ] **Step 4: Create `web/robots.txt`**

```
User-agent: *
Allow: /
Sitemap: https://cairn-ehr.org/sitemap.xml
```

- [ ] **Step 5: Create `web/sitemap.xml`**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>https://cairn-ehr.org/</loc></url>
</urlset>
```

- [ ] **Step 6: Create `web/_headers`** (Cloudflare Pages caching + security)

```
/assets/fonts/*
  Cache-Control: public, max-age=31536000, immutable
/assets/*
  Cache-Control: public, max-age=86400
/*
  X-Content-Type-Options: nosniff
  Referrer-Policy: strict-origin-when-cross-origin
  X-Frame-Options: DENY
```

- [ ] **Step 7: Serve, screenshot the og-card.png, and validate**

Confirm `og-card.png` renders correctly and the head contains all meta. Optionally validate HTML (`npx --yes html-validate web/index.html` if available; otherwise visually confirm well-formedness).

- [ ] **Step 8: Commit**

```bash
git add web/index.html web/assets/og-card.svg web/assets/og-card.png web/robots.txt web/sitemap.xml web/_headers
git commit -m "feat(web): SEO meta, social card, robots, sitemap, CF headers"
```

---

## Task 14: Deployment docs and final verification

**Files:**
- Create: `web/README.md`

- [ ] **Step 1: Write `web/README.md`**

```markdown
# Cairn landing page

Static landing page for https://cairn-ehr.org — no build step.

## Local preview
    uv run python -m http.server 8799 --directory web
    # open http://127.0.0.1:8799/

## Deploy (Cloudflare Pages)
- Connect this repository to a Cloudflare Pages project.
- Build command: *(none)*
- Build output directory: `web`
- Production branch: `main`
- Custom domain: `cairn-ehr.org`

The MkDocs specification site is deployed separately at `docs.cairn-ehr.org`.
All fonts are self-hosted; the page makes no third-party runtime requests.
```

- [ ] **Step 2: Final full-page verification**

Serve and screenshot the complete page at 1280px and 375px. Walk the acceptance criteria in the spec (§8) one by one and confirm each:
- structure & identity present; SVG mark + favicon + social card exist; fonts self-hosted (zero third-party requests in the network panel); primary CTA → docs subdomain, secondary → GitHub, no install CTA; copy consistent with `README.md`; responsive 320→desktop; AA contrast; keyboard + no-JS; within performance budget; deploy doc present.
- Measure transfer size (network panel total) and confirm < 200 KB excluding the OG card.

- [ ] **Step 3: Commit**

```bash
git add web/README.md
git commit -m "docs(web): Cloudflare Pages deployment notes"
```

---

## Self-review notes (author)

- **Spec coverage:** §2 build approach → Tasks 1,14; §3 identity (palette/logo/fonts) → Tasks 1,2,3; §4 structure (all 9 blocks) → Tasks 4–11; §5 a11y/responsive → Task 12; §6 SEO/sharing → Task 13; §7 out-of-scope respected (no analytics, no backend, no docs-migration work); §8 acceptance criteria → walked in Task 14. No gaps.
- **Open externalities for the executor to confirm before launch:** the real GitHub repo URL (assumed `github.com/cairn-ehr/cairn`); the exact `STEWARDSHIP-OF-THE-NAME` docs URL once the docs subdomain is live; availability of a raster converter for the OG PNG.
- **Consistency:** section ids (`mission`, `architecture`, `principles`) match the header nav anchors in Task 4; button classes (`btn-primary`, `btn-secondary`, `btn-teal`) are defined where first used and reused thereafter.
