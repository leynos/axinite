# Adopt the SolidJS front-end as the default Axinite browser UI

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`, `Surprises & Discoveries`,
`Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work
proceeds.

Status: COMPLETE

## Purpose / big picture

Axinite currently serves a handwritten browser UI (one large `app.js`, plus
`index.html` and `style.css`) compiled into the Rust binary from
`src/channels/web/static/`. The sibling repository `axinite-mockup` proves out
a SolidJS single-page application (SPA) with typed API modules, TanStack
Router/Query, feature flags, localization, and a Bun mock backend that mirrors
the browser contract over JSON and Server-Sent Events (SSE).

After this change:

- The SolidJS SPA is the default browser UI served by the Axinite gateway. A
  developer who builds and runs the daemon and opens the gateway URL sees the
  SolidJS app, not the legacy shell.
- A developer can run the SolidJS UI **without the daemon** via one command
  (`make frontend-stub`, wrapping the Bun mock backend + preview server), with
  deterministic HTTP fixtures, deterministic SSE events, and runtime feature
  flags served over `GET /api/features`.
- The legacy shell remains embedded solely as an operator rollback path,
  selected by an explicit gateway-side switch, and is documented as
  transitional.

This implements Stages 1–3 (and part of 5) of RFC 0018
(`docs/rfcs/0018-solidjs-front-end-adoption.md`), using the mock backend as the
daemon-free stub runtime, and a minimal environment-variable subset of RFC 0009
(`docs/rfcs/0009-feature-flags-frontend.md`) for `GET /api/features`.

## Constraints

- The Rust gateway remains the authoritative production server; the browser
  stays same-origin on `/api/*` (RFC 0018 §1). No separate front-end service.
- Production packaging must keep working: one binary, assets embedded at
  compile time (preserves Dockerfile/wix behaviour, which copy no extra asset
  directories). `cargo build` must succeed **without** Bun/Node installed.
- The Bun mock backend must not become a second daemon: contract-focused
  fixtures only; no auth logic beyond ignoring tokens, no durable state.
- Do not adopt extra state libraries (Zustand, XState, Dexie) — RFC 0018 §7.
- No network calls to external services in tests; fixtures deterministic.
- Do not delete existing regression tests that assert legacy behaviour;
  update them to assert SolidJS behaviour instead.
- Commit gates: `make all` (check-fmt, lint, test, spelling) must pass, plus
  the new front-end gates added by this plan.
- British English (en-GB-oxendict) in prose and documentation.

## Tolerances (exception triggers)

- Scope: if replacing a Python e2e scenario requires building a **new** UI
  surface larger than ~300 lines (for example, a full pairing flow or TEE
  panel), stop and record the decision rather than building it; mark the
  scenario as legacy-gated with documented rationale.
- Dependencies: no new Rust crate dependencies are expected for asset
  embedding; if one becomes necessary (for example, `include_dir`), record it
  in the Decision Log. No new JS runtime dependencies beyond what
  `axinite-mockup` already uses.
- Iterations: if a gate still fails after 4 fix attempts, stop and escalate.
- Interface: changes to daemon route semantics (beyond adding
  `GET /api/features`) require escalation; the SPA adapts to the daemon, not
  the reverse.

## Risks

- Risk: the Python e2e suite (`tests/e2e/scenarios/`) asserts legacy DOM
  selectors and flows (chat, extensions, skills, tool approval, SSE reconnect,
  HTML injection) that the SolidJS UI does not yet cover with equivalent
  affordances.
  Severity: high. Likelihood: high.
  Mitigation: milestone 8 audits each scenario; update selectors where the
  SolidJS UI has the surface; where it does not, keep the scenario running
  against the legacy fallback UI (explicit env-var opt-in) with a tracking
  note, rather than deleting it.
- Risk: the SPA lacks gateway auth (gap analysis G1), so serving it as
  default against a token-protected daemon would ship a broken UI.
  Severity: high. Likelihood: certain until fixed.
  Mitigation: milestone 4 ports the legacy token boot flow (sessionStorage
  token, `Authorization: Bearer`, `?token=` for SSE) into the typed client.
- Risk: hashed Vite asset filenames complicate compile-time embedding.
  Mitigation: configure stable output filenames so `include_str!` continues to
  work with a fixed, small file set.
- Risk: checked-in build artefacts drift from source.
  Mitigation: `make frontend-build` regenerates; a freshness check compares a
  rebuild against the committed artefacts in CI/gates.
- Risk: contract drift between mock backend and daemon (gap analysis §13).
  Mitigation: milestone 3 fixes the known breaks (`LogEntry.target`, job
  prompt `{content, done?}`, extension install `{name, url?, kind?}`) in the
  shared `contracts.ts`, which the mock backend imports directly.

## Progress

- [x] (2026-07-19 10:20Z) Recon complete: legacy web channel, mockup repo,
  build/CI wiring (three parallel reconnaissance passes).
- [x] (2026-07-19 10:30Z) ExecPlan drafted.
- [x] (2026-07-19 10:55Z) M1: imported `axinite-mockup` as `web-src/`;
  isolated build + checks pass (commit 8fa5d2b9).
- [x] (2026-07-19 11:00Z) M2: root base path, stable asset filenames, PWA
  and GitHub Pages scaffolding removed, preview-server SPA fallback
  (commit 056d90cc).
- [x] (2026-07-19 11:05Z) M3: contract fixes (`LogEntry.target`,
  `JobPromptRequest { content, done? }`, full extension install shape) in
  `contracts.ts` + mock backend, pinned by
  `api-contract-alignment.test.ts` (commit 6fc69902).
- [x] (2026-07-19 11:10Z) M4: auth boot flow — token module, bearer
  injection, SSE query token, `AuthGate` with anonymous-probe bypass for
  the stub; localized in all ten locales (commit bae63224).
- [x] (2026-07-19 11:20Z) M5: gateway serves the built SPA by default
  (embedded `src/channels/web/static/solid/`); legacy behind
  `AXINITE_WEB_UI=legacy`; Make targets `frontend-build`/`frontend-verify`
  etc.; five Rust serving tests (commit 617fb6c0).
- [x] (2026-07-19 11:25Z) M6: env-var-driven `GET /api/features` in the
  gateway with compiled defaults mirroring the SPA registry
  (commit 44750c29).
- [x] (2026-07-19 11:30Z) M7: stub flags flattened to the RFC 0009 map
  with `FEATURE_FLAG_*` overrides; `MOCK_FAILURES` failure fixtures;
  in-process contract tests for HTTP shapes and SSE ordering
  (commit d0d43217).
- [x] (2026-07-19 11:45Z) Browser validation via Playwright MCP against the
  stub: initial load from HTTP fixtures, SSE-driven chat turn, flag toggle
  hiding the Skills nav entry, `MOCK_FAILURES` error state, clean console on
  the happy path. Three defects found and fixed with regression tests
  (nav flag gating, jobs error notice, SSE error-event JSON parsing)
  (commit 96466513).
- [x] (2026-07-19 11:50Z) css-view layout validation on all six routes (84 to
  284 nodes per route): no element extends past the viewport except the
  decorative `position: fixed` watermark; `scrollWidth == innerWidth` at
  1280, 768, and 375 px; the jobs table scrolls inside its own
  `overflow-x: auto` wrap.
- [x] (2026-07-19 12:00Z) M8: docs (`docs/solidjs-frontend.md`, transitional
  banner in `docs/front-end-architecture.md`, README pointer, web module
  CLAUDE.md route tables); `tests/web_static_app.test.mjs` still passes
  (targets the retained legacy assets); Python e2e conftest pins
  `AXINITE_WEB_UI=legacy` with rationale.
- [x] (2026-07-19 12:40Z) M9: gates green — `make check-fmt`, `make lint`
  (clippy plus whitaker after splitting `ui_assets.rs` out of
  `static_files.rs`), `make typecheck`, `make markdownlint`, `make nixie`,
  `make frontend-test` (45 unit + 2 a11y), `make frontend-verify`, full
  `cargo nextest --workspace` (4252 passed), web-channel subset re-run after
  the module split (148 passed), `node --test tests/web_static_app.test.mjs`
  (8 passed). CodeRabbit CLI reviewed the branch diff (121 files): zero
  findings, no rate limiting (commit 586983fe).

## Surprises & discoveries

- Observation: the repo already contains RFC 0018 — a full staged adoption
  plan for exactly this migration — plus RFC 0009 (feature flags) and
  `docs/solidjs-pwa-gap-analysis.md` (a precise inventory of contract breaks).
  Evidence: `docs/rfcs/0018-solidjs-front-end-adoption.md`.
  Impact: this plan follows RFC 0018's stages and treats the gap analysis as
  the contract-fix backlog.
- Observation: `GET /api/jobs/{id}/events` is documented as SSE in module
  docs but is actually paginated JSON; live job events arrive on the global
  `/api/chat/events` stream.
  Evidence: `src/channels/web/handlers/job_control/events.rs`.
  Impact: the stub and SPA must not invent a per-job SSE stream.
- Observation: the legacy UI never opens the WebSocket endpoint; it is
  SSE-only. Evidence: no `new WebSocket` in `app.js`.
  Impact: the SPA can standardize on JSON + SSE (RFC 0018 open question
  resolved for this migration).
- Observation: `tests/web_static_app.test.mjs` string-slices function bodies
  out of `app.js` — it breaks the moment the legacy file is no longer the
  primary UI source. Impact: migrate its assertions to the SolidJS skills
  module (milestone 8).

## Decision log

- Decision: proceed with implementation without a separate approval pause.
  Rationale: the commissioning task explicitly instructs end-to-end delivery
  and validation and the session is autonomous; that instruction is treated as
  standing approval per the execplans skill's standing-instruction clause.
  Date/Author: 2026-07-19, Claude.
- Decision: place the browser workspace at `web-src/`.
  Rationale: matches the existing repo convention that `*-src/` directories
  are standalone source trees outside the Cargo workspace (`channels-src/`,
  `tools-src/`), which is exactly the relationship the SPA has to the binary.
  Date/Author: 2026-07-19, Claude.
- Decision: commit built SPA artefacts (small, stable-named set under
  `web-src/dist/`) and embed them with `include_str!`/`include_bytes!`, rather
  than building JS from `build.rs` or serving from disk.
  Rationale: keeps `cargo build` hermetic (no Bun requirement for Rust-only
  contributors, Docker, wix, release builds), preserves the one-binary
  operational model RFC 0009 explicitly prizes, and mirrors the current
  checked-in-asset model. Drift is controlled by a freshness gate.
  Date/Author: 2026-07-19, Claude.
- Decision: gateway-side entrypoint selection via `AXINITE_WEB_UI=legacy`
  environment variable rather than full `FeatureFlagRegistry` routing.
  Rationale: RFC 0009's deployment-scoped settings persistence is a large
  Rust work-item; an env-var switch satisfies RFC 0018's rollback requirement
  now and can be upgraded to registry-backed routing when RFC 0009 lands. The
  new UI is the default either way.
  Date/Author: 2026-07-19, Claude.
- Decision: keep the Python e2e suite (`tests/e2e/`) on the legacy shell by
  pinning `AXINITE_WEB_UI=legacy` in its conftest, rather than rewriting the
  scenarios against the SolidJS DOM in this change.
  Rationale: the suite binds to roughly eighty legacy selectors (tab bar,
  approval overlay, configure modal, `?token=` boot) whose SolidJS
  equivalents either differ structurally or do not exist yet; rewriting
  exceeds the per-scenario tolerance in this plan. The SolidJS UI has its
  own browser-level coverage (the `web-src` Playwright suite plus the
  Playwright MCP validation recorded above). Migrating the Python scenarios
  route-by-route is explicit follow-up work aligned with RFC 0018 Stage 4.
  Date/Author: 2026-07-19, Claude.
- Decision: implement `GET /api/features` now, minimally, resolving only
  `FEATURE_FLAG_<UPPER_SNAKE_NAME>` environment variables over compiled
  defaults (no settings-table persistence, no deployment header requirement).
  Rationale: the SPA's flag provider already consumes this endpoint; leaving
  it 404 would silently mask integration risk (gap G2). The env-var layer is
  the top of RFC 0009's precedence chain, so this is forward-compatible.
  Date/Author: 2026-07-19, Claude.

## Outcomes & retrospective

Delivered against the original purpose: the SolidJS SPA is the default
gateway UI (embedded, one-binary model preserved); the legacy shell survives
only behind `AXINITE_WEB_UI=legacy`; `make frontend-stub` runs the UI without
the daemon with deterministic HTTP fixtures, SSE streams, runtime flag
overrides, and failure fixtures; contract, unit, behaviour, a11y, browser
(Playwright MCP), and layout (css-view) validation all passed, and CodeRabbit
reported zero findings.

What browser validation earned beyond the test suites: three real defects
(nav ignoring route flags, silent list-failure state, SSE error-event JSON
crash) that no existing suite covered — each now has a regression test.

Remaining follow-up work:

- Rewrite the Python e2e scenarios (`tests/e2e/`) against the SolidJS DOM
  route-by-route (RFC 0018 Stage 4) and then retire the legacy shell and its
  assets (Stage 5), including `tests/web_static_app.test.mjs`.
- Implement the RFC 0009 settings-table/deployment-scoped flag layer beneath
  the env-var resolution in `handlers/features.rs`.
- Close the remaining UI parity gaps catalogued in
  `docs/solidjs-pwa-gap-analysis.md` (logs as a route, restart/TEE/pairing
  surfaces, chat media, jobs detail fidelity).

Lessons: the mockup's own e2e spec was stale against its components (chat and
memory headings), so imported suites need verification before trust; the
`typos` en-GB-oxendict gate interacts noisily with vendored front-end code
and needed a deliberate exclusion policy (generated artefacts, translations,
CSS syntax, vendored docs) rather than word-by-word fixes; and gate runs and
editing must not overlap in one worktree — a scrutineer pass ran while the
module split landed, which muddied its report.

## Context and orientation

Key current-state facts (verified by code inspection):

- Legacy assets: `src/channels/web/static/{index.html,style.css,app.js,favicon.ico}`
  embedded via `include_str!`/`include_bytes!` in
  `src/channels/web/handlers/static_files.rs` (`public_routes()` maps `/`,
  `/style.css`, `/app.js`, `/favicon.ico`).
- Auth: bearer token, constant-time compare (`src/channels/web/auth.rs`);
  `?token=` query fallback only for GET `/api/chat/events`,
  `/api/logs/events`, `/api/chat/ws`.
- SSE: `/api/chat/events` (event types `response, thinking, tool_started,
  tool_completed, tool_result, stream_chunk, status, job_started, job_message,
  job_tool_use, job_tool_result, job_status, job_result, approval_needed,
  auth_required, auth_completed, extension_status, image_generated, error,
  heartbeat`; Axum keep-alive every 30 s) and `/api/logs/events`
  (`event: log`, replays recent entries then streams).
- HTTP surface consumed by the legacy UI: chat (send/threads/history/new
  thread/approval/auth-token/auth-cancel), memory (tree/list/read/write/
  search), jobs (list/summary/detail/cancel/restart/prompt/events(JSON)/
  files), routines (list/summary/detail/runs/trigger/toggle/delete), skills
  (list/search/install/delete), extensions (list/tools/registry/install/
  activate/remove/setup), settings, logs level, gateway status, pairing,
  project file browser. Full inventory: recon notes in
  `docs/solidjs-pwa-gap-analysis.md` §2 and `src/channels/web/CLAUDE.md`.
- Mockup (`/home/leynos/Projects/axinite-mockup`): Bun workspace; SolidJS +
  TanStack Router/Query + Kobalte + Tailwind/DaisyUI + i18next/Fluent; typed
  API modules in `axinite/src/lib/api/` with all contracts in `contracts.ts`;
  feature-flag registry of 13 flags in `axinite/src/lib/feature-flags/` with
  `/api/features` fetch, localStorage overrides, and a `?debug-flags=1` debug
  panel; Bun mock backend (`mock-backend/src/server.ts`, port 8787) plus
  preview server (port 2020) proxying `/api/*`; `scripts/dev.ts` orchestrates
  mock API + `vite build --watch` + preview; vitest unit + a11y suites;
  Playwright e2e (`axinite/tests/e2e/app-shell.pw.ts`).
- Known contract breaks to fix (gap analysis): missing auth (G1),
  `/api/features` absent from gateway (G2), `LogEntry.source` vs daemon
  `target` (G3), job prompt `{prompt}` vs `{content, done?}` (G4), extension
  install narrowed to `{name}` (§11.1).
- Existing tests affected: `tests/web_static_app.test.mjs` (string-slices
  `app.js`), Python Playwright e2e in `tests/e2e/scenarios/` (drives legacy
  DOM against the real binary).

## Plan of work

M1 — Import the workspace. Copy `axinite-mockup` (from
`/home/leynos/Projects/axinite-mockup`, excluding `.git`, GitHub Pages
scaffolding, and mockup-repo docs that do not transfer) into `web-src/`.
Keep bun, biome, vitest, playwright configs. Run `bun install`,
`bun run check:types`, `bun run lint`, `bun run test` inside `web-src/` and
fix breakage caused by the move only.

M2 — Serving shape. Change the Vite base path to `/` (drop the
`/axinite-mockup` GH Pages base and the per-route MPA HTML duplication if it
exists only for Pages fallback), and configure stable build filenames
(`dist/index.html`, `dist/assets/app.js`, `dist/assets/app.css`, plus the
favicon) so compile-time embedding stays a fixed file list.

M3 — Contract fixes in `web-src/axinite/src/lib/api/contracts.ts` and the
mock backend: `LogEntry.target`, `JobPromptRequest { content, done? }`,
`InstallExtensionRequest { name, url?, kind? }`. Update the components and
fixtures that consume them, with unit tests.

M4 — Auth. Add a token boot flow to the SPA: an unauthenticated state that
prompts for the gateway token, stores it in `sessionStorage`, sends
`Authorization: Bearer` on `fetch`, and appends `?token=` to the two SSE
URLs. The mock backend ignores tokens (stub stays auth-free); a
`VITE_`-independent runtime check keeps the stub flow tokenless via a stub
flag (`/api/features` fixture) or by the mock accepting all requests.

M5 — Gateway integration. Add `make frontend-build` (bun build into
`web-src/dist/`, committed). Rewrite `static_files.rs` handlers to embed the
SolidJS artefacts as the default for `/`, `/assets/app.js`,
`/assets/app.css`, `/favicon.ico`; keep the legacy `index.html`/`app.js`/
`style.css` handlers reachable only when `AXINITE_WEB_UI=legacy` is set at
startup. Add a freshness check (`make frontend-verify`) comparing a rebuild
with the committed dist. Update `src/channels/web/CLAUDE.md`,
`docs/front-end-architecture.md`.

M6 — `GET /api/features` in the gateway: env-var-driven map, compiled
defaults for the 13 SPA flags, unit tests. Wire into `protected_routes()`.

M7 — Stub runtime. `make frontend-stub` at repo root runs the Bun dev
orchestration (mock API + build watch + preview proxy on a documented port).
Mock backend gains env-driven flag overrides (`FEATURE_FLAG_<NAME>`), and its
fixtures are checked for determinism. Add contract tests: vitest suites that
start the mock server and assert each stubbed HTTP route's response shape and
the SSE routes' event order, headers, and heartbeat framing.

M8 — Regression migration. Replace `tests/web_static_app.test.mjs` string
slicing with tests against the SolidJS skills module. Audit each Python e2e
scenario: update selectors/flows to the SolidJS UI where the surface exists;
gate any legacy-only scenario on `AXINITE_WEB_UI=legacy` with a documented
parity note. Update docs (README/docs) with stub usage, stubbed routes, flag
overrides, and remaining-legacy rationale.

M9 — Validation. Playwright MCP smoke against the stub (load, HTTP-populated
state, SSE-driven update, flag toggle, failure fixture, console cleanliness);
css-view layout checks at representative viewports; `make all`; wasm/github
tool gates untouched; scrutineer runs the full gate set; CodeRabbit CLI on
the final diff (15-minute retry once on rate limit).

## Concrete steps

Representative commands (run from repo root unless stated):

    cd web-src && bun install && bun run check:types && bun run lint \
      && bun run test
    make frontend-build   # bun vite build -> web-src/dist (committed)
    make frontend-stub    # daemon-free stub: mock API + preview on :2020
    make frontend-verify  # rebuild and diff against committed dist
    make all              # Rust gates: check-fmt, lint, test, spelling

Expected: all commands exit 0; `make frontend-stub` prints the preview URL;
opening it shows the SolidJS shell populated from mock fixtures.

## Validation and acceptance

- Unit: vitest suites in `web-src` pass, including new tests for contract
  shapes, flag resolution, and SSE client parsing.
- Contract: new vitest integration tests boot the mock backend on an
  ephemeral port and assert route shapes and SSE event sequences
  deterministically.
- Rust: `cargo nextest run --workspace` passes; new `/api/features` and
  entrypoint-selection tests pass; `make all` green.
- Browser: Playwright MCP scenario list in M9 all observed manually via MCP
  against `make frontend-stub`; no fatal console errors.
- Red-Green-Refactor: each code milestone adds its failing test first (for
  example, the contract tests for `LogEntry.target` fail against the unfixed
  mock, then pass); where a change is pure asset wiring, the observable
  substitute is the gateway integration test asserting the served
  `index.html` contains the SolidJS mount point.
- CodeRabbit CLI reviewed the final diff (or the rate-limit protocol was
  followed and recorded).

## Idempotence and recovery

Every milestone is committed separately; `git revert` of a milestone commit
restores the previous state. `make frontend-build` is idempotent. The legacy
UI remains embedded and selectable via `AXINITE_WEB_UI=legacy` until a future
cleanup removes it (RFC 0018 Stage 5 completion).

## Interfaces and dependencies

- `web-src/axinite/src/lib/api/client.ts`: gains
  `setGatewayToken(token: string)`, bearer-header injection, and
  `createEventStream(url)` token-query support.
- `src/channels/web/handlers/static_files.rs`: serves SolidJS artefacts by
  default; exposes `fn ui_variant() -> UiVariant` (Legacy | Solid) resolved
  from `AXINITE_WEB_UI` once at startup.
- `src/channels/web/handlers/features.rs` (new):
  `GET /api/features -> JSON object {flag_name: bool}` resolving
  `FEATURE_FLAG_<UPPER_SNAKE_NAME>` env vars over compiled defaults.
- `mock-backend/src/state.ts`: flag defaults overridable via
  `FEATURE_FLAG_<NAME>` env vars at stub start.

No new Rust dependencies. No new JS dependencies beyond the mockup's own.
