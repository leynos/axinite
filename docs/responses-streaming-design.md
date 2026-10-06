# Responses stream handling design

## Front matter

- **Status:** Proposed; design only, no runtime capability enabled.
- **Date:** 2026-09-06.
- **Scope:** Provider streams, Responses continuation, durable recovery, and
  safe projections into existing Axinite channels.
- **Audience:** Runtime, provider, persistence, and channel maintainers.
- **Precedence:** [RFC 0008](rfcs/0008-websocket-responses-api.md) owns the
  Responses feature; [ADR 002](adr-002-authoritative-intent-state-must-remain-human-auditable.md)
  governs authoritative state; [ADR 006](adr-006-dual-trait-pattern-for-dyn-backed-async-interfaces.md)
  governs async interfaces. Network controls in `src/NETWORK_SECURITY.md`
  remain authoritative. This document refines, rather than replaces, RFC 0008.
- **Delivery:** [Streaming roadmap](responses-streaming-roadmap.md) decomposes
  the existing [roadmap workstream 3.2](roadmap.md#32-openai-responses-over-websocket).

## 1. Problem and evidence

The reviewed baseline is Axinite commit
`b816fd3719099f553601eb3077be4ba0e8a620ba`. The following are source findings,
not runtime measurements:

- `src/llm/provider.rs` exposes buffered `complete` and
  `complete_with_tools` operations through the dual-trait boundary.
- `src/channels/channel/messages.rs` defines `StreamChunk(String)`, but that
  presentation notification does not create a provider-stream contract.
- `src/channels/repl/mod.rs` tracks streaming with a channel-wide flag and
  discards status-routing metadata. Rich concurrent displays need explicit
  response identity rather than a shared rendering flag.
- `src/channels/web/openai_compat/streaming.rs` divides a completed response
  into chunks and labels it `x-axinite-streaming: simulated`.
- `src/llm/chain.rs` owns provider decoration. Streaming must preserve its
  retry, routing, failover, circuit-breaker, cache, and recording semantics.

RFC 0008 already plans native call identifiers, WebSocket sessions, streamed
output items, and compaction. The missing detail is the end-to-end contract:
who consumes the stream, when output becomes safe to expose or execute, and
what survives interruption. A terminal replacement alone solves none of this.

## 2. Non-negotiable invariants

1. Axinite owns tool authorization, execution, canonical history, memory,
   budgets, and user-turn completion. Provider output and UI state cannot
   grant authority.
2. No local tool executes from argument deltas, an item-done event alone, an
   incomplete response, or a failed response. The initial implementation waits
   for a validated successful provider terminal response before admission.
3. A provider response is one model invocation. A user turn may contain many
   provider responses and tools. `response.completed` does not necessarily
   mean the user turn has finished.
4. Preserve provider response IDs, output-item IDs, and tool `call_id` values
   as distinct identities. Never synthesize replacement IDs for native
   Responses continuation or remap them by tool name alone.
5. End of transport without a valid terminal event means interruption, not
   success. Unknown usage is absent, not zero. A local cancellation request
   does not prove that the provider stopped or charged nothing.
6. Both PostgreSQL and libSQL support every durable state transition. Provider
   continuation is an optimization over recoverable local state, never its
   sole source of truth.
7. Backpressure, cancellation, and resource limits apply throughout the chain.
   No unbounded token queue, detached reader, or implicit retry loop is part of
   the new contract.
8. Existing channels, one-shot execution, buffered providers, tool attenuation,
   and final-output hooks retain their guarantees during opt-in rollout.

## 3. Boundaries and ownership

The following pipeline distinguishes untrusted provider material from public
presentation and executable intent. HTTP server-sent events (SSE) and WebSocket
frames have different framing but share response semantics.

```plaintext
resolved request + authorized context
  -> provider execution policy (admission, attempt budget, cancellation)
  -> transport adapter (HTTP SSE or WebSocket frames)
  -> Responses decoder and response accumulator
  -> validated provider outcome
  -> Axinite turn delegate -> existing tool admission/execution -> next request

accumulator previews -> output policy -> routed channel presentation
validated outcome -> canonical persistence -> continuation checkpoint
execution transitions -> existing persistence / future execution ledger
```

Figure 1. Provider, application, persistence, and presentation boundaries.

The transport adapter owns framing, authentication, connection I/O, and typed
transport failures. The decoder owns wire interpretation; a deterministic
accumulator owns response consistency. Neither owns tools, retries, database
writes, UI rendering, or user-turn transitions.

The application owns a stream driver and a per-thread turn lease. It consumes
one stream, applies policy, commits outcomes, and supplies finalized calls to
the existing delegate. It does not introduce a second agent loop or event bus.
Channel adapters consume scoped presentation events; they never consume raw
provider events directly.

## 4. Provider and event contracts

Extend the existing `LlmProvider` and `NativeLlmProvider` pair with one
stream-capable request operation, provisionally `start_response`. Introduce an
Axinite-owned request enum that can carry either the existing completion
request or native Responses input items. Do not force opaque reasoning or
compaction items through `ChatMessage` and lose them during conversion.

The operation returns an owned, cancellable response session. Its interface
provides a bounded event receiver, a terminal outcome, and an explicit
cancel-and-join operation. A completion await must drain or supervise the
receiver; it must not deadlock behind an unconsumed bounded channel. Dropping
an abandoned session requests cancellation, while its supervisor owns cleanup
and exposes abnormal termination. Concrete implementations retain ADR 006's
native async methods and dyn-safe adapter.

Legacy providers adapt their existing buffered operation into an outcome with
`delivery = buffered`; no invented token cadence is necessary. Native
Responses implements streaming once, and buffered callers collect that same
implementation. Defaults must not recurse between collect and stream methods.
Capability metadata distinguishes incremental generation from buffered
adaptation and cache delivery. Unsupported item types fail explicitly rather
than silently flattening to text.

The normalized envelope carries Axinite thread, turn, invocation, and attempt
IDs; provider identity and response ID when known; local sequence; optional
provider sequence; and typed payload. Output coordinates include item ID,
output index, content index, and optional function-call ID. Values originating
at a provider remain namespaced and untrusted.

Normalized payloads cover response start, output-item start, text/refusal or
reasoning-summary deltas, argument deltas, item completion, usage snapshots,
and provider terminal outcome. Opaque continuation items remain typed opaque
values. Lifecycle metadata is not interchangeable with assistant prose.

Public channel envelopes use Axinite IDs and a presentation revision, with
start, safe append, replace/snapshot, interrupted, blocked, and committed
states. Extend the existing channel vocabulary additively. Legacy channels
receive only final safe responses; consumers supporting previews negotiate or
advertise that ability explicitly. Finalization is idempotent by identity, not
by string equality or a channel-global boolean.

## 5. Accumulation and terminal validation

The accumulator moves through `Created`, `Receiving`, and exactly one terminal
classification: `Completed`, `Incomplete`, `Failed`, or `Cancelled`. It bounds
item counts, per-item arguments, event size, and aggregate response bytes.
Transport segmentation has no semantic significance: partial UTF-8, SSE
records, WebSocket fragments, and JSON strings may cross read boundaries.

Each item has its own bounded buffer and lifecycle. Interleaved calls therefore
cannot share argument state. Final argument and output-item values must agree
with the accumulated deltas where the protocol supplies both. A successful
terminal response must reconcile with all observed executable items. Reject
missing, contradictory, duplicated, or ambiguously correlated calls. A terminal
snapshot may supply fields omitted by a documented adapter contract, but it
cannot overwrite a contradiction or resurrect a call absent from a nonempty
terminal result.

A function call becomes an application candidate only after successful terminal
validation, complete JSON decoding, schema validation, and preservation of its
native IDs. The existing tool registry, approvals, policy gates, and sandbox
then decide admission. Provider-hosted tools are not local functions: the
initial profile advertises only supported local function tools and denies
unimplemented hosted/custom execution modes rather than approximating them.

Use a protocol capability profile for ordering and required events. Validate
monotonic provider sequences where supplied; exact contiguous ordering is not
a universal assumption. Identical duplicate records can be suppressed only
with a documented replay identity and matching payload. Conflicting duplicates
fail. Unknown additive metadata can be ignored with bounded diagnostics;
unknown executable items or unsupported terminal semantics cannot succeed.

The `[DONE]` marker of a Chat Completions SSE stream is not a Responses
completion event. Each wire adapter defines its own terminal mapping. A
truncated frame, malformed event, or EOF without that mapping produces a typed
interruption carrying safe partial-state metadata, never executable calls.

## 6. Safe previews and channel delivery

Separate internal accumulation from release to a sink. The default mode is
`final_only`. Opt-in `guarded_incremental` requires an output policy that can
approve irreversible prefixes and all applicable hooks to support incremental
release. A final-message transform or whole-message safety obligation forces
buffering for that response; a later replacement cannot undo an earlier leak.

The policy must handle secrets and control sequences split across chunks,
refusals, reasoning tags, and `SILENT_REPLY_TOKEN`. A fixed suffix buffer alone
does not prove protection for arbitrary patterns or future-dependent rules.
When safe bounded prefix release cannot be established, retain final-only
mode. Restrict raw argument previews and opaque encrypted items to protected
internal state. Reasoning summaries require explicit policy; no UI invents
unavailable reasoning content.

Apply terminal-control sanitization at the terminal boundary and browser-safe
rendering at the browser boundary, after content release policy. Logs, traces,
recordings, notifications, and accessibility announcements are sinks too.
Previews never enter memory extraction as completed assistant statements.

The REPL and browser reconcile final output by response identity. A failed
stream leaves an explicitly interrupted preview and a safe diagnostic rather
than disappearing or masquerading as a final answer. Broadcast notifications
have their own identities and cannot finalize the active response.

Keep one bounded lossless execution path and bounded presentation projections.
A slow viewer may receive a newer safe snapshot with a revision gap marker;
it cannot cause silent loss of execution or terminal state. Failure of the
required persistence path stops new tool admission. Viewer detachment follows
an explicit run-ownership policy and does not automatically kill an unrelated
background job. Cancellation controls must remain responsive under saturation.

## 7. Attempts, deadlines, and WebSocket sessions

One provider execution policy owns the overall attempt budget. Existing retry
and failover decorators must participate in that budget across stream polling,
not just stream creation. Do not multiply adapter, decorator, delegate, and UI
retry counts. Count circuit-breaker success only after terminal validation;
local cancellation and policy rejection are not provider outages.

Before request transmission, a transient failure may retry within budget.
After transmission with uncertain acceptance, do not replay automatically unless
the adapter proves non-acceptance or supports a verified deduplication contract.
An explicit rejection may permit a new attempt; once any preview is exposed,
automatic transparent retry is prohibited. Recovery creates a new, visible
attempt from canonical state. A network retry never reruns a dispatched tool.
Any separately authorized retry of an uncertain generation retains its possible
usage liability rather than reporting a free failed attempt.

Track queue/admission, connect/handshake, first semantic progress, idle semantic
progress, and overall request deadlines independently. Also measure first safe
visible text separately: a reasoning-only response can make legitimate
progress without visible prose. Pings and empty events do not reset semantic
deadlines. Use monotonic time, cancellable backoff, bounded jitter, and parsed
`Retry-After` values. Hold concurrency permits until supervised cleanup finishes.

A `ResponsesWsSession` is scoped to thread, provider configuration identity,
model, credential epoch, and context-policy generation. Serialize requests on
a leased connection; bound active sockets and idle retention. Configuration
changes invalidate incompatible continuation. Keep quota-sharing identity
separate from those keys and from display labels.

The reviewed OpenAI guide describes sequential responses and a connection
lifetime limit. Initial Axinite policy deliberately uses one in-flight response
per socket, rotates idle connections, and validates current transport limits
before release.[^1] Close and discard an interrupted socket unless a documented
provider cancellation handshake proves it reusable. Do not invent a Responses
`response.cancel` operation or borrow one from the Realtime API.

HTTP SSE and WebSocket adapters feed the same decoder and accumulator. HTTP
fallback is explicitly configured and preserves provider, privacy, model, and
context policy. It is not permission to send data to another endpoint.
WebSocket warmup and multiplexing are out of initial scope.

## 8. Durability, continuation, and compaction

Add versioned response-attempt and provider-continuation records behind the
existing `Database` boundary. Persist ownership, request digest, configuration
generation, response IDs, ordered opaque items, terminal classification,
nullable usage, and the relationship to native tool calls and outcomes. Keep
sensitive continuation payloads encrypted at rest with scoped access and
retention/deletion rules. They must not enter ordinary logs or memory search.

Persist accepted provider output and its continuation checkpoint atomically
before releasing calls for execution. Record dispatch intent before invoking a
tool, then persist its observed outcome before sending the next provider
request. Crash recovery treats a dispatch with no known outcome as uncertain
and requires reconciliation, not automatic replay. This provides conservative
recovery, not exactly-once external execution.

The initial durable metadata can use the current database services without
waiting for the full execution ledger. Align IDs and transitions with
[RFC 0011](rfcs/0011-execution-truth-ledger-and-action-provenance.md); later ledger
integration projects the same facts instead of creating another authority.
Transactions and crash tests must cover both supported database backends.

On reconnect, use `previous_response_id` only when the selected storage mode
and provider make it valid. A missing continuation starts a new chain from
complete retained input items and known tool outputs, with no tool re-execution.
If complete recoverable context is unavailable, stop with an explicit context
loss error rather than silently continuing with a shorter prompt.[^1]

Preserve opaque reasoning and compaction items as returned; do not deserialize
them into an imagined reasoning transcript. Inline server compaction and the
standalone compact endpoint are different operations. Inline chaining keeps the
latest valid response ID; standalone compaction yields an input window, not a
new response ID, and requires a new chain. Preserve the complete returned
window.[^2] ADR 002 still requires an inspectable local intent and decision
history independent of these provider-owned representations.

Choose one effective compaction strategy per request. Do not run local
summarization and native compaction competitively. Preserve existing memory
integration and workstream 3.2.5's memory-sidecar prerequisite for its full
rollout; pure stream/parser and persistence work can proceed earlier. A
stateless-only deployment without durable storage must not advertise restart
recovery or execute a durable agentic Responses workflow.

## 9. Compatibility, accounting, and observability

Keep existing buffered entrypoints. Audit every wrapper and delegate, including
chat, hosted workers, routines, and auxiliary calls, for capability loss or
accidental buffering. A wrapper returning a stream must observe errors and
usage until termination. Cancelled, failed, and incomplete outcomes never enter
the response cache. Preserve the existing exclusion of tool completions from
caching; stateful Responses caching stays disabled initially. Cache delivery
must not masquerade as newly measured inference.

Usage records distinguish provider-reported, estimated, and unavailable values,
including cached-input and reasoning-token fields when actually supplied.
Treat cumulative snapshots as replacements, not additive deltas. Settle known
usage once per attempt; retain conservative reservations for uncertain
attempts. Attribute costs to the effective provider/model, not the requested
alias. Budget exhaustion cancels locally but does not prove remote cancellation.

Expose low-cardinality counters/histograms for admission delay, connection time,
first provider progress, first safe visible output, inter-event gaps, terminal
latency, buffering mode, queue high-water marks, reconnects, validation failures,
and cancellation cleanup. Correlate thread/turn/attempt/response IDs in
access-controlled traces, not metric labels. Record transport, provider,
policy, persistence, and consumer failures separately. Do not log prompts or
credentials by default, and do not claim an exporter exists merely because
events exist.

The OpenAI-compatible endpoint is a provider facade, not the assistant runtime.
Upgrade its actual streaming separately; do not make it execute local tools or
inherit conversational memory implicitly. Retain the simulated header on the
legacy path until native generation really supplies output incrementally, and
add truthful buffered/native delivery metadata. Once HTTP headers are sent,
stream failures use the endpoint's supported error framing and connection close,
not a fictitious new HTTP status or fabricated successful finish reason.

## 10. Validation and rollout

Use deterministic decoder fixtures, generated event sequences, fake transports,
paused Tokio time, and fault-injecting repositories. Mock HTTP and WebSocket
servers must split bytes independently of logical events and exercise
 disconnect, backpressure, cancellation, and reconnect. Live paid inference
remains explicitly opt-in and outside normal CI.

The essential properties are segmentation invariance, bounded allocation,
identity fidelity, one terminal classification, no calls before validated
completion, no replay of uncertain effects, safe prefix release, and equivalent
canonical outcomes for streaming versus buffered collection of the same fixture.
Compare semantic outcomes, not nondeterministic live model prose.

Crash tests cover before/after request transmission, terminal commit, tool
 dispatch, tool outcome commit, continuation checkpoint, and output publication.
Exercise two concurrent threads, policy/configuration changes, Unicode secrets
split at every boundary, duplicate/conflicting events, malformed final calls,
missing usage, reasoning-only output, and a slow or vanished viewer.

Roll out in layers: offline contracts; provider transport with final-only
output; durable tool-loop parity; guarded previews for capable channels;
explicit native compatibility streaming; then wider opt-in use. Each layer has
a kill switch that affects new requests while active attempts drain or cancel
explicitly. Retain additive schema readers and old entrypoints during rollback.
No default provider, retention policy, or terminal interface changes in this
design PR.

## 11. Alternatives and review decisions

A UI callback inside the provider was rejected because it couples inference to
one sink and obscures lifecycle ownership. A second agent harness was rejected
because it duplicates tool, persistence, and approval policy. Tool execution on
item completion was deferred because lower latency does not justify uncertain
terminal reconciliation in the initial implementation.

The proposed defaults are final-only output, no transparent replay after
uncertain transmission, durable agentic sessions, explicit transport fallback,
and no stateful response cache. Implementation review must pin parser limits,
timeout values, retention policy, and the incremental-hook capability contract
using fixtures and measured resource budgets before enabling the feature. These
are release gates, not permission to skip the safety invariants.

## References

[^1]: [OpenAI WebSocket mode](https://developers.openai.com/api/docs/guides/websocket-mode),
      reviewed 2026-09-06. Transport limits are version-sensitive; validate
      against the selected endpoint before release.
[^2]: [OpenAI compaction guide](https://developers.openai.com/api/docs/guides/compaction),
      reviewed 2026-09-06. See also the
      [streaming guide](https://developers.openai.com/api/docs/guides/streaming-responses)
      and [function-calling guide](https://developers.openai.com/api/docs/guides/function-calling)
      for event and call-identity contracts. Axinite's stricter execution gate
      is application policy, not an assertion that the provider requires it.
