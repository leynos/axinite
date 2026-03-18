# Axinite Roadmap

This roadmap turns the current axinite design set into a sequence of
implementation activities. It is derived from the welcome guide, RFCs
0001-0003, 0006-0009 in this branch, and the two pending sibling-branch RFCs
that will become RFC 0004 and RFC 0005 when merged.

The roadmap follows the current documentation style guidance:

- phases are strategic milestones;
- steps are GIST-style workstreams with one delivery objective, one explicit
  learning opportunity, and clear sequencing value;
- headline tasks are atomic implementation activities written in dotted
  notation;
- dependencies are called out explicitly where work is not strictly linear;
- headline tasks include signposts to the RFC sections that justify them.

Two source RFCs are pending merge from a sibling branch but are treated here as
reserved numbers:

- **RFC 0004**: delegated authorized endpoint requests
- **RFC 0005**: Monty-based Python code execution environment

## Source documents

- [welcome-to-axinite.md](./welcome-to-axinite.md)
- [FEATURE_PARITY.md](../FEATURE_PARITY.md)
- [RFC 0001](./rfcs/0001-expose-mcp-tool-definitions.md)
- [RFC 0002](./rfcs/0002-expose-wasm-tool-definitions.md)
- [RFC 0003](./rfcs/0003-skill-bundle-installation.md)
- [Pending RFC 0004](./rfcs/2026-03-11-tokenized-delegated-authorized-endpoint-requests.md)
  delegated authorized endpoint requests, pending merge
- [Pending RFC 0005](./rfcs/2026-03-11-monty-code-execution-environment.md)
  Monty-based Python code execution environment, pending merge
- [RFC 0006](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md)
- [RFC 0007](./rfcs/0007-secure-memory-sidecar-design.md)
- [RFC 0008](./rfcs/0008-websocket-responses-api.md)
- [RFC 0009](./rfcs/0009-feature-flags-frontend.md)

## 1. Make tool contracts explicit

Phase objective: ensure axinite advertises accurate tool interfaces before it
widens the runtime surface.

### 1.1. Hosted MCP tool catalog parity

Objective: make hosted workers advertise the real orchestrator-owned MCP tools
instead of only local proxy tools.

Learning opportunity: determine whether one remote catalog contract can support
both model-facing schema fidelity and later observability needs.

Dependencies: unlocks 1.2 and reduces integration risk for 3.2.

- [ ] 1.1.1. Add worker-orchestrator transport for remote tool catalog fetch
  and generic remote tool execution.
  - See [RFC 0001 §Migration Plan](./rfcs/0001-expose-mcp-tool-definitions.md#migration-plan).
  - Success: the orchestrator exposes a hosted-visible catalog endpoint for
    active executable tools, and the worker can execute orchestrator-owned
    tools through one generic proxy path.
- [ ] 1.1.2. Filter the hosted-visible catalog from the canonical
  `ToolRegistry`. Requires 1.1.1.
  - See [RFC 0001 §Goals](./rfcs/0001-expose-mcp-tool-definitions.md#goals)
    and [RFC 0001 §Migration Plan](./rfcs/0001-expose-mcp-tool-definitions.md#migration-plan).
  - Success: only active MCP tools that are executable in hosted mode are
    advertised, and unavailable or approval-incompatible tools are excluded
    rather than described optimistically.
- [ ] 1.1.3. Merge remote MCP tool definitions into the worker reasoning
  context. Requires 1.1.1 and 1.1.2.
  - See [RFC 0001 §Summary](./rfcs/0001-expose-mcp-tool-definitions.md#summary)
    and [RFC 0001 §Migration Plan](./rfcs/0001-expose-mcp-tool-definitions.md#migration-plan).
  - Success: hosted model requests include the real tool descriptions and JSON
    Schemas, and worker-local tools plus orchestrator-owned tools appear as one
    unified tool surface.
- [ ] 1.1.4. Add hosted-mode tests for schema fidelity and execution routing.
  Requires 1.1.3.
  - See [RFC 0001 §Migration Plan](./rfcs/0001-expose-mcp-tool-definitions.md#migration-plan).
  - Success: tests fail if required MCP fields disappear or are rewritten
    incorrectly, and prove that advertised remote tools execute through the
    orchestrator rather than a local stub.

### 1.2. Proactive WASM schema publication

Objective: make proactive WASM schema advertisement the only normal contract
for active WASM tools.

Learning opportunity: verify how much provider-specific schema shaping can be
done without losing guest-defined semantics.

Dependencies: depends on 1.1 for the shared remote-catalog shape and informs
2.3 by tightening the contract around active WASM tools.

- [ ] 1.2.1. Audit and fix WASM registration paths so every active tool
  publishes `ToolDefinition.parameters`.
  - See [RFC 0002 §Current State](./rfcs/0002-expose-wasm-tool-definitions.md#current-state)
    and [RFC 0002 §Migration Plan](./rfcs/0002-expose-wasm-tool-definitions.md#migration-plan).
  - Success: guest-exported metadata or explicit host overrides are applied
    during registration, and active WASM tools never rely on a failure path to
    teach the model their arguments.
- [ ] 1.2.2. Extend the remote tool catalog to include orchestrator-owned WASM
  tools. Requires 1.1.1 and 1.2.1.
  - See [RFC 0002 §Problem](./rfcs/0002-expose-wasm-tool-definitions.md#problem)
    and [RFC 0002 §Migration Plan](./rfcs/0002-expose-wasm-tool-definitions.md#migration-plan).
  - Success: hosted workers receive proactive WASM definitions through the same
    catalog path used for MCP tools, and hosted mode stops omitting
    orchestrator-owned WASM tools from the tool array.
- [ ] 1.2.3. Demote schema-bearing retry hints to fallback diagnostics.
  Requires 1.2.1.
  - See [RFC 0002 §Summary](./rfcs/0002-expose-wasm-tool-definitions.md#summary)
    and [RFC 0002 §Migration Plan](./rfcs/0002-expose-wasm-tool-definitions.md#migration-plan).
  - Success: wrapper comments and behaviour describe retry hints as supplemental
    help rather than the primary contract, while parse and validation failures
    still surface actionable recovery guidance.
- [ ] 1.2.4. Add end-to-end tests for first-call WASM schema exposure. Requires
  1.2.2 and 1.2.3.
  - See [RFC 0002 §Goals](./rfcs/0002-expose-wasm-tool-definitions.md#goals)
    and [RFC 0002 §Migration Plan](./rfcs/0002-expose-wasm-tool-definitions.md#migration-plan).
  - Success: tests prove that the first request includes the advertised schema,
    and hosted plus non-hosted paths both fail if proactive schema publication
    regresses.

### 1.3. Multi-file skill bundles

Objective: replace the effective single-file skill model with a validated
bundle format and a narrow file-access surface.

Learning opportunity: measure whether progressive disclosure is sufficient for
multi-file skills without widening the generic filesystem surface.

Dependencies: independent of 1.1 and 1.2 at the transport layer, but should
land before 2.2 so codemode and later automation can rely on richer skill
content packaging.

- [ ] 1.3.1. Implement `.skill` archive validation and extraction.
  - See [RFC 0003 §Proposed Bundle Format](./rfcs/0003-skill-bundle-installation.md#proposed-bundle-format)
    and [RFC 0003 §Rollout Plan](./rfcs/0003-skill-bundle-installation.md#rollout-plan).
  - Success: the installer accepts only bundles with `SKILL.md` at the archive
    root and rejects unsupported top-level content or executable payloads.
- [ ] 1.3.2. Extend skill installation flows for uploaded bundles and `.skill`
  URLs. Requires 1.3.1.
  - See [RFC 0003 §Summary](./rfcs/0003-skill-bundle-installation.md#summary)
    and [RFC 0003 §Rollout Plan](./rfcs/0003-skill-bundle-installation.md#rollout-plan).
  - Success: install paths preserve `references/` and `assets/` when present,
    and installation failures report archive-shape errors explicitly.
- [ ] 1.3.3. Persist canonical skill roots in the loaded skill model. Requires
  1.3.1.
  - See [RFC 0003 §Reference Model](./rfcs/0003-skill-bundle-installation.md#reference-model)
    and [RFC 0003 §Rollout Plan](./rfcs/0003-skill-bundle-installation.md#rollout-plan).
  - Success: runtime state records the installed skill root and `SKILL.md`
    entrypoint, and active-skill injection can refer to a stable
    bundle-relative file layout.
- [ ] 1.3.4. Add a read-only `skill_read_file` interface for bundled resources.
  Requires 1.3.2 and 1.3.3.
  - See [RFC 0003 §Problem](./rfcs/0003-skill-bundle-installation.md#problem)
    and [RFC 0003 §Rollout Plan](./rfcs/0003-skill-bundle-installation.md#rollout-plan).
  - Success: the model can read bundle-relative files without raw filesystem
    access, and oversized or disallowed files fail through a skill-scoped error
    path.
- [ ] 1.3.5. Add installation and runtime tests for bundled skills. Requires
  1.3.2, 1.3.3, and 1.3.4.
  - See [RFC 0003 §Goals](./rfcs/0003-skill-bundle-installation.md#goals)
    and [RFC 0003 §Rollout Plan](./rfcs/0003-skill-bundle-installation.md#rollout-plan).
  - Success: tests cover valid bundles, malformed bundles, and lazy
    bundled-file reads, and prove that installation no longer drops ancillary
    files.

## 2. Introduce controlled execution surfaces

Phase objective: add new programmable execution paths without weakening
capability mediation, redaction, or approval boundaries.

### 2.1. Delegated endpoint requests

Objective: let axinite use confidential service endpoints on behalf of WASM
tools without exposing raw URLs to the extension or the model.

Learning opportunity: validate whether endpoint confidentiality can coexist
with understandable approvals and useful diagnostics.

Dependencies: depends on 1.2 for the stricter WASM contract and informs 2.3 by
establishing host-owned transport assembly for sensitive requests.

- [ ] 2.1.1. Add typed setup fields and delegated endpoint binding persistence.
  - See pending
    [RFC 0004 §Current Surface](./rfcs/2026-03-11-tokenized-delegated-authorized-endpoint-requests.md#current-surface)
    and
    [RFC 0004 §Rollout Plan](./rfcs/2026-03-11-tokenized-delegated-authorized-endpoint-requests.md#rollout-plan).
  - Success: extension setup can store non-secret endpoint configuration
    separately from secret material, and endpoint bindings are validated and
    stored through a dedicated service.
- [ ] 2.1.2. Add delegated endpoint capability schema and WIT runtime plumbing.
  Requires 2.1.1.
  - See pending
    [RFC 0004 §Goals](./rfcs/2026-03-11-tokenized-delegated-authorized-endpoint-requests.md#goals)
    and
    [RFC 0004 §Rollout Plan](./rfcs/2026-03-11-tokenized-delegated-authorized-endpoint-requests.md#rollout-plan).
  - Success: WASM capabilities can declare delegated endpoint use without
    naming the real host in a static allowlist, and the runtime exposes an
    `authorized-endpoint-request` path that resolves endpoint identities inside
    the host.
- [ ] 2.1.3. Add endpoint-aware redaction, approval, and audit behaviour.
  Requires 2.1.2.
  - See pending
    [RFC 0004 §Summary](./rfcs/2026-03-11-tokenized-delegated-authorized-endpoint-requests.md#summary)
    and
    [RFC 0004 §Rollout Plan](./rfcs/2026-03-11-tokenized-delegated-authorized-endpoint-requests.md#rollout-plan).
  - Success: logs, errors, and approval surfaces do not reveal configured
    endpoint URLs, while audit events retain enough structure to diagnose
    failures without leaking origin data.
- [ ] 2.1.4. Deliver a pilot extension against the delegated request path.
  Requires 2.1.3.
  - See pending
    [RFC 0004 §Problem](./rfcs/2026-03-11-tokenized-delegated-authorized-endpoint-requests.md#problem)
    and
    [RFC 0004 §Rollout Plan](./rfcs/2026-03-11-tokenized-delegated-authorized-endpoint-requests.md#rollout-plan).
  - Success: the pilot operates end to end without guest-visible raw endpoint
    URLs, and test coverage proves that agent-visible output does not leak the
    endpoint.

### 2.2. Monty codemode runner

Objective: add a constrained Python execution environment for tool-oriented
automation without introducing a general-purpose runtime.

Learning opportunity: determine how much practical automation value axinite can
get from a JSON-only, host-brokered codemode before considering richer Python
surfaces.

Dependencies: benefits from 1.3 if saved scripts are later packaged with richer
reference material, and provides an automation surface that routines can build
on after 2.3 and 3.2 settle.

- [ ] 2.2.1. Add a helper subprocess wrapper for Monty and expose `exec_code`.
  - See pending
    [RFC 0005 §Summary](./rfcs/2026-03-11-monty-code-execution-environment.md#summary)
    and
    [RFC 0005 §Rollout Plan](./rfcs/2026-03-11-monty-code-execution-environment.md#rollout-plan).
  - Success: Monty runs out of process so a panic does not terminate the parent
    runtime, and host callbacks remain constrained to an explicit per-run tool
    allowlist.
- [ ] 2.2.2. Implement the JSON ABI for tool calls, parameters, results, and
  state. Requires 2.2.1.
  - See pending
    [RFC 0005 §Goals](./rfcs/2026-03-11-monty-code-execution-environment.md#goals)
    and
    [RFC 0005 §Rollout Plan](./rfcs/2026-03-11-monty-code-execution-environment.md#rollout-plan).
  - Success: cross-boundary data is normalized to JSON-shaped values only, and
    host callback approval plus attenuation rules are shared with existing tool
    execution paths.
- [ ] 2.2.3. Add saved-script persistence with `save_script` and `run_script`.
  Requires 2.2.2.
  - See pending
    [RFC 0005 §Problem](./rfcs/2026-03-11-monty-code-execution-environment.md#problem)
    and
    [RFC 0005 §Rollout Plan](./rfcs/2026-03-11-monty-code-execution-environment.md#rollout-plan).
  - Success: script source and manifest data are stored under a dedicated
    workspace scripts area, and per-script state is explicit rather than hidden
    in interpreter globals.
- [ ] 2.2.4. Add run metadata and audit logging for script execution. Requires
  2.2.3.
  - See pending
    [RFC 0005 §Goals](./rfcs/2026-03-11-monty-code-execution-environment.md#goals)
    and
    [RFC 0005 §Rollout Plan](./rfcs/2026-03-11-monty-code-execution-environment.md#rollout-plan).
  - Success: each script run records version, inputs, outputs, and failure
    state, and reruns can distinguish code changes from parameter changes.
- [ ] 2.2.5. Integrate saved scripts into higher-level automation paths.
  Requires 2.2.3 and 2.2.4.
  - See pending
    [RFC 0005 §Rollout Plan](./rfcs/2026-03-11-monty-code-execution-environment.md#rollout-plan).
  - Success: routines or job orchestration can invoke saved scripts without
    bypassing approval or policy checks, and review or rerun surfaces expose
    script identity plus version clearly.

### 2.3. Provenance-enforced intent execution

Objective: replace plugin-controlled secret placement with host-assembled
intent execution and provenance-aware policy.

Learning opportunity: test whether a stable intent vocabulary can stay legible
to users while still being strict enough for enforceable policy decisions.

Dependencies: depends on 1.2 for WASM contract hygiene and on 2.1 for the
host-owned request model; it should land before 3.2 if Responses sessions are
expected to use these tools safely at scale.

- [ ] 2.3.1. Add `execution_model` plumbing and disable placeholder-based
  secret placement for zero-knowledge tools.
  - See [RFC 0006 §Migration checklist and prioritized
    plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritized-plan)
    and [RFC 0006 §Current IronClaw components and APIs relevant to an intent
    model](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#current-ironclaw-components-and-apis-relevant-to-an-intent-model).
  - Success: capability loading and registry state distinguish legacy and
    provenance-enforced execution modes, and zero-knowledge tools reject
    `UrlPath`-style credential placement plus other guest-controlled secret
    sinks.
- [ ] 2.3.2. Introduce the intent WIT package, bindings, and wrapper
  selection. Requires 2.3.1.
  - See [RFC 0006 §Design target: WIT-based intent ABI with provenance
    tokens](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#design-target-wit-based-intent-abi-with-provenance-tokens)
    and [RFC 0006 §Migration checklist and prioritized
    plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritized-plan).
  - Success: the runtime can load and instantiate intent-capable components
    alongside legacy WASM tools, and intent declarations are versioned
    independently from the existing `sandboxed-tool` world.
- [ ] 2.3.3. Build the template registry and transport assembler. Requires 2.1
  and 2.3.2.
  - See [RFC 0006 §Design target: WIT-based intent ABI with provenance
    tokens](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#design-target-wit-based-intent-abi-with-provenance-tokens)
    and [RFC 0006 §Migration checklist and prioritized
    plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritized-plan).
  - Success: plugins declare semantic operations rather than raw HTTP requests,
    and the host can assemble a concrete request, inject credentials, and apply
    redaction obligations at send time.
- [ ] 2.3.4. Add provenance token resources and policy-engine integration.
  Requires 2.3.2 and 2.3.3.
  - See [RFC 0006 §Executive
    summary](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#executive-summary)
    and [RFC 0006 §Migration checklist and prioritized
    plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritized-plan).
  - Success: the host can track provenance classes across intent execution and
    enforce allow or deny decisions through Rego, while policy outputs can
    require approval or redaction before a result reaches a public sink.
- [ ] 2.3.5. Deliver one concrete service profile on the intent path. Requires
  2.3.3 and 2.3.4.
  - See [RFC 0006 §Migration checklist and prioritized
    plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritized-plan).
  - Success: the pilot profile proves that authentication and templated
    transport can be handled without guest-visible secrets or endpoints, and
    integration tests cover both successful execution and blocked exfiltration
    attempts.
- [ ] 2.3.6. Add fuzzing and differential tests for noninterference
  constraints. Requires 2.3.4 and 2.3.5.
  - See [RFC 0006 §Migration checklist and prioritized
    plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritized-plan).
  - Success: tests exercise derived-data exfiltration paths rather than only
    literal token leakage, and failures localize whether the break occurred in
    template assembly, provenance tracking, or policy evaluation.

## 3. Move retrieval and conversation state onto durable boundaries

Phase objective: shift memory and long-running chat state onto components that
can be rolled out cautiously and observed directly.

### 3.1. Secure memory sidecar

Objective: replace the in-process memory path with a local sidecar that owns
extraction, recall, and structured memory storage.

Learning opportunity: compare shadow-mode recall and latency against the
current workspace search path before switching user-facing retrieval.

Dependencies: independent from 2.2, but should land before 3.2 if the provider
backend is expected to rely on richer memory recall during long-running
sessions.

- [ ] 3.1.1. Add transactional outbox support for memory-producing writes.
  - See [RFC 0007 §Executive
    summary](./rfcs/0007-secure-memory-sidecar-design.md#executive-summary)
    and [RFC 0007 §Rollout
    plan](./rfcs/0007-secure-memory-sidecar-design.md#rollout-plan).
  - Success: conversation and workspace writes emit outbox events in the same
    database transaction, and memory side effects can be replayed without
    inventing state after the fact.
- [ ] 3.1.2. Implement memoryd RPC over a Unix domain socket with capability
  tokens. Requires 3.1.1.
  - See [RFC 0007 §Security considerations, rollout plan, tests,
    monitoring](./rfcs/0007-secure-memory-sidecar-design.md#security-considerations-rollout-plan-tests-monitoring)
    and [RFC 0007 §Rollout
    plan](./rfcs/0007-secure-memory-sidecar-design.md#rollout-plan).
  - Success: memoryd exposes scoped read and write operations over a local-only
    socket, and invalid, expired, or over-scoped tokens are rejected
    deterministically.
- [ ] 3.1.3. Add extraction and consolidation workers backed by local stores.
  Requires 3.1.1 and 3.1.2.
  - See [RFC 0007 §Executive
    summary](./rfcs/0007-secure-memory-sidecar-design.md#executive-summary)
    and [RFC 0007 §Test
    plan](./rfcs/0007-secure-memory-sidecar-design.md#test-plan).
  - Success: the pipeline can extract facts and embeddings, write vectors to
    Qdrant, persist structured facts in Oxigraph, and run consolidation through
    queued workers with retry and timeout limits.
- [ ] 3.1.4. Run shadow-mode ingestion and recall alongside the existing search
  path. Requires 3.1.3.
  - See [RFC 0007 §Rollout
    plan](./rfcs/0007-secure-memory-sidecar-design.md#rollout-plan)
    and [RFC 0007 §Test
    plan](./rfcs/0007-secure-memory-sidecar-design.md#test-plan).
  - Success: shadow mode records recall overlap, latency, and error metrics,
    and deletion propagation retracts facts plus vectors when source content is
    removed.
- [ ] 3.1.5. Switch retrieval to memoryd-first with fallback and kill switch
  support. Requires 3.1.4.
  - See [RFC 0007 §Rollout
    plan](./rfcs/0007-secure-memory-sidecar-design.md#rollout-plan).
  - Success: user-facing recall prefers memoryd when active and falls back
    cleanly when unavailable, and one operator switch can disable the sidecar
    path without a schema rollback.

### 3.2. OpenAI Responses over WebSocket

Objective: add a stateful provider backend that supports multi-turn tool
calling and server-side compaction over a persistent WebSocket session.

Learning opportunity: determine whether a stateful provider session model fits
axinite's agent loop better than transcript replay for long-running tool-heavy
threads.

Dependencies: depends on 1.1 and 1.2 for canonical tool definitions, benefits
from 2.3 for safer tool execution at runtime, and should integrate with 3.1
rather than bypassing the memory path it introduces.

- [ ] 3.2.1. Add a new provider protocol and configuration surface for
  Responses WebSocket mode.
  - See [RFC 0008 §Requirements](./rfcs/0008-websocket-responses-api.md#requirements)
    and [RFC 0008 §Implementation
    plan](./rfcs/0008-websocket-responses-api.md#implementation-plan).
  - Success: provider selection can opt into a Responses WebSocket backend
    without disturbing the existing `open_ai_completions` path, and
    configuration covers base URL, storage mode, and compaction strategy.
- [ ] 3.2.2. Implement `ResponsesWsSession` connection management. Requires
  3.2.1.
  - See [RFC 0008 §Requirements](./rfcs/0008-websocket-responses-api.md#requirements)
    and [RFC 0008 §Stepwise
    tasks](./rfcs/0008-websocket-responses-api.md#stepwise-tasks).
  - Success: the session enforces authenticated connection setup, sequential
    in-flight behaviour, reconnect handling, and connection rotation, and
    disconnects do not silently orphan per-thread provider state.
- [ ] 3.2.3. Implement the streaming event parser and `response.create`
  builder. Requires 3.2.1 and 3.2.2.
  - See [RFC 0008 §Requirements](./rfcs/0008-websocket-responses-api.md#requirements)
    and [RFC 0008 §Stepwise
    tasks](./rfcs/0008-websocket-responses-api.md#stepwise-tasks).
  - Success: event handling reconstructs output text, function-call arguments,
    and final completion state correctly, and request construction maps axinite
    message plus tool state into Responses input items and tool definitions.
- [ ] 3.2.4. Preserve provider-native tool call state in thread persistence.
  Requires 3.2.2 and 3.2.3.
  - See [RFC 0008 §Feature gap
    analysis](./rfcs/0008-websocket-responses-api.md#feature-gap-analysis)
    and [RFC 0008 §Stepwise
    tasks](./rfcs/0008-websocket-responses-api.md#stepwise-tasks).
  - Success: tool turns store OpenAI `call_id` values and continuation
    identifiers, and continuation requests can emit `function_call_output`
    items without synthesizing incompatible identifiers later.
- [ ] 3.2.5. Integrate server-side compaction and retry controls. Requires
  3.1.5, 3.2.2, and 3.2.4.
  - See [RFC 0008 §Requirements](./rfcs/0008-websocket-responses-api.md#requirements)
    and [RFC 0008 §CI checks and rollout
    checklist](./rfcs/0008-websocket-responses-api.md#ci-checks-and-rollout-checklist).
  - Success: the delegate can enable Responses compaction without fighting the
    existing summarization path, and retry plus backoff rules handle rate
    limits, reconnects, and `previous_response_not_found` failures explicitly.
- [ ] 3.2.6. Add mock WebSocket tests and feature-flagged rollout controls.
  Requires 3.2.3, 3.2.4, and 3.2.5.
  - See [RFC 0008 §Stepwise
    tasks](./rfcs/0008-websocket-responses-api.md#stepwise-tasks)
    and [RFC 0008 §CI checks and rollout
    checklist](./rfcs/0008-websocket-responses-api.md#ci-checks-and-rollout-checklist).
  - Success: automated tests cover long tool loops, compaction events,
    reconnects, and fallback behaviour, and rollout can be enabled per provider
    or model with dashboards for reconnect, compaction, and rate-limit failure
    rates.

## 4. Harden operator lifecycle and control surfaces

Phase objective: make axinite operable as a long-running service and expose the
remaining control-plane capabilities through stable operator surfaces.

### 4.1. Service lifecycle and health supervision

Objective: turn axinite into a well-behaved long-running service under systemd
with explicit health aggregation, restart policy, and operator inspection.

Learning opportunity: determine which runtime states belong in readiness,
liveness, and restart logic once memoryd, hosted workers, and channels all run
concurrently.

Dependencies: builds on 3.1 and 3.2 so service health can include memory and
provider state instead of only HTTP reachability.

Outstanding design decisions: requires a companion ADR for user-vs-system
service ownership, unit override policy, and environment-file management before
4.1.2 and 4.1.3; requires a companion RFC for restart semantics and health
aggregation before 4.1.2.

- [ ] 4.1.1. Implement systemd unit generation, install, upgrade, and removal
  flows, and record the ownership model in a companion ADR.
  - See future RFC: systemd integration RFC §Summary, §Requirements, and
    §Rollout Plan. Companion ADR: service ownership and override policy.
  - Success: axinite can install and remove user-service units, write the
    required environment/config references, and preserve operator overrides
    without manual unit editing.
- [ ] 4.1.2. Implement runtime health aggregation and supervised restart
  policies for channels, workers, and sidecars. Requires 4.1.1 and 3.1.5.
  - See future RFC: health monitoring RFC §Problem, §Requirements, and
    §Rollout Plan.
  - Success: health state distinguishes degraded vs failed subsystems, channel
    health monitoring can restart configured components on policy, and restart
    storms are throttled with bounded backoff.
- [ ] 4.1.3. Expose service state, readiness, and restart history through the
  gateway and operator-facing surfaces. Requires 4.1.1 and 4.1.2.
  - See future RFC: health monitoring RFC §Compatibility and Migration.
  - Success: operators can inspect service health, readiness reasons, and
    restart history through stable APIs and CLI output rather than only logs.

### 4.2. Hook execution expansion and inspection

Objective: complete the remaining lifecycle and payload-inspection hook points
without letting hook behaviour drift across agent loops, routines, and gateway
surfaces.

Learning opportunity: determine how much mutability and failure handling the
hook system can support before it becomes non-deterministic across execution
paths.

Dependencies: builds on 2.3 for safer tool and message boundaries, and informs
5.3 because reasoning-trace capture should reuse the same inspection seams.

Outstanding design decisions: requires a companion RFC for hook ordering,
timeout policy, payload mutability, and redaction before 4.2.2 and 4.2.3.

- [ ] 4.2.1. Implement the missing `before_agent_start`,
  `before_message_write`, `llm_input`, and `llm_output` hook points across the
  conversational dispatcher, routines, and Responses session path.
  - See future RFC: hook execution RFC §Summary, §Requirements, and §Rollout
    Plan.
  - Success: the missing hook types fire consistently across the supported
    execution paths, and hook registration plus attenuation rules stay aligned
    with existing bundled, plugin, and workspace hooks.
- [ ] 4.2.2. Implement hook failure policy, timeout enforcement, and payload
  redaction controls. Requires 4.2.1.
  - See future RFC: hook execution RFC §Requirements and §Alternatives
    Considered.
  - Success: hook failures can be configured as fail-open or fail-closed per
    hook class, timeouts are enforced consistently, and sensitive payload fields
    are redacted before hook inspection or audit storage.
- [ ] 4.2.3. Implement hook inspection, replay, and operator tooling for the
  expanded hook surface. Requires 4.2.1 and 4.2.2.
  - See future RFC: hook execution RFC §Compatibility and Migration.
  - Success: operators can list, inspect, dry-run, and debug hook behaviour
    across bundled, plugin, and workspace hooks without editing files blindly.

### 4.3. Enhanced operator CLI

Objective: promote the CLI from a small admin subset to a full control-plane
surface that matches the parity matrix command set.

Learning opportunity: find a command taxonomy that stays scriptable and
discoverable while spanning service control, state inspection, and chat-driven
actions.

Dependencies: builds on 4.1 for service state, on 4.2 for hook inspection, and
on 5.2 for model-management verbs.

Outstanding design decisions: requires a companion ADR for command taxonomy,
output modes, and stability promises before 4.3.2-4.3.4.

- [ ] 4.3.1. Implement the shared CLI output and command taxonomy needed for an
  expanded control plane, and record the contract in a companion ADR.
  - See future ADR: enhanced CLI command taxonomy. See FEATURE_PARITY.md §CLI
    Commands.
  - Success: the CLI supports stable list/detail/status output modes and a
    coherent command hierarchy that future command families can reuse without
    re-litigating naming or output shape.
- [ ] 4.3.2. Implement the missing service and management command families:
  `gateway start/stop`, `channels`, `agents`, `sessions`, `nodes`, and
  `plugins`. Requires 4.3.1 and 4.1.3.
  - See future RFC: enhanced CLI RFC §Requirements and §Rollout Plan. See
    FEATURE_PARITY.md §CLI Commands.
  - Success: operators can manage gateway lifecycle, channels, agents,
    sessions, nodes, and plugins through the CLI with parity-grade inspection
    output and non-interactive scripting support.
- [ ] 4.3.3. Implement the missing automation and inspection command families:
  `cron`, `webhooks`, `logs`, improved `doctor`, and richer `models` output.
  Requires 4.3.1, 4.1.3, and 4.2.3.
  - See future RFC: enhanced CLI RFC §Requirements, §Compatibility and
    Migration, and §Rollout Plan. See FEATURE_PARITY.md §CLI Commands.
  - Success: operators can manage scheduled jobs, webhook config, log queries,
    health diagnostics, and model inventory from the CLI without falling back
    to direct database or config-file edits.
- [ ] 4.3.4. Implement the missing action workflows: `message send`, `browser`,
  `backup`, `update`, `/subagents spawn`, and `/export-session`. Requires 4.3.1
  and 4.3.3.
  - See future RFC: enhanced CLI RFC §Requirements and §Open Questions. See
    FEATURE_PARITY.md §CLI Commands.
  - Success: the CLI can trigger outbound channel sends, browser automation,
    local backups, self-update flows, subagent spawn, and session export with
    stable flags and audit-friendly output.

### 4.4. Feature flags for progressive front-end rollout

Objective: add a lightweight feature-flag delivery mechanism so the backend
can declare which front-end capabilities are enabled, the browser can gate
rendering accordingly, and operators can toggle flags at runtime without
restarting the gateway.

Learning opportunity: validate whether per-flag environment variables,
operator overrides, and subsystem-derived defaults provide sufficient control
for progressive feature rollout without introducing complex targeting or
percentage-based activation.

Dependencies: independent of other Phase 4 work, but provides a foundation for
Phase 6 front-end features (canvas hosting, advanced media handling) to roll
out behind flags.

- [ ] 4.4.1. Add per-flag environment variable parsing and registry
  initialization in `GatewayChannel` or a dedicated config module.
  - See [RFC 0009 §Configuration inputs](./rfcs/0009-feature-flags-frontend.md#1-configuration-inputs).
  - Success: `FEATURE_FLAG_<NAME>` environment variables are parsed into a
    mutable `FeatureFlagRegistry` with `Arc<RwLock<HashMap<String, bool>>>`,
    invalid flag names are silently discarded with a warning, and compiled
    defaults are applied for flags without environment overrides.
- [ ] 4.4.2. Add the `FeatureFlagRegistry` struct and `GatewayState`
  integration. Requires 4.4.1.
  - See [RFC 0009 §Data shape](./rfcs/0009-feature-flags-frontend.md#2-data-shape)
    and [RFC 0009 §GatewayState integration](./rfcs/0009-feature-flags-frontend.md#3-gatewaystate-integration).
  - Success: `GatewayState` holds an `Arc<FeatureFlagRegistry>` that supports
    runtime re-resolution when operator overrides change, and subsystem
    availability defaults are applied during initialization based on
    `GatewayState` field presence.
- [ ] 4.4.3. Extend the settings handler to detect `feature_flag:` keys and
  apply runtime overrides to the registry. Requires 4.4.2.
  - See [RFC 0009 §Configuration inputs](./rfcs/0009-feature-flags-frontend.md#1-configuration-inputs).
  - Success: `PUT /api/settings/feature_flag:<name>` writes persist to the
    database and immediately update the `FeatureFlagRegistry`, and subsequent
    `GET /api/features` requests reflect the updated flag state without a
    gateway restart.
- [ ] 4.4.4. Implement the `GET /api/features` endpoint. Requires 4.4.2.
  - See [RFC 0009 §API endpoint](./rfcs/0009-feature-flags-frontend.md#4-api-endpoint).
  - Success: the endpoint returns a JSON object mapping flag names to booleans
    (e.g. `{ "experimental_chat_ui": true, "dark_mode": false }`) by
    serializing the resolved registry state from `GatewayState`, with no
    database query on the hot path.
- [ ] 4.4.5. Add front-end `loadFeatureFlags()` integration in `app.js`.
  Requires 4.4.4.
  - See [RFC 0009 §Front-end consumption](./rfcs/0009-feature-flags-frontend.md#5-front-end-consumption).
  - Success: the browser fetches `/api/features` after authentication, stores
    the result in a plain object, and provides `featureFlags.experimental_chat_ui`
    checks for gating UI rendering and behaviour.
- [ ] 4.4.6. Add integration tests for per-flag resolution, operator overrides,
  subsystem defaults, mutability, and endpoint contract. Requires 4.4.3, 4.4.4,
  and 4.4.5.
  - See [RFC 0009 §Requirements](./rfcs/0009-feature-flags-frontend.md#requirements).
  - Success: tests cover per-flag environment variable parsing (including
    `FEATURE_FLAG_<NAME>` pattern matching), operator override persistence and
    immediate registry updates, subsystem/default fallback behavior, concurrent
    access to the mutable registry, endpoint response shape (boolean map), and
    no hot-path database hits, and prove that invalid flag names are discarded
    with warnings.

## 5. Add model, reasoning, and citation control

Phase objective: expose model discovery, model choice, compaction control,
reasoning visibility, and citations as first-class runtime controls.

### 5.1. Provider discovery and GLM-5 support

Objective: reduce manual model metadata upkeep while expanding provider support
to the missing GLM-5 family.

Learning opportunity: determine how discovered provider metadata should be
trusted, cached, and overridden when the runtime also carries a built-in model
registry.

Dependencies: informs 5.2 because in-app model switching needs richer model
metadata than the current static surface provides.

Outstanding design decisions: requires a companion RFC for `llms.txt`
discovery trust, cache invalidation, override precedence, and provider-capacity
mapping before 5.1.2 and 5.1.3.

- [ ] 5.1.1. Implement `llms.txt` discovery, caching, and operator controls,
  and record the trust model in a companion RFC.
  - See future RFC: `llms.txt` discovery RFC §Summary, §Requirements, and
    §Rollout Plan. See FEATURE_PARITY.md §Agent System and §Model & Provider
    Support.
  - Success: axinite can fetch and cache `llms.txt` metadata, operators can
    inspect or disable discovered metadata, and discovery failures do not
    silently corrupt the provider registry.
- [ ] 5.1.2. Integrate discovered metadata into the provider registry and model
  capability surface. Requires 5.1.1.
  - See future RFC: `llms.txt` discovery RFC §Compatibility and Migration.
  - Success: discovered metadata can augment provider capability views and
    model selection without overriding explicit local configuration by accident.
- [ ] 5.1.3. Add GLM-5 support with provider wiring, model metadata, and
  fallback tests. Requires 5.1.2.
  - See future RFC: GLM-5 support RFC §Summary, §Requirements, and §Rollout
    Plan. See FEATURE_PARITY.md §Model & Provider Support.
  - Success: GLM-5 models can be selected, tested, and fail over through the
    existing provider abstractions without provider-specific hacks leaking into
    unrelated code paths.

### 5.2. Runtime model selection and compaction control

Objective: let users and operators choose models at the right scope without
making override precedence or compaction behaviour ambiguous.

Learning opportunity: clarify the boundary between global defaults, per-session
selection, per-surface overrides, and compaction-only provider choice.

Dependencies: builds on 5.1 for richer model metadata and on 3.2 if the
Responses backend becomes one of the selectable model surfaces.

Outstanding design decisions: requires a companion ADR for override precedence
and a companion RFC for compaction-provider semantics before 5.2.2 and 5.2.3.

- [ ] 5.2.1. Implement persisted in-app model switching for the web control UI
  and WebChat surfaces, and record precedence rules in a companion ADR.
  - See future ADR: model override precedence. See FEATURE_PARITY.md §Web
    Interface and §Model Features.
  - Success: users can change the active model in-app, the chosen scope is
    persisted correctly, and the resulting effective model is visible rather
    than implicit.
- [ ] 5.2.2. Implement compaction model override and compaction-only provider
  selection. Requires 5.2.1.
  - See future RFC: compaction model override RFC §Problem, §Requirements, and
    §Rollout Plan. See FEATURE_PARITY.md §Agent System.
  - Success: summarization and compaction can run on a dedicated provider/model
    path, and cost attribution plus failure handling stay separate from the
    primary response model.
- [ ] 5.2.3. Expose model and compaction override inspection/edit surfaces
  through the gateway and CLI. Requires 5.2.1 and 5.2.2.
  - See future ADR: model override precedence. See future RFC: compaction model
    override RFC §Compatibility and Migration.
  - Success: operators can inspect, set, and clear model overrides from the
    UI, API, and CLI without guessing which layer currently wins.

### 5.3. Reasoning traces and citations

Objective: make reasoning output inspectable and attributable without leaking
unsafe internals or turning trace storage into an uncontrolled data sink.

Learning opportunity: balance transparency, privacy, and token cost when
capturing internal traces and surfacing citations to end users.

Dependencies: builds on 4.2 because trace capture should reuse the payload
inspection seams, and on 3.1 because citations should align with memory and
workspace provenance rather than inventing a second source model.

Outstanding design decisions: requires a companion RFC for trace retention,
redaction, and visibility policy before 5.3.2; requires a companion RFC for
citation provenance and rendering semantics before 5.3.3 and 5.3.4.

- [ ] 5.3.1. Implement the reasoning trace event model, retention controls, and
  redaction pipeline, and record the policy in a companion RFC.
  - See future RFC: reasoning traces RFC §Summary, §Requirements, and §Known
    Risks. See FEATURE_PARITY.md §Agent System.
  - Success: trace events are captured in one schema, sensitive fields can be
    redacted or disabled by policy, and trace storage does not grow without
    retention bounds.
- [ ] 5.3.2. Implement visible and configurable reasoning trace surfaces across
  the UI, API, and CLI. Requires 4.2.3 and 5.3.1.
  - See future RFC: reasoning traces RFC §Compatibility and Migration.
  - Success: operators and users can opt into or out of trace visibility at the
    supported scopes, and the active trace policy is obvious in each surface.
- [ ] 5.3.3. Implement citation capture across provider responses, tool
  outputs, and workspace-backed context. Requires 3.1.5 and 5.3.1.
  - See future RFC: citation support RFC §Requirements, §Proposed Design, and
    §Alternatives Considered. See FEATURE_PARITY.md §Agent System.
  - Success: citations preserve enough provenance to show where a claim came
    from, distinguish retrieved context from model synthesis, and survive
    session export or replay.
- [ ] 5.3.4. Implement citation rendering and export surfaces in chat, web UI,
  and CLI transcripts. Requires 5.3.2 and 5.3.3.
  - See future RFC: citation support RFC §Compatibility and Migration.
  - Success: citations render consistently across text outputs, exports, and
    debugging surfaces, and degrade gracefully when a provider or tool does not
    emit citation-ready provenance.

## 6. Deliver rich interaction and media surfaces

Phase objective: add the missing interactive UI and media-handling features
without splintering attachment semantics or weakening gateway boundaries.

### 6.1. Canvas hosting

Objective: add agent-driven canvas hosting with placement, resizing, and
runtime mutation support inside the existing web control surface.

Learning opportunity: validate how much agent-controlled UI can be exposed
without giving runtime code ambient authority over the host page.

Dependencies: benefits from 5.3.4 if citations and reasoning traces are later
rendered inside canvas surfaces, but can land independently of provider
selection work.

Outstanding design decisions: requires a companion RFC for canvas authority,
widget transport, placement persistence, asset loading, and isolation
boundaries before 6.1.2-6.1.4.

- [ ] 6.1.1. Implement the canvas session contract, event model, and placement
  persistence, and record the contract in a companion RFC.
  - See future RFC: canvas hosting RFC §Summary, §Requirements, and §Rollout
    Plan. See FEATURE_PARITY.md §Gateway & Control Plane and §Web Interface.
  - Success: axinite can persist canvas instances, placement state, and canvas
    lifecycle events without embedding canvas-specific assumptions into the
    generic chat thread model.
- [ ] 6.1.2. Implement the control UI canvas host with placement, resizing, and
  lifecycle management. Requires 6.1.1.
  - See future RFC: canvas hosting RFC §Proposed Design and §Compatibility and
    Migration.
  - Success: the web UI can host multiple canvases, preserve placement changes,
    and recover cleanly after reload or reconnect.
- [ ] 6.1.3. Implement agent/runtime canvas mutation APIs and gateway auth
  boundaries. Requires 6.1.1 and 6.1.2.
  - See future RFC: canvas hosting RFC §Requirements and §Known Risks.
  - Success: agents can update canvas content through explicit APIs, and the
    gateway enforces capability and origin boundaries instead of trusting raw
    client-side mutation requests.
- [ ] 6.1.4. Add end-to-end canvas tests and asset-resolution handling.
  Requires 6.1.2 and 6.1.3.
  - See future RFC: canvas hosting RFC §Rollout Plan.
  - Success: automated tests cover placement, resize, reconnect, and asset
    loading behaviour, and the canvas host resolves runtime assets without
    broken relative-path assumptions.

### 6.2. Advanced media handling

Objective: unify richer media ingest, transformation, caching, and rendering so
channels, tools, and UI surfaces stop growing ad hoc media code paths.

Learning opportunity: determine what one attachment contract must preserve
across caching, image manipulation, audio transcription, PDF handling, TTS, and
sticker-to-image conversion.

Dependencies: informs 4.2 because `transcribeAudio` hooks need a stable media
contract, and benefits from 6.1 if canvas surfaces are later used to present
rich media results.

Outstanding design decisions: requires a companion RFC for attachment caching,
provenance, transformation policy, and per-channel capability negotiation
before 6.2.2-6.2.4.

- [ ] 6.2.1. Implement a unified media ingest and caching pipeline covering
  images, PDFs, forwarded attachment downloads, and rich-text embedded media,
  and record the contract in a companion RFC.
  - See future RFC: advanced media handling RFC §Summary, §Requirements, and
    §Rollout Plan. See FEATURE_PARITY.md §Media handling, §Channel Features,
    and §Automation.
  - Success: media items are normalized into one cached attachment model with
    provenance, size, and channel metadata, and forwarded or embedded media can
    be fetched without bespoke per-channel storage code.
- [ ] 6.2.2. Implement media transformation paths for image manipulation,
  sticker-to-image conversion, PDF parsing/analysis, and multiple images per
  tool call. Requires 6.2.1.
  - See future RFC: advanced media handling RFC §Proposed Design and
    §Alternatives Considered.
  - Success: the runtime can apply the required transformations through one
    media service layer, and tool or channel paths can emit multiple images or
    PDF-derived assets without inventing one-off payload formats.
- [ ] 6.2.3. Implement audio transcription, `transcribeAudio` hook integration,
  TTS generation, and streaming-friendly audio surfaces. Requires 4.2.1 and
  6.2.1.
  - See future RFC: advanced media handling RFC §Requirements and §Compatibility
    and Migration. See FEATURE_PARITY.md §Automation and §Media handling.
  - Success: audio inputs can be transcribed through a stable pipeline, TTS can
    produce cached output artefacts, and hook integrations receive consistent
    media metadata instead of transport-specific payloads.
- [ ] 6.2.4. Implement per-channel media limits, fallback rendering, and media
  inspection tests. Requires 6.2.1, 6.2.2, and 6.2.3.
  - See future RFC: advanced media handling RFC §Rollout Plan and §Known
    Risks.
  - Success: per-channel limits and capability negotiation are enforced in one
    place, unsupported media falls back predictably, and automated tests cover
    preview, cache reuse, transcription, TTS, and transformed-media flows.

## Appendix: Completion criteria

The roadmap is complete when every step has shipped its headline tasks and the
resulting runtime satisfies the following product-level outcomes:

- hosted and local tool execution paths expose canonical machine-readable tool
  contracts before first use;
- extension packaging, delegated endpoints, codemode execution, and
  provenance-based intents all preserve explicit capability boundaries;
- memory and long-running provider state can be rolled out behind opt-in or
  shadow-mode controls rather than replacing current behaviour blindly;
- operators can install, supervise, inspect, and control axinite through first
  class service, health, hook, and CLI workflows;
- model choice, compaction policy, reasoning visibility, citations, canvas
  hosting, and rich media handling are explicit runtime capabilities rather than
  implicit or surface-specific behaviour.
