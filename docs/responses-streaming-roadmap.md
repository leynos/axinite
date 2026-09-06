# Responses stream handling roadmap

## Status and relationship to the main roadmap

**Proposed, 2026-09-06.** This is the detailed delivery breakdown for
[workstream 3.2](roadmap.md#32-openai-responses-over-websocket), governed by the
[streaming design](responses-streaming-design.md) and
[RFC 0008](rfcs/0008-websocket-responses-api.md). It does not mark any capability
implemented or renumber the main roadmap. Four-part task IDs refine their
existing three-part parent. The parent can close only when its existing
criteria and the applicable child criteria hold.

The implementation remains independent of VTCode availability. No external
component migration may hold the decoder or safety contracts hostage. Later
adapters must pass these contracts rather than redefine them.

## 1. Establish the stream contract

Objective: represent incremental and buffered inference without changing the
agent's authority model.

Learning opportunity: expose where provider decorators currently erase
capabilities or assume that returning a future completes an invocation.

- [ ] 3.2.1.1. Add owned request, event, terminal-outcome, and session contracts
  to the existing dual-trait provider boundary. Define cancellation ownership,
  bounded delivery, typed IDs, delivery mode, and a non-recursive buffered shim.
  - See design sections 3 and 4.
  - Success: fake native and buffered providers produce equivalent canonical
    outcomes; session completion cannot deadlock on an unread event receiver.
- [ ] 3.2.1.2. Resolve transport, privacy, limits, preview mode, and capability
  policy at composition time. Requires 3.2.1.1.
  - See design sections 2, 6, and 7.
  - Success: existing configuration remains valid; Responses is opt-in;
    unsupported capability combinations fail before network I/O; secrets and
    environment reads remain outside pure contracts.
- [ ] 3.2.1.3. Audit all buffered consumers and provider decorators, recording
  the conversion and fallback matrix. Requires 3.2.1.1.
  - See design section 9.
  - Success: chat, hosted workers, routines, auxiliary calls, recording, cache,
    retry, routing, and failover each have an explicit supported path and test.

## 2. Decode independently, then connect transports

Objective: prove event semantics before introducing connection recovery.

Learning opportunity: separate wire segmentation and provider dialects from
application-level consistency rules.

- [ ] 3.2.3.1. Implement the pure Responses accumulator and bounded wire event
  decoder. Requires 3.2.1.1; does not require a live WebSocket.
  - See design section 5.
  - Success: generated segmentation, interleaving, duplicate, mismatch, unknown
    item, incomplete JSON, and missing-terminal fixtures satisfy the invariants.
- [ ] 3.2.3.2. Build native Responses requests from ordered input items and the
  canonical tool catalogue. Requires 3.2.1.2 and 3.2.3.1.
  - See design sections 4, 5, and 8; main roadmap 1.1 and 1.2.
  - Success: native call IDs and opaque items survive round trips; unsupported
    hosted/custom tool modes fail explicitly; request builders do not execute.
- [ ] 3.2.2.1. Add HTTP SSE and authenticated WebSocket transport adapters using
  the same decoder. Requires 3.2.1.2, 3.2.3.1, and 3.2.3.2.
  - See design sections 3 and 7.
  - Success: local mock servers prove incremental delivery before terminal
    completion, bounded framing, typed errors, and correct transport fields.
- [ ] 3.2.2.2. Add bounded per-thread socket leasing, configuration identity,
  connection rotation, and supervised cancel-and-join. Requires 3.2.2.1.
  - See design section 7.
  - Success: two threads cannot mix state; cancellation under queue saturation
    releases permits and tasks; interrupted sockets cannot leak late events
    into a subsequent request.

The original parent 3.2.3 depends on 3.2.2 for integrated completion. The pure
child 3.2.3.1 is intentionally an earlier seed, not a circular completion gate.

## 3. Preserve durable response and tool state

Objective: make restart and continuation conservative and auditable.

Learning opportunity: determine the smallest checkpoint that preserves a full
provider replay window without promoting opaque provider state into truth.

- [ ] 3.2.4.1. Add versioned response-attempt and continuation persistence to
  PostgreSQL and libSQL. Requires 3.2.1.1 and 3.2.3.1.
  - See design section 8 and ADR 002.
  - Success: both backends pass the same repository contract; nullable usage,
    native IDs, ordered opaque items, retention, and encryption remain intact.
- [ ] 3.2.4.2. Integrate stream consumption into the existing loop delegate and
  thread lease, with terminal validation before tool admission. Requires
  3.2.2.2, 3.2.3.2, and 3.2.4.1.
  - See design sections 2, 3, 5, and 8.
  - Success: text -> tool -> text uses one existing agent loop; interrupted or
    contradictory responses dispatch no local tool; known tool outputs retain
    their native call identities on continuation.
- [ ] 3.2.4.3. Add dispatch-intent/outcome crash recovery and idempotent local
  publication. Requires 3.2.4.2.
  - See design section 8 and RFC 0011.
  - Success: crash-point tests never rerun an uncertain external effect or
    promote an uncommitted provider response to a successful user turn.

## 4. Unify execution policy and continuation

Objective: preserve bounded provider behaviour throughout stream polling.

Learning opportunity: measure genuine provider latency separately from safety
buffering, presentation lag, and connection setup.

- [ ] 3.2.5.1. Extend the provider chain with one attempt budget, deadline
  policy, typed failure classification, and stream-aware circuit accounting.
  Requires 3.2.1.3 and 3.2.2.1; may proceed alongside persistence.
  - See design sections 7 and 9.
  - Success: paused-time tests cover admission, first progress, idle, total,
    backoff cancellation, uncertain transmission, and no nested retry growth.
- [ ] 3.2.5.2. Implement continuation recovery and exclusive compaction strategy
  selection. Requires 3.2.4.3 and 3.2.5.1.
  - See design section 8 and RFC 0008.
  - Success: missing response state rebuilds complete input or reports context
    loss; standalone compaction starts a new chain; tools never replay.
  - Full parent 3.2.5 retains its 3.1.5 memory-integration prerequisite. Offline
    contracts and non-compacting transport pilots need not wait for memoryd.
- [ ] 3.2.5.3. Add usage settlement, conservative unknown-cost reservations,
  safe recording, and operational telemetry. Requires 3.2.5.1 and 3.2.4.1.
  - See design section 9.
  - Success: cumulative usage does not double-count; unknown is not zero;
    failed attempts retain liability; metrics exclude high-cardinality IDs.

## 5. Project safely into existing channels

Objective: deliver genuine incremental output without weakening safety or
making a new terminal UI a prerequisite.

Learning opportunity: establish which output policies support irreversible
prefix release and which require whole-response buffering.

- [ ] 3.2.6.1. Add a routed, versioned presentation envelope and output-release
  policy with final-only fallback. Requires 3.2.4.2 and 3.2.5.1.
  - See design sections 4 and 6.
  - Success: adversarial chunk boundaries cannot leak protected material;
    final-only hooks prevent previews; slow viewers cannot corrupt execution.
- [ ] 3.2.6.2. Adapt the existing REPL and browser stream consumers, retaining
  final-only delivery to other channels. Requires 3.2.6.1.
  - See design sections 6 and 10.
  - Success: response IDs prevent duplicate final output; interruptions and
    notifications remain distinct; accessible announcements avoid token spam.
  - Coordinate browser changes with PR #275's SolidJS migration; validate the
    actual integration base, not an assumed merged frontend.
- [ ] 3.2.6.3. Replace simulated streaming in the OpenAI-compatible facade for
  supported providers without introducing local tool execution. Requires
  3.2.6.1 and 3.2.5.3.
  - See design section 9.
  - Success: native streams reach the client before generation completes;
    legacy paths retain truthful delivery metadata; post-header failures do
    not claim success; disconnect cancellation respects request ownership.

## 6. Verify and release progressively

Objective: close parent workstream 3.2 only with behavioural and operational
proof, not merely a successful WebSocket connection.

Learning opportunity: distinguish protocol correctness from recoverability and
operator-visible latency under realistic failure and load conditions.

- [ ] 3.2.6.4. Add end-to-end fixture parity, crash injection, bounded-resource
  stress, and generated protocol/state-machine tests. Requires 3.2.4.3,
  3.2.5.2, 3.2.5.3, 3.2.6.2, and 3.2.6.3.
  - See design section 10.
  - Success: both database backends, both transports, buffered collection,
    multiple threads, and interrupted streams pass the same semantic scenarios.
- [ ] 3.2.6.5. Deliver operator documentation, dashboards where supported,
  feature gates, and rehearsed rollback. Requires 3.2.6.4 and the main roadmap's
  existing dependencies for full 3.2 completion.
  - See design sections 9 through 11.
  - Success: publish measured latency and memory baselines; retain old config
    and schema readers; unknown telemetry remains explicit; update
    `FEATURE_PARITY.md` only when implementation status actually changes.

Recommended implementation slices are contracts, pure codecs, transport,
persistence, policy/recovery, existing-channel previews, compatibility facade,
and rollout. Each slice carries its own tests; the final test task integrates
rather than postpones testing. No paid live probe is a normal CI prerequisite.

## Design review gates

Before implementation, agree the request/session surface, irreversible output
release contract, storage retention and encryption, failure taxonomy, and
configuration defaults. Record changes against this design and RFC 0008.
Before rollout, require decoder property tests, dual-backend crash evidence,
mock-server streaming evidence, and rollback rehearsal. A preview screenshot
or an HTTP 200 response is not proof of streaming or correct tool execution.
