<!-- markdownlint-disable-next-line MD013 -->
# Architectural decision record (ADR) 002: Authoritative intent state must remain human-auditable

## Status

Proposed.

## Date

2026-03-15.

## Context and problem statement

Axinite is integrating provider-specific continuation mechanisms that
carry opaque or encrypted state. RFC 0008 proposes a WebSocket
Responses API backend that supports:

- **Stateful continuation via `previous_response_id`**: the WebSocket
  connection keeps a local cache of the most recent previous-response
  state. Instructions are not automatically carried over when chaining
  with `previous_response_id`. [^1]
- **Server-side compaction via `context_management`**: the provider
  emits opaque encrypted compaction items that reduce token count but
  cannot be inspected, diffed, or audited by the operator. [^1]
- **Conversation IDs**: optional binding to a durable server-side
  conversation that persists across sessions. [^1]

These mechanisms offer real operational benefits (lower latency, reduced
token costs, multi-session continuity), but they share a common risk:
the authoritative representation of conversation state, intent, and
decision history can silently migrate from Axinite-controlled, human-
readable artefacts into provider-controlled, opaque artefacts.

The intent-engineering analysis warns that compaction must never be the
only place where critical intent state lives. [^2] If encrypted
compaction is the sole store of what the agent knows, a provider API
change, key rotation, or service disruption can destroy the agent's
operational memory without the operator being able to detect or recover
from the loss.

## Decision drivers

- **Auditability**: NIST AI RMF 1.0 requires accountability and
  transparency as trustworthy AI characteristics. The AI RMF Govern
  function requires clear accountability chains, and the Measure
  function requires that AI system behaviour be verifiable. [^3]
  Opaque provider state cannot be verified.
- **Recoverability**: if a provider session is lost (WebSocket
  disconnect, `previous_response_not_found`, or provider API change),
  Axinite must be able to reconstruct the conversation from its own
  records. RFC 0008 explicitly notes this failure mode and recommends
  policy for reconnect and resume. [^1]
- **Diffability**: operators must be able to diff the state of a
  conversation, intent contract, or decision history using standard
  tools (`diff`, version control, audit queries). Encrypted
  compaction items are opaque blobs that defeat this requirement.
- **Non-lock-in**: authoritative state stored exclusively in a
  provider's format creates vendor lock-in. If Axinite switches
  providers, migrates models, or operates in a multi-provider
  configuration, provider-specific state cannot transfer.
- **Intent contract enforcement**: RFC 0010 defines intent contracts
  that constrain agent behaviour. If the authoritative statement of
  what the agent should do lives in an opaque provider artefact, the
  contract cannot be enforced. [^4]
- **Execution ledger completeness**: RFC 0011 requires an append-only
  ledger of system actions. Provider-side compaction events and
  continuation state changes must be visible in that ledger. [^5]

## Options considered

### Option A: Provider state as authoritative source

Allow provider-side `previous_response_id`, compaction items, and
conversation IDs to serve as the authoritative record of conversation
state. Axinite-native persistence becomes a best-effort mirror.

This is rejected. It violates auditability, recoverability, and
diffability requirements. A provider outage or API change would leave
Axinite unable to reconstruct the conversation.

### Option B: Dual-write with provider as primary

Maintain Axinite-native state alongside provider state, but defer to
provider state when both are available. Use Axinite state only as a
fallback.

This is better than Option A but still problematic: it creates two
sources of truth with unclear precedence rules, and it normalizes
reliance on opaque state.

### Option C: Axinite-native state as authoritative, provider state as cache

Axinite's own conversation record, intent contracts, decision log, and
execution ledger are the authoritative stores. Provider-side
continuation state (`previous_response_id`, compaction items,
conversation IDs) is treated as a performance optimization and
continuity cache. When provider state is available, it is used for
efficiency. When it is unavailable, Axinite reconstructs from its own
records.

This is the recommended approach.

| Factor | Option A | Option B | Option C |
| --- | --- | --- | --- |
| Auditability | No | Partial | Yes |
| Recoverability | Provider-dependent | Partial | Full |
| Diffability | No | Partial | Yes |
| Vendor independence | No | Partial | Yes |
| Performance | Best | Good | Good |
| Complexity | Low | High (dual-write) | Medium |

_Table 1: Comparison of state authority options._

## Decision outcome / proposed direction

**Axinite-native state is the authoritative source of truth for user
intent, system policy, and decision history.** Provider-owned
continuation state may be used as a cache or performance optimization,
but it is never elevated to sole source of truth.

This means:

1. **Intent contracts** (RFC 0010) live in the workspace, thread, or
   job metadata — not in provider-side conversation state.
2. **Decision history** (RFC 0011) lives in the append-only execution
   ledger — not in provider-side compaction items.
3. **Conversation state** lives in Axinite's `Thread`/turn model — not
   solely in `previous_response_id` chains.
4. **Compaction** may be performed by the provider for efficiency
   (RFC 0008 `context_management`), but Axinite's native compaction
   writes summaries to workspace, daily logs, or the memory sidecar
   (RFC 0007), ensuring a human-readable record survives.
5. **Provider continuity events** (`previous_response_id` transitions,
   compaction item emissions, conversation ID bindings) are recorded
   in the execution ledger as `provider_event` entries.

When provider-side compaction is active, Axinite-native compaction
becomes a governed optimization or fallback path, not the place where
durable intent lives.

## Goals and non-goals

- Goals:
  - Ensure that an operator can always inspect, diff, and audit the
    current state of any conversation, intent contract, or decision
    history using human-readable artefacts.
  - Ensure that Axinite can recover from provider session loss without
    losing authoritative state.
  - Record all provider continuity events in the execution ledger.
- Non-goals:
  - Prohibit the use of provider-side state. It is a valid
    optimization.
  - Require Axinite to replicate every provider-side compaction
    decision. The invariant is about authoritativeness, not about
    redundancy of every intermediate state.

## Known risks and limitations

- **Performance overhead**: maintaining Axinite-native state alongside
  provider state adds write overhead. This is mitigated by the fact
  that Axinite already persists conversation turns and tool call
  results.
- **State drift**: provider-side state and Axinite-native state may
  diverge if the provider applies compaction or context management
  that is not reflected in Axinite's records. The execution ledger's
  `provider_event` entry type mitigates this by recording all
  observable provider state changes.
- **Opaque compaction items cannot be inspected**: Axinite cannot
  verify what was compacted on the provider side. This is a known
  limitation of the provider's API, not a deficiency of this ADR.
  The mitigation is that Axinite's own records are the authoritative
  source.

## Outstanding decisions

- Whether Axinite should periodically validate that its native state
  and provider-side state are consistent (e.g. by comparing token
  counts, turn counts, or content hashes).
- Whether the execution ledger should store compaction items verbatim
  (as opaque blobs) for potential future decryption or inspection,
  or merely record that a compaction event occurred.
- How the UI should indicate when provider-side state is being used
  versus Axinite-native state.

---

[^1]: RFC 0008: WebSocket Responses API. See
    `docs/rfcs/0008-websocket-responses-api.md`.

[^2]: Intent-engineering analysis, compaction as an intent hazard. See
    `docs/What Axinite can learn from the video's approach to intent
    engineering.md`.

[^3]: NIST AI Risk Management Framework 1.0. Trustworthy AI
    characteristics: accountable and transparent, valid and reliable.
    See <https://www.nist.gov/itl/ai-risk-management-framework> and
    <https://nvlpubs.nist.gov/nistpubs/ai/nist.ai.100-1.pdf>.

[^4]: RFC 0010: Intent contracts and fail-closed runtime gates. See
    `docs/rfcs/0010-intent-contracts-and-fail-closed-runtime-gates.md`.

[^5]: RFC 0011: Execution truth ledger and action provenance. See
    `docs/rfcs/0011-execution-truth-ledger-and-action-provenance.md`.
