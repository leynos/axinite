# VTCode component import design

## Front matter

- **Status:** Proposed; component selection and integration design only.
- **Date:** 2026-09-06.
- **Audience:** Axinite and VTCode maintainers.
- **Scope:** Reusable terminal presentation, pure Responses codecs, and
  selectively imported provider adapters. Not agent-runtime replacement.
- **Precedence:** The [streaming design](responses-streaming-design.md),
  RFC 0008, ADR 002, ADR 006, and Axinite's security contracts retain authority.
- **Delivery:** [Component import roadmap](vtcode-component-import-roadmap.md).
- **Stack:** This design follows [Axinite PR #360](https://github.com/leynos/axinite/pull/360).
  Stacking establishes the design baseline, not a requirement to finish every
  streaming implementation task before starting a terminal-only pilot.

## 1. Decision and evidence

Reuse components behind Axinite-owned adapters. Keep Axinite's agent, provider
policy, tool registry, permissions, durable history, and memory authoritative.
Do not import `vtcode-core` as a replacement REPL or run a second agent loop.

The source review used Axinite `b816fd3719099f553601eb3077be4ba0e8a620ba` and
VTCode `f188bcb0e47d7386886ab0c3db7e338b297a3d07`. At that VTCode revision,
`vtcode-ui` already exposes `SessionOptions`, `HostAdapter`, and a command/event
interface. However, it depends on VTCode configuration types; session creation
uses unbounded queues and a spawned runner without an exposed join result.
`vtcode-llm` has useful normalized streaming but directly includes terminal,
configuration, safety, and tool-specification dependencies.[^1]

These findings identify extraction seams, not proven runtime defects or measured
integration savings. The existing [architecture programme #39](https://github.com/leynos/vtcode/issues/39)
separately tracks internal application decomposition. Its baseline and
capability trains are distinct unmerged trees. Select and test one actual
integration revision; never add their advertised features together as though
merged.

## 2. Component selection

The adoption order expresses priority, not a single blocking dependency chain.

| Component | Decision | Required boundary |
| --- | --- | --- |
| Terminal rendering, input, overlays, themes | First pilot | Host-configured `vtcode-ui`, supervised lifecycle, capability gates. |
| Responses request/event codecs and accumulator | Evaluate alongside native streaming | Pure protocol package, complete identities, strict terminal reconciliation. |
| Selected provider transport adapters | Conditional later import | Headless package, injected configuration/credentials, one policy owner. |
| Shared provider execution service | Optional separate decision | Explicit transfer of retry/deadline/admission ownership. |
| Agent runner, tools, approvals, archives, memory, scheduling | Do not import | Axinite already owns these application responsibilities. |

Table 1. Candidate components and the boundaries required for adoption.

Prefer a maintained, versioned package after extraction. An immutable Git
revision is an acceptable bounded pilot dependency when provenance, lockfile,
licence notices, and upgrade ownership are recorded. Never follow a moving
branch. Avoid copying large source trees: that creates a second maintenance
line and obscures fixes. A small vendored patch requires an owner, upstream
issue, exact provenance, tests, and removal condition.

A component is worthwhile only if integration and continuing upgrade costs are
lower than maintaining the corresponding Axinite behaviour. Measure dependency
closure, cold/warm build time, binary size, interaction latency, and review
burden; do not infer savings from crate names or lines of code.

## 3. Terminal adapter architecture

The terminal is an outer presentation adapter, not an inference consumer with
its own application policy.

```plaintext
vtcode-ui <-> AxiniteTuiChannel : NativeChannel <-> existing Axinite Agent
                                                    |
                                      providers / tools / safety / Database

optional Responses codec -> Axinite Responses transport/provider adapter
optional provider adapter -> existing Axinite provider policy and stream driver
```

Figure 1. Component reuse without importing a competing runtime.

Provisional implementation locations are `src/channels/tui/` for the Axinite
adapter and `src/llm/` for provider/protocol adapters. Keep dependency-specific
conversions inside these modules. Do not expose VTCode UI commands or foreign
provider request types through Axinite's domain interfaces.

The terminal adapter supplies an Axinite application name, optional workspace,
resolved appearance/keybindings, and an Axinite command catalogue. Reuse the
single help/completion registry planned in [Axinite #59](https://github.com/leynos/axinite/issues/59)
rather than create a third command list. A presentation-only pilot may use
buffered responses while the streaming workstream proceeds independently.

Map submitted text into `IncomingMessage` with the existing user, thread,
metadata, and timezone semantics. Map cancellation and exit through Axinite's
submission handling, preserving the distinction between interrupting the active
turn, closing an interface, and shutting down a process that owns other
channels. Do not run a terminal event loop inside the agent or let UI callbacks
perform model requests.

Axinite owns queued-input and steering semantics. Advertise those controls only
when the backend implements them. The initial adapter disables unsupported
primary-agent, pseudo-terminal (PTY), file, editor, and provider-management
controls. File opening, clipboard access, URL launching, notifications, and
external editors require explicit host capabilities; embedding must not acquire
ambient authority merely because the upstream UI offers a control.

## 4. Approval, authentication, and safe presentation

Render approval requests using Axinite request IDs, scope, descriptions, and
redacted parameters. Return decisions through Axinite's approval path. Bind a
decision to its still-pending request and scope; reject stale or cross-thread
answers. A UI setting cannot independently grant persistent approval or turn
on VTCode's skip-confirmations policy.

Authentication remains an Axinite workflow. Prefer the existing host-mediated
setup or browser flow. Any in-terminal secret entry requires a dedicated secure
input path: no command history, transcript, recording, completion, or telemetry
capture. The UI displays a challenge and returns a scoped answer, not a second
credential store or automatic VTCode login flow.

The streaming design's output-release policy runs before presentation. The UI
receives safe snapshots/deltas, not raw model tokens, raw tool arguments, or
opaque continuation data. Its renderer still sanitizes terminal control
sequences. Accessibility announcements are rate-limited semantic updates, not
an independent stream of unsanitized tokens.

Maintain a view model keyed by Axinite turn/response identity and presentation
revision. Reconcile final output exactly once. Interruption retains an explicit
partial marker, and proactive notifications use distinct identities. A stale
UI session cannot finalize or approve work in its replacement session.

Keep the existing plain REPL, one-shot mode, and non-TTY behaviour. A proposed
terminal selection setting has `plain`, `rich`, and later `auto` modes; its
exact spelling belongs to implementation review, not the shipped configuration
reference. Default remains `plain` during the pilot. Rich startup failure can
fall back only before input submission, or after an explicit detach/reconnect
with a canonical snapshot. Never resubmit a prompt silently during fallback.

## 5. Lifecycle, backpressure, and isolation

Consume the supervised UI session from VTCode #80: readiness, bounded command
admission, typed failure, and shutdown-and-join. The adapter reports health from
that lifecycle instead of returning unconditional success. The terminal runner
owns terminal restoration; the Axinite channel owns its input/output tasks.

Coalesce replaceable activity and preview snapshots under pressure. User input,
approvals, and required final state cannot disappear silently. Keep cancellation
responsive through a separate bounded control path or equivalent priority
mechanism. A rendering failure cannot be mistaken for provider failure or cause
an inference retry. Viewer detachment must not cancel unrelated background work.

Guard the dependency graph for rich and plain builds. The rich terminal may
legitimately depend on Ratatui; Axinite's provider/domain slice must not acquire
it through shared types. Disabling the rich feature must remove its added
terminal dependency closure, not merely hide a command. VTCode configuration
discovery, tool execution, provider setup, and archive loading remain outside
the imported UI closure.

## 6. Responses codec and provider reuse

Start with the pure codec boundary in VTCode #85. Place a narrow conversion
adapter between its wire values and Axinite's stream contract. Preserve
response, item, content, and function-call identities; opaque continuation
items; terminal state; and nullable usage. Lossy conversion is a rejection
criterion.

Run the same sanitized corpus through the native Axinite implementation and the
candidate component. Compare validated outcomes and error classifications,
including arbitrary framing, missing terminal calls, incomplete arguments,
reasoning-only responses, and missing usage. The corpus tests protocol fidelity;
Axinite's stricter terminal admission and sink policy remain separate tests.

VTCode-specific ID-remapping compatibility must not silently become an Axinite
default. Any supported exception needs an explicit endpoint/model capability,
semantic bijection, provenance fixtures, and security review. Raw custom tools
must not inherit ordinary function-call exceptions. Do not import an older
codec that bypasses completion checks fixed in later capability work.

Provider reuse follows only after #86's headless adapter extraction and a
measured benefit. Inject resolved endpoint, credentials, limits, capabilities,
and diagnostics. Axinite wraps raw provider execution with its existing chain
and remains the sole retry, failover, deadline, quota, and budget owner. Reusing
#46's shared execution service instead requires an explicit ownership decision
and removal of equivalent Axinite retry layers. Never stack both by default.

Stateful connection caches remain scoped to Axinite thread/configuration/model
and credential epoch. Axinite persists authoritative input, output, and recovery
state in both database backends. Do not import VTCode archive formats or create
a competing continuation store. Provider-hosted tools and implicit
cross-provider fallback stay disabled unless Axinite explicitly authorizes
their capability.

Pointing VTCode at Axinite's compatibility endpoint is not this design: that
endpoint is a provider facade, not Axinite's assistant. Neither an Agent Client
Protocol (ACP) server nor a Model Context Protocol (MCP) tool bridge substitutes
for a presentation library or pure codec.

## 7. Cross-repository prerequisites

| VTCode work | Axinite consumer | Blocking scope |
| --- | --- | --- |
| #79: host-neutral UI contract | `AxiniteTuiChannel` | Supported rich terminal. |
| #80: supervised UI lifecycle | Channel health and shutdown | Supported rich terminal. |
| #85: pure Responses codecs | Responses conversion adapter | Codec import only. |
| #86: headless provider adapters | Existing provider chain | Provider import only. |
| #88: external-consumer release gates | Dependency selection and upgrades | Respective imported package. |
| Existing #41/#42/#46 | Identity, dependency boundary, optional execution policy | Only the reused slice. |
| Existing #47/#49/#50 | VTCode persistence, projections, turn service | Not prerequisites for terminal or codec import. |

Table 2. Upstream work must block only the corresponding import lane.

The [existing programme](https://github.com/leynos/vtcode/issues/39) retains its
implementation order. New issues extend it with external-consumer requirements:
[UI contract](https://github.com/leynos/vtcode/issues/79),
[UI lifecycle](https://github.com/leynos/vtcode/issues/80),
[Responses codecs](https://github.com/leynos/vtcode/issues/85),
[provider adapters](https://github.com/leynos/vtcode/issues/86), and
[release contracts](https://github.com/leynos/vtcode/issues/88).

Seed consumer fixtures alongside extraction. Final package acceptance follows
the relevant extraction, but initial fixture work does not depend on every
issue closing. Axinite's streaming implementation proceeds natively when a
candidate component misses the acceptance gate.

## 8. Acceptance, rollout, and maintenance

A rich-UI pilot must produce the same canonical provider requests, tool
admission decisions, and session state as the plain channel under scripted
input. Exercise approval/denial, authentication, interruption, background
notification, concurrent threads, resume, pasted Unicode, resizing, and a
vanished terminal. Compare semantics rather than screen escape sequences. Use
fake rendering backends and focused pseudo-terminal tests, plus manual
accessibility checks before default promotion.

Codec/provider adoption requires the streaming roadmap's identity, safety,
cancellation, crash, accounting, and dual-backend tests to keep passing. An
external-consumer Cargo build must prove actual dependency closure and package
resolvability. No paid model calls belong in routine validation.

Ship each import behind its own feature and runtime selection. Record a pinned
revision or compatible release requirement, dependency budget, fixture version,
local patch inventory, licence notices, maintainer ownership, and rollback path.
The initial acceptance budget is measured against the native baseline and agreed
before promotion; this design invents neither timing gains nor build savings.

Rollback UI presentation to the plain channel without replaying submitted work.
Rollback a codec/provider at a response boundary using versioned canonical
state; never silently transfer an in-flight native continuation to a different
backend. Keep old readers until the supported rollback window closes.

No runtime dependency, default interface, storage schema, or feature-parity
status changes in this design PR. A later decision to share whole-agent
execution needs separate evidence and review; it is not the automatic final
phase of this plan.

## References

[^1]: VTCode main source evidence:
      [UI manifest](https://github.com/leynos/vtcode/blob/f188bcb0e47d7386886ab0c3db7e338b297a3d07/crates/codegen/vtcode-ui/Cargo.toml),
      [host contract](https://github.com/leynos/vtcode/blob/f188bcb0e47d7386886ab0c3db7e338b297a3d07/crates/codegen/vtcode-ui/src/tui/host.rs),
      [session lifecycle](https://github.com/leynos/vtcode/blob/f188bcb0e47d7386886ab0c3db7e338b297a3d07/crates/codegen/vtcode-ui/src/tui/session_options.rs),
      and [LLM manifest](https://github.com/leynos/vtcode/blob/f188bcb0e47d7386886ab0c3db7e338b297a3d07/crates/codegen/vtcode-llm/Cargo.toml).
