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
TanStack Query plus Solid signals for state, Kobalte for accessible
primitives, Tailwind/daisyUI semantic classes for styling, and
i18next/Fluent bundles for ten locales.

The build output is copied into `src/channels/web/static/solid/` and embedded
into the gateway binary with `include_str!`/`include_bytes!`, exactly like the
legacy assets were. Operators still deploy one binary; no Node or Bun
toolchain is needed to build or run the Rust gateway.

The legacy handwritten shell (`src/channels/web/static/{index.html,style.css,
app.js}`) remains embedded purely as a rollback path. Setting
`AXINITE_WEB_UI=legacy` before starting the gateway serves the legacy shell
instead of the SPA. The Python end-to-end suite in `tests/e2e/` still drives
the legacy shell and pins this variable itself; migrating those scenarios to
the SolidJS DOM is tracked follow-up work.

## 2. Commands

All commands run from the repository root.

| Command                 | Purpose                                                        |
| ----------------------- | -------------------------------------------------------------- |
| `make frontend-install` | `bun install --frozen-lockfile` in `web-src/`.                 |
| `make frontend-build`   | Vite build, then refresh `src/channels/web/static/solid/`.     |
| `make frontend-verify`  | Rebuild and fail if the embedded copy is stale.                |
| `make frontend-check`   | Biome format check, Biome lint, TypeScript check.              |
| `make frontend-test`    | `frontend-check` plus vitest unit and accessibility suites.    |
| `make frontend-stub`    | Daemon-free stub runtime (see below).                          |

Inside `web-src/`, the underlying Bun scripts are available directly
(`bun run test`, `bun run test:e2e`, `bun run build`, and so on).

After changing anything in `web-src/` that affects the built app, run
`make frontend-build` and commit the refreshed
`src/channels/web/static/solid/` output together with the source change.
`make frontend-verify` is the staleness gate.

## 3. The stub runtime

`make frontend-stub` starts the front-end without the Axinite daemon:

- a Bun mock API (`web-src/mock-backend/src/server.ts`) on port 8787
  (override with `MOCK_API_PORT`),
- a Vite build watcher, and
- a preview server on <http://127.0.0.1:2020> (override with `PREVIEW_PORT`)
  that serves the built SPA, falls back to the app shell for extension-less
  routes, and proxies `/api/*` to the mock API.

The mock backend is a contract harness, not a second daemon: it holds
deterministic in-memory fixtures, ignores authentication, and persists
nothing.

### 3.1 Stubbed HTTP routes

The mock implements the routes the SPA consumes, with gateway-shaped
payloads (`web-src/axinite/src/lib/api/contracts.ts` documents the shapes):

- `GET /api/gateway/status`, `GET /api/features`
- Chat: `GET /api/chat/threads`, `POST /api/chat/thread/new`,
  `GET /api/chat/history`, `POST /api/chat/send` (returns 202),
  `POST /api/chat/approval`
- Memory: `GET /api/memory/tree`, `GET /api/memory/read`,
  `POST /api/memory/search`, `POST /api/memory/write`
- Jobs: `GET /api/jobs`, `GET /api/jobs/summary`, `GET /api/jobs/{id}`,
  `GET /api/jobs/{id}/events` (paginated JSON, mirroring the daemon —
  not SSE), `GET /api/jobs/{id}/files/list`, `GET /api/jobs/{id}/files/read`,
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
- Logs: `GET /api/logs/level`, `POST /api/logs/level`

Unknown routes return 404 with a JSON error body.

### 3.2 Stubbed SSE routes

- `GET /api/chat/events` — `text/event-stream`; frames use
  `event: <type>` matching the payload's `type` field (the daemon's
  `SseEvent` tagging). Sending a chat message produces a deterministic
  lifecycle: `thinking`, then `tool_started`/`tool_completed`/`tool_result`
  on fixed short delays, then `response`. Heartbeat `event: heartbeat`
  frames are emitted every 15 seconds.
- `GET /api/logs/events` — replays the fixture log history as
  `event: log` frames (entries carry `level`, `target`, `message`,
  `timestamp`, matching `log_layer.rs`), then streams new entries; comment
  keep-alives (`: keep-alive`) every 15 seconds.

Neither route implements `Last-Event-ID` or `retry:` hints; the real daemon
does not either — browsers rely on plain `EventSource` auto-reconnect and
history replay on reconnect.

### 3.3 Failure fixtures

Set `MOCK_FAILURES` to a comma-separated list of request paths to make the
stub return a deterministic HTTP 500 for those routes:

```sh
MOCK_FAILURES=/api/jobs make frontend-stub
```

The jobs route renders a visible, localized error notice in this state; use
the same mechanism to exercise other error paths.

### 3.4 Feature flags in the stub

The stub serves `GET /api/features` as a flat `{"flag_name": bool}` map
(RFC 0009 shape) with the same compiled defaults as the gateway. Overrides,
in increasing precedence:

1. Environment at stub start: `FEATURE_FLAG_<UPPER_SNAKE_NAME>=true|false`
   (for example `FEATURE_FLAG_ROUTE_SKILLS=false make frontend-stub`).
2. Browser-local override: open the app with `?debug-flags=1` and use the
   "Feature flags" maintainer panel to force any flag on or off; overrides
   persist in `localStorage` under `axinite.feature-flag-overrides`, which
   Playwright can also seed directly.

The resolution order in the SPA is local override, then server value, then
registry default (`web-src/axinite/src/lib/feature-flags/`).

## 4. Serving from the gateway

`src/channels/web/handlers/static_files.rs` embeds the built artefacts and
serves, by default (`UiVariant::Solid`):

- the app shell at `/` and every client route (`/chat`, `/memory`, `/jobs`,
  `/routines`, `/extensions`, `/skills`),
- `/assets/app.js`, `/assets/index.css`, `/assets/axinite32.ico`,
  `/favicon.ico`, and
- `/locales/{locale}/common.ftl` for the ten locale bundles.

Vite is configured to emit stable, hash-free artefact names so the embedded
file list stays fixed; everything is served with `Cache-Control: no-cache`.

`GET /api/features` on the gateway resolves `FEATURE_FLAG_<NAME>` environment
variables over compiled defaults (`src/channels/web/handlers/features.rs`).
The RFC 0009 settings-table override layer is not implemented yet.

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
- Only the routes listed above exist; settings, pairing, OAuth, project
  file browsing, WebSocket, and the OpenAI-compatible surface are absent.
- Job/routine/extension/skill mutations mutate in-memory state only.

## 6. Test layers

- `web-src` vitest (`make frontend-test`): unit and behaviour tests for the
  API clients, auth token handling, feature flags, components, plus
  `mock-backend-contract.test.ts`, which pins the stub's HTTP shapes and SSE
  framing/ordering, and `api-contract-alignment.test.ts`, which pins the
  browser contract to the daemon payload shapes.
- `web-src` Playwright (`bun run test:e2e` in `web-src/`): boots the full
  stub stack and exercises navigation, locales, the logs dialog, and the
  debug flag panel.
- Rust unit tests (`make test`): SPA shell serving on every route, stable
  asset names, locale bundles, legacy-variant fallback, and feature-flag
  resolution.
- Python e2e (`tests/e2e/`): drives the legacy shell against the real
  daemon (pinned via `AXINITE_WEB_UI=legacy` in its conftest).
