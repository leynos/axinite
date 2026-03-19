# RFC 0008: Implement the WebSocket Responses API in axinite with agentic compaction and multi-turn tool calling

## Preamble

- **RFC number:** 0008
- **Status:** Proposed
- **Created:** 2026-03-13

## Executive summary

Axinite currently integrates “OpenAI-compatible” providers through a Chat Completions–style protocol (`open_ai_completions`), configured in `providers.json` (the `openai` provider defaults to `https://api.openai.com/v1` and a default model `gpt-5-mini`). [^1] This pathway is built around Axinite’s `LlmProvider` trait, which exposes synchronous request/response methods (`complete`, `complete_with_tools`) but no streaming interface. [^2]

Axinite does have “context compaction” today, but it is Axinite-native: it summarizes or truncates old turns (and optionally writes a summary to workspace) before continuing, i.e. it does *not* use OpenAI’s encrypted “compaction item” mechanism. [^3] Concretely, Axinite’s compactor generates a summary by making an LLM call with a system summarization prompt and then rewrites the thread history. [^4]

OpenAI’s WebSocket Responses API introduces three capabilities that don’t fit cleanly into Axinite’s current “Chat Completions–shaped” adapter:

- A persistent WebSocket transport (`wss://api.openai.com/v1/responses`) where each turn begins by sending a `response.create` event whose payload mirrors `POST /responses` (excluding transport-only fields like `stream`). [^5]
- Stateful continuation via `previous_response_id`, including explicit error semantics (`previous_response_not_found`) and a WebSocket-local cache that only retains the most recent previous-response state for low-latency continuation (no multiplexing; one in-flight response per connection; 60-minute connection lifetime). [^6]
- Server-side (agentic) compaction via `context_management` + `compact_threshold`, which emits an opaque encrypted compaction item into the response stream/output and allows the conversation to continue with fewer tokens. [^7]

To add a *first-class* WebSocket Responses backend that supports GPT‑5.4 agentic compaction and multi-turn tool calling, Axinite needs a new provider protocol and a new set of abstractions around:

- “Input items” (Responses API) rather than only `ChatMessage[]`. [^8]
- Streaming event handling (WebSocket mode emits the same event model used for streaming Responses). [^9]
- Durable state: storing `previous_response_id`, optional durable `conversation` IDs, and encrypted compaction items to support restarts/reconnects without silently losing context. [^10]

## Repository audit

Axinite is written in Rust (so “language/runtime unspecified” does not reflect the current implementation reality in this repo), but the design constraints below remain language-agnostic. [^11]

Axinite’s existing provider stack and agent loop are organized around a small set of load-bearing modules:

| Area | What it does today | Why it matters for Responses WS |
|---|---|---|
| `src/llm/provider.rs` | Defines `ChatMessage`, tool call representation and sanitation; defines the `LlmProvider` trait (`complete`, `complete_with_tools`) used by the reasoning engine. [^12] | The Responses WS backend needs either (a) a new provider trait that supports streaming + input items, or (b) a compatibility shim that projects Responses into Axinite’s current `complete[_with_tools]` contract. |
| `src/llm/rig_adapter.rs` | Bridges Axinite tool definitions into a rig-core model interface; normalizes JSON Schema to comply with OpenAI “strict mode” function calling; converts Axinite messages into rig messages; extracts tool calls from completion responses. [^13] | This adapter targets “Chat Completions shape” tool calls, not Responses input items/events. Reuse is limited to tool schema normalization logic. |
| `src/llm/reasoning.rs` | Builds system prompts, constructs `ToolCompletionRequest`s from `ReasoningContext`, calls `llm.complete_with_tools`, and returns either text or `ToolCalls` to the agentic loop. [^14] | This component assumes the LLM call returns complete results (not streamed) and assumes tool calls come back as a list of `ToolCall`s in one response turn. |
| `src/agent/agentic_loop.rs` | Unified iteration engine that delegates “call LLM”, “execute tool calls”, and “auto-compaction/cost/rate-limit concerns” to a `LoopDelegate`. [^15] | This is a good insertion point: the delegate can own a stateful Responses WS session and can hide streaming/continuations behind its `call_llm` implementation. |
| `src/agent/session.rs` | Persistent “thread/turn” model; `Thread.messages()` serializes turns into an OpenAI-style message sequence including `assistant_with_tool_calls` then `tool_result` messages. [^16] | Multi-turn tool calling with Responses requires preserving OpenAI `call_id` values; Axinite currently synthesizes tool IDs when serializing (`turn{n}_{i}`), which is incompatible with Responses tool outputs unless extended. [^17] |
| `src/agent/compaction.rs` | Axinite-native compaction: truncate, summarize, and optionally write summaries to workspace; summary generation is an LLM call with a summarization prompt. [^18] | This compaction can conflict with server-side compaction. A Responses WS backend should usually disable Axinite summarization in favour of `context_management` compaction, or apply it only as a fallback. [^19] |
| `providers.json` + `src/llm/registry.rs` | Declares provider protocols and selection; `openai` uses `protocol: open_ai_completions` and model/env settings; registry deserializes built-ins and provides selection helpers. [^20] | Adding a WebSocket Responses backend likely means adding a new `ProviderProtocol` and new provider config keys (e.g., enable `store`, compaction settings, conversation strategy). |

Axinite’s own feature parity matrix indicates it already supports “Context compaction” (marked as “Auto summarization”) and implements an “OpenAI protocol” gateway at `/v1/chat/completions`, but it does not claim Responses/WebSocket support. [^21]

Practical extension points in-repo:

- Add a new provider protocol enum variant (e.g. `OpenAiResponsesWebSocket`) alongside the existing `OpenAiCompletions` mapping. [^22]
- Create a new provider implementation that speaks WebSocket Responses (do not try to wedge this into `RigAdapter` unless losing native streaming/event semantics is acceptable). [^23]
- Extend the `Thread`/`TurnToolCall` model to store provider-native call identifiers required by the Responses tool lifecycle (`call_id`). [^24]
- Implement compaction strategy selection at the agent-loop delegate layer (delegate `call_llm` explicitly mentions it should handle “rate limiting, auto-compaction, cost tracking”). [^25]

## Feature gap analysis

### Current vs required capability matrix

| Capability | Axinite today | Required for WebSocket Responses backend | Primary gap driver |
|---|---|---|---|
| Transport | HTTP-style request/response (provider-specific); no provider streaming interface in `LlmProvider`. [^26] | Maintain a persistent WebSocket connection to `wss://api.openai.com/v1/responses`; send `response.create` events; consume streaming server events; enforce “one in-flight response” constraint. [^27] | LlmProvider contract is non-streaming and stateless. [^28] |
| Stateful continuation | Axinite persists context by replaying/summarizing `ChatMessage[]` from `Thread.turns`. [^29] | Use `previous_response_id` (and optionally `conversation`) to carry state; handle connection-local cache semantics and `previous_response_not_found` on reconnect in ZDR/store=false mode. [^30] | Responses API state model differs from Axinite’s transcript replay model. [^31] |
| Tool calling | Axinite expects tool calls as `ToolCall{id,name,args}` from `complete_with_tools`; serializes tool calls into an “assistant tool_calls” message preceding tool results. [^32] | Responses uses `function_call` output items with a `call_id`; tool outputs are `function_call_output` input items referencing that `call_id`. [^33] | Axinite does not persist provider-owned `call_id` values (it synthesizes IDs later). [^34] |
| Multi-turn tool calling loop | Supported at application level: agentic loop iterates, executes tools, appends tool results, calls LLM again. [^35] | Same loop, but “call next turn” becomes “send a new `response.create` with `previous_response_id` + new `function_call_output` items (and possibly user input)”. [^36] | The loop must own a stateful Responses session and must translate tool lifecycle semantics. [^37] |
| Agentic compaction | Axinite supports “auto summarization” (LLM summarizes transcript) and/or truncation. [^38] | Enable server-side compaction via `context_management` + `compact_threshold`; preserve opaque compaction items and avoid manual pruning when using `previous_response_id`. [^39] | Compaction item is encrypted and not representable as a simple `ChatMessage`. [^40] |
| Streaming events | No first-class streaming interface in provider API; internal reasoning assumes full response. [^41] | Parse and act on streaming events (e.g. `response.output_text.delta`, `response.function_call_arguments.delta`, `response.completed`, `error`). [^42] | Need streaming event state machine and buffering. [^43] |
| Conversation IDs | Axinite has internal `Session`/`Thread` IDs. [^44] | Optionally bind an Axinite thread to an OpenAI `conversation` ID for durable storage across sessions/devices/jobs. [^45] | Requires new persistence + configuration and changes to retention semantics. [^46] |
| Auth | Axinite uses per-provider env vars and may supply extra headers. [^47] | WebSocket handshake must include `Authorization: Bearer …` header; optionally support org/project scoping headers in a provider-independent way. [^48] | Need a WS client that supports headers and renewals. [^49] |
| Rate limits & retries | Axinite has retry/circuit breaker modules, but provider-specific behaviour varies. [^50] | Explicitly handle 429/5xx; respect rate limit headers; apply exponential backoff with jitter; avoid retry storms. [^51] | WebSocket adds new failure modes (disconnects, connection lifetime limits). [^52] |

### Prioritized gaps and how they map to OpenAI Responses features

Axinite’s abstractions line up best with Responses if the Responses WS backend is treated as a *stateful session object* owned by the agent-loop delegate (not as a “pure function” provider).

Priority order:

- **State ownership (highest priority):** WebSocket mode supports `previous_response_id` chaining and keeps the most recent response cached in-memory on the connection. If Axinite calls providers in a stateless way, it will frequently hit `previous_response_not_found` in `store=false` mode after any reconnect, because there is no persisted fallback. [^53]
  This pushes design toward: one WS connection per active Axinite thread (or per worker that handles that thread), plus explicit policy for reconnect+resume. [^54]

- **Tool lifecycle fidelity:** Responses uses `call_id` as the join key for `function_call_output`. Axinite currently reconstructs tool-call messages and invents tool IDs when serializing turns, which works for Chat Completions (because Axinite controls both sides) but fails for Responses because OpenAI validates the `call_id` linkage. [^55]
  OpenAI call IDs need to be stored per tool call, rather than synthesized later.

- **Compaction item persistence:** Server-side compaction emits an opaque encrypted compaction output item; for stateless chaining, outputs must be appended “as usual” and items before the latest compaction item may be dropped to keep requests smaller, but `previous_response_id` mode must not prune manually. [^56]
  Axinite must (a) detect compaction items in streamed outputs, and (b) persist them to support fallback stateless replay if the WS cache is lost.

- **Streaming event processing:** WebSocket mode says “server events and ordering match the existing Responses streaming event model.” [^57]
  The platform reference enumerates relevant event types including `response.output_text.delta`, `response.function_call_arguments.delta`, `response.output_item.added/done`, and `error`. [^58]
  Axinite needs an event-driven parser that can build: final assistant text, function-call argument buffers, and a list of emitted output items.

- **Conversation ID support (policy choice):** Storing responses (`store=true`) enables hydration of older response IDs; conversations persist items without the 30-day TTL applied to response objects, which is attractive for durability but changes data retention properties. [^59]
  Axinite needs explicit configuration: ZDR-ish ephemeral mode vs durable mode.

## Requirements

### Functional requirements

Axinite should implement a new provider backend that supports the following end-to-end behaviours.

**Provider selection and configuration**

- A new provider protocol (e.g. `open_ai_responses_ws`) selectable via Axinite’s provider registry and `providers.json` conventions. [^60]
- Configuration for:
  - `base_url` (default `https://api.openai.com/v1`, but WS URL must resolve to `wss://…/v1/responses`). [^61]
  - `api_key` env var, plus optional extra headers (Axinite already supports `extra_headers_env` concept at registry level). [^62]
  - State strategy: `previous_response_id` chaining vs stateless input-array chaining vs Conversations API binding. [^63]
  - Retention policy: `store=true/false` (and how that interacts with WS cache/hydration). [^64]
  - Compaction settings: whether to enable `context_management` compaction and what thresholds to use (per model/worker). [^65]

**WebSocket session management**

- Establish a WS connection with `Authorization: Bearer …` header. [^66]
- Enforce one in-flight response per connection; no multiplexing; either queue turns or maintain a pool of connections (one per active thread). [^67]
- Handle connection lifetime limit (60 minutes): reconnect and recover according to storage strategy. [^68]
- Support “warmup” (`response.create` with `generate:false`) as an optimization if Axinite can predict tools/instructions for upcoming turns. [^69]

**Responses request construction**

- Construct a `response.create` payload that mirrors `POST /responses` (excluding streaming flags). [^70]
- Include:
  - `model` (e.g. `gpt-5.4` when configured).
  - `input` as an array of Responses input items; at minimum `message` items for user/system messages and `function_call_output` items for tool results during continuation. [^71]
  - `tools` definitions (function tools) derived from Axinite tool registry; preserve JSON schema constraints consistent with OpenAI function calling. [^72]
  - `tool_choice`, `parallel_tool_calls` mapping from Axinite’s notions (`auto/required/none`). [^73]
  - `previous_response_id` when continuing. [^74]
  - `context_management: [{type:"compaction", compact_threshold: …}]` when agentic compaction enabled. [^75]

**Streaming response handling**

- Consume WS events and build a coherent “turn result”:
  - Buffer `response.output_text.delta` to form final user-visible text. [^76]
  - Buffer `response.function_call_arguments.delta` to form complete JSON arguments for tool calls. [^77]
  - Capture output items (`response.output_item.added` / `done`) including:
    - `function_call` items (tool call requests) with `call_id`. [^78]
    - Compaction items emitted during generation when `context_management` triggers. [^79]
  - Terminate on `response.completed` / `response.failed` / `response.incomplete` and handle `error` events as hard failures (or retryable where appropriate). [^80]

**Multi-turn tool calling lifecycle**

- When the model emits one or more `function_call` items:
  - Map each tool call to Axinite `ToolCall` objects.
  - Execute each tool (possibly in parallel, subject to Axinite’s approvals/sandbox constraints).
  - Send a subsequent `response.create` event containing:
    - `previous_response_id` = last response id,
    - `input` includes `function_call_output` items, each referencing the original `call_id`, and optionally a follow-up user message depending on Axinite’s loop semantics. [^81]
- Preserve any “reasoning items” required for multi-turn tool calling in stateless mode (OpenAI notes that reasoning models may require returning reasoning items alongside tool outputs). [^82]
  In `previous_response_id` mode, the server-side chain should already retain these, but Axinite should not assume that unless validated in integration tests.

**Conversation identifiers**

- Support two modes:
  - `previous_response_id` chaining (ephemeral, WS-cache-sensitive). [^83]
  - Durable conversation mode: create and store an OpenAI conversation ID, pass it on subsequent responses, and persist the binding in Axinite thread metadata. [^84]

### Non-functional requirements

**Reliability and error handling**

- Implement structured retry policies:
  - 429: respect pacing; use exponential backoff with jitter; consider per-project token/request budgets. [^85]
  - 5xx / 503: retry with backoff; treat as transient. [^86]
  - WS-specific: on `websocket_connection_limit_reached`, reconnect and continue according to strategy. [^87]
  - On `previous_response_not_found`, fall back to: (a) start a new chain with full stateless input, or (b) hydrate via `store=true`, or (c) refuse with an actionable error if configured for ZDR strictly. [^88]

**Security**

- Keep API keys in env/secret manager; never store raw keys in thread/session records.
- Ensure tool outputs passed into `function_call_output.output` don’t leak secrets unnecessarily (Axinite already truncates tool results for context size; apply similar hygiene for outputs sent back to the model). [^89]

**Telemetry and observability**

- Emit structured spans/metrics per response:
  - Response id, previous_response_id, model, token usage, compaction triggered, number of tool calls, retries, WS reconnects.
- Surface rate limit headers (HTTP) where applicable; for WS, capture rate limits via any events/metadata received and treat them as signals to throttle. [^90]

**Performance and scalability**

- Use connection reuse: one WS per active thread to exploit the “most recent response” cache for low-latency continuation. [^91]
- Avoid sending full transcripts per turn in `previous_response_id` mode; send only new input items. [^92]
- If stateless operation is required, drop input items before the latest compaction item to keep payloads smaller (but never prune when using `previous_response_id`). [^93]

**Testing**

- Unit tests for:
  - Event parser (delta assembly, output item reconstruction, compaction item detection).
  - Call-id preservation and tool output formatting.
  - Reconnect behaviours and error-to-fallback mapping.
- Integration tests with a mock WS server that replays a deterministic event trace resembling OpenAI’s event model. [^94]
- End-to-end tests gated in CI that run against OpenAI in a “sandbox project” (if feasible) with strict cost/rate limiting. [^95]

## Technical design

### Architecture overview

The core design choice: implement a **stateful Responses session** owned by the agent-loop delegate, and keep Axinite’s generic reasoning loop unchanged (it still calls `delegate.call_llm()` and then dispatches tool execution). [^96]

Mermaid overview:

```mermaid
graph TD
  A[AgenticLoop] --> B[LoopDelegate.call_llm()]
  B --> C[ResponsesWsSession]
  C --> D[WS Client: wss /v1/responses]
  D --> C
  C --> E[Event Parser / State Machine]
  E --> F[TurnResult: text + tool_calls + output_items]
  B --> G[Tool Runner / Sandbox]
  G --> C
  C --> H[(DB / Persistence)]
  B --> I[Axinite Thread/Turn Model]
```

Design intent:

- `ResponsesWsSession` encapsulates:
  - WS connection lifecycle
  - last `previous_response_id`
  - optional OpenAI `conversation` id
  - compaction configuration
  - output-item log (including compaction items) for persistence and replay [^97]
- The delegate translates between Axinite’s `ReasoningContext` (messages/tools) and Responses’ `input` + `tools` payloads. [^98]

### Sequence diagram for multi-turn tool calling

This sequence assumes `previous_response_id` mode (preferred), and matches WebSocket mode guidance: send next `response.create` with only new items. [^99]

```mermaid
sequenceDiagram
  participant U as User
  participant L as AgenticLoop/Delegate
  participant S as ResponsesWsSession
  participant O as OpenAI WS /responses
  participant T as Tool Runner

  U->>L: user_message("Do X")
  L->>S: create_turn(input_items=[message:user], tools=[...], ctx_mgmt=compaction)
  S->>O: WS send response.create(model, store, input, tools, context_management)
  O-->>S: response.created / response.in_progress
  O-->>S: response.function_call_arguments.delta (stream args)
  O-->>S: response.output_item.done (function_call with call_id)
  O-->>S: response.completed (no final text yet OR partial)
  S-->>L: tool_calls=[(call_id,name,args)], partial_text?

  L->>T: execute tools with args
  T-->>L: tool outputs
  L->>S: continue(previous_response_id, input_items=[function_call_output(call_id,output)])
  S->>O: WS send response.create(previous_response_id, input=[...])
  O-->>S: response.output_text.delta (final answer)
  O-->>S: response.completed
  S-->>L: final_text
  L-->>U: render final_text
```

### Sequence diagram for agentic compaction

Server-side compaction triggers when the rendered token count crosses the configured threshold; the server emits a compaction output item in the same response stream and prunes context before continuing inference. [^100]

```mermaid
sequenceDiagram
  participant L as Delegate
  participant S as ResponsesWsSession
  participant O as OpenAI WS /responses
  participant DB as Storage

  L->>S: response.create(context_management.compact_threshold=...)
  S->>O: WS send response.create(...)
  O-->>S: response.output_text.delta
  O-->>S: response.output_item.done (compaction item emitted)
  S->>DB: persist compaction item + response linkage
  O-->>S: response.output_text.delta (continues after prune)
  O-->>S: response.completed (response_id=resp_n)
  S->>DB: persist resp_n, update previous_response_id=resp_n
  S-->>L: completed turn + compaction_was_emitted=true
```

### Data model mapping

Axinite currently models conversations as a sequence of `ChatMessage` objects derived from turns, including “assistant tool_calls” messages and “tool result” messages. [^101] Responses uses an “input items / output items” model. [^102]

A practical mapping for compatibility:

| Axinite concept | Responses API representation | Notes |
|---|---|---|
| System/user/assistant message | `{"type":"message","role":"user|assistant|system","content":[{"type":"input_text","text":...}]}` | WebSocket examples show `message` items inside `input`. [^103] |
| Tool call request from model | Output item `{"type":"function_call","call_id":...,"name":...,"arguments":"{...}"}` | `call_id` is the join key for outputs. [^104] |
| Tool output back to model | Input item `{"type":"function_call_output","call_id":...,"output":"..."}` | WebSocket docs demonstrate this in continuation. [^105] |
| Axinite compaction summary | Prefer: Responses “compaction item” emitted by server when `context_management` triggers | Compaction item is opaque and should be stored, not interpreted. [^106] |

### Schema examples

**Client → server (`response.create` event)**
(Example shape; fields shown in OpenAI docs. WebSocket mode indicates you send this as a WebSocket message with `"type":"response.create"`.) [^107]

```json
{
  "type": "response.create",
  "model": "gpt-5.4",
  "store": false,
  "previous_response_id": "resp_123",
  "context_management": [{ "type": "compaction", "compact_threshold": 200000 }],
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "search_docs",
        "description": "Search internal docs",
        "parameters": { "type": "object", "properties": { "q": { "type": "string" } } }
      }
    }
  ],
  "tool_choice": "auto",
  "parallel_tool_calls": true,
  "input": [
    {
      "type": "function_call_output",
      "call_id": "call_abc",
      "output": "{\"hits\": 5}"
    },
    {
      "type": "message",
      "role": "user",
      "content": [{ "type": "input_text", "text": "Continue." }]
    }
  ]
}
```

**Server → client events (selected)**
Event types in the Responses streaming model include `response.output_text.delta`, `response.function_call_arguments.delta`, `response.output_item.added/done`, `response.completed`, and `error`. [^108]

### Persistence schema

Axinite already persists session/thread state and compaction summaries in its own model. [^109] To support Responses WS robustly, add a small provider-specific persistence layer that captures the minimum recovery set:

1. **Thread binding state**
   - `openai_previous_response_id` (nullable)
   - `openai_conversation_id` (nullable; only if using Conversations API) [^110]
   - `store_flag` and compaction configuration

2. **Response log**
   - `openai_response_id`, timestamps, model
   - raw/filtered output items (including compaction items)
   - derived tool calls with `call_id` [^111]

3. **Compaction item ledger**
   - store the opaque compaction item JSON blob exactly as received
   - index by thread + emitted-at response id
   - mark as “last_compaction_item” for stateless replay trimming [^112]

A relational sketch (names illustrative):

```sql
-- Conversations/threads are Axinite concepts; store OpenAI linkage in thread metadata or a side table.
CREATE TABLE thread_openai_state (
  thread_id UUID PRIMARY KEY,
  openai_conversation_id TEXT NULL,
  openai_previous_response_id TEXT NULL,
  store_enabled BOOLEAN NOT NULL,
  compaction_enabled BOOLEAN NOT NULL,
  compaction_threshold INTEGER NULL,
  updated_at TIMESTAMP NOT NULL
);

CREATE TABLE openai_response_log (
  id UUID PRIMARY KEY,
  thread_id UUID NOT NULL,
  openai_response_id TEXT NOT NULL,
  previous_response_id TEXT NULL,
  model TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at TIMESTAMP NOT NULL,
  response_json JSONB NOT NULL
);

CREATE TABLE openai_response_item (
  id UUID PRIMARY KEY,
  openai_response_log_id UUID NOT NULL,
  item_type TEXT NOT NULL,          -- message|function_call|function_call_output|compaction|...
  call_id TEXT NULL,                -- for function_call / function_call_output
  item_json JSONB NOT NULL,
  output_index INTEGER NULL,
  created_at TIMESTAMP NOT NULL
);
```

### Migration strategy

Axinite’s thread model currently stores tool calls without preserving provider-native IDs (it synthesizes stable IDs during `Thread.messages()` rendering). [^113] For Responses WS, OpenAI `call_id` values need to be preserved long enough to send `function_call_output` items in the next turn. [^114]

Minimal-impact migration approach:

- Extend `TurnToolCall` to include `provider_call_id: Option<String>` (or a generic `call_id`) and populate it when the Responses backend returns tool calls.
- Update `Thread.messages()` so that:
  - if `provider_call_id` is present, use it rather than generating `turn{n}_{i}` IDs when constructing tool call messages (for any adapters that still rely on message replay). [^115]
- Preserve backwards compatibility by treating missing IDs as legacy and continuing to synthesize.

## Implementation plan

Effort estimates are coarse (low/med/high) because runtime/language constraints were specified as open-ended, even though Axinite is Rust. [^116]

### Stepwise tasks

| Task | Effort | Key outputs |
|---|---|---|
| Add new provider protocol for Responses WS | Med | `ProviderProtocol` enum update; `providers.json` schema extension; configuration plumbing. [^117] |
| Implement `ResponsesWsSession` connection manager | High | WS connect/auth header support; reconnection policy; sequential in-flight enforcement; 60-minute rotation. [^118] |
| Implement streaming event parser/state machine | High | Correct handling of event types and deltas; output item reconstruction; error framing. [^119] |
| Implement request builder for `response.create` | Med | Map Axinite message/tool state into Responses `input` + `tools`; map `tool_choice`/`parallel_tool_calls`; insert `context_management`. [^120] |
| Tool call lifecycle integration | High | Translate `function_call` items into Axinite `ToolCall`s; preserve `call_id`; emit `function_call_output` on continuation; support multiple tool calls per turn and parallelism config. [^121] |
| Integrate agentic compaction | Med | Enable `context_management.compact_threshold`; detect compaction items; persist them; ensure the delegate disables Axinite summarization compaction when enabled. [^122] |
| Extend thread/turn persistence for call IDs and OpenAI state | Med | Add `call_id` storage and per-thread OpenAI linkage (`previous_response_id`, optional `conversation` id). [^123] |
| Add robust retries, throttling and backoff | Med | Respect rate limit headers (HTTP); handle 429/5xx; ensure WS retries don’t create retry storms. [^124] |
| Tests + CI | High | Deterministic mock WS traces; integration tests that exercise tool loops and compaction; regression tests for legacy compaction. [^125] |
| Rollout plan with feature flags | Low/Med | Per-provider opt-in; fallback to `open_ai_completions` on failure; operational dashboards. [^126] |

### Test cases

**Unit tests**

- Event parsing:
  - Assemble text from `response.output_text.delta` and terminate at `response.output_text.done`/`response.completed`. [^127]
  - Assemble tool arguments from `response.function_call_arguments.delta` and emit a parsed JSON object on `.done`. [^128]
  - Detect and persist compaction items whenever `context_management` compaction triggers. [^129]
- Tool lifecycle:
  - Ensure tool outputs reference the exact `call_id` observed in `function_call`. [^130]
  - Validate behaviour when the model calls multiple tools in one turn (and when `parallel_tool_calls=false`). [^131]

**Integration tests (mock WS server)**

- Happy path: user → tool call → tool output → final answer, with correct `previous_response_id` chaining. [^132]
- Compaction path: server emits a compaction item mid-turn; session persists it and continues. [^133]
- Failure path:
  - `previous_response_not_found` triggers fallback strategy.
  - `websocket_connection_limit_reached` forces reconnect and recovery behaviour. [^134]

**End-to-end tests (real OpenAI project, optional)**

- Exercise a long multi-tool task that triggers compaction at least once.
- Force reconnect mid-chain and verify recovery semantics under both `store=true` and `store=false`. [^135]

### CI checks and rollout checklist

Add the rollout flag as a documented environment-variable entry:

<!-- markdownlint-disable MD013 MD060 -->
| Variable | Meaning | Default or rule |
|----------|---------|-----------------|
| `OPENAI_RESPONSES_WS_ENABLED` | Enables WebSocket-based OpenAI Responses API for streaming responses. | Default: off. Treat as a rollout-controlled flag or provider configuration knob. |
<!-- markdownlint-enable MD013 MD060 -->

- Add a feature flag: `OPENAI_RESPONSES_WS_ENABLED` (or provider config knob)
  default off.
- Add “contract tests” that compare the tool call loop semantics against existing Chat Completions backend (for equivalent prompts).
- Rollout:
  - Canary enable for a subset of sessions/threads or only for GPT‑5.4 model selection.
  - Add dashboards for: WS reconnect rate, `previous_response_not_found` incidence, compaction trigger frequency, tool-call latency, 429 rate-limit errors. [^136]

Mermaid Gantt for an illustrative phased timeline (durations indicative):

```mermaid
gantt
  title WebSocket Responses backend implementation phases
  dateFormat  YYYY-MM-DD
  axisFormat  %d %b

  section Foundations
  Provider protocol + config plumbing     :a1, 2026-03-16, 10d
  Thread model call_id persistence        :a2, after a1, 10d

  section Core WebSocket Backend
  WS session manager + reconnect policy   :b1, after a1, 15d
  Streaming event parser/state machine    :b2, after b1, 20d
  Request builder + tools mapping         :b3, after b1, 15d

  section Agent Features
  Multi-turn tool call loop integration   :c1, after b2, 20d
  Agentic compaction integration          :c2, after b2, 10d

  section Quality and Rollout
  Integration + mock WS tests             :d1, after c1, 15d
  Rate limit/retry hardening              :d2, after b1, 10d
  Canary rollout + dashboards             :d3, after d1, 10d
```

## Risks, mitigations, and backward compatibility

### Double-compaction risk

If Axinite enables server-side compaction (`context_management`) and also runs its own summarization compaction, it will effectively “compress twice” and can lose important state, especially around tool usage and intermediate reasoning that OpenAI’s compaction item intends to preserve in an opaque format. [^137]

Mitigation:

- When the provider is Responses WS and compaction is enabled, disable Axinite’s summarization compaction strategy for that thread, or restrict it to a fallback used only after a hard “cannot continue chain” event. [^138]

### Loss of continuity on reconnect in `store=false` mode

WebSocket mode keeps only the most recent response state in a connection-local cache. If the connection drops and responses aren’t stored, continuing with an older `previous_response_id` returns `previous_response_not_found`. [^139]

Mitigation options (make explicit in configuration):

- **Durable mode:** set `store=true` so the server can hydrate older response IDs, at the cost of storing response objects for up to 30 days by default. [^140]
- **Conversation mode:** use a `conversation` ID so items persist beyond the response TTL (note: this changes retention semantics materially). [^141]
- **Stateless fallback:** persist compaction items and enough of the recent transcript to restart a new chain without the full history (drop items before the latest compaction item). [^142]

### Tool call ID mismatch

Axinite currently synthesizes tool call IDs (`turn{n}_{i}`) when building `ChatMessage` tool call sequences from stored turns. That will not match OpenAI’s `call_id` values for Responses tool calls. [^143]

Mitigation:

- Store provider-native `call_id` and use it when generating tool output items.
- Keep synthetic IDs only for legacy compatibility paths. [^144]

### Concurrency and head-of-line blocking

A single WebSocket connection runs `response.create` sequentially; it does not support multiplexing, and multiple connections are required to run parallel responses. [^145]

Mitigation:

- Bound WS connections by Axinite “active thread” count (pool per worker).
- Queue per-thread messages and enforce backpressure to avoid OOM when tools are slow.

### Rate limiting and retry storms

OpenAI rate limits apply at organization and project level; headers expose current limits/remaining/reset. [^146] Over-aggressive retry can worsen 429s because failed requests still count against per-minute limits. [^147]

Mitigation:

- Global token/request budgeter per project and per worker.
- Exponential backoff with jitter; cap retries; respect reset headers. [^148]

## Recommended libraries, concurrency model, and pseudocode

### Recommended libraries

Because the language/runtime was specified as open-ended, choose libraries per deployment environment:

- **Rust (Axinite-native):** `tokio` + a WS client that supports custom headers (e.g. `tokio-tungstenite`), `serde_json` for event decoding, and Axinite’s existing retry/circuit breaker modules for resilience. [^149]
- **Python:** use a WS client that supports headers and async iteration; OpenAI’s examples show the `websocket` module usage for WebSocket mode, but production implementations typically prefer an asyncio-native library. [^150]
- **TypeScript/Node:** `ws` or equivalent; implement an explicit event decoder and backpressure handling.

### Concurrency model

Use a **single-reader / multi-producer** model per WS session:

- One task reads from the socket, decodes JSON events, and pushes typed events into an internal channel/queue.
- Callers send “commands” (create response, continue with tool outputs) into a command queue; the session serializes them, enforcing one in-flight request.
- Tool execution occurs outside the WS task; when tool results are available, they enqueue a “continue” command that sends a new `response.create` with `previous_response_id` + `function_call_output`. [^151]

### Sample pseudocode: streamed compaction items + `previous_response_id` chaining

Python-like pseudocode (illustrative; do not treat as an exact API surface):

```python
class ResponsesWsSession:
    def __init__(self, api_key, model, store=False, compaction_threshold=None):
        self.api_key = api_key
        self.model = model
        self.store = store
        self.compaction_threshold = compaction_threshold
        self.ws = None
        self.previous_response_id = None

    async def connect(self):
        # Connect to wss://api.openai.com/v1/responses with Authorization header
        self.ws = await ws_connect(
            url="wss://api.openai.com/v1/responses",
            headers={"Authorization": f"Bearer {self.api_key}"},
        )

    async def run_turn(self, new_user_text=None, tool_outputs=None, tools=None):
        input_items = []

        # Tool outputs from prior tool calls
        for out in tool_outputs or []:
            input_items.append({
                "type": "function_call_output",
                "call_id": out.call_id,   # MUST match model-emitted call_id
                "output": out.output_text,
            })

        # New user message (incremental)
        if new_user_text is not None:
            input_items.append({
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": new_user_text}],
            })

        payload = {
            "type": "response.create",
            "model": self.model,
            "store": self.store,
            "tools": tools or [],
            "input": input_items,
        }

        if self.previous_response_id:
            payload["previous_response_id"] = self.previous_response_id

        if self.compaction_threshold is not None:
            payload["context_management"] = [{
                "type": "compaction",
                "compact_threshold": self.compaction_threshold,
            }]

        await self.ws.send_json(payload)

        # Streaming state
        text_buf = []
        pending_tool_calls = {}   # call_id -> {name, args_buf}
        compaction_items = []

        async for event in self.ws:
            t = event["type"]

            if t == "response.output_text.delta":
                text_buf.append(event["delta"])

            elif t == "response.function_call_arguments.delta":
                call_id = event["call_id"]
                pending_tool_calls.setdefault(call_id, {"args_buf": "", "name": None})
                pending_tool_calls[call_id]["args_buf"] += event["delta"]

            elif t == "response.output_item.done":
                item = event["item"]
                if item["type"] == "function_call":
                    call_id = item["call_id"]
                    pending_tool_calls.setdefault(call_id, {"args_buf": item["arguments"], "name": item["name"]})
                    pending_tool_calls[call_id]["name"] = item["name"]
                    pending_tool_calls[call_id]["args_buf"] = item["arguments"]
                elif item["type"] == "compaction":
                    # Opaque encrypted item; persist as-is
                    compaction_items.append(item)

            elif t == "response.completed":
                resp = event["response"]
                self.previous_response_id = resp["id"]  # chain to next turn
                break

            elif t == "error":
                raise RuntimeError(event["error"])

        final_text = "".join(text_buf).strip()

        tool_calls = []
        for call_id, st in pending_tool_calls.items():
            if st["name"] is None:
                continue
            tool_calls.append({
                "call_id": call_id,
                "name": st["name"],
                "arguments_json": st["args_buf"],
            })

        return {
            "text": final_text,
            "tool_calls": tool_calls,
            "compaction_items": compaction_items,
            "previous_response_id": self.previous_response_id,
        }
```

This pseudocode directly reflects:

- WebSocket mode: connect to `wss://api.openai.com/v1/responses`, send `response.create`, continue with `previous_response_id` and only new input items. [^152]
- Tool outputs: `function_call_output` items reference `call_id`. [^153]
- Compaction: server-side compaction emits an encrypted compaction item in the response stream when the threshold is crossed. [^154]
- Streaming events: key event types include output text deltas, function call argument deltas, output item lifecycle events, completion/error events. [^155]

## References

[^1]: Research note token bundle:
    `turn6view4`, `turn41view1`.

[^2]: Research note token bundle:
    `turn10view2`.

[^3]: Research note token bundle:
    `turn4view0`, `turn21view0`.

[^4]: Research note token bundle:
    `turn21view0`.

[^5]: Research note token bundle:
    `turn24view0`, `turn24view3`.

[^6]: Research note token bundle:
    `turn24view3`, `turn24view5`.

[^7]: Research note token bundle:
    `turn29view0`, `turn24view3`.

[^8]: Research note token bundle:
    `turn22search6`, `turn31view2`.

[^9]: Research note token bundle:
    `turn24view3`, `turn28view2`.

[^10]: Research note token bundle:
    `turn33view0`, `turn29view0`, `turn24view3`.

[^11]: Research note token bundle:
    `turn7view0`.

[^12]: Research note token bundle:
    `turn10view1`, `turn10view2`.

[^13]: Research note token bundle:
    `turn11view2`, `turn11view3`, `turn12view2`.

[^14]: Research note token bundle:
    `turn36view3`, `turn36view0`.

[^15]: Research note token bundle:
    `turn34view2`, `turn34view3`.

[^16]: Research note token bundle:
    `turn39view2`, `turn40view1`.

[^17]: Research note token bundle:
    `turn39view5`, `turn31view1`.

[^18]: Research note token bundle:
    `turn21view0`, `turn21view1`.

[^19]: Research note token bundle:
    `turn29view0`, `turn21view0`.

[^20]: Research note token bundle:
    `turn6view4`, `turn41view1`, `turn41view2`.

[^21]: Research note token bundle:
    `turn4view0`.

[^22]: Research note token bundle:
    `turn41view2`, `turn41view1`.

[^23]: Research note token bundle:
    `turn24view3`, `turn28view2`.

[^24]: Research note token bundle:
    `turn39view5`, `turn31view1`.

[^25]: Research note token bundle:
    `turn34view2`, `turn34view3`.

[^26]: Research note token bundle:
    `turn10view2`.

[^27]: Research note token bundle:
    `turn24view0`, `turn24view3`.

[^28]: Research note token bundle:
    `turn10view2`.

[^29]: Research note token bundle:
    `turn39view2`, `turn21view0`.

[^30]: Research note token bundle:
    `turn24view3`, `turn24view5`, `turn33view0`.

[^31]: Research note token bundle:
    `turn33view1`, `turn22search6`.

[^32]: Research note token bundle:
    `turn10view1`, `turn39view2`.

[^33]: Research note token bundle:
    `turn31view1`, `turn24view3`.

[^34]: Research note token bundle:
    `turn39view5`, `turn31view1`.

[^35]: Research note token bundle:
    `turn34view1`, `turn34view2`.

[^36]: Research note token bundle:
    `turn24view3`, `turn31view0`.

[^37]: Research note token bundle:
    `turn24view3`, `turn31view2`.

[^38]: Research note token bundle:
    `turn4view0`, `turn21view0`.

[^39]: Research note token bundle:
    `turn29view0`, `turn24view3`.

[^40]: Research note token bundle:
    `turn29view0`.

[^41]: Research note token bundle:
    `turn10view2`, `turn36view3`.

[^42]: Research note token bundle:
    `turn28view2`, `turn24view5`.

[^43]: Research note token bundle:
    `turn28view2`.

[^44]: Research note token bundle:
    `turn39view0`.

[^45]: Research note token bundle:
    `turn33view1`.

[^46]: Research note token bundle:
    `turn33view0`.

[^47]: Research note token bundle:
    `turn6view4`, `turn41view2`.

[^48]: Research note token bundle:
    `turn24view0`.

[^49]: Research note token bundle:
    `turn24view0`.

[^50]: Research note token bundle:
    `turn8view0`, `turn43view1`.

[^51]: Research note token bundle:
    `turn43view0`, `turn43view1`.

[^52]: Research note token bundle:
    `turn24view3`, `turn24view5`.

[^53]: Research note token bundle:
    `turn24view3`, `turn24view5`.

[^54]: Research note token bundle:
    `turn24view3`.

[^55]: Research note token bundle:
    `turn31view1`, `turn39view5`.

[^56]: Research note token bundle:
    `turn29view0`.

[^57]: Research note token bundle:
    `turn24view3`.

[^58]: Research note token bundle:
    `turn28view2`.

[^59]: Research note token bundle:
    `turn24view3`, `turn33view0`, `turn33view1`.

[^60]: Research note token bundle:
    `turn6view4`, `turn41view2`.

[^61]: Research note token bundle:
    `turn6view4`, `turn24view0`.

[^62]: Research note token bundle:
    `turn41view2`, `turn6view4`.

[^63]: Research note token bundle:
    `turn33view1`, `turn29view0`.

[^64]: Research note token bundle:
    `turn24view3`, `turn33view0`.

[^65]: Research note token bundle:
    `turn29view0`, `turn24view3`.

[^66]: Research note token bundle:
    `turn24view0`.

[^67]: Research note token bundle:
    `turn24view3`.

[^68]: Research note token bundle:
    `turn24view3`.

[^69]: Research note token bundle:
    `turn24view3`.

[^70]: Research note token bundle:
    `turn24view3`.

[^71]: Research note token bundle:
    `turn24view3`, `turn31view0`.

[^72]: Research note token bundle:
    `turn31view5`, `turn11view2`.

[^73]: Research note token bundle:
    `turn31view4`, `turn31view3`.

[^74]: Research note token bundle:
    `turn24view3`, `turn33view0`.

[^75]: Research note token bundle:
    `turn29view0`.

[^76]: Research note token bundle:
    `turn28view2`.

[^77]: Research note token bundle:
    `turn28view2`.

[^78]: Research note token bundle:
    `turn31view1`, `turn28view2`.

[^79]: Research note token bundle:
    `turn29view0`.

[^80]: Research note token bundle:
    `turn28view2`, `turn24view5`.

[^81]: Research note token bundle:
    `turn24view3`, `turn31view0`.

[^82]: Research note token bundle:
    `turn31view2`.

[^83]: Research note token bundle:
    `turn24view3`, `turn33view0`.

[^84]: Research note token bundle:
    `turn33view1`, `turn33view0`.

[^85]: Research note token bundle:
    `turn43view0`, `turn43view1`.

[^86]: Research note token bundle:
    `turn43view1`.

[^87]: Research note token bundle:
    `turn24view5`.

[^88]: Research note token bundle:
    `turn24view3`, `turn24view5`.

[^89]: Research note token bundle:
    `turn39view5`, `turn24view3`.

[^90]: Research note token bundle:
    `turn43view0`, `turn24view3`.

[^91]: Research note token bundle:
    `turn24view3`.

[^92]: Research note token bundle:
    `turn24view3`, `turn29view0`.

[^93]: Research note token bundle:
    `turn29view0`.

[^94]: Research note token bundle:
    `turn28view2`, `turn24view3`.

[^95]: Research note token bundle:
    `turn43view0`, `turn43view1`.

[^96]: Research note token bundle:
    `turn34view2`, `turn24view3`.

[^97]: Research note token bundle:
    `turn24view3`, `turn33view1`, `turn29view0`.

[^98]: Research note token bundle:
    `turn36view3`, `turn24view3`, `turn31view5`.

[^99]: Research note token bundle:
    `turn24view3`, `turn29view0`, `turn31view0`.

[^100]: Research note token bundle:
    `turn29view0`.

[^101]: Research note token bundle:
    `turn39view2`, `turn10view1`.

[^102]: Research note token bundle:
    `turn22search6`, `turn31view2`.

[^103]: Research note token bundle:
    `turn24view3`.

[^104]: Research note token bundle:
    `turn31view1`.

[^105]: Research note token bundle:
    `turn24view3`, `turn31view0`.

[^106]: Research note token bundle:
    `turn29view0`.

[^107]: Research note token bundle:
    `turn24view3`, `turn29view0`, `turn31view4`.

[^108]: Research note token bundle:
    `turn28view2`, `turn24view5`.

[^109]: Research note token bundle:
    `turn39view0`, `turn39view3`, `turn21view0`.

[^110]: Research note token bundle:
    `turn33view1`, `turn33view0`.

[^111]: Research note token bundle:
    `turn31view1`, `turn29view0`.

[^112]: Research note token bundle:
    `turn29view0`.

[^113]: Research note token bundle:
    `turn39view5`, `turn39view2`.

[^114]: Research note token bundle:
    `turn31view1`, `turn24view3`.

[^115]: Research note token bundle:
    `turn39view5`, `turn10view1`.

[^116]: Research note token bundle:
    `turn7view0`.

[^117]: Research note token bundle:
    `turn41view2`, `turn6view4`.

[^118]: Research note token bundle:
    `turn24view0`, `turn24view3`.

[^119]: Research note token bundle:
    `turn28view2`, `turn24view5`.

[^120]: Research note token bundle:
    `turn24view3`, `turn31view4`, `turn29view0`.

[^121]: Research note token bundle:
    `turn31view1`, `turn31view3`, `turn24view3`.

[^122]: Research note token bundle:
    `turn29view0`, `turn21view0`, `turn34view2`.

[^123]: Research note token bundle:
    `turn31view1`, `turn33view1`.

[^124]: Research note token bundle:
    `turn43view0`, `turn43view1`.

[^125]: Research note token bundle:
    `turn21view0`, `turn28view2`, `turn24view3`.

[^126]: Research note token bundle:
    `turn6view4`, `turn24view5`.

[^127]: Research note token bundle:
    `turn28view2`.

[^128]: Research note token bundle:
    `turn28view2`.

[^129]: Research note token bundle:
    `turn29view0`.

[^130]: Research note token bundle:
    `turn31view1`, `turn31view0`.

[^131]: Research note token bundle:
    `turn31view3`.

[^132]: Research note token bundle:
    `turn24view3`, `turn31view0`.

[^133]: Research note token bundle:
    `turn29view0`.

[^134]: Research note token bundle:
    `turn24view5`, `turn24view3`.

[^135]: Research note token bundle:
    `turn24view3`, `turn33view0`.

[^136]: Research note token bundle:
    `turn24view5`, `turn43view1`, `turn29view0`.

[^137]: Research note token bundle:
    `turn29view0`, `turn21view0`.

[^138]: Research note token bundle:
    `turn29view0`, `turn24view5`.

[^139]: Research note token bundle:
    `turn24view3`, `turn24view5`, `turn33view0`.

[^140]: Research note token bundle:
    `turn24view3`, `turn33view0`.

[^141]: Research note token bundle:
    `turn33view0`, `turn33view1`.

[^142]: Research note token bundle:
    `turn29view0`.

[^143]: Research note token bundle:
    `turn39view5`, `turn31view1`.

[^144]: Research note token bundle:
    `turn31view0`, `turn39view2`.

[^145]: Research note token bundle:
    `turn24view3`.

[^146]: Research note token bundle:
    `turn43view0`.

[^147]: Research note token bundle:
    `turn43view0`, `turn43view2`.

[^148]: Research note token bundle:
    `turn43view0`, `turn43view1`.

[^149]: Research note token bundle:
    `turn7view0`, `turn8view0`, `turn24view0`.

[^150]: Research note token bundle:
    `turn24view0`, `turn24view3`.

[^151]: Research note token bundle:
    `turn24view3`, `turn31view0`, `turn34view2`.

[^152]: Research note token bundle:
    `turn24view0`, `turn24view3`.

[^153]: Research note token bundle:
    `turn31view0`, `turn24view3`.

[^154]: Research note token bundle:
    `turn29view0`.

[^155]: Research note token bundle:
    `turn28view2`, `turn24view5`.
