# RFC 0010: Execution truth ledger and action provenance

## Preamble

- **RFC number:** 0010
- **Status:** Proposed
- **Created:** 2026-03-15

## Summary

Axinite should add an append-only execution ledger that records actual
tool invocations, approvals, network requests in canonicalized or
redacted form, file and workspace writes, delegated jobs, and
provider-side continuity state. The ledger should be exposed through
browser history and live status surfaces as a plane separate from
assistant prose.

Axinite's current chat model intentionally stores a lossy durable
conversation record even though the runtime keeps richer in-memory turn
structure and emits richer live status events via Server-Sent Events
(SSE). [^1] The missing piece is a durable truth surface that lets the
UI distinguish "the assistant said it did this" from "the system actually
did this".

## Problem

The intent-engineering analysis identifies a concrete operational
failure: the model claims work was done when it was not. [^2] This is
not a hallucination problem in the usual sense; it is a systems problem
where the model's narrative is not a reliable source of execution truth.

Axinite's chat model preserves tool causality by rebuilding transcripts
from turns and tool calls, and by persisting user messages early for
crash recoverability. [^1] However, the same document notes that the
durable record is intentionally lossy, and that some paths (e.g. auth
token submission) deliberately bypass normal conversational
persistence. [^1] These are reasonable engineering trade-offs, but they
become trust gaps when an operator cannot independently verify what
the system actually did.

OWASP LLM09:2025 (Misinformation) identifies confabulation — confident
but false statements — as a top-level risk. [^3] NIST AI 600-1 lists
confabulation as one of twelve generative AI risk categories, noting
that confidently stated but false outputs are particularly dangerous in
high-stakes domains. [^4] An execution truth ledger directly addresses
this by providing a ground-truth record independent of the model's
narrative.

## Current state

### Existing instrumentation

Axinite already emits rich runtime data through several channels:

- **SSE events**: the web channel broadcasts structured events for
  assistant messages, tool calls, job status, and other lifecycle
  transitions. [^1]
- **Turn structure**: the in-memory `Thread` model preserves turns with
  tool-call metadata, including tool name, parameters, results, and
  timing. [^1]
- **Job persistence**: the `JobStore` interface persists job metadata,
  status transitions, and `job_actions` records. [^5]
- **LLM call recording**: an optional `RecordingLlm` wrapper captures
  LLM request/response pairs for trace capture. [^6]
- **Routine run records**: each routine run produces a `RoutineRun`
  record with trigger detail, status, summary, and token count. [^5]

### Gaps

- **No append-only durable ledger**: the existing data stores are
  mutable. Turn data can be compacted, summarized, or truncated. Job
  records can be updated. There is no immutable audit trail.
- **No independent truth surface**: the UI renders assistant messages and
  tool results from the same conversation model. An operator cannot
  easily distinguish "the model said X happened" from "the system
  recorded that X happened".
- **Bypass paths are invisible**: auth token submissions, provider-native
  compaction events, and other non-standard paths do not appear in the
  normal conversation transcript. [^1]
- **Policy decisions are not recorded**: when a safety check, approval
  gate, or policy evaluation (ADR 001) produces a decision, that
  decision is not durably stored in a queryable form.

## Goals and non-goals

- Goals:
  - Create an append-only ledger that records system-level execution
    events independently of the conversation transcript.
  - Record tool invocations, approvals, policy decisions, file writes,
    workspace mutations, delegated-job creation, and provider-side
    continuity state.
  - Expose the ledger through a UI surface that renders execution truth
    separately from assistant prose.
  - Support redaction and canonicalization for sensitive content
    (secrets, auth tokens, request bodies) without losing the fact that
    the action occurred.
  - Enable the UI to correlate completion claims in assistant prose
    with matching ledger entries.
- Non-goals:
  - Replace the existing conversation transcript. The ledger
    complements the transcript, not replaces it.
  - Provide real-time streaming of ledger events. The existing SSE
    channel handles live status; the ledger is a durable audit trail.
  - Define log retention or compliance policies. Those are
    deployment-specific concerns.

## Proposed design

### 1. Ledger entry schema

Each ledger entry is an immutable record with the following fields:

| Field | Type | Description |
| --- | --- | --- |
| `id` | UUID | Unique entry identifier. |
| `timestamp` | ISO 8601 | When the action occurred. |
| `workspace_id` | string | Workspace scope. |
| `thread_id` | string | Thread scope, if applicable. |
| `job_id` | string | Job scope, if applicable. |
| `entry_type` | enum | `tool_invocation`, `approval_decision`, `policy_decision`, `file_write`, `workspace_mutation`, `delegation`, `provider_event`, `bypass_event`. |
| `actor` | enum | `system`, `model`, `operator`, `routine`, `child_job`. |
| `detail` | JSON | Type-specific payload. |
| `redacted` | boolean | Whether sensitive fields have been redacted. |
| `content_hash` | string | SHA-256 hash of the unredacted detail, if redaction was applied. |
| `contract_version` | string | Intent contract version in effect at the time of the action. |

_Table 1: Ledger entry fields._

### 2. Entry types and payloads

**Tool invocation**: records the tool name, sanitized parameters, result
summary, execution duration, and whether the call was approved,
auto-approved, or escalated.

**Approval decision**: records the action that required approval, the
decision (granted, denied, timed out), the approver (operator or
policy), and the approval context.

**Policy decision**: records the policy evaluation input, the Rego
policy version, the decision (allow, deny, escalate), and the
machine-readable reason. This is the decision artefact produced by the
gate evaluation in RFC 0009.

**File write**: records the file path, operation type (create, update,
append, delete), a content hash, and the byte count.

**Workspace mutation**: records workspace document changes, including
path, operation, and content hash.

**Delegation**: records child-job creation, including the child job ID,
the delegation contract, tool allowlist, and budget parameters.

**Provider event**: records provider-side continuity state changes such
as `previous_response_id` transitions, compaction events, and connection
lifecycle events (see RFC 0008 and ADR 002).

**Bypass event**: records actions that bypass normal conversational
persistence, such as auth token submissions, with a redacted detail
payload.

### 3. Storage

The ledger should be stored in an append-only table in the existing
database, separate from the conversation tables. Both PostgreSQL and
libSQL backends must support the ledger schema. The table must enforce
append-only semantics at the application level; rows are never updated
or deleted during normal operation.

Retention and archival policies are deployment-specific and out of scope
for this RFC.

### 4. UI surface

The execution ledger should be exposed through a dedicated UI panel
(or tab) in the browser interface, separate from the conversation
view. This panel should:

- Display ledger entries in chronological order with filtering by entry
  type, actor, and scope.
- Correlate ledger entries with conversation turns where possible,
  linking tool invocation entries to the assistant message that
  triggered them.
- Highlight mismatches where an assistant message claims an action
  (e.g. "I've written the file") but no matching ledger entry exists.
- Display policy decisions with their machine-readable reasons.

### 5. Completion claim verification

An optional enhancement: when the assistant produces a message
containing completion-indicating language (e.g. "done", "sent",
"deleted", "created"), the UI can check whether a matching ledger entry
exists within a recent time window. If no match is found, the UI
renders the claim with a visual indicator (e.g. an "unverified" badge).

This is not a blocking requirement for the initial implementation but
is the logical endpoint of the "execution truth vs model narrative"
distinction.

## Requirements

### Functional requirements

- Every tool invocation, approval decision, policy decision, file
  write, workspace mutation, delegation, and provider event must
  produce a ledger entry.
- Ledger entries must be immutable once written.
- Sensitive content must be redactable without losing the fact that the
  action occurred.
- The ledger must be queryable by workspace, thread, job, entry type,
  actor, and time range.

### Technical requirements

- The ledger must support both PostgreSQL and libSQL backends.
- Ledger writes must not block the critical path of tool execution.
  Writes may be buffered and flushed asynchronously if necessary.
- The ledger schema must be versioned and migration-safe.
- Content hashes must use SHA-256.

## Compatibility and migration

The ledger is additive and does not modify existing tables or
behaviour. Migration involves:

1. Adding the ledger table to both database backends.
2. Adding ledger-write calls to existing gate points (tool execution,
   approval, safety checks) and new gate points (policy evaluation,
   delegation).
3. Adding a ledger query API to the web gateway.
4. Adding the ledger UI panel.

Existing conversations and jobs do not retroactively receive ledger
entries.

## Alternatives considered

### Option A: Extend existing SSE events with persistence

Persist the existing SSE event stream to a durable store. This is
simpler but conflates live status with audit trail, lacks the
immutability guarantee, and does not provide a clean separation between
assistant prose and execution truth.

### Option B: External audit log service

Delegate audit logging to an external service (e.g. a SIEM or log
aggregator). This is appropriate for enterprise deployments but should
not be the only option. A local, self-contained ledger ensures that
the execution truth surface is available even in offline or
local-first deployments.

## Open questions

- How much of each action should be stored verbatim versus hashed or
  redacted? The proposed design uses per-field redaction with a content
  hash, but the specific redaction policy needs further definition.
- Should completion claims such as "done", "sent", or "deleted" require
  a matching ledger entry before the UI renders them as successful?
  The proposed design makes this optional, but it could be a
  configurable strictness level.
- How should bypass paths such as auth-token submission or
  provider-native compaction events appear when they do not map neatly
  onto the normal chat transcript? The proposed `bypass_event` entry
  type handles this, but the redaction policy for auth tokens needs
  careful specification.
- Should the ledger support cryptographic integrity verification
  (e.g. hash chaining or Merkle trees) for tamper detection? This is
  valuable for high-assurance deployments but adds complexity.

## Recommendation

Implement the append-only execution ledger as a core infrastructure
component, not an optional plugin. Record every action at every gate
point, redact sensitive content with hash-based verification, and
expose the ledger as a first-class UI surface. Start with the
`tool_invocation`, `approval_decision`, and `policy_decision` entry
types, then extend to file writes, workspace mutations, delegation,
and provider events as those subsystems mature.

---

[^1]: Chat model and durable conversation record. See
    `docs/chat-model.md`.

[^2]: Intent-engineering analysis, execution truth vs model narrative.
    See `docs/What Axinite can learn from the video's approach to
    intent engineering.md`.

[^3]: OWASP Top 10 for LLM Applications 2025. LLM09:2025
    (Misinformation). See <https://genai.owasp.org/llm-top-10/>.

[^4]: NIST AI 600-1: Generative AI Profile. Confabulation risk
    category. See
    <https://nvlpubs.nist.gov/nistpubs/ai/NIST.AI.600-1.pdf>.

[^5]: Jobs and routines architecture. See
    `docs/jobs-and-routines.md`.

[^6]: LLM provider chain, `RecordingLlm` wrapper. See
    `src/llm/mod.rs`.
