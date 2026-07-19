# SolidJS adoption follow-ups: flag persistence, UI parity, e2e migration

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`, `Surprises & Discoveries`,
`Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work
proceeds. It continues `docs/execplans/adopt-solidjs-ui.md` (COMPLETE), which
established the SolidJS SPA as the default gateway UI.

Status: IN PROGRESS

## Purpose / big picture

Three follow-up streams complete the SolidJS adoption:

1. **RFC 0009 persistence**: deployment-scoped feature-flag overrides stored
   in the database and served through `GET /api/features`, beneath the
   existing `FEATURE_FLAG_<NAME>` environment-variable resolution, updated at
   runtime through the settings API without a restart.
2. **UI parity** (docs/solidjs-pwa-gap-analysis.md): logs as a top-level
   route; gateway restart affordance; TEE attestation surface; pairing
   approval for WASM channels; chat media (image attach, generated images),
   auth cards, and job-start cards; jobs detail fidelity (tabs, transitions,
   live activity, done signal, file tree).
3. **e2e migration**: the Python Playwright suite in `tests/e2e/` drives the
   SolidJS DOM against the real daemon, and the `AXINITE_WEB_UI=legacy` pin
   is removed from its conftest.

Observable outcomes: an operator can toggle a flag for a deployment via
`PUT /api/settings/feature_flag:<name>` and see `GET /api/features` change
immediately; every legacy browser affordance listed above exists in the
SolidJS UI and is exercisable against the stub; `pytest tests/e2e/` passes
against the SolidJS UI.

## Constraints

- The daemon's wire contracts are authoritative; the SPA and mock adapt to
  them. New daemon surface is limited to the RFC 0009 flag layer.
- Both database backends (postgres, libsql) must gain the persistence layer;
  libsql migrations must not rebuild the `settings` table.
- The mock backend stays contract-focused: fixtures for the new routes and
  events, deterministic, no durable state.
- `make all`, frontend gates, and `make frontend-verify` stay green at every
  commit; commit after each milestone.
- Do not delete regression tests; rewrite them. The legacy shell and its
  assets stay in place (their removal is RFC 0018 Stage 5, out of scope
  here).
- en-GB-oxendict prose; UI strings localized in all ten locales.

## Tolerances (exception triggers)

- If the TEE external-host contract (`api.<domain>`) proves unverifiable
  beyond what `app.js` encodes, implement to the `app.js`-observed contract
  and stop there; do not invent server behaviour.
- If a Python e2e scenario cannot be expressed without a UI affordance this
  plan does not build (for example the legacy "Always" approval action if
  the daemon rejects it), record the decision and adapt the scenario rather
  than growing scope.
- Dependencies: no new Rust crates; no new JS runtime dependencies. Escalate
  otherwise.
- Iterations: a gate failing after 4 fix attempts stops the milestone.

## Risks

- Risk: e2e scenarios depend on injectable JS hooks (`showApproval`,
  `addMessage`, `connectSSE`, …) that a compiled SPA does not expose.
  Severity: high. Likelihood: certain.
  Mitigation: expose a deliberate, minimal test-hook object
  (`window.__axinite`) from the SPA — chat-stream close/reconnect and an
  `emitChatEvent` injector — always mounted (tiny, no security surface
  beyond what the browser console already allows) and documented.
- Risk: restart completion detection relies on SSE reconnection heuristics.
  Mitigation: mirror the legacy heuristic (tool_completed name `restart` or
  response containing "restart initiated", cleared on stream re-open); unit
  test the state machine with a fake EventSource.
- Risk: deployment-scoping is new infrastructure; requiring an
  `X-Deployment-Id` header on `GET /api/features` (as RFC 0009 writes it)
  would break the existing SPA fetch.
  Mitigation: resolve to a `"default"` deployment when the header is absent
  on reads; writes require the header per RFC. Recorded in the Decision Log.
- Risk: the jobs detail rework is the largest component change and could
  destabilize existing behaviour tests.
  Mitigation: keep the existing list/summary intact; build detail tabs as
  new components with their own tests before swapping in.

## Progress

- [x] (2026-07-19 13:10Z) Recon: e2e scenario inventory, settings/migration
  conventions, restart/TEE/media/pairing contracts, mock gaps.
- [x] (2026-07-19 13:20Z) ExecPlan drafted.
- [x] (2026-07-19 14:45Z) F1: RFC 0009 flag persistence (Rust):
  `feature_flag_overrides` table
  (postgres V18 + libsql incremental), `SettingsStore` deployment-flag
  methods, `FeatureFlagRegistry` in `GatewayState`, settings-handler
  interception, `/api/features` layering, tests.
- [x] (2026-07-19 14:05Z) F2: logs top-level route (SPA): `/logs` route +
  `route_logs` flag
  (registry, Rust defaults, mock), filters (level, target, text),
  pause/resume, clear, auto-scroll; gateway serves `/logs` shell; web-src
  e2e updated.
- [x] (2026-07-19 14:30Z) F3: stub surface extensions (mock backend):
  pairing routes and
  `pairing` activation fixture; `/api/chat/auth-token` + `auth-cancel`;
  `job_started` + `image_generated` emissions; `/restart` command fixture;
  images accepted on send; contract tests.
- [x] (2026-07-19 15:10Z) F4: chat media + auth cards + job cards (SPA):
  image staging
  (attach/paste, caps, previews), `images[]` on send, generated-image
  rendering, `auth_required` dispatch (OAuth card vs configure modal),
  `auth_completed` dismissal + toast, `job_started` card; `ChatSseEvent`
  union extended to the full daemon event set; tests.
- [x] (2026-07-19 15:40Z) F5: restart + TEE + pairing surfaces (SPA):
  restart button/modal/
  loader driven by `restart_enabled` and the `/restart` chat command; TEE
  shield + popover behind `surface_tee_attestation` (external-host client
  per the legacy contract); pairing rows + approve + stepper states on the
  extensions route with 10 s polling; tests.
- [x] (2026-07-19 16:10Z) F6: jobs detail fidelity (SPA):
  Overview/Activity/Files tabs,
  transitions timeline, `browse_url`, mode/kind, restart/prompt gating,
  persisted+live activity merge, done signal, recursive file tree; tests.
- [x] (2026-07-19 17:10Z) F7: Python e2e migration: `?token=` boot +
  `data-testid` contract in
  the SPA (`auth-screen`, `sse-status`, message roles), `window.__axinite`
  hooks, rewrite `helpers.py` SEL + all seven scenarios to the SolidJS DOM,
  drop `AXINITE_WEB_UI=legacy` from conftest, run the suite against the
  real daemon.
- [ ] F8: validation closure: frontend-build/verify, Playwright MCP +
  css-view on the new surfaces, full gates via scrutineer, CodeRabbit,
  retrospective.

## Surprises & discoveries

- Observation: gateway restart is not an HTTP endpoint — the legacy UI
  sends the `/restart` slash command through `POST /api/chat/send` and
  treats SSE reconnection as completion; `restart_enabled` comes from
  `AXINITE_IN_DOCKER`.
  Evidence: `app.js:145-248`, `src/tools/builtin/restart.rs`.
  Impact: the SolidJS restart affordance replicates the command flow; no
  daemon change needed.
- Observation: TEE attestation is served by a separate host
  (`https://api.<domain>/instances/{name}/attestation` and
  `/attestation/report`), derived from the browser hostname and inert on
  localhost.
  Evidence: `app.js:3583-3687`.
  Impact: the SPA client mirrors that contract; the stub does not model it
  (unit tests mock `fetch`).
- Observation: the mock backend lacks pairing, chat auth-token/cancel,
  `job_started`, `image_generated`, image acceptance, and any `/restart`
  behaviour — all needed before the SPA parity work can be exercised.
  Evidence: recon of `web-src/mock-backend/src/server.ts`, `state.ts`.
  Impact: F3 lands before F4–F6.
- Observation: `tests/e2e/test_extensions.py` mocks every API via
  `page.route()` interception and never touches the real daemon; the skills
  scenario deliberately hits the live ClawHub registry with self-skips.
  Impact: extension scenarios can be migrated purely against the SolidJS
  DOM; skills keeps its skip guards.

## Decision log

- Decision: store deployment-scoped flags in a new
  `feature_flag_overrides (deployment_id, flag_name, enabled, updated_at)`
  table (PK `(deployment_id, flag_name)`) rather than adding a nullable
  `deployment_id` to `settings`.
  Rationale: libsql cannot ALTER the composite-keyed `settings` table
  without a rebuild; a dedicated table keeps the `(user_id, key)` settings
  contract untouched, satisfies RFC 0009's intent (deployment-scoped rows
  not keyed by user), and is a plain `CREATE TABLE` on both backends. The
  settings API surface (`feature_flag:` key prefix, `X-Deployment-Id`
  header) is unchanged from the RFC.
  Date/Author: 2026-07-19, Claude.
- Decision: `GET /api/features` resolves the deployment from an optional
  `X-Deployment-Id` header, defaulting to `"default"`; writes via
  `PUT /api/settings/feature_flag:<name>` require the header (400 without).
  Rationale: the RFC requires the header on both; requiring it on reads
  would break the existing SPA boot fetch, and this single-instance product
  has no deployment identity source yet. Reads defaulting keeps the
  contract additive; the strict write path preserves the RFC's persistence
  semantics.
  Date/Author: 2026-07-19, Claude.
- Decision: precedence is environment variable > deployment override >
  compiled default, per RFC 0009 §Precedence (subsystem-availability
  defaults are not implemented — no current flag needs one).
  Date/Author: 2026-07-19, Claude.
- Decision: expose a minimal `window.__axinite` test-hook object from the
  SPA (chat stream close/reconnect, `emitChatEvent`, ready marker) instead
  of recreating the legacy globals the Python e2e suite pokes.
  Rationale: the compiled SPA has no reachable globals; a deliberate,
  documented hook surface keeps scenarios deterministic without shipping
  the legacy's implicit global soup. It adds no capability an open console
  does not already have.
  Date/Author: 2026-07-19, Claude.
- Decision: logs become a `/logs` route gated by a new `route_logs` flag;
  the logs dialog is retired and the topbar button becomes a nav entry.
  The `panel_logs` flag continues to gate the log stream surface itself so
  existing deployments' flag semantics survive.
  Date/Author: 2026-07-19, Claude.

## Context and orientation

Established by the prior plan: `web-src/` SolidJS workspace; embedded assets
under `src/channels/web/static/solid/`; `handlers/ui_assets.rs` (variant
serving), `handlers/features.rs` (env-var flags); Bun mock backend with
contract tests; Make targets `frontend-*`. Daemon contracts for the parity
surfaces (verified by recon, with exact shapes):

- Restart: `restart_enabled` in `GatewayStatusResponse`; `/restart` chat
  command; completion via `tool_completed {name:"restart", success}` or a
  `response` containing "restart initiated"; loader cleared on SSE re-open.
- TEE: external `GET {api-base}/instances/{name}/attestation`
  (`image_digest`), `GET {api-base}/attestation/report`
  (`tls_certificate_fingerprint`, `report_data`, `vm_config`), copy-report.
- Media: `SendMessageRequest.images: [{media_type, data(base64)}]`, 5 MB
  and 5-image caps; SSE `image_generated {data_url, path?, thread_id?}`.
- Auth cards: SSE `auth_required {extension_name, instructions?, auth_url?,
  setup_url?}` (auth_url → token/OAuth card, else configure modal);
  `auth_completed {extension_name, success, message}`; POST
  `/api/chat/auth-token {extension_name, token}` and `/api/chat/auth-cancel
  {extension_name}`.
- Jobs: SSE `job_started {job_id, title, browse_url}`;
  `JobDetailResponse` carries `project_dir, browse_url, job_mode,
  transitions, can_restart, can_prompt, job_kind`.
- Pairing: `GET /api/pairing/{channel}` →
  `{channel, requests:[{code, sender_id, meta?, created_at}]}`;
  `POST /api/pairing/{channel}/approve {code}` → `ActionResponse`; 429 on
  rate-limited approvals; WASM channel `activation_status` in
  {installed, configured, pairing, active, failed} drives the stepper.
- Settings: `(user_id, key)`-keyed `settings` table (postgres `V8`,
  libsql base schema); `SettingsStore`/`NativeSettingsStore` traits in
  `src/db/traits/settings.rs`; the 5-step recipe in `src/db/CLAUDE.md`
  (trait → forwarders → postgres → libsql → migration); postgres
  migrations are refinery `V<N>__*.sql`, libsql uses
  `INCREMENTAL_MIGRATIONS` tuples; `NullDatabase` needs stub impls;
  `X-Confirm-Action` in `handlers/skills.rs` is the header-validation
  precedent.

Python e2e (tests/e2e/): boots the daemon with `GATEWAY_AUTH_TOKEN`, boots
a mock OpenAI-compatible LLM with canned regex responses (including a
deliberate XSS payload for "html test"), navigates to `/?token=…`, waits
for `#auth-screen` to hide, and asserts against ~80 legacy selectors and
several injectable globals. `AXINITE_WEB_UI=legacy` is pinned in
`conftest.py:105`.

## Plan of work

F1 (Rust, independent): migration `V18__feature_flag_overrides.sql` and a
libsql incremental creating the table; `SettingsStore` +
`NativeSettingsStore` gain `list_deployment_flags(deployment)`,
`set_deployment_flag(deployment, name, enabled)`; forwarders, postgres
(via `history/store/settings.rs`), libsql, `NullDatabase` impls;
`FeatureFlagRegistry` (deployment → name → bool) in `GatewayState`
(struct + `GatewayChannel::new()` + `rebuild_state()`), hydrated from the
store at startup; `settings_set_handler` intercepts `feature_flag:` keys
(validate name `[a-z0-9_]+`, require `X-Deployment-Id`, coerce value to
bool, persist, update registry); `features_handler` resolves env >
registry(deployment) > default. Red tests first: registry precedence,
handler 400 without header, immediate visibility of a write, libsql
round-trip via `LibSqlBackend::new_memory()`.

F2 (SPA): `route_logs` flag added to `registry.ts`, Rust `FLAG_DEFAULTS`,
mock defaults; `/logs` route in the router + `SOLID_APP_ROUTES`; new
`logs-preview.tsx` route component (stream via existing `connectLogEvents`,
level filter for display, target substring filter, pause/resume, clear,
auto-scroll toggle, level set via existing `/api/logs/level`); retire
`logs-dialog.tsx`; shell nav gains Logs; ten-locale strings; behaviour
tests; update `app-shell.pw.ts`.

F3 (mock): pairing GET/approve routes + a `pairing`-status WASM channel
fixture; `/api/chat/auth-token` + `/api/chat/auth-cancel`; `sendMessage`
recognizes `/restart` (emits `tool_started`/`tool_completed` named
`restart` and a "Restart initiated" response), a prompt containing
"image" (emits `image_generated` with an inline data URL), and "job"
(emits `job_started`); accepts and echoes `images[]` count in the reply;
`auth_required` fixture variant with `auth_url`; contract tests for each.

F4 (SPA chat): extend `ChatSseEvent` to the daemon's full tagged set;
image staging + previews + caps + paste; send includes `images`;
generated-image cards; auth card + configure-modal dispatch and
`auth_completed` handling with toasts; `job_started` card linking to the
job; behaviour tests for each path (fake stream via the test hook).

F5 (SPA shell/extensions): restart button + confirm modal + loader with
the legacy completion heuristics; TEE shield/popover client (hostname
derivation, localhost-inert, report cache, copy) behind
`surface_tee_attestation`; pairing rows + approve + 10 s poll + stepper
(`installed/configured/pairing/active/failed`) on extensions; tests.

F6 (SPA jobs): tabbed detail; overview (transitions, browse_url, mode,
kind, metadata, gated restart/cancel/prompt); activity merging persisted
events with live `job_*` SSE bucketed by job id; done-signal checkbox on
prompt; recursive file tree; tests.

F7 (e2e): SPA testability contract — `?token=` boot (AuthGate reads and
stores the query token, strips it from the URL), `data-testid`/ids:
`auth-screen`, `sse-status` (text "Connected"/"Disconnected"), message
roles, approval/auth-card/toast/testids as needed; `window.__axinite`
hooks; then rewrite `tests/e2e/helpers.py` SEL and all seven scenarios;
remove the legacy pin; run the suite (build with libsql features) and fix
fallout. Scenarios that asserted legacy-only mechanics are rewritten to
the SolidJS equivalents (documented per scenario in the commit).

F8: validation closure as in the Progress list.

Sequencing: F1 ∥ (F2 → F3 → F4/F5/F6) → F7 → F8. F4–F6 are delegated as
bounded implementation tasks with tests where practical.

## Concrete steps

Per milestone: red tests → implement → `make frontend-test` (web-src) or
targeted `cargo nextest` (Rust) → `make frontend-build` when the SPA
changed → commit. Final: `make all`, `make frontend-verify`,
`pytest tests/e2e/ -v` (with `cargo build --no-default-features --features
libsql` first), Playwright MCP + css-view spot checks, CodeRabbit CLI.

## Validation and acceptance

- F1: `PUT /api/settings/feature_flag:panel_logs` with
  `X-Deployment-Id: production` and value `"false"` makes
  `GET /api/features` (same header) return `panel_logs: false` without
  restart; without the header the PUT returns 400; env var still wins.
- F2–F6: each surface has behaviour tests, and the stub exercises it
  (`make frontend-stub` + documented interaction).
- F7: `pytest tests/e2e/ -v` passes against the SolidJS UI with no
  `AXINITE_WEB_UI` pin (skills scenario may self-skip offline).
- Gates: `make all`, frontend gates, `frontend-verify`, CodeRabbit clean
  or findings addressed.

## Idempotence and recovery

Milestone commits allow `git revert`. Migrations are additive
(`CREATE TABLE IF NOT EXISTS`). The mock stays stateless per process.

## Outcomes & retrospective

To be completed.
