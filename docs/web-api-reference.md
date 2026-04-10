# Axinite web API reference

This document is a stub for the HTTP API exposed by the web gateway.
The in-code route table in `src/channels/web/CLAUDE.md` remains the
authoritative index until this document is completed in a future PR.

The gateway listens on the port set by `PORT` (default 3000) and requires
authentication for all `/api/` routes via the configured auth middleware.

## 1. Chat endpoints

All chat routes are mounted under `/api/chat/` and require an authenticated
session.

### 1.1 Fetch chat history

```http
GET /api/chat/history
```

Returns paginated conversation history for a thread.

**Query parameters:**

| Parameter | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `thread_id` | UUID string | No | Session's active thread | Target conversation thread |
| `limit` | integer | No | 50 | Maximum number of turns to return. Must be ≥ 1 and ≤ 200. |
| `before` | string | No | — | Opaque pagination cursor in the form `<RFC 3339 timestamp>\|<UUID>` or `<RFC 3339 timestamp>` |

**Validation rules for `limit`:**

- `limit=0` returns `400 Bad Request` with the body
  `conversation message pagination limit must be > 0`.
- Omitting `limit` defaults to 50.
- Values greater than 200 are clamped to 200.
- Values less than 0 are rejected with the same 400 response as zero.

**Response (200 OK):**

```json
{
  "thread_id": "uuid",
  "turns": [TurnInfo],
  "has_more": false,
  "oldest_timestamp": "2026-04-10T12:00:00Z|uuid",
  "pending_approval": null
}
```

**Error responses:**

| Status | Condition | Body |
| --- | --- | --- |
| 400 | `limit` is zero or negative | `conversation message pagination limit must be > 0` |
| 400 | `before` cursor is malformed | `Invalid 'before' cursor` |
| 400 | `thread_id` is not a valid UUID | `Invalid thread_id` |
| 404 | No `thread_id` provided and no active thread | `No active thread` |
| 404 | Thread not owned by this user and not in-memory | `Thread not found` |
| 500 | Internal database or serialisation error | Error description |

### 1.2 Other chat endpoints

These endpoints are documented in `docs/chat-model.md` and will be expanded
here in a future PR.

| Method | Path | Purpose |
| --- | --- | --- |
| POST | `/api/chat/send` | Send a user message |
| GET | `/api/chat/events` | SSE stream of chat events |
| GET | `/api/chat/ws` | WebSocket duplex channel |
| GET | `/api/chat/threads` | List conversation threads |
| POST | `/api/chat/thread/new` | Create a new thread |

## 2. Remaining routes

The full route table (jobs, memory, routines, settings, skills, extensions,
and static assets) will be added in a future PR. For now, see
`src/channels/web/CLAUDE.md` for the authoritative route index.
