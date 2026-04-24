# Axinite front-end architecture

## Front matter

- **Status:** Draft design reference for the currently implemented web front
  end.
- **Scope:** The browser-facing web gateway, including static asset delivery,
  client-side interface generation, backend integration, and runtime
  communication paths.
- **Primary audience:** Maintainers and contributors who need to change the web
  front end or the gateway-backed APIs without breaking the user-visible
  browser experience.
- **Precedence:** `src/NETWORK_SECURITY.md` remains the authoritative reference
  for network-facing security controls. `docs/chat-model.md` remains the
  authoritative reference for the session-backed chat lifecycle. `docs/jobs-and-routines.md`
  remains the authoritative reference for scheduler and job semantics. This
  document explains how those backend capabilities are exposed through the
  browser UI.

## 1. Design scope

Axinite's web front end is not a separate single-page application (SPA) build
that is compiled, versioned, and deployed independently of the host. It is a
browser gateway surface embedded directly into the Rust application. The same
binary
that starts the agent runtime also:

- constructs the gateway channel,
- binds the HTTP server,
- serves the HTML, CSS, and JavaScript assets,
- exposes authenticated JSON and streaming APIs, and
- forwards browser-originated requests into the same `ChannelManager` and agent
  loop used by other interactive channels.

That design makes the front end operationally simple. A local instance can be
started with one process and one gateway port, while the browser UI still gains
access to memory browsing, job control, routine status, extension management,
  and live model activity.

This document focuses on the implemented browser path. It does not try to be a
complete product guide, nor does it restate the entire agent architecture.
Instead, it explains how the front end is hosted, how the interface is
generated, which backend surfaces it depends on, and how state moves between
browser and runtime.

## 2. Front-end role in the runtime

The browser gateway is one interaction surface within the broader channel
system. It is implemented as `GatewayChannel`, which satisfies the same
`Channel` trait used by REPL, HTTP, Signal, and WASM-backed channels. The key
consequence is that the browser is not treated as a privileged special case at
the agent boundary. Browser-originated chat traffic is normalized into
`IncomingMessage`, and browser-bound agent output is translated back into
channel-specific events through `OutgoingResponse` and `StatusUpdate`.

Table 1. Browser gateway responsibilities within the host runtime.

<!-- markdownlint-disable MD013 MD060 -->
| Concern | Front-end role | Primary evidence |
|---------|----------------|------------------|
| User interface shell | Presents the authenticated SPA shell, tabs, editors, status widgets, and live activity cards | `src/channels/web/static/index.html`, `src/channels/web/static/style.css`, `src/channels/web/static/app.js` |
| HTTP server surface | Serves embedded static assets and JSON APIs from the same Axum router | `src/channels/web/server.rs`, `src/channels/web/handlers/static_files.rs` |
| Chat ingress | Converts REST and WebSocket browser actions into `IncomingMessage` values | `src/channels/web/handlers/chat.rs`, `src/channels/web/ws.rs` |
| Chat egress | Maps agent responses and status events into browser Server-Sent Events (SSE) and WebSocket events | `src/channels/web/mod.rs`, `src/channels/web/sse.rs`, `src/channels/web/types.rs` |
| Runtime control plane | Exposes memory, jobs, routines, extensions, skills, settings, logs, and gateway status | `src/channels/web/handlers/` |
| Integration hub | Holds references to optional runtime subsystems inside `GatewayState` so handlers can expose only what is available | `src/channels/web/server.rs`, `src/channels/web/mod.rs` |
<!-- markdownlint-enable MD013 MD060 -->

The browser-facing runtime is therefore a composition layer. It does not own
the agent, workspace, database, or extension system, but it is the surface that
assembles those capabilities into one coherent UI.

## 3. How the web front end is served

### 3.1 Startup and channel wiring

The front end becomes available during normal application startup in
`src/main.rs`. After `AppBuilder::build_all()` creates the core runtime
components, the host checks whether gateway configuration is present in
`config.channels.gateway`. If so, it builds a `GatewayChannel` and injects the
runtime dependencies it can expose to the browser:

- the language model provider for the OpenAI-compatible proxy,
- the workspace for memory browsing and editing,
- the session manager for thread and history state,
- the log broadcaster and live log-level control,
- the tool registry for tool listings,
- the extension manager for installation, auth, setup, and activation,
- the database store for persisted history, jobs, settings, and routines,
- the container job manager and prompt queue for sandbox-job controls,
- the scheduler for follow-up agent job prompts,
- the skill registry and skill catalogue,
- the cost guard for gateway status metrics, and
- registry catalogue entries for the "available extensions" view.

This wiring is done through `with_*` builder methods on `GatewayChannel`. Each
builder rebuilds the internal `GatewayState` so the final gateway state is a
single shared bundle of optional `Arc` references. The gateway can therefore
start even when some subsystems are disabled, and each handler can degrade
gracefully by returning `503 Service Unavailable` or `501 Not Implemented`
instead of assuming every service exists.

### 3.2 HTTP server model

When the channel starts, it binds a TCP listener and launches an Axum server
through `start_server()`. The resulting router has two broad layers:

- public routes, which include the static assets, health check, and OAuth
  callback entry point;
- protected routes, which include chat, memory, jobs, routines, extensions,
  skills, settings, logs, project file browsing, and the OpenAI-compatible
  endpoints.

Authentication is injected as router middleware rather than being repeated in
each handler. All protected routes expect `Authorization: Bearer <token>`,
except for the streaming endpoints that must also support query-string tokens
because browser `EventSource` and WebSocket upgrades cannot set arbitrary
headers.

### 3.3 Static asset delivery

The HTML, CSS, JavaScript, and favicon are embedded into the binary at compile
time with `include_str!` and `include_bytes!`. The gateway serves them from:

- `/`
- `/style.css`
- `/app.js`
- `/favicon.ico`

This has a few important implications:

- there is no separate front-end asset pipeline, bundle manifest, or CDN
  deployment step;
- the served assets are exactly the files checked into
  `src/channels/web/static/`;
- a host rebuild is required for front-end code changes to reach users; and
- the gateway can be distributed as one self-contained binary without copying a
  separate web root at runtime.

The static asset handlers mark the HTML, CSS, and JavaScript responses as
`Cache-Control: no-cache`, while the favicon is cached for one day.

### 3.4 Serving constraints and transport policies

The web server applies several policies that shape the front-end deployment
model:

- CORS is restricted to the gateway's own origin and `localhost` on the bound
  port, reinforcing the local-first assumption.
- Responses carry `X-Content-Type-Options: nosniff` and `X-Frame-Options:
  DENY`.
- Request bodies are limited to 10 MB at the server level, which is large
  enough for browser image uploads and `.skill` bundle uploads. Bundle archive
  contents are still constrained by the stricter skill-bundle validator after
  request extraction.
- The browser-side image uploader adds a stricter client-side limit of 5 MB per
  image and a maximum of five staged images.
- If `GATEWAY_AUTH_TOKEN` is absent, the gateway generates a random token at
  startup and uses that for all authenticated access.

## 4. How the interface is generated

### 4.1 Static shell plus runtime Document Object Model (DOM) construction

The browser interface is a hand-written SPA assembled from three checked-in
files:

- `index.html` defines the shell, tab layout, modals, placeholders, and the
  containers that later receive dynamic content.
- `style.css` defines the complete presentation layer, using CSS custom
  properties for the visual theme and responsive layout.
- `app.js` performs all authentication, state management, API calls, streaming
  subscriptions, and DOM updates.

There is no React, Vue, Svelte, or template compiler in this implementation.
The browser logic uses direct DOM APIs such as `document.createElement`,
`innerHTML`, event listeners, and small in-memory state objects. The front end
is therefore generated incrementally in the browser at runtime rather than by a
component compiler.

### 4.2 Boot sequence in the browser

The browser boot flow is:

1. `index.html` loads the basic shell, the stylesheet, Google Fonts, and the
   `marked` Markdown parser from a CDN.
2. `app.js` attempts auto-authentication from either `?token=` in the URL or a
   token stored in `sessionStorage`.
3. `authenticate()` tests the token by calling `/api/chat/threads` through
   `apiFetch()`.
4. On success, the script hides the auth screen, reveals the main application
   container, strips the token from the address bar, opens the chat SSE stream,
   opens the logs SSE stream, starts periodic gateway-status polling, checks
   TEE attestation state, and loads the initial thread, memory, and job data.

This boot model means the browser does not need a server-rendered session page.
Authentication is a lightweight runtime handshake against the gateway APIs.

### 4.3 SPA shell structure

The HTML shell divides the application into a tab bar and tab panels. The main
implemented tabs are:

- Chat
- Memory
- Jobs
- Routines
- Extensions
- Skills
- Logs

The chat tab also includes:

- a thread sidebar with a pinned assistant thread and normal conversations,
- the message transcript area,
- slash-command autocomplete,
- image staging and preview support, and
- the message composer.

Global shell elements include:

- the gateway connection/status indicator,
- the live status popover with uptime and cost data,
- TEE attestation status and report popover when available, and
- restart confirmation and progress modals.

The layout is intentionally static at the shell level. Dynamic behaviour enters
through content panes and runtime-generated cards rather than by replacing the
entire page.

### 4.4 Dynamic rendering patterns

`app.js` uses a small set of recurring rendering patterns:

- `innerHTML` for coarse-grained fragments such as tables, badges, and summary
  cards;
- `createElement()` for interactive structures that need event handlers, such
  as tool cards, approval cards, file-tree nodes, extension cards, and modals;
- in-memory UI state for active tabs, current thread, unread counts, job-event
  caches, memory-tree expansion state, and staged images;
- lazy loading on tab switches so panels only fetch their data when needed; and
- polling for low-frequency status data alongside SSE for high-frequency live
  activity.

This produces a hybrid rendering model. The browser keeps an always-loaded
shell, but most heavy content is built only when the relevant tab becomes
active or an event arrives.

### 4.5 Markdown and rich-output rendering

Assistant responses and several rich-detail panels are rendered as Markdown in
the browser by the `marked` library. The flow is:

1. raw text arrives from the backend,
2. `marked.parse()` converts it to HTML,
3. `sanitizeRenderedHtml()` strips dangerous tags and attributes,
4. copy buttons are injected into `<pre>` blocks, and
5. the sanitized HTML is inserted into the DOM.

The sanitization step removes script-like elements, embedded objects, form
elements, dangerous metadata tags, inline event handlers, and `javascript:` or
`data:` URLs in key attributes. The implementation is deliberately lightweight
and local to the browser bundle rather than relying on a heavier sanitization
dependency.

### 4.6 Tab-specific rendering model

Table 2. Main UI surfaces and how the browser builds them.

<!-- markdownlint-disable MD013 MD060 -->
| Surface | Rendering approach | Primary backend inputs |
|---------|--------------------|------------------------|
| Chat transcript | Append user bubbles, Markdown-render assistant bubbles, and live inline tool/status cards | `/api/chat/send`, `/api/chat/history`, `/api/chat/events` |
| Thread list | Rebuild sidebar items from JSON thread metadata and unread counters | `/api/chat/threads`, `/api/chat/thread/new` |
| Memory browser | Build expandable file tree nodes lazily and render Markdown file contents | `/api/memory/list`, `/api/memory/read`, `/api/memory/search`, `/api/memory/write` |
| Jobs | Render summary cards, list table, detail sub-tabs, file tree, and activity terminal | `/api/jobs`, `/api/jobs/summary`, `/api/jobs/{id}`, `/api/jobs/{id}/events`, file endpoints, job SSE events |
| Routines | Render summary cards, list table, and detailed JSON-backed views | `/api/routines`, `/api/routines/summary`, `/api/routines/{id}` |
| Extensions | Render installed and available extension cards, setup modals, and pairing controls | `/api/extensions`, `/api/extensions/tools`, `/api/extensions/registry`, setup and install endpoints |
| Skills | Render installed-skill cards and ClawHub search/install results | `/api/skills`, `/api/skills/search`, `/api/skills/install` |
| Logs | Prepend log rows from a dedicated SSE stream with local filters | `/api/logs/events`, `/api/logs/level` |
| Status widgets | Poll gateway status and optional attestation data for popovers | `/api/gateway/status`, external attestation API |
<!-- markdownlint-enable MD013 MD060 -->

## 5. How the browser communicates with the backend

### 5.1 Request model

The main browser request helper is `apiFetch()`. It automatically:

- adds `Authorization: Bearer <token>`,
- serializes JavaScript objects as JSON request bodies, and
- turns non-2xx responses into rejected promises with the response body text.

Most front-end interactions are standard JSON-over-HTTP requests. The browser
uses those requests for:

- chat send and approval actions,
- thread listing and creation,
- history retrieval,
- memory browsing, reading, writing, and search,
- job summaries, details, event history, file browsing, cancellation, restart,
  and follow-up prompts,
- routine summaries, details, toggles, triggers, and deletions,
- extension listing, setup, install, activation, removal, and registry
  browsing,
- skill listing, search, install, and removal,
- runtime log-level control, and
- gateway status polling.

### 5.2 Primary live transport: Server-Sent Events

The front end's main real-time channel is SSE over `/api/chat/events`. After
authentication, `connectSSE()` opens an `EventSource` using the query-string
token path allowed by the gateway auth middleware.

The SSE stream carries a tagged `SseEvent` contract. The browser listens for
events such as:

- `response`
- `stream_chunk`
- `thinking`
- `tool_started`
- `tool_completed`
- `tool_result`
- `status`
- `approval_needed`
- `auth_required`
- `auth_completed`
- `job_started`
- `job_message`
- `job_tool_use`
- `job_tool_result`
- `job_status`
- `job_result`
- `image_generated`
- `extension_status`
- `error`

Those events drive the UI's most dynamic behaviour:

- streaming assistant output is appended to the current message bubble,
- tool execution becomes inline activity cards,
- approvals become interactive approval cards,
- auth interruptions become auth cards or setup modals,
- job events are cached for the jobs activity terminal, and
- extension activation updates trigger tab refreshes.

The gateway keeps SSE state through `SseManager`, which fans one broadcast
channel out to connected clients. The stream uses a bounded buffer, and slow
clients may miss events. The browser is expected to recover through reconnects,
thread reloads, history fetches, and job-event history APIs rather than relying
on guaranteed delivery.

### 5.3 Secondary live transport: WebSocket

The gateway also exposes `/api/chat/ws` as a bidirectional WebSocket path. It
subscribes to the same broadcast event stream as SSE, but it also accepts
client frames for:

- new messages,
- approval decisions,
- auth token submission,
- auth cancellation, and
- ping/pong keepalive behaviour.

In the current browser implementation, the main UI path uses SSE rather than
WebSocket for chat updates. The WebSocket path exists as an alternative channel
surface and shares the same event contract through `WsServerMessage::from_sse_event()`.

### 5.4 Dedicated logs stream

Live logs are handled separately from chat activity. `connectLogSSE()` opens
`/api/logs/events`, receives `log` events, and prepends them into the logs
panel. The browser then applies local level and target filtering without asking
the server to re-run the query.

### 5.5 Auth token transport

The browser uses the auth token in three places:

- as a bearer token on normal JSON API requests,
- as a query parameter on the chat SSE stream, and
- as a query parameter on the logs SSE stream.

The browser also accepts a token via `?token=` at page load time so a gateway
URL can auto-open the authenticated UI. After successful auth, the script
removes the token from the visible URL and retains it in `sessionStorage`.

## 6. How the front end integrates with the application

### 6.1 GatewayState as the integration seam

`GatewayState` is the browser gateway's integration seam. It holds the
subsystem references that handlers need in order to expose browser features.
Key fields include:

- `msg_tx` for injecting browser-originated messages into the channel stream,
- `sse` for broadcasting live browser events,
- `workspace` for memory features,
- `session_manager` for threads and in-memory conversation state,
- `store` for persisted history, jobs, settings, and routines,
- `extension_manager` for extension lifecycle actions,
- `tool_registry` for the tools view,
- `job_manager` and `prompt_queue` for sandbox job controls,
- `scheduler` for follow-up agent job prompts,
- `skill_registry` and `skill_catalog` for skill management,
- `log_broadcaster` and `log_level_handle` for the logs view,
- `cost_guard` for gateway status telemetry, and
- `routine_engine` for manual routine triggers.

This state object is the reason the front end can expose such a broad control
surface without duplicating backend orchestration logic in the browser layer.

### 6.2 Chat ingress path

The main browser chat path is:

1. the user submits a message through the chat composer,
2. the browser sends `/api/chat/send`,
3. the handler builds `IncomingMessage`,
4. the gateway writes that message into `msg_tx`,
5. `ChannelManager` merges the gateway stream with other channel streams, and
6. the agent loop consumes the message as normal channel input.

The same basic normalization applies to:

- WebSocket message frames, and
- approval actions, which are serialized into message content so the agent can
  resume the paused thread through the same submission parser.

The browser can attach images, and the gateway decodes the Base64 payload into
inline `IncomingAttachment` values before handing the message to the agent.

### 6.3 Chat egress path

The main browser chat egress path is:

1. the agent finishes a response or emits a status update,
2. `GatewayChannel::respond()` or `GatewayChannel::send_status()` maps that
   output into `SseEvent`,
3. `SseManager` broadcasts the event to connected clients, and
4. `app.js` updates the relevant UI elements.

This mapping keeps the channel boundary explicit. The agent does not know how
the browser renders a tool card or a transcript bubble. It emits channel-level
status updates, and the gateway translates them into browser-specific event
shapes.

### 6.4 History, thread, and persistence integration

The browser's thread model spans both in-memory session state and durable
storage:

- `/api/chat/threads` lists conversations, including the pinned assistant
  thread stored in the database for the gateway channel;
- `/api/chat/history` prefers the in-memory thread when it is present and falls
  back to persisted conversation records when necessary;
- browser thread IDs are already UUID-shaped, so the gateway can rehydrate
  stored threads into active session state; and
- pending approvals are only in memory, but the history response can re-render
  them for the current thread while the process is still alive.

This is why the chat UI can survive page refreshes and reconnects without
forgetting the whole conversation, while still supporting richer in-memory turn
state than the persisted transcript format can represent.

### 6.5 Workspace integration

The memory tab is a thin browser client over the workspace subsystem. It does
not mirror a client-side filesystem. Instead it:

- lists directories and files from the workspace,
- reads and renders Markdown documents,
- edits and writes documents back through the workspace write API, and
- runs server-side search over workspace content.

The front end therefore exposes workspace memory as a repository-like document
browser backed by the host runtime's persistent memory layer.

### 6.6 Job and routine integration

The jobs and routines panels are browser views over backend process state, not
independent browser workflows.

For jobs, the browser combines:

- summary and list endpoints for current state,
- detail endpoints for metadata and restartability,
- persisted event-history endpoints for older activity,
- live SSE job events for current activity, and
- project-file endpoints for browsing generated job workspaces.

For routines, the browser reads persisted routine definitions and recent run
history, then delegates manual trigger, enable/disable, and delete operations
back to the backend.

### 6.7 Extension and skill integration

The extensions and skills tabs expose extension-management capabilities that are
already part of the host application:

- the extensions UI lists installed extensions from `ExtensionManager`,
  available catalogue entries from registry manifests, and registered tools
  from `ToolRegistry`;
- setup and auth flows call extension-manager operations and react to gateway
  SSE events such as `auth_required`, `auth_completed`, and
  `extension_status`;
- the skills UI talks to the skill registry and skill catalogue, including
  catalogue install, HTTPS `SKILL.md` or `.skill` URL install, local `.skill`
  upload, and removal flows that require explicit confirmation headers to avoid
  accidental mutating actions.

These tabs are effectively operational front ends for existing backend
subsystems rather than separate front-end business logic domains.

## 7. Key design constraints and trade-offs

The current front-end architecture makes several deliberate trade-offs.

### 7.1 Strengths of the current design

- One binary serves both UI assets and backend APIs, which simplifies local
  deployment.
- The browser reuses the same channel abstraction as other ingress surfaces,
  which keeps the agent boundary consistent.
- Static compile-time assets avoid packaging drift between host and UI.
- SSE gives the browser a simple, browser-native live-update path for rich
  streaming agent activity.
- Optional subsystem wiring through `GatewayState` lets the gateway expose
  feature subsets cleanly.

### 7.2 Known constraints

- The front end is tightly coupled to the host binary release cadence because
  the assets are embedded at compile time.
- The SPA is hand-written and DOM-oriented, which keeps dependencies low but
  makes large UI refactors more manual.
- SSE delivery is best-effort rather than durable; reconnect recovery depends
  on history and detail APIs.
- Pending tool approvals are stored in memory only and disappear on process
  restart.
- Streaming endpoints need query-string token support, which is a practical
  compromise for browser APIs even though normal bearer headers are preferred.
- The browser-side HTML sanitization is custom and intentionally lightweight,
  which keeps the bundle small but puts more burden on careful maintenance.

### 7.3 Boundary worth preserving

The most important architectural boundary is that the browser should remain a
projection of backend state, not a second source of truth. The current design
gets that mostly right:

- thread, job, routine, memory, extension, and skill state remain owned by the
  backend;
- the browser keeps only ephemeral presentation state and local caches; and
- live updates are derived from backend events rather than speculative client
  modelling.

That boundary is what allows the front end to stay broad in capability without
duplicating the agent's core state machine inside JavaScript.
