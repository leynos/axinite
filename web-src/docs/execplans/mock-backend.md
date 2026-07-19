# Build a Bun mock backend for the SolidJS preview

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`, `Surprises & Discoveries`,
`Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work
proceeds.

Status: IMPLEMENTED

## Purpose / big picture

After this change, `bun run dev` starts a local demonstration stack that is
much closer to the real Axinite browser architecture than the previous static
preview. A Bun and TypeScript mock gateway serves API-correct JSON and
Server-Sent Events (SSE), the site rebuilds into `dist/` whenever source files
change, and a Bun preview gateway serves the built output while proxying
`/api/*` on the same browser origin for realistic static-site testing.

The important observable change is that the front end will stop reading route
content from in-component fixture arrays. Chat, memory, jobs, routines,
extensions, skills, gateway status, logs, and feature flags will all be backed
by the mock service. The user will be able to open the site, click through the
interactive controls, and see API-correct requests and responses without
running the real Rust backend.

## Approval gates

This plan began in `DRAFT` status and was approved by the user before
implementation started.

Approval means agreement on all of the following:

1. `bun run dev` will become a supervisor for three cooperating tasks: the Bun
   mock API, the static-build watcher, and the `http-server` static preview.
2. The mock backend will mirror the Rust gateway contracts in
   `../axinite/src/channels/web/types.rs` and the route layout in
   `../axinite/src/channels/web/handlers/`, but it will remain an in-memory
   demonstration service rather than a persistent clone of the Rust runtime.
3. Front-end preview components may be refactored substantially so that their
   data and mutating actions come from the mock API rather than front-end
   fixture constants.

If approval is withheld on a future revision, the correct next action is to
revise this document, not to start coding.

## Constraints

- `axinite/` remains the source of truth for copy, structure, semantic classes,
  imagery, and overall visual design. This work may rewire data flow, but it
  must not casually redesign the prototype.
- The delivered development loop must use Bun and TypeScript for the mock
  gateway and must serve the built static site with
  `bunx http-server -p <port> dist`, not Vite's development server.
- If `http-server` proves unable to support the required browser contract, the
  fallback must remain single-origin from the browser's point of view. Do not
  switch the front end to a separate runtime API base without explicit user
  approval.
- Presented values in the browser must not come from hard-coded front-end
  fixture arrays or route-local constants once the plan is implemented.
- The mock API must follow the Rust gateway's public browser contracts. The
  source material is `../axinite/src/channels/web/types.rs` and the handlers in
  `../axinite/src/channels/web/handlers/`.
- Feature-flag overrides from the front-end debug panel must remain available.
  Backend-provided defaults may change, but the local override mechanism in the
  browser must still layer on top.
- The mock backend is a potemkin village, not a digital twin. It must be rich
  enough to prove browser integrations, but it must not grow persistent
  storage, real authentication, or deep subsystem emulation without explicit
  approval.
- The existing GitHub Pages base-path handling must continue to work for built
  static assets and route entry points.
- Router-local state, Solid signals, and TanStack Query remain the default
  front-end state model. Introduce a heavier abstraction only when a concrete
  behaviour becomes too complex for that baseline to remain legible.

## Tolerances (exception triggers)

- Scope: if implementation appears likely to exceed roughly 40 files or 2,500
  net lines changed, stop and re-check the slice before continuing.
- Dependencies: adding up to two new packages is acceptable if they are small
  and directly serve the requested workflow. If more than two new packages are
  needed, stop and escalate.
- State model: if router-local state plus Solid signals plus TanStack Query are
  no longer sufficient for a route, document the exact pressure first, then
  choose from the approved escalation options in `docs/v2a-front-end-stack.md`
  and this plan instead of introducing an ad hoc pattern.
- Contracts: if the Rust browser contract is ambiguous or contradictory for a
  required endpoint, stop and document the specific mismatch before guessing.
- Transport: if `http-server` proxying proves unable to carry required SSE
  traffic after one direct experiment and one fallback attempt, stop and
  escalate before inventing a more elaborate server stack.
- Fidelity: if the front-end must diverge from Rust request or response shapes
  in order to keep moving, stop and escalate rather than silently inventing a
  mock-only API.
- Time: if the initial transport prototype consumes more than half a working
  day without proving static serving plus API routing, stop and reassess.

## Risks

- Risk: `http-server` may not proxy SSE cleanly enough for `/api/chat/events`
  or `/api/logs/events`.
  Severity: high
  Likelihood: medium
  Mitigation: validate the transport path early with a dedicated prototype, and
  if proxying is unreliable, switch to an alternative preview backend that can
  serve built `dist/` output and the mock API behind one browser origin.

- Risk: the current Solid preview components are still organised as static
  design previews, so API integration may require more state management work
  than the route count suggests.
  Severity: medium
  Likelihood: high
  Mitigation: move route data access into dedicated API modules and query hooks
  first, then rebind components incrementally route by route.

- Risk: some backend behaviours needed for a convincing demo, especially log
  streams, extension auth events, and job event history, are only partially
  represented in the current preview shell.
  Severity: medium
  Likelihood: medium
  Mitigation: include stubbed but API-correct behaviour for those flows, while
  keeping the implementation in-memory and explicitly bounded.

- Risk: the current built bundle assumes relative `/api/...` requests, which
  creates tension between static hosting on one port and a Bun API on another.
  Severity: high
  Likelihood: medium
  Mitigation: treat request routing as the first prototyping milestone and
  validate both root and prefixed paths before changing the rest of the app.

- Risk: feature flags can become confusing if some are controlled by server
  fixtures and others still by client defaults.
  Severity: medium
  Likelihood: medium
  Mitigation: define a single merge rule: server defaults from `/api/features`,
  then local debug overrides from browser storage, then route rendering.

- Risk: route behaviour may become awkward if complex forms, optimistic
  mutations, transient drafts, and event-stream coordination are all forced
  into route-local signals.
  Severity: medium
  Likelihood: medium
  Mitigation: start with the baseline router-plus-query model, but explicitly
  consider `@tanstack/solid-form`, `solid-state`, Zustand, and XState if a
  route grows beyond what is reasonable for a router-only solution.

- Risk: hand-rolled SSE lifecycle code for chat and logs may become brittle if
  reconnect logic, cleanup, and event fan-out are spread across components.
  Severity: medium
  Likelihood: medium
  Mitigation: if the event-stream wiring stops being trivial, consider
  `useEventSource` rather than building a bespoke subscription layer.

## Progress

- [x] (2026-03-26 12:21Z) Inspected the current `axinite-mockup` frontend data
  flow, scripts, and route components.
- [x] (2026-03-26 12:21Z) Inspected the upstream Rust gateway routes, handler
  modules, and DTO definitions in `../axinite`.
- [x] (2026-03-26 12:21Z) Drafted this ExecPlan in
  `docs/execplans/mock-backend.md`.
- [x] (2026-03-26 12:36Z) User approved implementation of this ExecPlan.
- [x] (2026-03-26 13:05Z) Prototyped the static-server plus mock-API transport
  path and confirmed that `http-server` proxying was not preserving the
  required `/api/*` request path for this setup.
- [x] (2026-03-26 13:21Z) Implemented the Bun mock backend, front-end API
  refactor, and same-origin preview gateway.
- [x] (2026-03-26 13:26Z) Validated the built preview, API flows, SSE streams,
  and representative CSS output on a free local preview port.
- [x] (2026-03-26 15:00Z) Followed up on the occupied-port startup failure so
  `bun run dev` now chooses a nearby free preview port when the default `2020`
  port is already in use, while still failing fast for an explicitly requested
  `PREVIEW_PORT`.
- [x] (2026-03-26 14:29Z) Added an optimistic chat-send update so the preview
  shows the just-sent user turn immediately and renders an assistant spinner
  while the response is still pending.

## Surprises & Discoveries

- Observation: the current Solid preview only fetches `/api/gateway/status` and
  `/api/features`; route data for chat, memory, jobs, routines, extensions, and
  skills still lives in front-end component constants.
  Evidence: `axinite/src/lib/api/gateway.ts` only exports
  `fetchGatewayStatus()` and `fetchRuntimeFeatureFlags()`, while
  `axinite/src/components/*.tsx` still define arrays such as `JOBS`,
  `ROUTINES`, `EXTENSIONS`, `INSTALLED_SKILLS`, and `MEMORY_TREE`.
  Impact: most of the implementation effort is a data-source migration, not a
  mock-server-only addition.

- Observation: the current development command is still `vite`, while built
  previews and Playwright expectations already distinguish between development
  and static preview ports.
  Evidence: `package.json` maps `"dev"` to `vite`, `vite.config.ts` sets
  `server.port` to `5173`, and `preview.port` to `4173`.
  Impact: `bun run dev` can be repurposed into the requested supervisor without
  conflicting with an established Bun backend loop.

- Observation: the Rust repository already documents the browser-facing API
  surface in both prose and code.
  Evidence: `../axinite/docs/front-end-architecture.md` enumerates the route
  surfaces and endpoints, while `../axinite/src/channels/web/types.rs`
  defines the DTOs.
  Impact: the mock service can be driven from documented contracts instead of
  inventing an ad hoc TypeScript API.

- Observation: `http-server` proxying did not preserve the required API path in
  this repository's preview flow.
  Evidence: a direct probe of `/api/gateway/status` through the static preview
  reached the mock backend as `GET /` instead of `GET /api/gateway/status`.
  Impact: the original transport assumption was wrong; the correct fix was a
  Bun preview gateway that serves `dist/` and proxies `/api/*` directly.

- Observation: local validation could not use preview port `2020` because that
  port was already occupied by an existing `caddy file-server` process in the
  environment.
  Evidence: `ss -ltnp '( sport = :2020 )'` showed `caddy` listening on port
  `2020`, so validation ran on `PREVIEW_PORT=43111` instead.
  Impact: the implementation works, but default-port validation remains
  environment-dependent whenever another process owns `2020`.

- Observation: the default developer workflow still felt brittle after the
  initial implementation because `bun run dev` aborted instead of selecting a
  free port when `2020` was occupied.
  Evidence: a direct `bun run dev` run failed with `EADDRINUSE` from
  `mock-backend/src/preview-server.ts` when `2020` was already bound.
  Impact: the supervisor now probes for a nearby free port by default, which
  keeps the local demo stack usable without changing the browser API contract.

- Observation: the chat preview still felt inert immediately after pressing
  Send because the just-submitted turn and the pending assistant state were not
  rendered until later history or SSE updates arrived.
  Evidence: `sendMutation` previously cleared the composer and updated status
  text, but the conversation view did not append a local user turn or any
  visible assistant waiting state until follow-up events arrived.
  Impact: the chat preview now performs an optimistic UI update with a pending
  user bubble and assistant spinner, which makes the route feel responsive
  while still deferring final content to the mock API and SSE stream.

## Decision log

- Decision: the mock service will mirror the Rust web gateway's browser
  contracts rather than inventing preview-specific JSON shapes.
  Rationale: the user explicitly wants Axinite API-correct integration proof,
  and the Rust repository already exposes the required DTOs and routes.
  Date/Author: 2026-03-26 / Codex

- Decision: `bun run dev` will become a process supervisor for the Bun API, the
  build watcher, and the static server instead of staying as a Vite dev-server
  alias.
  Rationale: this matches the requested workflow and forces the front end to
  exercise the same built static artefacts used in preview and deployment.
  Date/Author: 2026-03-26 / Codex

- Decision: the mock backend will remain in-memory and fixture-driven, but the
  fixtures will be mutable through API-correct requests.
  Rationale: this satisfies the acceptance criteria without turning the branch
  into a parallel backend product.
  Date/Author: 2026-03-26 / Codex

- Decision: the first implementation milestone will explicitly validate
  `http-server` request routing for both static routes and proxied API traffic,
  especially SSE.
  Rationale: if that transport assumption is wrong, the fallback must be a
  different preview backend topology rather than a different browser API
  contract.
  Date/Author: 2026-03-26 / Codex

- Decision: replace the originally proposed `http-server` preview hop with a
  Bun preview gateway that serves `dist/` and proxies `/api/*` to the mock API
  on the same origin.
  Rationale: the direct transport probe showed that the original `http-server`
  setup was not forwarding the request path correctly for this repository, and
  the plan already allowed a same-origin fallback if `http-server` proved
  unsuitable.
  Date/Author: 2026-03-26 / Codex

- Decision: when no explicit `PREVIEW_PORT` is provided, `bun run dev` should
  fall back to a nearby free port instead of aborting on an occupied `2020`
  port.
  Rationale: the repository guidance already expects port `2020` to be used for
  live previewing, so the development supervisor needs a safe default that does
  not fail merely because another local preview process already owns that port.
  Date/Author: 2026-03-26 / Codex

- Decision: state-management escalation should remain conditional, not
  automatic.
  Rationale: `docs/v2a-front-end-stack.md` places Zustand and XState in the
  fuller v2a stack rather than the baseline mockup layer. This rollout should
  first prefer router-local state, Solid signals, and TanStack Query, then
  reach for `@tanstack/solid-form`, `solid-state`, Zustand, or XState only
  when the resulting behaviour would otherwise become unreasonably implicit or
  fragile.
  Date/Author: 2026-03-26 / Codex

## Outcomes & retrospective

This plan now achieves the implemented outcome. The repository has an in-memory
Bun mock backend, a same-origin Bun preview gateway for the built site, and a
front end that reads chat, memory, jobs, routines, extensions, skills, logs,
gateway status, and feature flags through typed API modules instead of route
fixture arrays. Validation succeeded against a free preview port and confirmed
static routes, JSON endpoints, SSE flows, Playwright coverage, and
representative computed CSS for status pills in the jobs view.

## Context and orientation

The current repository is a SolidJS and Vite front-end preview rooted under
`axinite/`. The visual structure is already restored route by route, but the
data flow is still mostly static. The files below matter most:

- `package.json` currently defines `bun run dev` as `vite` and `bun run build`
  as `vite build && node scripts/postbuild-routes.mjs`.
- `vite.config.ts` builds the app with the GitHub Pages base path and outputs
  into `dist/`.
- `axinite/src/lib/api/gateway.ts` currently exposes only gateway-status and
  feature-flag fetch helpers.
- `axinite/src/lib/feature-flags/runtime.tsx` merges `/api/features` with local
  debug overrides from browser storage.
- `axinite/src/components/chat-preview.tsx`,
  `axinite/src/components/memory-preview.tsx`,
  `axinite/src/components/jobs-preview.tsx`,
  `axinite/src/components/routines-preview.tsx`,
  `axinite/src/components/extensions-preview.tsx`, and
  `axinite/src/components/skills-preview.tsx` still hold route content in local
  arrays and signals.
- `docs/v2a-front-end-stack.md` describes the intended state-management ladder:
  baseline Solid signals and route state first, then richer tools such as
  Zustand and XState in the fuller v2a application stack when behaviour
  becomes more complex.

The upstream Rust repository in `../axinite` is the contract source for the
mock backend. The most important files are:

- `../axinite/src/channels/web/types.rs` for request and response shapes.
- `../axinite/src/channels/web/handlers/chat.rs`,
  `chat_history.rs`, and `chat_threads.rs` for chat endpoints and SSE.
- `../axinite/src/channels/web/handlers/memory.rs` for memory list, read,
  write, search, and tree endpoints.
- `../axinite/src/channels/web/handlers/jobs.rs` and `job_control.rs` for jobs,
  job summaries, detail views, follow-up prompts, restart, cancel, file reads,
  and event history.
- `../axinite/src/channels/web/handlers/routines.rs` for routine listing,
  summary, detail, trigger, toggle, delete, and run-history contracts.
- `../axinite/src/channels/web/handlers/extensions/` for installed-extension
  listings, tool listings, registry search, install, activate, remove, and
  setup flows.
- `../axinite/src/channels/web/handlers/skills.rs` for installed-skill
  listings, search, install, and remove flows.
- `../axinite/docs/front-end-architecture.md` for the intended browser/back-end
  interaction model, including SSE and logs.

The route inventory that must move off front-end fixtures is:

1. Chat thread list, chat history, chat send, approvals, and SSE event flow.
2. Memory tree, list, file read, search, and write.
3. Job summary cards, job table, job detail, event history, file listing and
   reads, restart, cancel, and follow-up prompt actions.
4. Routine summary cards, routine table, routine detail, runs list, toggle,
   trigger, and delete.
5. Extension inventory, tool list, registry results, install, setup, activate,
   remove, and auth-related events.
6. Skill inventory, skill search results, install, and remove.
7. Gateway status, runtime feature flags, and log-stream demonstration data.

## Plan of work

The work should proceed in five stages, with a stop after each stage if the
validation for that stage does not pass.

Stage A is a transport and contract prototype. Add a small Bun entry point,
likely under `mock-backend/` or `scripts/`, that can answer a minimal
`/api/gateway/status` request and a minimal `/api/features` request. Pair that
with a supervisor script that can:

1. watch source files and rebuild `dist/`,
2. start the Bun mock API on a dedicated local port, and
3. start `bunx http-server -p 2020 dist`.

The purpose of Stage A is to prove one of two viable routing strategies:
`http-server` proxying unresolved `/api/*` requests to the Bun service, or a
single-origin alternative preview backend that serves the built `dist/`
artefacts itself while fronting the Bun mock API. Do not proceed until one of
those strategies is working for normal JSON requests and at least one SSE
endpoint.

Stage B is the contract and fixture scaffold. Create TypeScript interfaces that
mirror the Rust DTOs from `../axinite/src/channels/web/types.rs`. Keep them in
one obvious place, such as `mock-backend/src/contracts.ts` plus matching
front-end types under `axinite/src/lib/api/contracts.ts`, so the mock gateway
and the browser share the same schema vocabulary. Build in-memory fixture state
for each browser surface:

- several chat threads, turns, tool calls, and queued SSE events;
- a nested memory tree with readable and writable documents;
- jobs in pending, in-progress, completed, failed, and stuck states, with
  detail views, transitions, event history, and project files;
- routines in active, disabled, and failing states, with recent runs;
- extensions spanning setup-required, authenticated, active, pairing, and
  failed states, plus registry entries and registered tools;
- installed skills and searchable catalogue results;
- log events and log-level state; and
- feature-flag sets that demonstrate both visible and hidden surfaces.

Stage C is the Bun mock backend itself. Implement endpoint handlers grouped by
domain, mirroring the Rust handler layout as closely as is practical. The mock
service should support at least the following routes, using the Rust request and
response shapes:

1. `GET /api/gateway/status`
2. `GET /api/features`
3. `GET /api/chat/threads`
4. `POST /api/chat/thread/new`
5. `GET /api/chat/history`
6. `POST /api/chat/send`
7. `GET /api/chat/events`
8. `POST /api/chat/approval`
9. `GET /api/memory/tree`
10. `GET /api/memory/list`
11. `GET /api/memory/read`
12. `POST /api/memory/write`
13. `POST /api/memory/search`
14. `GET /api/jobs`
15. `GET /api/jobs/summary`
16. `GET /api/jobs/{id}`
17. `GET /api/jobs/{id}/events`
18. `GET /api/jobs/{id}/files/list`
19. `GET /api/jobs/{id}/files/read`
20. `POST /api/jobs/{id}/restart`
21. `POST /api/jobs/{id}/cancel`
22. `POST /api/jobs/{id}/prompt`
23. `GET /api/routines`
24. `GET /api/routines/summary`
25. `GET /api/routines/{id}`
26. `GET /api/routines/{id}/runs`
27. `POST /api/routines/{id}/trigger`
28. `POST /api/routines/{id}/toggle`
29. `DELETE /api/routines/{id}`
30. `GET /api/extensions`
31. `GET /api/extensions/tools`
32. `GET /api/extensions/registry`
33. `POST /api/extensions/install`
34. `POST /api/extensions/{name}/activate`
35. `POST /api/extensions/{name}/remove`
36. `GET /api/extensions/{name}/setup`
37. `POST /api/extensions/{name}/setup`
38. `GET /api/skills`
39. `POST /api/skills/search`
40. `POST /api/skills/install`
41. `DELETE /api/skills/{name}`
42. `GET /api/logs/events`
43. `POST /api/logs/level`

Stage D is the front-end data migration. Replace route-local fixture constants
with API modules and query hooks under `axinite/src/lib/api/`. Each route
component should fetch its own data and send mutating actions through typed
helpers. The acceptance rule for this stage is strict: no visible route value
may come from a front-end fixture constant. Local UI state is still fine for
presentation concerns such as selection, expanded rows, or currently open
dialogs, but the content being presented must originate from the mock backend.

State management in this stage should follow a deliberate escalation ladder:

1. start with route-local state, Solid signals, and TanStack Query;
2. use `@tanstack/solid-form` when complex form state or validation makes
   plain signals too implicit;
3. use `solid-state` or Zustand when shared client-side interaction state
   crosses route or component boundaries cleanly enough to justify a dedicated
   store; and
4. use XState only for genuinely multistep or event-heavy workflows such as
   extension auth orchestration, chat approval flows, or other logic that
   benefits from an explicit state machine.

Do not introduce these libraries merely because they are available. The plan is
to keep the simplest model that still makes the behaviour obvious.

For SSE specifically, start with ordinary `EventSource` wiring only if the
subscription remains simple and local. If chat or logs need enough reconnect,
teardown, or listener coordination that the code starts to sprawl, prefer
`useEventSource` over a bespoke helper stack.

During this stage, preserve the existing debug-panel feature flag overrides.
The merge order should stay:

1. backend defaults from `/api/features`,
2. browser overrides from `localStorage`, then
3. route visibility and interaction gating in the UI.

Stage E is hardening and validation. Add unit tests for the mock backend's
contract responses and mutation behaviour, update front-end tests to assert API
usage rather than fixture rendering, and extend Playwright to exercise the full
static preview stack driven by `bun run dev`. Validate a representative route in
`css-view` so the static build, not just component tests, is known-good.

## Concrete steps

Run every command from the repository root:
`/data/leynos/Projects/axinite-mockup`.

1. Prototype the dev supervisor.

   ```plaintext
   bun run dev
   ```

   Expected behaviour after implementation:

   - one process reports the Bun mock API port,
   - one process reports static rebuilds into `dist/`, and
   - one process reports `http-server` serving `dist` on port `2020`.

2. Validate the basic API wiring.

   ```plaintext
   curl http://127.0.0.1:<api-port>/api/gateway/status
   curl http://127.0.0.1:<api-port>/api/features
   ```

   Expected behaviour after implementation:

   - gateway status returns JSON with connection and version or preview fields,
   - feature flags return a JSON object or `{ "flags": { ... } }`, and
   - the browser shell uses those responses without local fixture fallbacks.

3. Validate the static site against the mock backend.

   ```plaintext
   bunx playwright test
   ```

   Expected behaviour after implementation:

   - Playwright boots through `bun run dev`,
   - route pages load from the static server on port `2020`, and
   - chat, memory, jobs, routines, extensions, and skills interact through the
     mock API successfully.

4. Validate the static-rendered CSS on a live route.

   ```plaintext
   css-view http://127.0.0.1:2020/axinite-mockup/jobs | jq '.matched[] | select(.selector == ".pill")'
   ```

   Expected behaviour after implementation:

   - the built route loads from the static server, and
   - the computed CSS still reflects the intended classes after the data-source
     migration.

5. Run the repository gates after the implementation.

   ```plaintext
   make check-fmt
   make lint
   make typecheck
   make test
   make test-a11y
   make test-e2e
   make markdownlint
   make nixie
   ```

   If the mock backend gains its own dedicated unit-test entrypoint, add it to
   this list and wire it into `make` and `package.json`.

## Validation and acceptance

Acceptance will be demonstrated by the following observable behaviour:

- `bun run dev` starts the mock API, the build watcher, and the static
  `http-server` preview without needing Vite's dev server.
- Opening `http://127.0.0.1:2020/axinite-mockup/chat` shows route data loaded
  from the mock backend rather than local component arrays.
- Chat send, memory write, routine trigger or toggle, extension setup or
  install, and skill search or install all emit API-correct requests and update
  the UI with returned data.
- The debug flag panel still allows local feature-flag overrides even though the
  backend now supplies the default flag set.
- Rich demo data exists for every route so the UI can showcase active, empty,
  successful, failing, and warning states without editing source files.

Quality criteria:

- Tests: front-end unit tests and Playwright tests pass against the mock stack.
- Lint and formatting: `make check-fmt`, `make lint`, and `make markdownlint`
  pass.
- Type safety: `make typecheck` passes for the Solid app and the Bun mock code.
- Docs/process: `make nixie` still passes after scripts and docs are updated.

## Idempotence and recovery

The dev supervisor must be safe to re-run. Killing `bun run dev` and starting
it again must rebuild `dist/`, restart the Bun API, and leave no persistent
state behind except ordinary browser storage such as feature-flag overrides.

The mock backend should keep its mutable state in memory only. Restarting the
process should restore the demo fixtures to a known baseline. That reset
behaviour is desirable here because it keeps manual testing repeatable.

If the build watcher or static server dies independently, the supervisor should
fail loudly and tear down sibling processes rather than leaving a half-live
stack behind.

If Stage A proves that `http-server` cannot carry the required API and SSE
traffic, the recovery path is to replace the preview-serving edge with a
different single-origin backend. The front-end request contract should stay on
relative `/api/*` paths.

## Artifacts and notes

Useful implementation artefacts to preserve during execution:

- a short transcript of `bun run dev` showing all three cooperating processes;
- one `curl` transcript for a JSON endpoint and one `curl -N` transcript for an
  SSE endpoint;
- a Playwright trace or screenshot proving the built route is using backend
  data; and
- one `css-view` JSON capture against the static server.

The most important note for whoever executes this plan is that the current UI
already looks roughly correct; the hard part is replacing local preview data
with backend-driven state without degrading the restored design.

## Interfaces and dependencies

Use Bun and TypeScript for the mock backend. A minimal dependency set is
preferred. `chokidar` is an acceptable choice for the build watcher if Bun's
native watch facilities are not sufficient. `http-server` may stay as a `bunx`
tool invocation unless making it a dev dependency materially improves
reliability.

For front-end state and behaviour abstraction, the allowed escalation options
are:

- `@tanstack/solid-form` for richer form state and validation;
- `solid-state` for explicit shared client-side stores in Solid-native code;
- Zustand for broader interactive client and UI state when the behaviour aligns
  with the fuller v2a stack; and
- XState for explicit workflow orchestration when simpler stores become too
  implicit.

For SSE handling, `useEventSource` is an allowed abstraction when direct
`EventSource` management would otherwise become repetitive or fragile.

These are options, not baseline requirements. Prefer the lightest tool that
keeps the resulting behaviour legible.

The mock backend should expose clearly named modules, for example:

- `mock-backend/src/contracts.ts` for shared request and response types,
- `mock-backend/src/state.ts` for mutable in-memory fixtures,
- `mock-backend/src/routes/chat.ts`, `memory.ts`, `jobs.ts`, `routines.ts`,
  `extensions.ts`, `skills.ts`, and `logs.ts` for route handlers, and
- `mock-backend/src/server.ts` for Bun server bootstrap.

The front end should gain corresponding client modules under
`axinite/src/lib/api/`, for example:

- `chat.ts`
- `memory.ts`
- `jobs.ts`
- `routines.ts`
- `extensions.ts`
- `skills.ts`
- `logs.ts`

Each module should export typed request helpers that mirror the Rust gateway's
DTOs. Those helpers should then be consumed by TanStack Query or direct mutation
handlers in the route components.

## Revision note

Created the initial draft of the mock-backend ExecPlan on 2026-03-26 after
inspecting the current `axinite-mockup` frontend data flow and the upstream
Rust gateway contracts in `../axinite`. This draft establishes the intended
development loop, the endpoint inventory, the bounded scope, and the required
approval gate before implementation begins.

Revised the draft on 2026-03-26 to remove the split-origin runtime-API-base
fallback. If `http-server` cannot support the required transport path, the plan
now calls for choosing an alternative single-origin preview backend instead.

Revised the draft again on 2026-03-26 to align its state-management guidance
with `docs/v2a-front-end-stack.md` and the user's follow-up. The plan now
explicitly treats `@tanstack/solid-form`, `solid-state`, Zustand, and XState as
conditional escalation options if router-local state plus Solid signals and
TanStack Query become unreasonably complex.

Revised the draft once more on 2026-03-26 to note `useEventSource` as an
explicit option for chat and log SSE handling when direct `EventSource`
management would otherwise become too bespoke.
