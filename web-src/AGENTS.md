# Axinite Website Agent Guidance

## Scope

This repository contains the new SolidJS-based front end for Axinite, a
Rust-based autonomous AI agent. It is a standalone browser application backed by
a Bun mock backend for local preview.

For the full architectural rationale, see
[`docs/axinite-v2a-frontend-architecture.md`](docs/axinite-v2a-frontend-architecture.md).

## Repository Layout

- `axinite/` — SolidJS SPA source (components, API modules, i18n, styles,
  tests).
- `mock-backend/` — Bun in-memory mock backend serving realistic browser
  contracts via JSON and SSE.
- `scripts/` — dev orchestrator (`dev.ts`) and post-build helpers.
- `example-screens/` — reference HTML screens captured from the real Axinite
  browser gateway in `../axinite`.
- `docs/` — architecture documents and execution plans.
- `dist/` — Vite build output (not checked in).

## Architecture at a Glance

The front end is a SolidJS SPA built by Vite:

- **Providers:** TanStack Query, i18n (i18next + Fluent), feature flags.
- **Routing:** explicit route tree in `axinite/src/app/router.tsx` using
  TanStack Router.
- **Route components:** `ChatPreview`, `MemoryPreview`, `JobsPreview`,
  `RoutinesPreview`, `ExtensionsPreview`, `SkillsPreview`, each backed by typed
  API modules in `axinite/src/lib/api/`.
- **State:** TanStack Query for server state, Solid signals/memos for local
  interaction state, narrow context providers for cross-cutting concerns.
- **Transport:** JSON over `fetch` for request/response, SSE via `EventSource`
  for live updates (chat, logs).
- **Styling:** semantic CSS in `axinite/src/styles/semantic.css`.
- **Localization:** Fluent (`.ftl`) bundles in `axinite/public/locales/`.
  Only fully translated locales are exposed.

## Source of Truth

- `axinite/` is the source of truth for UI content, structure, classes,
  imagery, and CSS.
- `../axinite` (the Rust application) is the source of truth for runtime
  semantics, API contracts, and data models. The mock backend and typed
  contracts should stay aligned with it.

## Current Priorities

- Accuracy of browser contracts against the Rust backend.
- CSS, copy, imagery, semantic HTML, and semantic class names.
- Localization completeness across all supported locales.
- Test coverage (unit, a11y, e2e).

## What Not to Optimize Yet

Do not invest effort in large-scale build pipeline work. The deployment model
(GitHub Pages) is temporary and exists only so the prototype can be shared
before it is incorporated into the larger Axinite product.

## Preview Workflow

- Run `bun run dev` (or `make dev` if available) to start the full dev stack:
  mock API, Vite build watcher, and preview server.
- The preview server listens on port `2020` by default (configurable via
  `PREVIEW_PORT`). The mock API listens on port `8787` by default (configurable
  via `MOCK_API_PORT`).
- The mock API subprocess runs with `--watch`, so it auto-restarts when its
  source files change.
- When using Playwright for previewing, point it at `http://localhost:2020`.
- The dev server should not be started unless explicitly requested by the user.

## Commit Gating

Use Makefile targets for all verification. Run gates sequentially (build
caching benefits from serial execution).

- `make fmt` — format with Biome.
- `make check-fmt` — check formatting.
- `make lint` — lint with Biome.
- `make typecheck` — TypeScript type checking.
- `make test` — Vitest unit and behaviour tests.
- `make test-a11y` — accessibility tests.
- `make test-e2e` — Playwright end-to-end tests.
- `make lint-ftl-vars` — Fluent placeholder alignment and coverage.
- `make semantic` — semantic checks.
- `make ff` — full verification (all of the above).

## CSS Debugging

The `css-view` command is available for debugging. It produces a JSON dump of
the computed and de-duped CSS cascade for the site. See the `$css-view` skill.

## Imagery

Use the `$nanobanana` skill for generation of non-SVG imagery.
