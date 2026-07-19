# Migrate `axinite/` to a SolidJS single-page PWA

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`, `Surprises & Discoveries`,
`Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work
proceeds.

Status: IN PROGRESS

## Purpose / big picture

The current `axinite/` prototype is a multi-page static site made from separate
HTML documents such as `axinite/chat/index.html` and
`axinite/memory/index.html`, with browser-loaded Tailwind, inline scripts, and
page-local state. The target state is one typed SolidJS application that keeps
the current information architecture but runs as a single-page Progressive Web
Application (PWA), uses Kobalte for interactive accessibility primitives,
Tailwind CSS v4 plus daisyUI v5 for styling, TanStack Router for route typing,
TanStack Query for request-driven server state, and i18next plus Fluent for all
user-facing copy.

After this migration, a user should be able to open one application shell,
navigate between Chat, Memory, Jobs, Routines, Extensions, Skills, and logs
without full page reloads, switch between English (GB), French, German,
Italian, Dutch, Polish, Hindi, Japanese, Simplified Chinese, and Arabic, see
correct left-to-right and right-to-left layout behaviour, and interact with
only the features that the backend says are available. The application must be
WCAG 2.2 conformant, must prefer semantic classes over utility sprawl, must
compile with full TypeScript checking, and must be covered by unit, behaviour,
accessibility, and end-to-end tests.

This plan was approved for implementation by the user on 2026-03-25. The work
below therefore serves as both the execution guide and the live implementation
record.

## Visual restoration follow-up

The runtime migration is complete, but the current SolidJS shell still loses
too much of the original mock-up's route-specific design. This follow-up stream
reopens the plan to restore the original shared chrome, page structure, and
visual hierarchy inside the SPA rather than treating the generic hero/card
layout as the final design.

The restoration order is:

1. shared shell chrome and watermark treatment
2. Chat and Memory, because they are the most visually distinctive
3. Jobs and Routines, because they share a dashboard/table design family
4. Extensions and Skills, because they share a catalogue/panel design family

## Repository and system orientation

The current mock-up lives in `axinite/` inside this repository. Today it is a
set of page-specific HTML files:

- `axinite/chat/index.html`
- `axinite/memory/index.html`
- `axinite/jobs/index.html`
- `axinite/routines/index.html`
- `axinite/extensions/index.html`
- `axinite/skills/index.html`

Those files already express the product surface that the SPA must preserve:
shared navigation, connection status, logs access, and the route families for
chat, memory, jobs, routines, extensions, and skills. They also carry the
current visual language, so they are the migration reference until route parity
is reached.

The backend integration target is documented in
`../axinite/docs/front-end-architecture.md`. The important consequence is that
the real Axinite frontend is currently embedded in the Rust host as static
assets and talks to authenticated JSON, Server-Sent Events (SSE), and status
endpoints. That means this migration must not assume a separate server-rendered
frontend, and it must keep a clean seam between the SPA build and the Rust host
that will eventually serve it.

The feature-flag design target is documented in
`../axinite/docs/rfcs/0009-feature-flags-frontend.md`. The SPA must therefore
be built around a typed flag registry fetched from `GET /api/features`, with
deployment-scoped values, and any surface that is present in the mock-up but
missing in the backend must stay behind explicit flags. The plan below adds a
debug-only flag override mode so maintainers can exercise hidden paths without
lying about production availability.

Validation examples already exist in sibling repositories. `../corbusier-mockup`
shows the top-level `Makefile` target style for `check-fmt`, `typecheck`,
`lint`, and `test`. `../wildside-mockup-v2a` shows the broader browser
application validation surface, including `test:a11y`, `test:e2e`,
`lint:ftl-vars`, and semantic CSS enforcement.

## Constraints

- `axinite/` remains the source of truth for website content, structure,
  imagery, CSS intent, and route inventory. The migration may reshape the files
  inside `axinite/`, but it must not move the product source of truth into an
  unrelated top-level directory.
- Default locale is `en-GB`, and all authored prose in the application and plan
  stays in British English unless it is translation content for another locale.
- The application must remain a client-rendered SPA. Do not introduce a server
  rendering framework such as SolidStart unless the user explicitly asks for
  that change later.
- The implementation must use SolidJS, Kobalte, daisyUI v5, Tailwind CSS v4,
  TanStack Query, i18next, and Fluent. These are not suggestions.
- Use Kobalte for behaviour-heavy interactive primitives such as dialogs, tabs,
  menus, popovers, disclosures, and listboxes. daisyUI provides visual tokens
  and component skinning, not the accessibility contract.
- Keep semantic HTML and semantic classes as hard requirements. Utility classes
  may still be used for layout composition, but interactive, repeated, or
  stateful elements must expose named semantic classes with reusable state
  styling.
- The app must be fully typechecked. `any`, untyped JSON plumbing, or
  route/state holes are not acceptable as a migration shortcut.
- Every visible user string must come from i18next plus Fluent resources. No
  hardcoded English may remain in production UI code after migration.
- Arabic must be a first-class right-to-left locale. Direction must drive the
  document root, layout primitives, icons that imply direction, and tests.
- The migration must not pretend that unimplemented backend features exist. If
  the backend does not expose a stable contract yet, that surface stays behind a
  feature flag and degrades to a clear "not available" state.
- The app must satisfy WCAG 2.2, not merely "look accessible". Keyboard access,
  focus order, focus visibility, target size, status announcements, colour
  contrast, reduced-motion handling, and language/direction metadata all count.
- The plan must preserve the repository's existing documentation gates
  (`make markdownlint`, `make nixie`) and extend rather than bypass the current
  top-level quality gates.

## Tolerances (exception triggers)

- Scope: if the implementation requires moving the source of truth out of
  `axinite/`, stop and escalate.
- Dependencies: if the migration needs more than four new runtime dependencies
  beyond the required stack and the minimum PWA/testing glue, stop and justify
  each one before continuing.
- Backend contract drift: if the backend integration requires API semantics not
  present in `../axinite/docs/front-end-architecture.md` or
  `../axinite/docs/rfcs/0009-feature-flags-frontend.md`, stop and align the
  contract first.
- Accessibility waivers: if any flow appears to require an accessibility
  exception or a disabled rule to ship, stop and escalate instead of carrying a
  waiver by default.
- Locale quality: if any locale is left with placeholder English in user-facing
  screens at milestone completion, the milestone is not done.
- Testing: if a feature cannot be covered by unit or behavioural tests, stop
  and document the gap before implementation continues.
- Route parity: if preserving the current route inventory requires dropping or
  renaming an existing surface, stop and get approval before making that break.
- Time: if route-parity work reveals that a single milestone hides two or more
  independent subsystem migrations, split the milestone before continuing.

## Risks

- Risk: the backend currently serves embedded static assets rather than a
  history-API SPA shell.
  Severity: high
  Likelihood: high
  Mitigation: treat backend hosting changes as an explicit integration stream.
  Keep the mock-up build deployable as a static app first, then add a backend
  embedding checklist for `../axinite`.

- Risk: not every mock-up surface has a corresponding implemented backend
  endpoint today.
  Severity: high
  Likelihood: high
  Mitigation: design a typed feature-flag registry early and require a flag for
  every route section or action whose backend contract is absent, unstable, or
  read-only.

- Risk: daisyUI examples can encourage presentation-first markup that weakens
  semantics.
  Severity: medium
  Likelihood: high
  Mitigation: put Kobalte in charge of interaction semantics, lint for semantic
  classes, and add accessibility tests for every primitive-heavy flow.

- Risk: locale expansion to ten languages, including Arabic RTL, can expose
  layout assumptions deep in the design.
  Severity: high
  Likelihood: high
  Mitigation: adopt logical CSS properties and direction-aware semantic classes
  from the first shell milestone, and run RTL snapshots and behavioural tests
  before route migration is considered complete.

- Risk: a PWA service worker can cache authenticated assets too aggressively and
  create stale or unsafe behaviour around live agent state.
  Severity: high
  Likelihood: medium
  Mitigation: cache only the shell, static assets, locale bundles, and clearly
  read-only data. Treat API, auth, and SSE as network-first or uncached.

- Risk: SolidJS reactivity and TanStack Query solve different state problems,
  and a confused boundary will create duplication.
  Severity: medium
  Likelihood: medium
  Mitigation: write the state model down before route work begins. Use
  TanStack Query for request/response server state, and use Solid signals,
  stores, or resources for local UI state and streaming state.

## Target architecture

The SPA should live inside `axinite/` as a Vite-based Solid workspace. The
current page-specific HTML files become migration references rather than the
runtime architecture. The target directory shape should be close to this:

```plaintext
axinite/
  package.json
  tsconfig.json
  vite.config.ts
  public/
    icons/
    manifest.webmanifest
  src/
    app/
      root.tsx
      router.tsx
      providers/
    features/
      chat/
      memory/
      jobs/
      routines/
      extensions/
      skills/
      logs/
      debug-flags/
    lib/
      api/
      feature-flags/
      i18n/
      pwa/
      streaming/
      accessibility/
    styles/
      index.css
      semantic.css
      themes.css
  tests/
    unit/
    behaviour/
    a11y/
    e2e/
  tools/
    semgrep-semantic.yml
    semantic-lint.config.json
```

The application shell should use TanStack Router nested routes so the top bar,
status strip, locale switcher, debug controls, and any persistent logs access
live once at the shell level. Child routes should preserve the current product
areas instead of inventing a new information architecture:

1. `/chat`
2. `/memory`
3. `/jobs`
4. `/routines`
5. `/extensions`
6. `/skills`
7. `/logs` or a shell-level logs panel if route-backed logs is cleaner

TanStack Query should own request-driven data such as threads, memory listings,
jobs, routines, extension inventories, settings, and feature flags. SSE-driven
or long-lived state such as chat streaming, logs streaming, and connection
status should live in a typed streaming layer that can invalidate relevant
queries on events without pretending those streams are ordinary queries.

Kobalte should supply the primitives for dialogs, tabs, switches, selects,
popovers, menus, disclosures, and any focus-managed composite widgets. DaisyUI
v5 and Tailwind CSS v4 should provide the skin, tokens, and layout system.
Semantic classes should wrap the final rendered structure so the codebase reads
like product UI rather than a wall of inline utility tokens.

## Internationalization and localization design

The SPA must ship with these locales from the first production-ready release:

- `en-GB`
- `fr`
- `de`
- `it`
- `nl`
- `pl`
- `hi`
- `ja`
- `zh-CN`
- `ar`

`ar` must declare `rtl`, and all other listed locales start as `ltr`. Create a
typed `SUPPORTED_LOCALES` registry modelled on the sibling repositories, with
`code`, `label`, `nativeLabel`, and optional `direction`. That registry must
drive the locale selector, the document `lang`, the document `dir`, the
manifest language metadata where possible, and any layout branches that cannot
be expressed by logical CSS alone.

Use i18next as the runtime orchestrator and Fluent (`.ftl`) as the source
format for user-visible copy. All UI strings, route titles, button labels,
field descriptions, status text, empty states, error text, and accessibility
labels must come through this pipeline. Use Fluent because several required
locales benefit from richer grammatical flexibility than plain key-value
strings. The `projectfluent.org` guide explicitly frames Fluent as a format for
complex natural-language concepts, which makes it the right default rather than
an optional embellishment.

Locale detection should follow a deterministic order. Start with explicit
user selection in the URL or local storage, then fall back to browser language
matching, and finally to `en-GB`. The selector must persist the choice and must
be test-covered for locale switching, fallback, and direction changes.

All CSS and layout code must use logical properties and direction-safe utility
choices wherever possible. Prefer `text-start`, `ms-*`, `me-*`, logical
padding, and semantic wrappers over hardcoded `left` and `right` assumptions.
Directional icons that imply previous/next or open/close must flip correctly in
RTL.

## Feature flags and debug mode

Feature flags are not an afterthought in this migration. They are the safety
valve that keeps the mock-up honest while the backend catches up.

Build a typed flag system around three concepts:

1. A canonical flag registry in `src/lib/feature-flags/registry.ts` that names
   every gated surface and documents its owner, default, and backend contract.
2. A runtime loader that fetches `GET /api/features` after authentication, as
   required by the RFC.
3. A debug override layer that is disabled by default and can be enabled only
   in a deliberate debug mode.

The debug mode should be available in development builds and in an explicit
preview mode such as `?debug-flags=1` or an equivalent guarded toggle. It must
show, for each flag, the server value, any local override, and the effective
value. It must also allow clearing overrides. This is important because the
debug surface is not just a toy switchboard; it must help maintainers see
whether a screen is hidden because the backend disabled it or because the
preview environment lacks the subsystem entirely.

Flag categories should be route-level and action-level. Candidate early flags
include route visibility for Jobs, Routines, Extensions, Skills, and logs, plus
action-level flags for editable memory, restart controls, summarization flows,
TEE attestation surfaces, and any catalogue or installation workflow that the
backend has not stabilized yet. Final flag names must respect the RFC naming
constraints: lowercase ASCII, digits, and underscores.

Production behaviour must never depend on local debug overrides. Debug mode is a
maintainer tool, not a release contract.

## Accessibility and semantic styling

WCAG 2.2 compliance must be designed into the architecture rather than patched
in later. The migration should adopt the following operational rules:

- Every interactive primitive must be keyboard-reachable and must expose visible
  focus states that satisfy WCAG 2.2 focus appearance expectations.
- No modal, drawer, popover, or menu may ship without tested focus management,
  escape handling, and return-focus behaviour.
- Dragging-only interactions are forbidden unless an equivalent click or
  keyboard path exists.
- Pointer targets and touch affordances must satisfy the target-size success
  criteria for common controls.
- Status changes such as connection state, upload completion, save success,
  errors, and long-running job activity must be announced through appropriate
  live regions where a sighted-only change would otherwise hide the update.
- Colour contrast and reduced-motion support must be enforced in both LTR and
  RTL themes.

The styling rule is simple: use daisyUI and Tailwind for composition, but name
product concepts semantically. A chat thread list item should not stay as a
copy-pasted stack of fifteen utility classes. It should become something like
`.chat-thread-list__item`, with state variants and shared utility extraction in
`semantic.css` or feature-local CSS modules. This follows the enforcement model
already used in the sibling repositories and prevents the Solid migration from
turning into utility noise.

## PWA behaviour

This site should be a real PWA, but only within the constraints of a live,
authenticated agent product.

Use a Vite-compatible PWA plugin so the SPA can emit:

- `manifest.webmanifest`
- a service worker
- app icons
- installability metadata

The service worker should cache the shell, fonts that are actually shipped with
the app, icons, locale bundles, and read-only static assets. It should not
cache authenticated API responses or SSE streams in a way that risks stale
agent state. Where offline support is not safe, the UI should say so plainly.
An offline shell that can explain "the Axinite runtime is unavailable" is
valuable. A fake offline chat or stale job screen is not.

The PWA milestone is complete only when installability works, the manifest is
valid, the shell loads from cache, and the offline behaviour is explicit rather
than undefined.

## Implementation stream

The migration should proceed in bounded streams rather than as one rewrite.

### Stream 1: Establish the Solid workspace inside `axinite/`

Replace the browser-loaded Tailwind and page-local HTML runtime with a typed
SolidJS and Vite application. Add TypeScript strictness, Tailwind CSS v4,
daisyUI v5, Kobalte, TanStack Router, TanStack Query, the i18next plus Fluent
stack, and the minimum PWA tooling. Preserve existing imagery, typefaces, and
design tokens long enough to avoid visual churn while the architecture changes.

The key acceptance point for this stream is that one Solid route shell can
render a placeholder version of each current product area, compile cleanly, and
switch routes without page reloads.

### Stream 2: Build the shared shell and cross-cutting providers

Create the application shell, providers, and support layers:

- router provider
- query client
- i18n provider
- feature-flag provider
- theme and semantic CSS entry points
- connection and streaming services
- a11y helpers for announcements and focus policies

This is where locale selection, direction switching, logs entry point,
connection status, and debug-flag entry should move into one shell-level
layout. Do not migrate feature pages deeply until this shell is stable.

### Stream 3: Migrate route families one by one

Migrate each current page family into a feature module while preserving route
identity and semantics:

1. Chat, because it exercises the heaviest mix of routing, streaming, prompts,
   dialogs, and status feedback.
2. Memory, because it exercises tree navigation, editing states, markdown
   rendering, and breadcrumb semantics.
3. Jobs and Routines, because they exercise structured server state and status
   polling.
4. Extensions and Skills, because they exercise catalogues, filters, file
   upload, install flows, and feature gating.
5. Logs as either a first-class route or a shell panel, depending on which
   model best preserves keyboard navigation and route clarity.

Each migrated route must reach behavioural parity before the next route family
begins. "Parity" means navigation, keyboard flow, localization, semantics,
feature gating, and test coverage, not just visual resemblance.

### Stream 4: Connect to backend contracts cleanly

Introduce a typed API layer that matches the documented backend surfaces in
`../axinite/docs/front-end-architecture.md`. This layer should isolate:

- authenticated fetch helpers
- typed request and response models
- SSE setup and reconnection
- gateway status polling
- feature-flag loading
- optimistic and pessimistic update rules per feature

Where the backend contract is missing or incomplete, add flags and explicit
placeholder states instead of mock data hidden inside production paths.

This stream also owns the eventual backend-hosting checklist for `../axinite`:
history fallback to `index.html`, static asset embedding or packaging,
manifest/service-worker delivery, and the `GET /api/features` endpoint from the
RFC.

### Stream 5: Enforce quality gates and remove static-page leftovers

After route parity is achieved, remove the obsolete multi-page HTML runtime and
leave one canonical Solid code path. Add or extend repository gates so the app
cannot regress on typing, semantics, localization, accessibility, or route
behaviour.

This stream is only done when the old page-specific HTML implementation is no
longer the runtime path, the new tests are authoritative, and the repository
quality gates fail correctly when semantic or localization regressions are
introduced.

## Validation and enforcement

Validation must mirror the neighbouring repositories rather than invent a looser
 standard. At minimum, the migration should add or extend top-level commands so
the following can be run from this repository:

```plaintext
make check-fmt
make typecheck
make lint
make test
make test-a11y
make test-e2e
make lint-ftl-vars
make semantic
make markdownlint
make nixie
```

The implementation behind those targets should follow the examples already in
`../corbusier-mockup` and `../wildside-mockup-v2a`:

- format and lint the Solid application with Bun-driven tooling
- run strict TypeScript checks with no emit
- run unit and behaviour tests in a browser-like environment
- run separate accessibility tests with axe
- run Playwright end-to-end tests
- lint Fluent variable usage so translations cannot silently drift
- enforce semantic styling rules through semgrep, stylelint, and project-local
  semantic class checks

Test coverage expectations are concrete:

- Unit tests cover pure functions such as locale metadata, flag resolution,
  route guard logic, query key construction, and formatters.
- Behaviour tests cover interactive components with keyboard and pointer flows,
  using `@solidjs/testing-library` and user-event style helpers.
- Accessibility tests cover each route shell and each primitive-heavy component
  family with automated axe checks and focused assertions for screen-reader
  names and focus order.
- End-to-end tests cover auth entry, route navigation, locale switching,
  Arabic RTL rendering, feature-flag debug mode, and one full flow per major
  product area.

The migration is not complete if tests only assert rendering. They must assert
behaviour.

## Acceptance criteria

The approved implementation should be considered complete only when all of the
following are true:

1. `axinite/` is a SolidJS SPA rather than a collection of standalone HTML
   pages.
2. Route navigation between the current product areas occurs without full page
   reloads.
3. All required locales are present, selectable, and test-covered.
4. Arabic sets `dir="rtl"` and the layout remains usable and semantically
   correct.
5. Every unavailable backend surface is guarded by a feature flag, and debug
   mode can inspect and override flags locally without changing production
   defaults.
6. Interactive primitives rely on Kobalte behaviour, not ad hoc DOM scripting.
7. The app is fully typechecked and fails CI on type regressions.
8. Semantic class enforcement is active and prevents utility sprawl from
   becoming the default style.
9. Unit, behaviour, accessibility, and end-to-end tests all pass.
10. The PWA manifest and service worker are valid, installability works, and
    offline behaviour is explicit and safe.

## Progress

- [x] 2026-03-25 19:13 GMT: Confirmed this work is on branch
  `solidjs-translation`, not `main`.
- [x] 2026-03-25 19:18 GMT: Reviewed the backend design reference in
  `../axinite/docs/front-end-architecture.md`.
- [x] 2026-03-25 19:19 GMT: Reviewed the frontend feature-flag RFC in
  `../axinite/docs/rfcs/0009-feature-flags-frontend.md`.
- [x] 2026-03-25 19:24 GMT: Reviewed validation patterns in
  `../corbusier-mockup` and `../wildside-mockup-v2a`.
- [x] 2026-03-25 19:31 GMT: Verified the current `axinite/` route inventory and
  confirmed the existing static product areas to preserve.
- [x] 2026-03-25 19:36 GMT: Verified current upstream stack direction against
  official docs for Solid, Kobalte, TanStack Query and Router, daisyUI v5,
  Tailwind CSS v4, and Fluent.
- [x] 2026-03-25 19:43 GMT: Authored the first draft of this execution plan in
  `docs/execplans/solidjs-translation.md`.
- [x] 2026-03-25 20:05 GMT: User approved execution by asking to proceed with
  implementation of this plan, to use `css-view` and Playwright for
  validation, and to import the `ff` test and lint pipeline pattern from
  `../wildside-mockup-v2a/package.json`.
- [x] 2026-03-25 20:11 GMT: Re-verified that the repository still contains only
  the original static prototype and no pre-existing Solid or Bun workspace to
  preserve.
- [x] 2026-03-25 20:34 GMT: Replaced the static prototype runtime with a
  Vite-based Solid SPA rooted in `axinite/`, preserving the existing route
  inventory as typed SPA routes and semantic shell components.
- [x] 2026-03-25 20:35 GMT: Wired i18next plus Fluent locale bundles for
  `en-GB`, `fr`, `de`, `it`, `nl`, `pl`, `hi`, `ja`, `zh-CN`, and `ar`, and
  added document-level `lang`/`dir` handling plus an in-shell locale picker.
- [x] 2026-03-25 20:36 GMT: Added typed feature-flag loading, local debug
  overrides, a logs dialog, and gateway-status polling seams so hidden runtime
  surfaces remain explicit instead of implied.
- [x] 2026-03-25 20:38 GMT: Imported the Bun-based `ff` validation pipeline
  shape from `../wildside-mockup-v2a/package.json` and adapted the repository
  scripts, tests, semantic checks, and top-level `Makefile` targets to the SPA.
- [x] 2026-03-25 20:42 GMT: Validated the rendered SPA with Playwright route,
  dialog, and RTL coverage; validated computed RTL shell cascade values with
  `css-view`; and passed `make ff`, `make markdownlint`, and `make nixie`.
- [x] 2026-03-25 20:47 GMT: Committed the migration as `0cd73d5`
  (`Translate the Axinite mockup into a Solid SPA`).
- [x] 2026-03-25 20:48 GMT: Pushed `solidjs-translation` to
  `GitHub: leynos/axinite-mockup`. The push output did not include a web
  URL.
- [x] 2026-03-25 21:04 GMT: Verified the user design-regression report against
  the current code. The original static route pages still exist in
  `axinite/*/index.html`, while the SPA currently renders a generic route hero
  and card grid that discards most of the source layout.
- [x] 2026-03-25 21:11 GMT: Restored the shared shell chrome to a closer match
  for the original mock-up by bringing back the flatter glass topbar, underline
  nav treatment, denser control strip, mesh background, and route watermark
  atmosphere.
- [x] 2026-03-25 21:12 GMT: Ported the Chat and Memory routes to dedicated
  Solid preview components with the original sidebar/composer and
  tree/breadcrumb grammar instead of the generic hero/card layout.
- [x] 2026-03-25 21:12 GMT: Revalidated the first restoration slice with
  `make ff`, Playwright browser screenshots of Chat and Memory, and `css-view`
  snapshots showing the restored glass chrome and sidebar surfaces under RTL.
- [x] 2026-03-25 21:13 GMT: Committed the first restoration slice as `7bd0954`
  (`Restore the original shell, chat, and memory design`).
- [x] 2026-03-25 21:13 GMT: Pushed `solidjs-translation` to
  `GitHub: leynos/axinite-mockup`. The push output did not include a web
  URL.
- [x] 2026-03-25 21:28 GMT: Audited the original `jobs/` and `routines/`
  static pages against the current SPA and confirmed both routes were still
  falling back to the generic hero/card preview.
- [x] 2026-03-25 21:34 GMT: Ported Jobs and Routines into dedicated Solid
  dashboard previews with summary cards, semantic data tables, and detail
  panels that preserve the original operator-facing hierarchy more closely than
  the generic route template.
- [x] 2026-03-25 21:44 GMT: Revalidated the second restoration slice with
  `make ff`, `make markdownlint`, `make nixie`, Playwright screenshots of Jobs
  and Routines, and `css-view` captures confirming `direction: rtl` on the
  Arabic routes plus the restored dashboard grid and glass panel surfaces.
- [x] 2026-03-25 21:47 GMT: Committed the second restoration slice as
  `38edcc5` (`Restore the original jobs and routines design`).
- [x] 2026-03-25 21:48 GMT: Pushed `solidjs-translation` to
  `GitHub: leynos/axinite-mockup`. The push output did not include a web
  URL.
- [x] 2026-03-25 21:55 GMT: Audited the original `extensions/` and `skills/`
  static pages against the current SPA and confirmed both routes still
  rendered through the generic hero/card fallback.
- [x] 2026-03-25 22:02 GMT: Ported Extensions and Skills into dedicated Solid
  catalogue previews with installed-card grids, intake panels, inventory
  lists, and a bundled-skill detail view that preserve the original route
  family shape more closely than the generic route template.
- [x] 2026-03-25 22:11 GMT: Revalidated the final restoration slice with
  `make ff`, `make markdownlint`, `make nixie`, Playwright screenshots of
  Extensions and Skills, and `css-view` captures confirming `direction: rtl`
  on the Arabic routes plus the restored catalogue grids and intake panels.
- [x] 2026-03-25 22:14 GMT: Committed the final restoration slice as `0e6bebc`
  (`Restore the original extensions and skills design`).
- [x] 2026-03-25 22:14 GMT: Pushed `solidjs-translation` to
  `GitHub: leynos/axinite-mockup`. The push output did not include a web
  URL.
- [x] 2026-03-25 23:44 GMT: Verified that the deployed SPA still assumed a
  root base path. `vite.config.ts` used Vite's default `/`, the router still
  relied on root-absolute redirects, and prefixed GitHub Pages deploys would
  therefore break route and asset resolution.
- [x] 2026-03-25 23:47 GMT: Wired an explicit `/axinite-mockup/` base path
  through Vite, the router, shell link generation, and the generated PWA
  manifest/workbox configuration.
- [x] 2026-03-25 23:52 GMT: Revalidated the prefixed deploy path with
  `make ff`, `make build`, `node scripts/test-build.mjs`, Playwright against
  `/axinite-mockup/chat`, and `css-view` confirming `direction: rtl` on the
  prefixed Arabic route.
- [x] 2026-03-25 23:54 GMT: Committed the base-path fix as `058478e`
  (`Configure the GitHub Pages base path explicitly`).
- [x] 2026-03-25 23:54 GMT: Pushed `solidjs-translation` to
  `GitHub: leynos/axinite-mockup`. The push output did not include a web
  URL.
- [x] 2026-03-26 09:17 GMT: Verified the user's local-build 404 report against
  the current output. The prefixed Vite `base` was correct for GitHub Pages,
  but `scripts/postbuild-routes.mjs` still emitted only root-level route
  folders, so a static `dist/` preview could not satisfy
  `/axinite-mockup/...` asset and route requests.
- [x] 2026-03-26 09:24 GMT: Updated the postbuild output to mirror the built
  SPA under `dist/axinite-mockup/` and turned the old root route folders into
  compatibility redirects so existing `/chat`-style preview URLs now forward
  into the deploy-prefixed app instead of failing.
- [x] 2026-03-26 10:01 GMT: Audited the restored Extensions route against the
  original static `axinite/extensions/index.html` page and confirmed the
  largest remaining drift was visual: the shared catalogue intro and oversized
  detail panel still made the route read more like a generic library screen
  than the flatter operator-focused extensions page.
- [x] 2026-03-26 10:08 GMT: Reworked the Solid Extensions route toward the
  original visual grammar by flattening the section wrappers, tightening the
  installed-extension cards, keeping the dual WASM and MCP intake panels
  side-by-side, and replacing the large bottom detail block with a compact
  selected-extension strip.
- [x] 2026-03-26 10:18 GMT: Audited the current Skills route against the
  original static `axinite/skills/index.html` page and confirmed the main
  visual regression was structural clutter: the always-open selected-skill
  viewer and shared wrapping form-row styles made the route read as one long
  catalogue stack instead of search, inventory, and intake surfaces.
- [x] 2026-03-26 10:24 GMT: Reworked the Solid Skills route to behave more like
  the original page by hiding search results until a query exists, removing the
  default always-open skill viewer, keeping search and install rows aligned on
  desktop, and applying route-specific Skills styling on top of the shared
  catalogue primitives.
- [x] 2026-03-26 10:41 GMT: Verified the missing-translation report against the
  current locale bundles. The site was not emitting raw message IDs; instead,
  every non-`en-GB` locale silently lacked the newer route strings for chat
  controls plus the Jobs, Routines, Extensions, and Skills surfaces, which
  caused mixed translated-and-English pages.
- [x] 2026-03-26 10:48 GMT: Gated the locale picker and runtime to locales with
  complete site coverage, currently `en-GB` only, and added an explicit Fluent
  coverage check so incomplete locale bundles cannot be advertised silently.
- [x] Restore the original shared shell chrome and route watermark treatment in
  the SolidJS app.
- [x] Port the Chat and Memory layouts into dedicated Solid preview components
  that match the original static pages more closely than the generic route
  cards.
- [x] Revalidate the Jobs and Routines restoration slice with `make ff`,
  Playwright screenshots, and `css-view`, then commit and push before moving to
  Extensions and Skills.
- [x] Revalidate the Extensions and Skills restoration slice with `make ff`,
  Playwright screenshots, and `css-view`, then commit and push the final route
  restoration work.
- [x] Commit and push the first restoration slice before
  continuing to Jobs, Routines, Extensions, and Skills.
- [x] Commit and push the gated implementation changes.
- [x] Replace the static runtime with a Vite-based Solid SPA inside `axinite/`.
- [x] Wire i18next plus Fluent, the supported-locale registry, document
  direction handling, and locale-aware route copy.
- [x] Add typed feature-flag loading plus debug overrides and expose them in
  the shell.
- [x] Import the Bun-based `ff` validation pipeline shape and adapt the repo
  scripts, tests, and Makefile targets to the new SPA.
- [ ] Validate the rendered SPA with Playwright and `css-view`, then gate,
  commit, and push the implementation changes.

## Surprises & Discoveries

- The corrected feature-flag RFC path is
  `../axinite/docs/rfcs/0009-feature-flags-frontend.md`, not
  `../docs/rfcs/...`. The plan should keep that exact path to avoid future
  confusion.
- The current mock-up is not one page with internal tabs. It is six separate
  route directories under `axinite/`, each with its own HTML document. The SPA
  migration therefore replaces the runtime architecture rather than simply
  swapping libraries.
- The backend reference explicitly says the real frontend is currently embedded
  as static assets in the Rust binary, so backend hosting changes are a real
  integration stream, not a deployment footnote.
- daisyUI v5 is explicitly aligned with Tailwind CSS v4 and now supports CSS
  plugin import from the stylesheet, which fits a Vite-based Solid workspace
  cleanly.
- Kobalte's own introduction frames it as an accessible SolidJS UI toolkit with
  unstyled primitives, which confirms the intended split: Kobalte for behaviour,
  daisyUI for visual system.
- The current repository scripts are still static-site copies and smoke tests.
  Importing the `ff` pipeline therefore means replacing the build, lint, and
  test wiring rather than layering new targets on top of an existing SPA stack.
- Keeping route identity on a purely static preview host still needs extra
  build output. A history-based SPA served from static hosting will otherwise
  404 on deep links, so the build must emit route-folder `index.html` copies or
  an equivalent fallback strategy.
- The initial SPA render exposed a real localization race: static locale assets
  were not being served because Vite's `publicDir` still pointed at the
  repository root, and shell primitives mounted before `i18nReady` kept their
  raw Fluent message IDs. Pointing `publicDir` at `axinite/public/` and waiting
  for `i18nReady` before the first render removed that untranslated first-paint
  state.
- The repo's original `.gitignore` only covered `dist/` and screenshots. A Bun
  and Playwright workflow also needs `node_modules/`, `test-results/`, and
  `tmp/` ignored, otherwise a routine install/test cycle pollutes the worktree.
- `css-view` confirmed that the Arabic route applies computed `direction: rtl`
  through the document root and key shell containers such as `.shell-topbar`,
  `.shell-nav`, and `.shell-controls`, with logical paddings preserved.
- The original visual design is still recoverable from source, because the
  legacy static pages and shared stylesheet remain in `axinite/`.
- The biggest design regression is structural rather than purely stylistic: the
  first Solid pass replaced six different route bodies with one generic layout.
- The remaining route families decompose naturally after Chat and Memory:
  Jobs/Routines share one dashboard grammar, and Extensions/Skills share one
  catalogue grammar.
- Restoring the shared chrome first had an outsized visual effect. The flatter
  topbar, left rail, and watermark treatment immediately make the SPA read like
  the original prototype again even before all route bodies are ported.
- The first restoration slice can keep the Solid/i18n/runtime seams intact. The
  old layout grammar translated cleanly into dedicated route components without
  reopening the earlier router or feature-flag work.
- Jobs and Routines are structurally close enough that one shared dashboard
  styling grammar works well, but they still need separate route components
  because their table semantics and detail emphasis differ.
- Extensions and Skills likewise share a route family, but the shared grammar
  is catalogue-plus-intake rather than dashboard-plus-detail.
- The GitHub Pages deployment seam is broader than just Vite `base`: the shell
  can still escape the prefix if fallback links, router redirects, or generated
  manifest URLs remain root-absolute.
- The local static preview seam is broader than route copies alone. Once the
  SPA emits prefixed asset and service-worker URLs, the postbuild output also
  has to mirror the deploy tree itself or provide explicit compatibility
  redirects from the older root-level preview routes.
- The Extensions and Skills routes should not share identical visual density
  just because they share a broad catalogue family. The original Extensions
  page is notably flatter and more operator-panel driven than the bundle-heavy
  Skills view, so route-specific styling is necessary to preserve that
  distinction.
- The Skills route needs interaction-driven density in a way Extensions does
  not. Leaving the selected-skill viewer open by default and letting shared
  wrapping form rows govern the search/install controls makes the page feel
  visually broken even when the underlying grid math is correct.
- Fluent fallback hid a real product problem here. Missing locale messages did
  not surface as raw keys; they fell back to English inside otherwise localized
  pages, which made the defect easy to miss unless the route was checked in a
  non-default locale.

## Decision Log

- Decision: keep the migration inside `axinite/` rather than creating a sibling
  frontend root.
  Rationale: repository guidance says `axinite/` is the source of truth for the
  site. Keeping the new SPA there preserves that contract.

- Decision: prefer plain Solid plus Vite over SolidStart.
  Rationale: the backend already owns hosting, auth, and API orchestration. A
  client-rendered SPA is sufficient and keeps the integration surface smaller.

- Decision: use TanStack Router in addition to TanStack Query.
  Rationale: the user requested TanStack Query, and a single-page migration also
  needs typed route structure. The Solid adapter for TanStack Router gives the
  route typing and nested shell model needed for parity with the current page
  families.

- Decision: keep the explicit `/axinite-mockup/` Vite base for GitHub Pages and
  fix the static preview output rather than reverting to a root base path.
  Rationale: the deploy prefix is a real requirement. The correct compatibility
  fix is to emit a mirrored prefixed tree and root-level redirect stubs so the
  same build works both on GitHub Pages and when `dist/` is served directly for
  preview.

- Decision: give Extensions route-specific catalogue modifiers instead of
  forcing it to share the same layout density as Skills.
  Rationale: the original static page uses flatter section framing, denser
  installed cards, and a compact operator-centric hierarchy. Preserving that
  appearance is easier and safer with route-specific CSS hooks layered on top
  of the shared catalogue primitives.

- Decision: keep Skills search results and the selected-skill viewer
  interaction-driven instead of always rendering them in the default route
  state.
  Rationale: the original static page hides search results until the operator
  searches and uses a separate viewer surface for skill details. Matching that
  behaviour reduces clutter and restores the intended visual hierarchy without
  dropping the current Solid data fixtures.

- Decision: expose only locales with full site coverage in the runtime and
  picker until the missing route strings are translated.
  Rationale: the defect was mixed-language UI caused by incomplete bundles, not
  missing fallback machinery. Hiding incomplete locales is the only honest
  user-facing fix short of translating hundreds of missing strings in one pass,
  and the new Fluent coverage check now enforces that contract.

- Decision: model feature flags as a first-class provider and test target.
  Rationale: the backend does not yet expose every mock-up feature. Treating
  flags as infrastructure prevents the frontend from drifting away from reality.

- Decision: require Fluent for all locale resources rather than mixing formats.
  Rationale: one message format keeps localization consistent and supports the
  required language set better than ad hoc string tables.

- Decision: split streaming state from TanStack Query state.
  Rationale: chat and logs use SSE-like flows. Trying to force those into plain
  query semantics would create unnecessary indirection and stale-state bugs.

- Decision: implement the first executable delivery as a route-preserving SPA
  foundation rather than attempting live backend parity for every control in
  one patch.
  Rationale: the repository starts from static HTML only. The first honest
  milestone is a typed, localized, validated Solid runtime that preserves the
  product areas, shell behaviour, direction handling, and feature-flag seams so
  backend integration can follow without another front-end rewrite.

- Decision: keep the existing static HTML documents as short-term migration
  references during this first implementation pass.
  Rationale: they still contain the product copy, layout cues, and control
  inventory needed to preserve route intent while the Solid code path becomes
  the runtime.

- Decision: block the first client render on `i18nReady`.
  Rationale: several shell primitives mounted before the locale bundles were
  available and kept raw Fluent IDs in accessible names, which broke both first
  paint quality and Playwright addressability.

- Decision: point Vite `publicDir` at `axinite/public/`.
  Rationale: locale bundles and static assets are part of the `axinite/` source
  tree, so leaving `publicDir` at the repository root silently broke runtime
  localization fetches.

- Decision: reopen the execplan after the runtime migration shipped.
  Rationale: the user correctly identified that visual parity with the original
  mock-up still needs explicit tracked work rather than an informal follow-up.

- Decision: restore shared shell chrome and the Chat/Memory routes first.
  Rationale: those surfaces carry the strongest original identity and provide
  the clearest test of whether the SPA can preserve the source design language.

- Decision: keep the remaining four route families on the generic fallback for
  this first restoration slice.
  Rationale: the shared chrome plus Chat/Memory restoration is already enough
  to prove the translation approach, and it keeps the first recovery commit
  small enough to validate and review cleanly.

- Decision: use one shared dashboard/table semantic CSS layer for Jobs and
  Routines, but keep route-specific Solid components.
  Rationale: the two routes share summary-card and dense-table framing, but
  their column meaning, status labelling, and detail emphasis are different
  enough that another generic abstraction would repeat the first regression.

- Decision: use one shared catalogue/intake semantic CSS layer for Extensions
  and Skills, but keep route-specific Solid components.
  Rationale: both routes combine installed inventory with install/search
  surfaces, yet Extensions centers on external capability registration while
  Skills centers on catalogue discovery and bundle inspection.

- Decision: hardcode `/axinite-mockup/` as the Vite and runtime base path for
  this repository's deploy target, and derive router/shell/PWA URLs from that
  same source.
  Rationale: the repo is intended for GitHub Pages sharing as a project page,
  so treating the non-root prefix as optional leaves navigation and assets one
  config drift away from breaking.

- Decision: keep real SPA entry points at both `dist/<route>/index.html` and
  `dist/axinite-mockup/<route>/index.html` rather than generating redirect
  stubs at the root route paths.
  Rationale: GitHub Pages project URLs can serve `/axinite-mockup/chat` from
  the top-level `chat/index.html`, so redirect stubs self-loop on reload while
  duplicated SPA entry points preserve direct navigation under both layouts.

## Outcomes & Retrospective

- Shipped a Vite-based Solid SPA under `axinite/` with typed TanStack Router
  routes for Chat, Memory, Jobs, Routines, Extensions, and Skills; Kobalte
  shell primitives; semantic CSS; typed feature-flag seams; and gateway status
  polling placeholders.
- Shipped Fluent locale bundles for the required ten-language set, including
  Arabic RTL, plus document `lang`/`dir` updates, a locale picker, and route
  copy that stays behind localization resources instead of hardcoded strings.
- Shipped Bun, Biome, Vitest, Playwright, Semgrep, Stylelint, and Fluent
  linting integration at the repository root, following the imported `ff`
  pipeline shape from `../wildside-mockup-v2a/package.json`.
- The plan was right to treat static-host deep links and feature-flag honesty as
  first-order concerns. `scripts/postbuild-routes.mjs` was needed immediately
  for static route copies, and the feature provider made it possible to keep
  hidden runtime surfaces explicit.
- The route-copy workaround needed one more iteration after the GitHub Pages
  base-path fix. Redirect stubs looked tidy, but they broke deep-link reloads
  on project pages; keeping duplicated SPA entry points is the safer short-term
  deploy shape for this repository.
- The plan under-estimated first-paint localization risk. The implementation
  needed both a corrected Vite `publicDir` and a render gate on `i18nReady` to
  avoid raw Fluent message IDs leaking into visible text and accessible names.
- No tracked risk required escalation during this pass. The main residual risk
  is still backend integration: the shell currently uses honest preview seams
  and status placeholders where live contracts do not yet exist.
- The most valuable validation gates were `make ff` for the integrated Bun
  quality stack, Playwright for translated route/dialog/RTL behaviour, and
  `css-view` for confirming computed RTL cascade values on the live shell.
- Before backend integration, tighten the remaining mocked gateway seams, then
  decide whether the Rust host will serve the generated SPA directly or adopt a
  different history-fallback strategy than the current static route copies.
- The first route-restoration slice landed in commit `7bd0954`, but the
  visual-restoration stream remains open until Jobs, Routines, Extensions, and
  Skills are translated away from the generic fallback layout.
