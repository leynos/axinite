# v2a front-end stack

This document describes the df12 Productions v2a front-end stack for the
Wildside and Corbusier mockups in two layers:

- the target stack for the mockup as it moves from static prototype markup to an
  application runtime, and
- the fuller v2a application stack described elsewhere in this repository’s
  design and architecture documents.

That distinction matters because the prototype already exercises much of the UI,
styling, routing, localization, and map stack, while the broader product
architecture adds local-first data and orchestration tooling that can sit on top
of the UI layer without changing the rendering model.

The current repository still ships a static prototype under `axinite/`. Treat
the file paths and module names below as the intended SPA layout for the
SolidJS + Kobalte implementation, not as a claim that the static prototype has
already been migrated.

## Overview

Both Wildside and Corbusier are client-side single-page applications built on
the same v2a front-end stack: Bun, Vite, SolidJS, TanStack Router for Solid,
Tailwind CSS v4, and DaisyUI v5. They share Kobalte primitives for interactive
components, i18next with Fluent translation bundles for localization, and a
common data-model-driven card architecture for presenting domain entities.

The map canvas, tile rendering, and location-aware UI are specific to the
Wildside product domain. They are part of the Wildside mockup because it models
a map-based exploration application, not because every front-end in this repo
family would need them.

## Stack layers at a glance

### Target mockup stack

The current documented mockup direction uses:

- Bun,
- Vite 5,
- SolidJS,
- TanStack Router,
- Tailwind CSS v4,
- DaisyUI v5,
- Kobalte,
- i18next plus Fluent,
- MapLibre GL JS, and
- the current test, lint, and type-check toolchain described below.

### Full v2a application stack

The fuller v2a stack described across the repo’s architecture documents adds:

- **Zustand** for interactive client and UI state,
- **TanStack Query** for server-state fetching, caching, and synchronization,
- **Dexie** for durable browser-side storage of offline bundles, map tiles, and
  related heavier local data, and
- **XState** for modelling more complex interaction and workflow orchestration
  where a reducer or plain context store becomes too implicit.

In other words, the mockup shows the presentation and navigation layer already
working, while the full product stack includes a richer local-first state and
data architecture.

## Runtime and build toolchain

- **Package manager and runner:** Bun drives local scripts, tests, and token
  generation via `package.json` and `bunfig.toml`.
- **Bundler and dev server:** Vite 5 is the application bundler and development
  server.
- **Solid integration:** `vite-plugin-solid` handles JSX compilation and Solid
  development ergonomics under Vite.
- **Tailwind integration:** Tailwind v4 runs through `postcss.config.cjs`, which
  Vite uses during both development and production builds.
- **Module format:** The project is ESM-only (`"type": "module"` in
  `package.json`).
- **Base-path handling:** `vite.config.ts`, `src/i18n.ts`, and
  `src/app/routes/app-routes.tsx` normalize `APP_BASE_PATH` and
  `import.meta.env.BASE_URL` so the SPA can run correctly under a subpath such
  as GitHub Pages.

In practice, the front-end entry path is:

1. `src/main.tsx` mounts the application with Solid's `render`.
2. `src/app/app.tsx` wires global providers and shell state.
3. `src/app/routes/app-routes.tsx` mounts the TanStack Router tree for Solid.

## Core application framework

### SolidJS

The UI is built with SolidJS and `solid-js/web`. Solid's rendering model is
based on fine-grained reactivity rather than component-wide re-rendering.
Signals are the primary state primitive: they hold primitive or structured
values, track reactive dependencies, and update only the DOM work that depends
on them. The main bootstrap file, `src/main.tsx`, mounts the SPA into `#root`
and can provide asynchronous loading fallbacks via `Suspense` where useful.

### Routing

Routing is handled by TanStack Router's Solid framework support:

- `src/app/routes/app-routes.tsx` creates the router.
- `src/app/routes/route-tree.tsx` assembles the route tree.
- Route modules live under `src/app/routes/`.

The route tree is explicit and file-based in practice, even though it is wired
manually rather than generated. Current route groups include welcome, discover,
explore, customize, map, saved, wizard, walk-complete, offline, and
safety-accessibility flows.

### State management

In the currently documented mockup, there is no requirement for a heavyweight
global client-state library in the baseline UI layer. State is managed through:

- Solid signals and stores,
- small context providers for cross-cutting concerns such as theme or locale,
  and
- route-local and component-local state for shell and screen behaviour.

That keeps the mockup closer to a Solid signals-plus-context architecture than
to a reducer-heavy state machine or cache-led design.

For the fuller v2a application stack, the state split is broader:

- **Zustand** owns interactive client and UI state,
- **TanStack Query** owns server and synchronized domain state, and
- **XState** is the right fit for explicit multistep workflows or long-running
  interaction logic that benefits from a formal state machine.

## Styling and design system

### Tailwind CSS and DaisyUI

The styling stack is built on Tailwind CSS v4 and DaisyUI v5.

- `src/index.css` imports Tailwind and the generated token CSS first, then
  layers project-specific component and utility styles on top.
- `tailwind.config.cjs` loads generated Tailwind theme extensions from
  `tokens/dist/tailwind.theme.cjs`.
- DaisyUI is registered as a Tailwind plugin in `tailwind.config.cjs`.

The styling approach is not utility-only. The project uses a hybrid of:

- Tailwind utilities,
- DaisyUI component classes such as `btn`, and
- semantic project classes defined in `src/index.css` and the files under
  `src/styles/`.

### Design tokens

Design tokens are a first-class part of the stack.

- `tokens/build/style-dictionary.js` uses Style Dictionary to generate token
  artefacts.
- `bun run tokens:build` produces:
  - `tokens/dist/tokens.css` for runtime CSS variables and theme output,
  - `tokens/dist/tailwind.theme.cjs` for Tailwind theme extension data, and
  - legacy helper artefacts under `tokens/dist/` for incidental tooling.
- `vite.config.ts` watches the generated token outputs and triggers a full-page
  reload when they change.

This means the front-end theme layer is not hand-maintained in one place.
Instead, the design source of truth lives in the token package, and both CSS and
Tailwind consume generated outputs.

### Theme handling

Theme selection is handled with a small Solid context backed by signals rather
than a separate theme framework:

- `src/app/providers/theme-provider.tsx` persists the active theme in
  `localStorage`.
- The provider sets `data-theme` on the document root and body.
- The shipped theme names are `corbusier-mockup-night` and
  `corbusier-mockup-day`.

### PostCSS

`postcss.config.cjs` applies:

- `@tailwindcss/postcss`,
- `autoprefixer`, and
- a small custom plugin that removes DaisyUI's `@property --radialprogress` rule
  to avoid Lightning CSS warnings during Vite builds.

## Component primitives and icons

The UI component layer uses Kobalte for interaction primitives, including
dialogs, tabs, selects, popovers, switches, accordions, and related overlay
behaviour. Kobalte is intentionally unstyled, built for SolidJS, and follows
WAI-ARIA authoring practices while handling keyboard support, focus management,
and assistive-technology wiring. Representative examples from the official
component docs include:

- Dialog support for modal and non-modal modes, focus trapping, Escape to close,
  scroll blocking, and screen-reader announcements via title and description.
- Tabs support for semantic tab-to-panel linking, horizontal or vertical
  orientation, automatic or manual activation, and both LTR and RTL keyboard
  navigation.
- Select support for listbox semantics, tab-stop focus management, arrow-key
  opening behaviour, typeahead, and form integration via a hidden native
  `<select>`.

This gives the repository a Kobalte-behaviour plus Tailwind/DaisyUI-
presentation shape, rather than a single monolithic component framework.

## Localization stack

Localization is one of the more specialized parts of this stack.

- `src/i18n.ts` configures `i18next`.
- A Solid-oriented binding layer exposes translations to components and keeps
  locale changes reactive.
- `i18next-fluent` and `i18next-fluent-backend` load Fluent `.ftl` bundles.
- Locale files live in `public/locales/<locale>/common.ftl`.
- `src/app/i18n/supported-locales.ts` defines supported locales and direction
  metadata.

The current detection order is intentionally narrow:

- query string first, then
- `localStorage`.

Navigator-based detection is deliberately excluded to keep first loads more
deterministic. The runtime also updates `lang`, `dir`, and related attributes on
the document so right-to-left languages such as Arabic and Hebrew are handled
correctly.

## Map stack

Interactive map views use MapLibre GL JS, but this should be understood as a
domain-specific layer rather than a foundational part of the general front-end
stack. The map, tile, and location elements exist because Wildside is a
map-based application with itinerary, quick-walk, and saved-route flows.

- The fuller map stack is expected to lazy-load `maplibre-gl` and its CSS.
- OpenMapTiles-backed styling supplies the base map presentation.
- The integration is expected to register the MapLibre RTL text plugin when the
  runtime supports it.
- Shared viewport and layer state should live in a dedicated map-state module.

The map layer is therefore intended to be a real interactive map integration,
not only a static mockup image.

In the full v2a architecture, map-related persistence is also expected to touch
the broader local-first stack:

- TanStack Query for route and place data,
- Dexie for offline bundles, map tile storage, and other heavier offline
  artefacts, and
- UI state or workflow orchestration layered above that with Zustand and, where
  useful, XState.

## Testing and verification stack

The repository has a broader-than-average front-end verification setup.

### Unit and component tests

- `bun test` is the primary unit and component test runner.
- `tests/setup-happy-dom.ts` creates the DOM environment with Happy DOM.
- Solid component tests use `@solidjs/testing-library`-style patterns.
- `tests/setup-snapshot-guard.ts` fails the run if snapshots are left unchecked
  or updated unexpectedly.

### Accessibility-focused tests

There are two accessibility layers:

- `bun run test:a11y` runs Vitest in a dedicated `jsdom` configuration for
  `*.a11y.test.ts(x)` files.
- `bun run test:e2e` runs Playwright tests, including `@axe-core/playwright`
  sweeps against key routes.

### End-to-end tests

`playwright.config.ts` launches or reuses a Vite dev server, targets a mobile
viewport, and runs the browser suite under Chromium. This fits the mobile-first
nature of the mockup.

## Linting, type-checking, and semantic checks

The repo’s front-end quality stack goes beyond a simple formatter plus linter.

- `bun fmt` runs Biome formatting.
- `bun lint` runs Biome checks across source, tests, tools, and docs.
- `bun check:types` runs strict TypeScript checking with `tsc --noEmit`.
- `bun run semantic` layers on extra structural checks:
  - class-list length and near-duplicate class scripts,
  - Semgrep rules from `tools/semgrep-semantic.yml`, and
  - Stylelint checks for CSS.
- `bun run lint:ftl-vars` validates Fluent variable usage.

The strict TypeScript settings in `tsconfig.json` include:

- `strict`,
- `noUncheckedIndexedAccess`,
- `exactOptionalPropertyTypes`,
- `noImplicitOverride`, and
- `noPropertyAccessFromIndexSignature`.

## Data model-driven card architecture

All v2a front ends — Wildside and Corbusier alike — share a common pattern for
presenting domain entities on cards, lists, and detail screens. Entity models
carry their own localized strings rather than delegating display-text
responsibility to the Fluent translation bundles. This keeps Fluent bundles
focused on UI chrome (button labels, ARIA labels, section headings, format
strings) while letting each entity own its names, descriptions, and badge text
per locale.

### Shared primitives

The following types form the shared vocabulary across all v2a front ends:

- **`LocaleCode`** — a branded string identifying a supported locale (e.g.
  `en-GB`, `ja`).
- **`LocalizedStringSet`** — a record mapping `LocaleCode` keys to translated
  display strings.
- **`EntityLocalizations`** — a per-entity bundle that groups together every
  `LocalizedStringSet` the entity needs (name, description, badge text, and so
  on).
- **`LocalizedAltText`** — a `LocalizedStringSet` specifically for image alt
  text, kept as a distinct type so that accessibility tooling can enforce its
  presence.
- **`ImageAsset`** — a reference to an image file together with its
  `LocalizedAltText`.

### Locale resolution

A small pure helper, `pickLocalization(localizations, locale)`, resolves a
`LocalizedStringSet` for the requested locale. If the exact locale is not
present, the helper falls back to `en-GB`. This single function is the only
place where locale-fallback logic lives, keeping the rest of the rendering code
free of null checks or conditional chains.

### Descriptor registries

Stable internal identifiers (e.g. `task-status:in-progress`, `priority:high`)
are resolved to localized display strings through descriptor registries. Each
registry maps an identifier to a descriptor object whose labels are
`LocalizedStringSet` values. This means that badge colours, icons, and display
names for status values, priority levels, and similar enumerations are defined
once and shared by every component that renders them.

### Folder layout

The conventional folder layout is:

- `src/app/domain/entities/` — TypeScript interfaces and type aliases for entity
  models and their localization shapes.
- `src/data/entities/` — fixture data used during the mockup phase, structured
  to match the entity interfaces exactly.
- `src/data/registries/` — descriptor registry modules that map stable
  identifiers to localized descriptors.

### Application-specific schemas

Each application maintains its own
`docs/data-model-driven-card-architecture.md`, which defines the
application-specific entity schemas, enumerations, and migration roadmap. That
document is the authoritative reference for which entities exist, what fields
they carry, and how the card architecture will evolve as the mockup matures
toward production data sources.

## Effective stack summary

For the shortest accurate summary of the checked-in mockup, the current
front-end stack is:

- Bun for package management, scripts, and the primary test runner,
- Vite 5 for bundling and development,
- SolidJS for the SPA runtime,
- TanStack Router for Solid for routing,
- Tailwind CSS v4 plus DaisyUI v5 for styling,
- Style Dictionary for generated design tokens and themes,
- Kobalte primitives for interactive components,
- i18next plus Fluent for localization,
- a data-model-driven card architecture for entity display strings,
- MapLibre GL JS for map screens (Wildside only),
- Biome, TypeScript, Stylelint, Semgrep, and custom scripts for code quality,
  and
- Happy DOM, Testing Library, Vitest, Playwright, and axe-core for testing.

For map/location-based applications specifically, add a domain layer atop that
stack:

- MapLibre GL JS for interactive maps,
- OpenMapTiles-backed tile styling, and
- location-centric UI and state for itinerary and route experiences.

For the shortest accurate summary of the fuller v2a application architecture,
add:

- Zustand for interactive state,
- TanStack Query for server-state caching and synchronization,
- Dexie for offline bundle and map tile storage, and
- XState for explicit interaction orchestration where state machines are
  warranted.

## What is not currently declared in the checked-in mockup

To avoid confusion, the following section refers to the currently checked-in
mockup on `origin/main`, not the broader architecture described above.

The following common front-end tools are outside the baseline SolidJS + Kobalte
approach described here:

- TanStack Table,
- Redux,
- Next.js, Remix, or any SSR framework, and
- Framer Motion.
