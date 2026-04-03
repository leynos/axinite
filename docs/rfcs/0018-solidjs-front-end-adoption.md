# RFC 0018: Adopt the SolidJS browser front end

## Preamble

- **RFC number:** 0018
- **Status:** Proposed
- **Created:** 2026-04-01
- **Depends on:** RFC 0009
- **Related:** <https://github.com/leynos/axinite/blob/main/docs/front-end-architecture.md>,
  <https://github.com/leynos/axinite-mockup/blob/main/docs/axinite-v2a-frontend-architecture.md>,
  and <https://github.com/leynos/axinite-mockup/blob/main/docs/v2a-front-end-stack.md>

## Summary

Axinite's current browser surface is still the Rust-hosted gateway UI built from
checked-in `index.html`, `style.css`, and `app.js` assets. In parallel, the
`axinite-mockup` repository has proven out a new browser implementation as a
SolidJS single-page application (SPA) with typed API modules, semantic CSS,
TanStack Router, TanStack Query, localization, and a Bun mock backend that
mirrors the browser contract through JSON and Server-Sent Events (SSE).

This RFC proposes the staged adoption plan for that new front end inside the
main Axinite repository. The recommendation is not to turn the browser into a
separately deployed product, nor to make the Bun mock backend part of
production. Instead, Axinite should keep the Rust gateway as the authoritative
runtime and same-origin entry point, but replace the handwritten browser shell
with a built SolidJS application that is integrated into the main repository,
served by the gateway, and rolled out behind feature flags.

The migration should happen in explicit phases: freeze and verify the browser
contract, import the SolidJS application and its test toolchain, bridge the
application to the real gateway APIs, run both UIs behind RFC 0009 feature
flags, and cut over route by route until the legacy static assets can be
removed.

## Problem

### The current browser UI is expensive to evolve

The current front end is a handwritten browser shell generated from one
JavaScript file and a small set of embedded assets. That has kept dependencies
low, but it now imposes three concrete costs:

- UI changes require editing a large DOM-oriented script rather than working in
  route-local components and typed modules.
- The browser test surface is narrower than the SolidJS mockup already proves
  out with TypeScript-aware unit, accessibility, and end-to-end checks.
- The architectural centre of gravity for browser work has already moved to
  `axinite-mockup`, which risks a long-running divergence between the real
  product and the prototype that is informing future UI decisions.

### The mockup has demonstrated a better implementation shape, but not yet an adoption path

The new SolidJS front end already establishes several improvements:

- explicit route modules instead of one large rendering script,
- typed API contracts and client helpers instead of route-local fixture data,
- a state split built around TanStack Query, Solid signals, and narrow context
  providers,
- semantic CSS and localization infrastructure that fit Axinite's current UI
  direction, and
- same-origin `/api/*` assumptions that match the gateway model better than a
  separate browser-only API origin would.

What is still missing is a source-grounded plan for how that implementation
lands in the main repository without breaking Axinite's local-first deployment,
gateway authentication, streaming behaviour, and operator expectations.

### A direct replacement would create avoidable risk

A big-bang swap from the legacy browser shell to the new SPA would create
several risks at once:

- browser routes could drift from the existing JSON and SSE contracts,
- authentication and streaming semantics could regress while the UI is being
  replatformed,
- the feature-flag rollout work proposed in RFC 0009 could be bypassed instead
  of used for progressive delivery, and
- the team could end up maintaining a parallel Bun preview backend and a real
  Rust gateway contract without a disciplined compatibility boundary.

Axinite therefore needs an adoption sequence, not only a preferred frontend
technology.

## Current state

### Main-repository browser architecture

The current `docs/front-end-architecture.md` describes a browser gateway that:

- is served directly from the Rust application,
- embeds static assets from `src/channels/web/static/`,
- authenticates browser traffic against gateway-issued bearer tokens,
- uses JSON, SSE, and WebSocket-backed runtime paths where needed, and
- exposes browser-facing control surfaces for chat, memory, jobs, routines,
  extensions, skills, logs, settings, and gateway status.

The browser is therefore operationally tied to the same binary that runs the
agent runtime.

### Mockup browser architecture

The `axinite-mockup` repository proves a different implementation shape:

- a SolidJS SPA built by Vite,
- an explicit route tree,
- typed API contracts and route-local modules,
- TanStack Query plus Solid signals for state ownership,
- localization and semantic CSS as first-class concerns, and
- a Bun mock backend plus preview server that preserve same-origin `/api/*`
  browser requests during local development.

The mockup is intentionally not a production backend. Its Bun server exists to
exercise browser contracts during preview and testing, not to replace Axinite's
runtime semantics.

### Existing rollout mechanism

RFC 0009 already proposes a deployment-scoped feature-flag mechanism for the
web front end. That work is directly relevant here. Axinite needs a path where
the SolidJS application can be imported, tested, and exposed gradually without
forcing every deployment to switch immediately.

## Goals and non-goals

- Goals:
  - Adopt the SolidJS front end into the main Axinite repository.
  - Preserve the Rust gateway as the authoritative production runtime and
    same-origin browser entry point.
  - Reuse the mockup's component, routing, API-module, localization, styling,
    and test architecture where it fits Axinite's real product surface.
  - Define a phased migration from the current embedded static UI to the new
    SPA.
  - Use RFC 0009 feature flags to support progressive exposure and rollback.
  - Keep the browser contract explicit so the UI, mock backend, and real
    gateway do not drift silently.

- Non-goals:
  - Deploy the Bun mock backend in production.
  - Turn Axinite into a browser-only service with a separate API origin.
  - Replatform every browser transport onto a new protocol during the same
    change.
  - Replace Rust gateway ownership of authentication, authorization, and
    runtime mediation.
  - Commit Axinite immediately to the fuller v2a state stack such as Zustand,
    Dexie, or XState for all browser state.

## Proposed design

### 1. Keep the gateway, replace the browser implementation

Axinite should preserve the current product boundary:

- the Rust gateway remains the production HTTP server,
- the gateway remains responsible for authentication and runtime mediation,
- browser traffic continues to use same-origin `/api/*` routes, and
- the browser remains one interaction surface within the broader runtime rather
  than a separate service.

What changes is the implementation of the browser UI. Instead of serving
manually authored `index.html`, `style.css`, and `app.js` as the primary UI, the
gateway should serve a built SolidJS SPA.

This preserves Axinite's local-first deployment model while moving browser code
onto a component-oriented, typed, and better-tested implementation.

### 2. Treat the browser contract as a first-class migration seam

The migration should start by defining and verifying the browser contract that
the SolidJS application will consume. The real gateway remains the source of
truth for runtime semantics, but the browser-facing contract needs to become
more explicit in three places:

- typed request and response definitions inside the main repository,
- compatibility tests that exercise gateway JSON and SSE routes against those
  types, and
- a mock-backend update rule that keeps the Bun preview server aligned with the
  same typed contract rather than with hand-copied fixture shapes.

This contract-first step matters because the mockup currently proves the UI
architecture, while Axinite proper owns the real semantics. Adoption must join
those two truths rather than letting them drift.

### 3. Import the SolidJS application as an in-repo browser workspace

The SolidJS application should move into the main Axinite repository as a
dedicated browser workspace with its own package metadata, TypeScript config,
Vite build, and browser-focused test suite.

The exact directory name is open, but the structure should preserve the mockup's
major boundaries:

- application bootstrap and providers,
- explicit route tree,
- typed API modules,
- semantic styling,
- localization bundles, and
- browser-focused tests.

The goal is to keep browser implementation concerns local to the browser
workspace instead of scattering them back across Rust static-file directories.

### 4. Build the SPA as a distinct artefact, then serve it through the gateway

The new browser application should be built as a normal SPA artefact during the
Axinite build process, then served by the Rust gateway in production. This
creates a cleaner separation between authoring and serving:

- browser code is authored in TypeScript and SolidJS,
- the build outputs static assets,
- the Rust gateway serves those assets from the same origin as the protected
  APIs, and
- operators still deploy one Axinite system rather than a separate front-end
  service.

This RFC does not require one specific asset-packaging mechanism. Two viable
options are acceptable:

- embed the built artefacts into the binary at compile time, or
- ship the built artefacts with the application package and serve them from a
  controlled runtime location.

The decision criteria are local-first operability, reproducible builds,
cross-platform packaging, and simple operator workflow. Separate origin hosting
is not recommended.

### 5. Use a staged rollout rather than a big-bang cutover

Adoption should happen in six stages.

#### Stage 0: Freeze and verify the current browser contract

- Inventory the JSON and SSE routes the current browser uses.
- Introduce typed browser-contract definitions in the main repository.
- Add regression coverage that proves the gateway still satisfies those
  contracts.
- Align the mock backend with the same contract definitions or generated
  artefacts where practical.

#### Stage 1: Land the browser workspace and toolchain

- Import the SolidJS application structure from `axinite-mockup`.
- Wire package-manager, formatter, linter, type-check, unit-test,
  accessibility-test, and end-to-end-test commands into Axinite's maintainer
  workflow.
- Keep browser verification separate enough that UI iteration is fast, but make
  it mandatory in the aggregate quality gates before browser-affecting commits.

#### Stage 2: Bridge the SPA to the real gateway APIs

- Replace mock-only client assumptions with adapters for real gateway
  authentication and route semantics.
- Keep browser requests on same-origin `/api/*` paths.
- Prefer the existing JSON and SSE surfaces when they already satisfy the UI
  need; do not widen the transport mix without a specific problem statement.
- Preserve current browser affordances such as thread loading, memory browsing,
  jobs, routines, extensions, skills, logs, and feature-flag consumption.

#### Stage 3: Run both UIs behind feature flags

- Implement RFC 0009 with a deployment-scoped `solidjs_ui_enabled` flag held in
  `FeatureFlagRegistry`.
- Resolve that flag in the gateway at request time and use it to choose which
  browser entrypoint to serve. This RFC chooses gateway-side routing, not
  client-side bootstrapping, for entry-point selection.
- Keep the legacy UI available as a rollback path while the SPA is exercised
  against the real runtime.
- Use the flag to gate entry-point selection, not merely to hide a button.
- Keep `GET /api/features` for post-auth feature consumption inside the
  selected UI, but do not require the browser to fetch `/api/features` before
  it knows which entrypoint to bootstrap.

Example workflow:

1. An operator writes `feature_flag:solidjs_ui_enabled=true` for deployment
   `production`.
2. The settings handler updates `FeatureFlagRegistry` for `production`
   immediately, as required by RFC 0009.
3. The next browser request for that deployment reaches the gateway, which
   reads `solidjs_ui_enabled` from `FeatureFlagRegistry` and serves either the
   SolidJS entrypoint or the legacy static entrypoint.
4. After authentication, the selected front end calls `GET /api/features` to
   load the rest of the deployment-scoped UI flags for in-app behaviour.

Required changes:

- The gateway routing layer must identify the active deployment before serving
  the browser shell and must branch between the SolidJS and legacy entrypoints
  using `FeatureFlagRegistry`.
- RFC 0009 does not need per-entrypoint scope extensions for this stage, but
  its implementation must explicitly allow gateway-side consumers to read the
  deployment-scoped registry before the browser loads.

#### Stage 4: Migrate route surfaces incrementally

- Move the browser shell, navigation, and status surfaces first.
- Migrate route families one by one, starting with the least risky and most
  contract-stable screens.
- Treat chat, logs, and other streaming-heavy surfaces as later migrations,
  because they depend on the most nuanced runtime semantics.
- Keep parity notes explicit so operator-visible regressions are visible during
  review.

#### Stage 5: Cut over and remove the legacy static assets

- Switch the SolidJS front end to the default browser implementation.
- Remove the legacy `src/channels/web/static/` application assets once rollback
  is no longer required.
- Update the architecture and maintainer docs so the documented front-end
  implementation matches reality.

### 6. Preserve the mock backend, but narrow its job

The Bun mock backend should continue to exist after adoption, but only as a
preview and testing harness for the browser workspace. It should not become a
parallel production backend or a second source of truth for runtime behaviour.

Its responsibilities after adoption should be:

- support fast UI iteration without running the full Rust runtime,
- mirror the typed browser contract closely enough for local browser tests, and
- provide realistic JSON and SSE behaviour for preview and accessibility work.

Its responsibilities should not expand to include:

- production authentication,
- durable runtime state,
- operator configuration semantics,
- complete host orchestration, or
- independent product logic.

### 7. Keep state-management escalation disciplined

The imported front end should keep the mockup's current restraint:

- TanStack Query for fetched and synchronized server state,
- Solid signals and memos for local interaction state, and
- narrow context providers for cross-cutting browser concerns.

Additional state machinery such as `@tanstack/solid-form`, Zustand,
`solid-state`, or XState should be introduced only when concrete browser
behaviour becomes too implicit or too repetitive without them. Adoption of the
new front end is not, by itself, sufficient reason to widen the state stack.

## Requirements

### Functional requirements

- The gateway must remain able to serve the browser UI and its APIs from one
  origin.
- The imported SPA must cover the existing browser surface: chat, memory, jobs,
  routines, extensions, skills, logs, settings-aware status, and feature-flag
  consumption, unless a route is explicitly flagged as deferred.
- The browser must continue to operate against authenticated gateway routes and
  existing streaming semantics.

### Technical requirements

- Browser contracts must be explicit and testable.
- The browser build must be reproducible in repository gates and Continuous
  Integration (CI).
- The browser workspace must integrate with Axinite's Make-based verification
  workflow.
- Production serving must not require operators to stand up a second public
  web service just to use the browser UI.

### Operational requirements

- Operators must have a rollback path during rollout.
- Feature flags must support deployment-level enablement of the new UI.
- Documentation must make it clear which browser implementation is current,
  which is transitional, and which documents take precedence during migration.

## Compatibility and migration

The adoption plan should preserve compatibility at four layers.

### Product and operator compatibility

Axinite should remain a local-first, gateway-served browser application. The
operator experience should not change into a two-service deployment unless a
later RFC explicitly chooses that direction.

### API compatibility

The SolidJS application should target the existing browser-facing gateway APIs
where they are adequate. Where the new UI needs contract changes, those changes
should be made deliberately, documented, and tested so the browser and gateway
move together.

### Rollback compatibility

The legacy browser shell should remain available behind a feature flag until the
new SPA has passed real-runtime validation for the full adopted route set.

### Documentation migration

During the migration window:

- `docs/front-end-architecture.md` should continue to describe the currently
  shipped browser implementation, with explicit notes about the transitional
  state where necessary.
- Once cutover occurs, that document should be rewritten so it describes the
  SolidJS browser implementation in the main repository rather than the retired
  static shell.
- The mockup architecture documents should remain implementation history and
  design input, not the primary source of truth for shipped Axinite behaviour.

## Alternatives considered

### Keep the current handwritten browser shell

This keeps dependencies low, but it does not solve the maintainability,
testing, and architectural-drift problem. The existence of `axinite-mockup`
already shows that significant browser design work is happening elsewhere.
Choosing not to adopt it would preserve the split-brain state.

### Replace the browser with a separately deployed front-end service

This matches common web-application practice, but it conflicts with Axinite's
local-first deployment model and adds avoidable operational complexity. It
would also create a new cross-origin auth and streaming problem that the
current gateway design avoids.

### Copy mockup output back into `src/channels/web/static/`

This would let Axinite keep serving static assets from Rust, but it would throw
away most of the benefits of the new architecture. A compiled SPA should remain
authored as a browser workspace with its own build, tests, and local preview
story rather than being flattened back into hand-maintained static files.

### Adopt the fuller v2a state stack immediately

The broader v2a stack is useful context, but forcing Zustand, Dexie, and
XState into the initial adoption would widen scope without clear immediate
value. The browser should first land on the mockup's lighter Query-plus-signals
shape and earn more machinery later if product behaviour demands it.

## Open questions

- Should the built SPA be embedded into the binary at compile time or served
  from a packaged runtime asset directory?
- Which route family should be the first real-runtime migration target after the
  shell and status surfaces?
- Should the mock backend consume shared generated contract artefacts from the
  main repository, or should the repositories share contract definitions through
  a separate package?
- Does the existing gateway WebSocket path still serve a browser need that the
  adopted SPA should keep, or can browser interactivity standardize on JSON plus
  SSE for the first cutover?
- Where should feature-flag-driven entry-point selection live: inside the Rust
  gateway handlers, inside the browser bootstrap, or in both places as a
  defence-in-depth measure?

## Recommendation

Axinite should adopt the SolidJS front end from `axinite-mockup`, but do so as
an in-repo browser workspace served by the existing Rust gateway and rolled out
behind RFC 0009 feature flags. The migration should be contract-first,
same-origin, and route-by-route. The Bun mock backend should remain a preview
and test harness only. This path captures the architectural benefits already
demonstrated in the mockup without giving up the local-first deployment model
that defines Axinite's browser experience.
