# Web Gateway Module

Browser-facing HTTP API and Server-Sent Events (SSE) / WebSocket real-time
streaming. The gateway is Axum-based, single-user, and protected by
bearer-token auth.

## File Map

| File | Role |
| --- | --- |
| `mod.rs` | Gateway builder, startup, and `WebChannel` implementation. |
| `server.rs` | `GatewayState`, `start_server()`, and router composition. |
| `types.rs` | Request/response DTOs and the `SseEvent` wire contract. |
| `sse.rs` | `SseManager` broadcast hub for connected SSE clients. |
| `ws.rs` | WebSocket handler and `WsConnectionTracker`. |
| `auth.rs` | Bearer token middleware. |
| `log_layer.rs` | Tracing layer for `/api/logs/events`. |
| `handlers/` | Handler modules grouped by API domain. |
| `openai_compat.rs` | OpenAI-compatible proxy routes. |
| `util.rs` | Shared handler helpers. |
| `static/` | Embedded single-page app assets. |

## API Routes

### Public

| Method | Path | Description |
| --- | --- | --- |
| GET | `/api/health` | Health check. |
| GET | `/oauth/callback` | OAuth callback for extension auth. |

### Chat

| Method | Path | Description |
| --- | --- | --- |
| POST | `/api/chat/send` | Send a message to the agent loop. |
| GET | `/api/chat/events` | SSE stream of agent events. |
| GET | `/api/chat/ws` | WebSocket alternative to SSE. |
| GET | `/api/chat/history` | Paginated turn history for a thread. |
| GET | `/api/chat/threads` | List assistant and regular threads. |
| POST | `/api/chat/thread/new` | Create a new thread. |
| POST | `/api/chat/approval` | Approve or deny a pending tool call. |
| POST | `/api/chat/auth-token` | Submit an extension auth token. |
| POST | `/api/chat/auth-cancel` | Cancel a pending auth flow. |

### Memory

| Method | Path | Description |
| --- | --- | --- |
| GET | `/api/memory/tree` | Workspace directory tree. |
| GET | `/api/memory/list` | List files at a path. |
| GET | `/api/memory/read` | Read a workspace file. |
| POST | `/api/memory/write` | Write a workspace file. |
| POST | `/api/memory/search` | Hybrid full-text search (FTS) and vector search. |

### Jobs

| Method | Path | Description |
| --- | --- | --- |
| GET | `/api/jobs` | List sandbox jobs. |
| GET | `/api/jobs/summary` | Aggregated job stats. |
| GET | `/api/jobs/{id}` | Job detail. |
| POST | `/api/jobs/{id}/cancel` | Cancel a running job. |
| POST | `/api/jobs/{id}/restart` | Restart a failed job. |
| POST | `/api/jobs/{id}/prompt` | Send a follow-up prompt to a job. |
| GET | `/api/jobs/{id}/events` | SSE stream for one job. |
| GET | `/api/jobs/{id}/files/list` | List files in a job workspace. |
| GET | `/api/jobs/{id}/files/read` | Read a job workspace file. |

### Skills

| Method | Path | Description |
| --- | --- | --- |
| GET | `/api/skills` | List installed skills. |
| POST | `/api/skills/search` | Search ClawHub and local skills. |
| POST | `/api/skills/install` | Install from ClawHub, URL/content, or `.skill` upload. |
| DELETE | `/api/skills/{name}` | Remove an installed skill. |

`POST /api/skills/install` accepts either JSON or multipart form data. JSON
requests may specify exactly one install source: inline `content`, a direct
`url`, or a catalogue `name`/`slug`. Multipart requests use a single file field
named `bundle` and are archive-only `.skill` uploads. Mutating requests still
require `X-Confirm-Action: true`.

### Extensions

| Method | Path | Description |
| --- | --- | --- |
| GET | `/api/extensions` | Installed extensions. |
| GET | `/api/extensions/tools` | All registered tools. |
| POST | `/api/extensions/install` | Install an extension. |
| GET | `/api/extensions/registry` | Available registry manifests. |
| POST | `/api/extensions/{name}/activate` | Activate an extension. |
| POST | `/api/extensions/{name}/remove` | Remove an extension. |
| GET/POST | `/api/extensions/{name}/setup` | Extension setup wizard. |

### Routines

| Method | Path | Description |
| --- | --- | --- |
| GET | `/api/routines` | List routines. |
| GET | `/api/routines/summary` | Aggregated routine stats. |
| GET | `/api/routines/{id}` | Routine detail and recent runs. |
| POST | `/api/routines/{id}/trigger` | Manually trigger a routine. |
| POST | `/api/routines/{id}/toggle` | Enable or disable a routine. |
| DELETE | `/api/routines/{id}` | Delete a routine. |
| GET | `/api/routines/{id}/runs` | List runs for one routine. |

### Settings

| Method | Path | Description |
| --- | --- | --- |
| GET | `/api/settings` | List all settings. |
| GET | `/api/settings/export` | Export settings as a map. |
| POST | `/api/settings/import` | Bulk-import settings from a map. |
| GET | `/api/settings/{key}` | Get one setting. |
| PUT | `/api/settings/{key}` | Set one setting. |
| DELETE | `/api/settings/{key}` | Delete one setting. |

### Other

| Method | Path | Description |
| --- | --- | --- |
| GET | `/api/logs/events` | Live log stream by SSE. |
| GET/PUT | `/api/logs/level` | Get or set log level at runtime. |
| GET | `/api/pairing/{channel}` | List pending pairing requests. |
| POST | `/api/pairing/{channel}/approve` | Approve a pairing request. |
| GET | `/api/gateway/status` | Server uptime, clients, and config. |
| POST | `/v1/chat/completions` | OpenAI-compatible Large Language Model (LLM) proxy. |
| GET | `/v1/models` | OpenAI-compatible model list. |

### Static And Project Files

| Method | Path | Description |
| --- | --- | --- |
| GET | `/` | Single-page app HTML. |
| GET | `/style.css` | App stylesheet. |
| GET | `/app.js` | App JavaScript. |
| GET | `/favicon.ico` | Favicon, cached for one day. |
| GET | `/projects/{project_id}/` | Redirect into the job browser. |
| GET | `/projects/{project_id}/{*path}` | Serve an authenticated job file. |

## SSE Event Types

The SSE contract is `#[serde(tag = "type")]` in `types.rs`.

| Type | When Emitted |
| --- | --- |
| `response` | Final text response from the agent. |
| `stream_chunk` | Streaming token or partial response. |
| `thinking` | Agent status update during reasoning. |
| `tool_started` | Tool call began. |
| `tool_completed` | Tool call finished. |
| `tool_result` | Tool output preview. |
| `status` | Generic status message. |
| `job_started` | Sandbox job created. |
| `job_message` | Message from sandbox worker. |
| `job_tool_use` | Tool invoked inside sandbox. |
| `job_tool_result` | Tool result from sandbox. |
| `job_status` | Sandbox job status update. |
| `job_result` | Sandbox job final result. |
| `approval_needed` | Tool requires user approval. |
| `auth_required` | Extension needs auth credentials. |
| `auth_completed` | Extension auth flow finished. |
| `extension_status` | WebAssembly (WASM) channel activation status changed. |
| `error` | Error from the agent or gateway. |
| `heartbeat` | SSE keepalive. |

Events use `#[serde(tag = "type")]`, so the wire format is
`{"type":"<variant>", ...fields}`. The SSE frame's `event:` field is set to
the same value as `type` for browser `addEventListener` use.

Over WebSocket, SSE events are wrapped as
`{"type":"event","event_type":"<variant>","data":{...}}`. Ping/pong uses
`{"type":"ping"}` and `{"type":"pong"}`. Client-to-server messages are defined
in `WsClientMessage` in `types.rs`.

To add a new SSE event, use the `add-sse-event` skill (`/add-sse-event`). It
scaffolds the Rust variant, serialization, broadcast call, and frontend
handler. Also add a matching arm to `WsServerMessage::from_sse_event()` in
`types.rs`.

## Auth

All protected routes require `Authorization: Bearer <GATEWAY_AUTH_TOKEN>`.
The token is set through the `GATEWAY_AUTH_TOKEN` environment variable.
Missing or incorrect tokens return 401. The `Bearer` prefix is compared
case-insensitively, as required by RFC 6750.

Because `EventSource` and WebSocket upgrades cannot set custom headers from
the browser, these endpoints also accept `?token=...`:

- `/api/chat/events`
- `/api/logs/events`
- `/api/chat/ws`

All other endpoints reject query-string tokens. When a new SSE or WebSocket
endpoint is added, its path must be registered in `allows_query_token_auth()`
in `auth.rs`.

If no `GATEWAY_AUTH_TOKEN` is configured, a random 32-character alphanumeric
token is generated at startup and printed to the console.

Chat send endpoints are rate limited to 30 messages per 60-second sliding
window.

## GatewayState

The shared state struct in `server.rs` holds references to all subsystems.
Fields are `Option<Arc<T>>` so the gateway can start even when optional
subsystems are disabled. Always null-check optional subsystems in handlers.

Key fields:

- `msg_tx`: sends messages to the agent loop after `Channel::start()`.
- `sse`: broadcast hub for handler-originated events.
- `ws_tracker`: tracks WebSocket connection count separately from SSE.
- `chat_rate_limiter`: 30 requests per 60-second sliding window.
- `scheduler`: injects follow-up messages into running agent jobs.
- `cost_guard`: exposes token usage and cost totals.
- `startup_time`: used to compute gateway uptime.
- `registry_entries`: registry manifests for the extensions API.

Subsystems are wired via `with_*` builder methods on `GatewayChannel` in
`mod.rs`. Each call rebuilds `Arc<GatewayState>`. Call these methods before
`start()`, not after.

## SSE / WebSocket Connection Limits

Both SSE and WebSocket share the same `SseManager` broadcast channel.

- Broadcast buffer: 256 events. Slow clients may miss events and should
  reconnect and re-fetch history.
- Max connections: 100 total SSE and WebSocket connections. Connections beyond
  the limit receive 503 or are dropped immediately.
- SSE keepalive: Axum's `KeepAlive` sends an empty event every 30 seconds to
  prevent proxy timeouts.
- WebSocket: each connection uses a sender task and a receiver loop. When the
  client disconnects, counters are decremented and the sender is aborted.

## CORS And Security Headers

Cross-Origin Resource Sharing (CORS) is restricted to the gateway's own origin,
including the same IP and port and `localhost` on the same port. Allowed
methods are GET, POST, PUT, and DELETE. Allowed headers are `Content-Type` and
`Authorization`. Credentials are
allowed.

All responses include:

- `X-Content-Type-Options: nosniff`
- `X-Frame-Options: DENY`

Request bodies are limited to 10 MB with
`DefaultBodyLimit::max(10 * 1024 * 1024)`. Larger payloads return 413. Skill
bundle uploads still flow through the stricter `.skill` bundle validator after
request extraction.

## Pending Approvals

Tool approval state is in-memory only and is not persisted to the database.
Server restart clears all pending approvals. The `pending_approval` field in
`HistoryResponse` is re-populated on thread switch from in-memory state.

## Adding A New API Endpoint

1. Define request/response types in `types.rs`.
2. Implement the handler in the appropriate `handlers/*.rs` file.
3. Register the route in that module's route helper, then merge it from
   `start_server()` in `server.rs`.
4. For an SSE or WebSocket endpoint, add its path to
   `allows_query_token_auth()` in `auth.rs`.
5. If it requires a new `GatewayState` field, add it to the struct and to
   `GatewayChannel::new()` and `rebuild_state()` in `mod.rs`, then add a
   `with_*` builder method.
