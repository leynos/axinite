# Axinite Front‑End Semantic Linting — Implementation Guide

**Audience:** Implementers working on the `web-src/axinite` SolidJS workspace.  
**Goal:** Enforce semantic, accessible HTML with clean, token‑driven
Tailwind/DaisyUI usage. The implemented gate combines **Biome**, **Stylelint**
(`web-src/tools/stylelint.config.cjs`), **Semgrep**
(`web-src/tools/semgrep-semantic.yml`), and two TypeScript checkers
(`web-src/scripts/check-classlist-length.ts` and
`web-src/scripts/find-near-duplicate-classes.ts`). GritQL rules are a design
target described below; no `tools/grit/` rules are checked in yet.  
**Outcome:** Readable, queryable markup; reusable semantic classes via
`@apply`; consistent DaisyUI/Kobalte-aligned naming; one CLI for local and CI.  
**Verify:** run `bun run semantic` from `web-src/`, or `make frontend-full`
from the repository root.

______________________________________________________________________

## 0) Scope & Philosophy

- **Files:** `.tsx`, `.html`, `.css` (Tailwind v4 + DaisyUI v5).
- **Primary tool:** **BiomeJS** linter. GritQL rules below remain a design
  target and are not part of the current gate.
- **Fallbacks:**
  - **Stylelint** — design‑token enforcement, CSS `@apply` hygiene,
    units/specificity.
  - **Semgrep** — simple cross‑language regex/AST patterns (HTML/JSX) where
    easier than GritQL.
- **Design doctrine:**
  1. **Semantics first** — native elements/landmarks; ARIA fills gaps.
  2. **Tokens over literals** — DaisyUI/Tailwind tokens, not raw hex.
  3. **Compose w/ DaisyUI components** — refine with utilities.
  4. **Readable HTML** — extract repeated utility strings into **semantic
     classes** via `@apply` and sensible cascades.
  5. **Configurable thresholds** — warn on repetition beyond agreed limits.

______________________________________________________________________

## 1) Repository Layout (suggested)

Paths are relative to `web-src/`; the application sources live under
`axinite/src/` (for example `axinite/src/styles/semantic.css`). The tree below
is the suggested target layout, not a description of what is checked in.

```text
web-src/
├─ axinite/src/
│  ├─ app/...                 # TSX
│  └─ styles/
│     ├─ index.css           # @import "tailwindcss" + @plugin "daisyui"
│     ├─ tokens.css          # (generated or hand-written tokens)
│     ├─ semantic.css        # project semantic classes using @apply
│     └─ utilities.css       # optional @utility shorthands (Tailwind v4)
├─ scripts/
│  ├─ check-classlist-length.ts
│  └─ find-near-duplicate-classes.ts
├─ tools/
│  ├─ grit/
│  │  ├─ rule-a11y.grit
│  │  ├─ rule-daisyui.grit
│  │  ├─ rule-repetition.grit
│  │  └─ rule-allowlist.grit
│  ├─ semgrep-semantic.yml
│  ├─ stylelint.config.cjs
│  └─ semantic-lint.config.json   # thresholds & allowlists
├─ biome.jsonc
├─ package.json
└─ .github/workflows/semantic-lint.yml
```

The current application entry point is `axinite/src/styles/index.css`, which
imports `semantic.css` and Tailwind v4. The rest of the tree above is a
proposed layout, not a list of checked-in files. `semantic.css` stores reusable
classes via `@apply`.

______________________________________________________________________

## 2) Tailwind v4 + DaisyUI v5 baseline (CSS‑first)

**`src/styles/index.css`**

```css
@import "tailwindcss";

/* DaisyUI themes */
@plugin "daisyui" {
  themes:
    light --default,
    dark --prefersdark;
}

/* Tailwind v4: optionally hint sources if needed */
@source "../**/*.{ts,tsx,js,jsx,html}";
```

**Where to put semantic classes** — `src/styles/semantic.css` (imported from
the entry). Apply DaisyUI component classes directly in markup, and only use
`@apply` with Tailwind utility classes when defining semantic wrappers:

```css
/* Prefer DaisyUI roles directly in markup: class="btn btn-primary" */
.primary-action {
  @apply inline-flex items-center justify-center rounded-field px-4 py-2 font-semibold bg-primary text-primary-content;
}
.form-section-title {
  @apply text-lg font-semibold text-base-content;
}
.nav__link {
  @apply inline-flex items-center gap-2 text-sm font-medium text-base-content hover:text-primary;
}
.nav__link--active {
  @apply text-primary;
}
```

> Use semantic classes for **repeated patterns**; keep one‑offs directly in
> markup.

______________________________________________________________________

## 3) Configure BiomeJS + GritQL

**Install** (example):

```bash
pnpm add -D @biomejs/biome
```

**`biome.jsonc`** (excerpt from the repository today)

```jsonc
{
  "$schema": "https://biomejs.dev/schemas/stable/schema.json",
  "plugins": [
    "./tools/grit/rule-a11y.grit",
    "./tools/grit/rule-a11y-trailing.grit",
    "./tools/grit/rule-a11y-leading.grit",
    "./tools/grit/rule-a11y-span.grit",
    "./tools/grit/rule-a11y-span-trailing.grit",
    "./tools/grit/rule-a11y-span-leading.grit",
    "./tools/grit/rule-daisyui-btn-div.grit",
    "./tools/grit/rule-daisyui-btn-span.grit",
    "./tools/grit/rule-daisyui-btn-section.grit",
    "./tools/grit/rule-daisyui-btn-article.grit",
    "./tools/grit/rule-daisyui-btn-li.grit",
    "./tools/grit/rule-daisyui-btn-p.grit",
    "./tools/grit/rule-daisyui-input-div.grit",
    "./tools/grit/rule-daisyui-input-span.grit",
    "./tools/grit/rule-daisyui-input-section.grit",
    "./tools/grit/rule-daisyui-input-article.grit",
    "./tools/grit/rule-daisyui-input-li.grit",
    "./tools/grit/rule-daisyui-input-p.grit",
    "./tools/grit/rule-utility-wrapper.grit",
    "./tools/grit/rule-landmark-slot.grit",
    "./tools/grit/rule-state-slot-data-state.grit",
    "./tools/grit/rule-state-slot-aria-selected.grit",
    "./tools/grit/rule-state-slot-aria-current.grit",
    "./tools/grit/rule-state-slot-role-tab.grit",
    "./tools/grit/rule-layout-wrapper-div-flex.grit",
    "./tools/grit/rule-layout-wrapper-div-grid.grit",
    "./tools/grit/rule-layout-wrapper-section-flex.grit",
    "./tools/grit/rule-layout-wrapper-section-grid.grit",
    "./tools/grit/rule-heading-semantic-h1.grit",
    "./tools/grit/rule-heading-semantic-h2.grit",
    "./tools/grit/rule-heading-semantic-h3.grit",
    "./tools/grit/rule-heading-semantic-h4.grit",
  ],
  "linter": { "enabled": true, "rules": { "recommended": true } },
}
```

> Biome’s current Grit integration accepts one pattern per file. The repository
> therefore keeps the rules granular (for example, separate files for `<div>`
> and `<span>` button misuse). If the plugin grows support for multi‑pattern
> files, this list can be collapsed into fewer modules. When a Biome build does
> not natively load GritQL, use a small Node wrapper that executes Grit rules
> and prints Biome‑style diagnostics. Maintain the same paths and thresholds.

**Proposed `tools/semantic-lint.config.json` schema**

This is an unimplemented design example. The current checkers do not read this
file; their limits and behaviour are defined in their TypeScript sources.

```json
{
  "repeatMinClasses": 4,
  "repeatMinOccurrences": 2,
  "maxClasslistLength": 24,
  "allowProjectPrefixes": ["btn", "card", "nav__", "form-", "app-"],
  "disallowRawHex": true,
  "nearDuplicateClasses": {
    "minTokenCount": 4,
    "maxJaccardDistance": 0.25,
    "minOccurrences": 2,
    "tokenIndexSample": 5,
    "suppressPrefixes": ["prose-", "wysiwyg-"],
    "failOnViolation": false
  },
  "loopSiblingHints": {
    "loopMinOccurrences": 3,
    "siblingMinOccurrences": 3
  },
  "utilitySemanticScoring": {
    "minUtilityTokens": 8,
    "scoreThreshold": 6,
    "semanticWeight": 2
  },
  "conceptHints": {
    "disabled": []
  }
}
```

______________________________________________________________________

## 4) GritQL rules (practical starters)

> Syntax below illustrates intent; adapt to the configured Grit adapter’s exact
> grammar. Keep messages **actionable**.

### A) Semantic interactivity (no clickable `<div>` / `<span>`)

Files:

- `tools/grit/rule-a11y.grit`
- `tools/grit/rule-a11y-trailing.grit`
- `tools/grit/rule-a11y-leading.grit`
- `tools/grit/rule-a11y-span.grit`
- `tools/grit/rule-a11y-span-trailing.grit`
- `tools/grit/rule-a11y-span-leading.grit`

Each file targets a common attribute ordering, for example:

```grit
`<div ... onClick={$CALL}>$body</div>` where {
  register_diagnostic(
    span = $CALL,
    message = "Replace this clickable <div> with a semantic <button> or <a>."
  )
}
```

### B) Landmarks & labels (gentle suggestions)

```grit
language tsx

// Div used as nav
`<div role="navigation" $attrs>$body</div>` => diag("Prefer <nav> for primary navigation.")

// Input without associated label (simple heuristic)
`<input $attrs />`
where { not(match($attrs, /aria-label=|id=/)) }
=> diag("Inputs need a label: use <label htmlFor=...> in TSX (or <label for=...> in plain HTML) or aria-label.")
```

### C) DaisyUI usage sanity

Files:

- `tools/grit/rule-daisyui-btn-div.grit`
- `tools/grit/rule-daisyui-btn-span.grit`
- `tools/grit/rule-daisyui-btn-section.grit`
- `tools/grit/rule-daisyui-btn-article.grit`
- `tools/grit/rule-daisyui-btn-li.grit`
- `tools/grit/rule-daisyui-btn-p.grit`
- `tools/grit/rule-daisyui-input-div.grit`
- `tools/grit/rule-daisyui-input-span.grit`
- `tools/grit/rule-daisyui-input-section.grit`
- `tools/grit/rule-daisyui-input-article.grit`
- `tools/grit/rule-daisyui-input-li.grit`
- `tools/grit/rule-daisyui-input-p.grit`

Each file inverts a common misuse, e.g.

```grit
`<div ... className="... btn ..."` ...`>$body</div>` where {
  register_diagnostic(
    span = $body,
    message = "DaisyUI `btn` belongs on interactive elements (<button>, <a>, or submit <input>)."
  )
}
```

The duplication keeps messages targeted whilst Biome’s current Grit plugin
restricts repositories to one pattern per file.

### D) Repetition → extract with `@apply`

- `scripts/check-classlist-length.ts`
- `scripts/find-near-duplicate-classes.ts`

Both TypeScript checks share `scripts/scan-class-attributes.ts`. The scanner
reads static `class="..."` attributes in `axinite/src/**/*.tsx`.

- `check-classlist-length.ts` fails when an attribute contains more than 12
  whitespace-separated class tokens.
- `find-near-duplicate-classes.ts` fails when a single attribute repeats the
  same token. It does not compare separate attributes or measure similarity.

The 12-token limit is a constant in the length checker. Neither checker reads
`tools/semantic-lint.config.json`; changing the limit requires changing the
source. `bun run semantic` runs both checks along with the other semantic lint
steps.

### E) Duplicate tokens within one class attribute

The duplicate-token checker catches repeated tokens such as `class="flex flex"`
in one static attribute. It does not identify similar class lists across
different elements. Extracting a shared semantic class remains a useful manual
refactoring when repeated patterns are found.

______________________________________________________________________

## Proposed meaning-first semantic checks (roadmap)

These heuristics focus on **structural intent** rather than raw class
similarity. Each item describes what to detect, why it matters, and the
preferred fix to recommend in future lint passes.

### 1) “Div/span soup” (utility-only wrapper chains)

- **Signal**: Two or more nested `<div>`/`<span>` nodes whose only attribute is
  a utility-heavy `className` (no `role`, `aria-*`, `id`, `data-*`, event
  handlers).
- **Meaning**: The wrapper is expressing a _layout concept_ (stack, cluster,
  surface) without naming it.
- **Action**: Warn and suggest extracting a named container class (e.g.
  `.stack`, `.cluster`, `.card__body`) via `@apply`.
- **Implementation notes**: GritQL rule that walks parent/child utility-only
  nodes and fires when depth ≥ 2; fallback AST walker if needed.

### 2) Landmarks and slot semantics

- **Signal**: Utility-only child elements inside semantic regions such as
  `<nav>`, `<header>`, `<ul>`, `<form>`.
- **Meaning**: These are _slots_ (navigation links, menu items, form controls)
  that deserve a named semantic class.
- **Action**: Suggest contextual names like `.nav__link`, `.nav__link--active`,
  `.menu__item`, `.form__label`.
- **Implementation notes**: GritQL ancestor constraint rules that tailor the
  suggestion to the enclosing landmark.

### 3) Repeated sibling pattern (loop intent)

- **Signal**: Identical or near-identical class lists applied to siblings,
  especially those produced by `.map()` in TSX.
- **Meaning**: Indicates a row/tile/card abstraction that should become a shared
  class.
- **Action**: Recommend extracting both a container class (e.g. `.card-grid`)
  and an item class (e.g. `.card`).
- **Implementation notes**: Extend the existing near-duplicate script to
  consider siblings grouped by parent, or add a dedicated GritQL rule for
  `.map()` return values.

### 4) Token-to-concept mapping

- **Signal**: Utility combinations that scream a known component (button, chip,
  badge, toolbar, tabs trigger).
- **Meaning**: The element already behaves like a component but lacks a semantic
  name.
- **Action**: Issue prescriptive suggestions derived from a concept dictionary
  (e.g. `.chip`, `.toolbar`, `.tabs__trigger`).
- **Implementation notes**: Dictionary-driven matcher in the companion script or
  GritQL predicates that match presence/absence token sets.

### 5) Stateful slots (Kobalte/DaisyUI attributes)

- **Signal**: Elements with `data-state`, `aria-selected`, `aria-current`,
  `role="tab"` and long utility lists.
- **Meaning**: State-driven UI pieces should expose a semantic class for styling
  the base and active states.
- **Action**: Recommend classes like `.tabs__trigger` plus state modifiers in
  CSS.
- **Implementation notes**: GritQL rule watching for state attributes plus
  utility load.

### 6) Anonymous layout wrappers

- **Signal**: Elements using only layout utilities (flex/grid/gap/justify/space)
  without semantics.
- **Meaning**: These are layout patterns (stack/cluster/switcher) that deserve
  named abstractions.
- **Action**: Suggest layout semantics (e.g. `.stack`, `.cluster`, `.sidebar`).
- **Implementation notes**: Token classifier that recognizes layout-only sets;
  fire in GritQL or the script.

### 7) Heading/title repetition

- **Signal**: The same utility bundle repeated across multiple headings
  (`<h1>`-`<h4>`) or section titles.
- **Meaning**: Editorial roles should be expressed via `.section-title`,
  `.card__title`, etc.
- **Action**: Recommend extracted title classes with `@apply`.
- **Implementation notes**: GritQL rule that tracks heading class repetition
  counts.

### 8) Utility load vs. semantic signal score

- **Signal**: Elements with heavy utility usage but little semantic metadata (no
  role/id/aria/data).
- **Meaning**: Frequent offender list for “this should have a name”.
- **Action**: Soft warning nudging authors to introduce a semantic class
  summarizing intent.
- **Implementation notes**: Extend the companion script to compute a heuristic
  score `(tokenCount - semanticSignals)` and warn above a configurable
  threshold.

Each proposal will ship with a concrete class name suggestion and will reference
`src/styles/semantic.css` (or `@utility` helpers) so fixes are straightforward.

### F) Class allowlist (Tailwind, DaisyUI, project semantics)

The current Biome plugin does not yet expose helpers such as `split_classes`.
Until that changes, the repository enforces the allowlist via **Semgrep**
(`tools/semgrep-semantic.yml`, rule `class-token-uppercase`), which catches
camelCase tokens and other non‑Tailwind/DaisyUI class names. Migration back
into Grit can be performed when the integration grows richer predicates.

______________________________________________________________________

## 5) Stylelint (focused rules only)

**Install**

```bash
pnpm add -D stylelint stylelint-declaration-strict-value
```

**`tools/stylelint.config.cjs`**

```js
module.exports = {
  plugins: ["stylelint-declaration-strict-value"],
  rules: {
    // Disallow hex/named colours; require tokens or Tailwind classes
    "color-no-hex": true,
    "color-named": "never",

    // Enforce tokens for key props (allow CSS variables + keywords)
    "scale-unlimited/declaration-strict-value": [
      [
        [
          "/^color$/",
          "^background(-color)?$",
          "^border(-.*)?-color$",
          "fill",
          "stroke",
          "outline-color",
        ],
      ],
      { ignoreValues: ["transparent", "currentColor", "/^var\\(--/"] },
    ],

    // Discourage !important; keep specificity sane
    "declaration-no-important": true,

    // Optional unit policy (px allowed in borders etc.)
    "declaration-property-unit-disallowed-list": {
      "/^(margin|padding|gap|inset|top|right|bottom|left)$/": ["px"],
    },
  },
};
```

> Stylelint is **not** used for general formatting; it only enforces tokens and
> a few hygiene rules.

______________________________________________________________________

## 6) Semgrep (targeted HTML/JSX checks)

**Install & config**

```bash
uv tool install semgrep
```

**`tools/semgrep-semantic.yml`**

```yaml
rules:
  # Raw Tailwind colour utilities instead of semantic roles
  - id: tailwind-raw-color-class
    patterns:
      - pattern-regex: "\\b(text|bg|border)-(red|blue|green|gray|slate|zinc|stone|amber|rose)-[0-9]{2,3}\\b"
    message:
      "Prefer DaisyUI role classes (e.g., bg-primary, text-base-content) over
      raw Tailwind colour scale."
    languages: [javascript, typescript, jsx, tsx, html]
    severity: WARNING

  # Dynamic daisyUI token in template literal (build-time discovery issue)
  - id: daisyui-dynamic-token
    pattern-regex: "`[^`]*\\b(btn|badge|alert|tab)-\\${[^}]+}[^`]*`"
    message:
      "Avoid dynamic DaisyUI token names (e.g. `btn-${color}`) — Tailwind cannot
      statically include them."
    languages: [javascript, typescript, jsx, tsx]
    severity: WARNING

  # Overly long class attribute (coarse backstop)
  - id: classlist-too-long
    pattern-regex: "class(Name)?=\\{?['\"][^'\"]{120,}['\"]\\}?"
    message: "Long class list — extract into a semantic class with @apply."
    languages: [javascript, typescript, jsx, tsx, html]
    severity: WARNING
```

______________________________________________________________________

## 7) Unified CLI & Dev Workflow

**`package.json`** (current scripts)

<!-- markdownlint-disable MD013 -->
```json
{
  "scripts": {
    "semantic:lint": "bunx biome lint axinite/src axinite/tests scripts docs && bun run lint:classlist && bun run lint:class-duplicates && uvx semgrep --config tools/semgrep-semantic.yml --include axinite/src/**/*.tsx --include axinite/tests/**/*.tsx && bunx stylelint 'axinite/src/**/*.css' --config tools/stylelint.config.cjs",
    "semantic": "bun run semantic:lint",
    "lint:classlist": "bun run scripts/check-classlist-length.ts",
    "lint:class-duplicates": "bun run scripts/find-near-duplicate-classes.ts"
  }
}
```
<!-- markdownlint-enable MD013 -->

- **Local dev:** `bun run semantic` runs Biome, the class-length and
  near-duplicate checkers, Semgrep, and Stylelint.
- **Pre-commit (optional):** wire a Husky hook to `bun run semantic`.
- **CI:** `bun run semantic` is part of the frontend gate; see
  `.github/workflows/frontend.yml` and `make frontend-full`.

> `uvx semgrep` expects `uv` to be installed
> (<https://github.com/astral-sh/uv>). The repo keeps the command consistent
> across local + CI environments.

______________________________________________________________________

## 8) Proposed companion semantic checks

The ideas in this section are future work. The current TypeScript checkers only
apply the fixed length limit and detect duplicate tokens within one static
class attribute. They do not implement these analyses:

- **Cross-attribute similarity:** compare separate class attributes, potentially
  using token-set similarity such as Jaccard distance.
- **Loop and sibling repetition:** find repeated classes in `.map()` output or
  sibling elements and suggest a shared class.
- **Concept hints:** recognize button, chip, card, toolbar, tab, or navigation
  patterns and suggest semantic class names.
- **Utility-to-semantics scoring:** identify long utility lists without semantic
  signals such as roles, state attributes, or semantic elements.
- **Configurable thresholds and allowlists:** read policies from a checked-in
  configuration file and report actionable diagnostics.

______________________________________________________________________

## 9) Proposed configuration and tuning ideas

No configuration file is currently consumed by these checkers. The class-list
limit is fixed at 12 tokens, and duplicate-token detection has no tuning
options. If the proposed cross-attribute, loop, sibling, concept, or scoring
checks are implemented, a configuration file could define their thresholds,
allowlists, and narrowly scoped suppressions. Until then, those settings are
examples of future work, not operator controls.

______________________________________________________________________

## 10) How to refactor into `@apply` (+ cascades)

1. **Spot repetition:** during review, identify a class chunk that appears in
   multiple attributes. The current checker does not detect cross-attribute
   repetition.
2. **Name the concept:** choose a **semantic** name that reflects purpose, not
   appearance: e.g., `.nav__link`.
3. **Define once:** in `src/styles/semantic.css`:

   ```css
   .nav__link {
     @apply inline-flex items-center gap-2 text-sm font-medium text-base-content hover:text-primary;
   }
   .nav__link--active {
     @apply text-primary;
   }
   ```

4. **Replace usages:** change `className` to the new class (keep any **unique**
   one-off utilities alongside if needed: `className="nav__link md:pl-3"`).
5. **Use cascades where logical:** if a set of children share
   typography/spacing, apply a parent class and let children inherit or use
   low‑specificity selectors:

   ```css
   .form-section {
     @apply space-y-3;
   }
   .form-section .form-section-title {
     @apply text-lg font-semibold;
   }
   ```

**Naming guidance:**

- Mirror DaisyUI where it fits: `.btn`, `.btn-primary`, `.card`, `.card-title`.
- For bespoke patterns: BEM‑ish and role‑based, e.g. `.walk-card`,
  `.walk-card__title`; or `.nav__link--active`.
- Prefer **role tokens** (`primary`, `secondary`, `accent`, `success`, `error`)
  to colour words.

______________________________________________________________________

## 11) Quick rule checklists (what the lints enforce)

- **Semantic & a11y**
  - No clickable `div/span` without role+keyboard; prefer `<button>/<a>`.
  - Use `<nav>/<main>/<header>/<footer>` over `div[role=...]`.
  - Inputs must be labelled (`<label htmlFor=...>` in TSX, `<label for=...>` in
    plain HTML, or `aria-label`).

- **DaisyUI composition**
  - `btn` belongs on interactive elements.
  - `input`/`select`/`textarea` use matching DaisyUI classes.

- **Design tokens**
  - No hex/named colours in CSS.
  - Prefer role utilities (`bg-primary`, `text-base-content`) over raw Tailwind
    colour scales.

- **Repetition & readability**
  - Warn on long/repeated class chunks → extract with `@apply` into
    `semantic.css`.
  - Unknown class tokens flagged (must be Tailwind, DaisyUI, or project semantic
    class prefix).

______________________________________________________________________

## 12) IDE tips

- Enable Tailwind IntelliSense for class hints and token names.
- Configure Biome to format on save; run `bun run semantic:lint` before commits.
- Configure an editor task to open `semantic.css` quickly when a repetition
  warning appears.

______________________________________________________________________

## 13) Future extensions

- **Cross‑file repetition index** (cache normalized chunks to detect repeats
  across the whole repo).
- **Contrast checks** (static heuristics on text/background utility pairs).
- **Autofix codemods** (Grit rewrite scripts that scaffold a new class and
  replace duplicates in a file).

______________________________________________________________________

## 14) FAQ

**Q: Will `@apply` bloat CSS?**  
A: Only repeated patterns are extracted. Tailwind still tree‑shakes class‑based
styles; the few semantic classes added remain minimal and intentionally reused.

**Q: When is it appropriate to keep utilities inline?** A: One‑offs, quick
prototypes, and tiny adjustments local to a component. Once the same chunk
appears twice, prefer extracting it.

**Q: What limits do the current class checkers enforce?**

A: The class-list check fails above 12 whitespace-separated tokens. The
duplicate-token check flags repeats within one static class attribute. Neither
check has configurable thresholds.

______________________________________________________________________

## 15) Current tooling and proposed work

### Existing checks to maintain

`bun run semantic` in `web-src/package.json` runs Biome, both class-attribute
checks, Semgrep, and Stylelint. Semgrep and Stylelint use
`web-src/tools/semgrep-semantic.yml` and `web-src/tools/stylelint.config.cjs`.
The shared scanner is under `web-src/scripts/`; semantic styles already live in
`web-src/axinite/src/styles/semantic.css`.

### Proposed extensions

1. Add Grit rules and wire them through a Biome plugin or wrapper.
2. Add policy configuration only if future checks need configurable thresholds
   or allowlists. The current class checks have no tuning options.
