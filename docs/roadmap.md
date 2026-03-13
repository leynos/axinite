# Axinite Roadmap

This roadmap turns the current axinite design set into a sequence of
implementation activities. It is derived from the welcome guide, RFCs
0001-0003 and 0006-0008 in this branch, and the two pending sibling-branch
RFCs that will become RFC 0004 and RFC 0005 when merged.

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
- [RFC 0001](./rfcs/0001-expose-mcp-tool-definitions.md)
- [RFC 0002](./rfcs/0002-expose-wasm-tool-definitions.md)
- [RFC 0003](./rfcs/0003-skill-bundle-installation.md)
- `../../rfcs/docs/rfcs/2026-03-11-tokenized-delegated-authorized-endpoint-requests.md`
  pending merge as RFC 0004
- `../../rfcs/docs/rfcs/2026-03-11-monty-code-execution-environment.md`
  pending merge as RFC 0005
- [RFC 0006](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md)
- [RFC 0007](./rfcs/0007-secure-memory-sidecar-design.md)
- [RFC 0008](./rfcs/0008-websocket-responses-api.md)

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
  - See pending RFC 0004
    `../../rfcs/docs/rfcs/2026-03-11-tokenized-delegated-authorized-endpoint-requests.md`
    §Current Surface and §Rollout Plan.
  - Success: extension setup can store non-secret endpoint configuration
    separately from secret material, and endpoint bindings are validated and
    stored through a dedicated service.
- [ ] 2.1.2. Add delegated endpoint capability schema and WIT runtime plumbing.
  Requires 2.1.1.
  - See pending RFC 0004
    `../../rfcs/docs/rfcs/2026-03-11-tokenized-delegated-authorized-endpoint-requests.md`
    §Goals and §Rollout Plan.
  - Success: WASM capabilities can declare delegated endpoint use without
    naming the real host in a static allowlist, and the runtime exposes an
    `authorized-endpoint-request` path that resolves endpoint identities inside
    the host.
- [ ] 2.1.3. Add endpoint-aware redaction, approval, and audit behaviour.
  Requires 2.1.2.
  - See pending RFC 0004
    `../../rfcs/docs/rfcs/2026-03-11-tokenized-delegated-authorized-endpoint-requests.md`
    §Summary and §Rollout Plan.
  - Success: logs, errors, and approval surfaces do not reveal configured
    endpoint URLs, while audit events retain enough structure to diagnose
    failures without leaking origin data.
- [ ] 2.1.4. Deliver a pilot extension against the delegated request path.
  Requires 2.1.3.
  - See pending RFC 0004
    `../../rfcs/docs/rfcs/2026-03-11-tokenized-delegated-authorized-endpoint-requests.md`
    §Problem and §Rollout Plan.
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
  - See pending RFC 0005
    `../../rfcs/docs/rfcs/2026-03-11-monty-code-execution-environment.md`
    §Summary and §Rollout Plan.
  - Success: Monty runs out of process so a panic does not terminate the parent
    runtime, and host callbacks remain constrained to an explicit per-run tool
    allowlist.
- [ ] 2.2.2. Implement the JSON ABI for tool calls, parameters, results, and
  state. Requires 2.2.1.
  - See pending RFC 0005
    `../../rfcs/docs/rfcs/2026-03-11-monty-code-execution-environment.md`
    §Goals and §Rollout Plan.
  - Success: cross-boundary data is normalised to JSON-shaped values only, and
    host callback approval plus attenuation rules are shared with existing tool
    execution paths.
- [ ] 2.2.3. Add saved-script persistence with `save_script` and `run_script`.
  Requires 2.2.2.
  - See pending RFC 0005
    `../../rfcs/docs/rfcs/2026-03-11-monty-code-execution-environment.md`
    §Problem and §Rollout Plan.
  - Success: script source and manifest data are stored under a dedicated
    workspace scripts area, and per-script state is explicit rather than hidden
    in interpreter globals.
- [ ] 2.2.4. Add run metadata and audit logging for script execution. Requires
  2.2.3.
  - See pending RFC 0005
    `../../rfcs/docs/rfcs/2026-03-11-monty-code-execution-environment.md`
    §Goals and §Rollout Plan.
  - Success: each script run records version, inputs, outputs, and failure
    state, and reruns can distinguish code changes from parameter changes.
- [ ] 2.2.5. Integrate saved scripts into higher-level automation paths.
  Requires 2.2.3 and 2.2.4.
  - See pending RFC 0005
    `../../rfcs/docs/rfcs/2026-03-11-monty-code-execution-environment.md`
    §Rollout Plan.
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
  - See [RFC 0006 §Migration checklist and prioritised
    plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritised-plan)
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
    and [RFC 0006 §Migration checklist and prioritised
    plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritised-plan).
  - Success: the runtime can load and instantiate intent-capable components
    alongside legacy WASM tools, and intent declarations are versioned
    independently from the existing `sandboxed-tool` world.
- [ ] 2.3.3. Build the template registry and transport assembler. Requires 2.1
  and 2.3.2.
  - See [RFC 0006 §Design target: WIT-based intent ABI with provenance
    tokens](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#design-target-wit-based-intent-abi-with-provenance-tokens)
    and [RFC 0006 §Migration checklist and prioritised
    plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritised-plan).
  - Success: plugins declare semantic operations rather than raw HTTP requests,
    and the host can assemble a concrete request, inject credentials, and apply
    redaction obligations at send time.
- [ ] 2.3.4. Add provenance token resources and policy-engine integration.
  Requires 2.3.2 and 2.3.3.
  - See [RFC 0006 §Executive
    summary](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#executive-summary)
    and [RFC 0006 §Migration checklist and prioritised
    plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritised-plan).
  - Success: the host can track provenance classes across intent execution and
    enforce allow or deny decisions through Rego, while policy outputs can
    require approval or redaction before a result reaches a public sink.
- [ ] 2.3.5. Deliver one concrete service profile on the intent path. Requires
  2.3.3 and 2.3.4.
  - See [RFC 0006 §Migration checklist and prioritised
    plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritised-plan).
  - Success: the pilot profile proves that authentication and templated
    transport can be handled without guest-visible secrets or endpoints, and
    integration tests cover both successful execution and blocked exfiltration
    attempts.
- [ ] 2.3.6. Add fuzzing and differential tests for noninterference
  constraints. Requires 2.3.4 and 2.3.5.
  - See [RFC 0006 §Migration checklist and prioritised
    plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritised-plan).
  - Success: tests exercise derived-data exfiltration paths rather than only
    literal token leakage, and failures localise whether the break occurred in
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
    items without synthesising incompatible identifiers later.
- [ ] 3.2.5. Integrate server-side compaction and retry controls. Requires
  3.1.5, 3.2.2, and 3.2.4.
  - See [RFC 0008 §Requirements](./rfcs/0008-websocket-responses-api.md#requirements)
    and [RFC 0008 §CI checks and rollout
    checklist](./rfcs/0008-websocket-responses-api.md#ci-checks-and-rollout-checklist).
  - Success: the delegate can enable Responses compaction without fighting the
    existing summarisation path, and retry plus backoff rules handle rate
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

## Appendix: Completion criteria

The roadmap is complete when every step has shipped its headline tasks and the
resulting runtime satisfies the following product-level outcomes:

- hosted and local tool execution paths expose canonical machine-readable tool
  contracts before first use;
- extension packaging, delegated endpoints, codemode execution, and
  provenance-based intents all preserve explicit capability boundaries;
- memory and long-running provider state can be rolled out behind opt-in or
  shadow-mode controls rather than replacing current behaviour blindly.
