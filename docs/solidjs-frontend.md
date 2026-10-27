# SolidJS front-end: development, stub runtime, and serving

## Front matter

This document describes the SolidJS browser front-end adopted from the
`axinite-mockup` repository (RFC 0018), how the gateway serves it, and how to
develop and validate it without running the full Axinite daemon. It supersedes
the legacy description in `docs/front-end-architecture.md`, which is retained
for the legacy fallback shell.

## 1. Overview

The default browser UI is a SolidJS single-page application (SPA) authored in
`web-src/`. It uses Vite for builds, TanStack Router for the route tree,
TanStack Query plus Solid signals for state, Kobalte for accessible primitives,
Tailwind/daisyUI semantic classes for styling, and i18next/Fluent bundles for
ten locales.

The build output is copied into `src/channels/web/static/solid/` and embedded
into the gateway binary with `include_str!`/`include_bytes!`, exactly like the
legacy assets were. Operators still deploy one binary; no Node or Bun toolchain
is needed to build or run the Rust gateway.

The legacy handwritten shell
(`src/channels/web/static/{index.html,style.css, app.js}`) remains embedded
purely as a rollback path. Setting `AXINITE_WEB_UI=legacy` before starting the
gateway serves the legacy shell instead of the SPA. The Python end-to-end suite
in `tests/e2e/` drives the SolidJS UI against the real daemon.

## 2. Commands

All commands run from the repository root.

| Command                 | Purpose                                                    |
| ----------------------- | ---------------------------------------------------------- |
| `make frontend-install` | `bun install --frozen-lockfile` in `web-src/`.             |
| `make frontend-build`   | Vite build, then refresh `src/channels/web/static/solid/`. |
| `make frontend-verify`  | Rebuild and fail if the embedded copy is stale.            |
| `make frontend-check`   | Biome format/lint, TypeScript, semantic-CSS rules.         |
| `make frontend-test`    | `frontend-check` plus unit, a11y, and Fluent suites.       |
| `make frontend-full`    | The complete `verify:full` verification chain.             |
| `make frontend-stub`    | Daemon-free stub runtime (see below).                      |

The semantic-CSS rules cover the classlist, semgrep, and stylelint checks;
`verify:full` adds the Tailwind compile check, the workspace Playwright spec,
and `moz-fluent-lint` on top of every other suite. Inside `web-src/`, the
underlying Bun scripts are available directly (`bun run test`,
`bun run test:e2e`, `bun run build`, and so on). The `semantic` and
`verify:full` scripts fetch semgrep and moz-fluent-linter through `uvx` on
first use. Continuous integration runs `make frontend-full` and the
`make frontend-verify` staleness gate for any pull request touching `web-src/`
or the embedded assets (`.github/workflows/frontend.yml`); Playwright's
Chromium must be installed for the workspace spec
(`bunx playwright install chromium`).

After changing anything in `web-src/` that affects the built app, run
`make frontend-build` and commit the refreshed `src/channels/web/static/solid/`
output together with the source change. `make frontend-verify` is the staleness
gate.

## 3. The stub runtime

`make frontend-stub` starts the front-end without the Axinite daemon:

- a Bun mock API (`web-src/mock-backend/src/server.ts`) on port 8787
  (override with `MOCK_API_PORT`),
- a Vite build watcher, and
- a preview server on <http://127.0.0.1:2020> (override with `PREVIEW_PORT`)
  that serves the built SPA, falls back to the app shell for extension-less
  routes, and proxies `/api/*` to the mock API.

The mock API and watcher start first. The preview starts after the watcher
emits the initial `built in …` completion line; an early child exit or a
shutdown signal prevents it from starting. The supervisor monitors the preview
along with the other children. If preview startup fails, it reports the cause.
An unexpected child exit stops and waits for the other owned children and
returns a failure; SIGINT and SIGTERM stop them cleanly.

The mock backend is a contract harness, not a second daemon: it holds
deterministic in-memory fixtures, ignores authentication, and persists nothing.
Its fixture clock is fixed at `2026-03-26T11:30:00Z` by default, so timestamps
and gateway uptime are reproducible between runs. Tests can inject another
clock when they need to advance time. The default server composition parses
`FEATURE_FLAG_*` values once and injects recognized overrides into mock state;
direct state tests inject the same override map. Feature-flag and chat-history
reads return detached data. Reading history does not select a thread; explicit
create and send operations do.

### 3.1 Stubbed HTTP routes

The mock implements the routes the SPA consumes, with gateway-shaped payloads
(`web-src/axinite/src/lib/api/contracts.ts` documents the shapes):

- `GET /api/gateway/status`, `GET /api/features`
- Chat: `GET /api/chat/threads`, `POST /api/chat/thread/new`,
  `GET /api/chat/history`, `POST /api/chat/send` (returns 202),
  `POST /api/chat/approval`, `POST /api/chat/auth-token`,
  `POST /api/chat/auth-cancel`
- Pairing: `GET /api/pairing/{channel}`,
  `POST /api/pairing/{channel}/approve` (a deterministic pending request
  `PAIR-1234` exists on the `whatsapp` channel; approving the code
  `rate-limited` returns the daemon's plain-text 429)
- Memory: `GET /api/memory/tree`, `GET /api/memory/read`,
  `POST /api/memory/search`, `POST /api/memory/write`
- Jobs: `GET /api/jobs`, `GET /api/jobs/summary`, `GET /api/jobs/{id}`,
  `GET /api/jobs/{id}/events` (paginated JSON, mirroring the daemon — not SSE),
  `GET /api/jobs/{id}/files/list`, `GET /api/jobs/{id}/files/read`,
  `POST /api/jobs/{id}/cancel|restart|prompt`
- Routines: `GET /api/routines`, `GET /api/routines/summary`,
  `GET /api/routines/{id}`, `GET /api/routines/{id}/runs`,
  `POST /api/routines/{id}/trigger|toggle`, `DELETE /api/routines/{id}`
- Extensions: `GET /api/extensions`, `GET /api/extensions/tools`,
  `GET /api/extensions/registry`, `POST /api/extensions/install`,
  `POST /api/extensions/{name}/activate|remove`,
  `GET/POST /api/extensions/{name}/setup`
- Skills: `GET /api/skills`, `POST /api/skills/search`,
  `POST /api/skills/install`, `DELETE /api/skills/{name}`
- Logs: `GET /api/logs/level`, `PUT /api/logs/level`

Unknown routes return 404 with a JSON error body.

### 3.2 Stubbed SSE routes

- `GET /api/chat/events` — `text/event-stream`; frames use
  `event: <type>` matching the payload's `type` field (the daemon's `SseEvent`
  tagging). Sending a chat message produces a deterministic lifecycle:
  `thinking`, then `tool_started`/`tool_completed`/`tool_result` on fixed short
  delays, then `response`. Heartbeat `event: heartbeat` frames are emitted
  every 15 seconds. Deterministic extras: the exact message `/restart` emits a
  `restart`-named tool sequence and a "Restart initiated" response; a prompt
  containing "image" additionally emits `image_generated` with an inline data
  URL; a prompt containing "job" emits `job_started`; attached `images[]` are
  acknowledged in the response text; a successful `POST /api/chat/auth-token`
  publishes `auth_completed`.
- `GET /api/logs/events` — replays the fixture log history as
  `event: log` frames (entries carry `level`, `target`, `message`, `timestamp`,
  matching `log_layer.rs`), then streams new entries; comment keep-alives
  (`: keep-alive`) every 15 seconds.

Neither route implements `Last-Event-ID` or `retry:` hints; the real daemon
does not either — browsers rely on plain `EventSource` auto-reconnect and
history replay on reconnect.

### 3.3 Failure fixtures

Set `MOCK_FAILURES` to a comma-separated list of request paths to make the stub
return a deterministic HTTP 500 for those routes:

```sh
MOCK_FAILURES=/api/jobs make frontend-stub
```

The jobs route renders a visible, localized error notice in this state; use the
same mechanism to exercise other error paths.

### 3.4 Restart acceptance fixture

The restart fixture is disabled by default. Enable it when running the stub:

```sh
MOCK_RESTART_FIXTURE=1 make frontend-stub
```

Using the UI's Restart control and confirming the action sends the normal
`/restart` chat command. After its successful tool acknowledgement, the fixture
waits 650 ms, closes the chat and log SSE streams, and returns
`503 Service Unavailable` for API requests for five seconds. The API then
becomes available again. This is synthetic downtime: it does not restart or
stop a process. The fixture owns its timers and disposes them when the mock
server stops.

The default remains disabled, so an ordinary `make frontend-stub` does not
simulate an outage.

To exercise a failed restart command, make its chat-send request return HTTP
500:

```sh
MOCK_RESTART_FIXTURE=1 MOCK_FAILURES=/api/chat/send make frontend-stub
```

Confirm the restart in the UI to see the retryable alert and Retry button. The
browser console reports only `restart_failed` and the bounded category; it does
not include the response body or URL. Each retry starts a fresh attempt. Late
callbacks from an earlier attempt cannot change the current phase after
failure, retry, completion, or disposal.

### 3.5 Feature flags in the stub

The stub serves `GET /api/features` as a flat `{"flag_name": bool}` map (RFC
0009 shape) with the same compiled defaults as the gateway. Overrides, in
increasing precedence:

1. Environment at stub start: `FEATURE_FLAG_<UPPER_SNAKE_NAME>=true|false`
   (for example `FEATURE_FLAG_ROUTE_SKILLS=false make frontend-stub`).
2. Browser-local override: open the app with `?debug-flags=1` and use the
   "Feature flags" maintainer panel to force any flag on or off; overrides
   persist in `localStorage` under `axinite.feature-flag-overrides`, which
   Playwright can also seed directly.

The resolution order in the SPA is local override, then server value, then
registry default (`web-src/axinite/src/lib/feature-flags/`).

## 4. Serving from the gateway

`src/channels/web/handlers/ui_assets.rs` embeds the built artefacts and serves,
by default (`UiVariant::Solid`):

- the app shell at `/` and every client route (`/chat`, `/memory`, `/jobs`,
  `/routines`, `/extensions`, `/skills`, `/logs`),
- `/assets/app.js`, `/assets/index.css`, `/assets/axinite32.ico`,
  `/favicon.ico`, and
- `/locales/{locale}/common.ftl` for the ten locale bundles.

Vite is configured to emit stable, hash-free artefact names so the embedded
file list stays fixed; everything is served with `Cache-Control: no-cache`.

`GET /api/features` on the gateway resolves, per flag: `FEATURE_FLAG_<NAME>`
environment variable, then the deployment-scoped operator override (persisted
via `PUT /api/settings/feature_flag:<name>` with an `X-Deployment-Id` header),
then subsystem availability (an unavailable backing runtime disables the flag),
then the compiled default. A cached snapshot is returned when available;
otherwise, the gateway loads a query-local snapshot from the store. An uncached
read returns 503 if a configured store cannot load overrides. With no store,
there are no persisted overrides to apply. The SPA then uses browser-local
overrides and compiled defaults after a read error and logs a generic
diagnostic. Successful responses include `X-Axinite-Version`. See RFC 0009's
implementation notes for the full contract and deviations.

Framework-independent flag identifiers, snapshots, precedence, and
subsystem-availability policy live in `src/feature_flags/`. The web gateway
owns its per-deployment cache, persistence port, queries, and override
commands. Reads return a detached cached snapshot or load from storage without
changing cache state; uncached reads consult the store on each request. Writes
serialize per deployment, hydrate when needed, persist the override, and then
publish it to the cache. The SPA has its own typed registry and client-side
policy in `web-src/axinite/src/lib/feature-flags/`; local browser overrides
take precedence over the gateway response, which takes precedence over SPA
defaults. Gateway cache and request metrics use fixed operation and outcome
labels, including the no-store default; aggregate gauges omit deployment IDs
and flag names. Read telemetry is emitted at the HTTP delivery boundary,
outside the snapshot query and its storage adapter. Separate admission-wait and
complete command timings include queued overrides. Absent subsystems remain
observable even when explicit overrides enable their surfaces;
availability-driven disable observations apply only below both explicit
override layers. Tracing uses random request and operation IDs with bounded
failure categories.

The `X-Deployment-Id` value is trimmed and must be 1 to 64 characters of
`[a-z0-9_]`. On `GET /api/features` an absent or blank header selects
`"default"` and an invalid one returns 400; on the `PUT` the header is required
and must be valid (400 otherwise).

### 4.1 Authentication

The gateway protects `/api/*` with a bearer token. The SPA probes
`GET /api/gateway/status` at boot: if the gateway answers anonymously (the
stub), the app loads directly; on 401 it presents a token form, verifies the
token, stores it in `sessionStorage`, sends it as `Authorization: Bearer` on
every request, and appends `?token=` to the two SSE URLs (which cannot carry
headers).

## 5. How the stub differs from the daemon

- No authentication, no rate limiting, no persistence.
- Chat responses are canned fixtures on fixed timers, not model output.
- Only the routes listed above exist; settings, OAuth, project
  file browsing, WebSocket, and the OpenAI-compatible surface are absent.
- Job/routine/extension/skill mutations mutate in-memory state only.

## 6. Test layers

- `web-src` vitest (`make frontend-test`): unit and behaviour tests for the
  API clients, auth token handling, feature flags, components, plus
  `mock-backend-contract.test.ts`, which pins the stub's HTTP shapes and SSE
  framing/ordering, and `api-contract-alignment.test.ts`, which pins the
  browser contract to the daemon payload shapes.
- `web-src` Playwright (`bun run test:e2e` in `web-src/`): boots the full
  stub stack and exercises navigation, locales, the logs route (`/logs`), and
  the debug flag panel.
- Rust unit tests (`make test`): SPA shell serving on every route, stable
  asset names, locale bundles, legacy-variant fallback, and feature-flag
  resolution.
- Python e2e (`tests/e2e/`): drives the SolidJS UI against the real
  daemon through a documented testability contract — the `?token=` boot
  parameter, the `#auth-screen` marker, the `sse-status` indicator, and the
  minimal `window.__axinite` hook object (close/reconnect the chat stream,
  inject a chat event). See `tests/e2e/CLAUDE.md`.
