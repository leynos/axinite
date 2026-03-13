# Axinite Roadmap

This roadmap turns the current axinite design set into a sequence of
implementation activities. It is derived from the welcome guide, RFCs
0001-0003 and 0006-0008 in this branch, and the two pending sibling-branch
RFCs that will become RFC 0004 and RFC 0005 when merged.

The document follows three constraints from the documentation style guide:

- it is structured as phases, steps, and tasks;
- every task is an implementation activity with a bounded finish line;
- the roadmap avoids dates and release promises.

Two source RFCs are pending merge from a sibling branch but are treated here as
reserved numbers:

- **RFC 0004**: delegated authorized endpoint requests
- **RFC 0005**: Monty-based Python code execution environment

## Source documents

- [welcome-to-axinite.md](./welcome-to-axinite.md)
- [RFC 0001](./rfcs/0001-expose-mcp-tool-definitions.md)
- [RFC 0002](./rfcs/0002-expose-wasm-tool-definitions.md)
- [RFC 0003](./rfcs/0003-skill-bundle-installation.md)
- `../rfcs/docs/rfcs/2026-03-11-tokenized-delegated-authorized-endpoint-requests.md`
  pending merge as RFC 0004
- `../rfcs/docs/rfcs/2026-03-11-monty-code-execution-environment.md`
  pending merge as RFC 0005
- [RFC 0006](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md)
- [RFC 0007](./rfcs/0007-secure-memory-sidecar-design.md)
- [RFC 0008](./rfcs/0008-websocket-responses-api.md)

## Sequencing principles

- Build explicit contracts before adding new execution surfaces.
- Land capability mediation before higher-risk automation features.
- Prefer shadow-mode and opt-in rollout paths where retrieval or provider
  behaviour can regress silently.
- Use steps as GIST-style workstreams: each step has one operational objective
  and one learning opportunity that should inform the next step.

## Phase 1: Make tool contracts explicit

Phase objective: ensure axinite advertises accurate tool interfaces before it
widens the runtime surface.

### Step 1: Hosted MCP tool catalog parity

Objective: make hosted workers advertise the real orchestrator-owned MCP tools
instead of only local proxy tools.

Learning opportunity: determine whether one remote catalog contract can support
both correct model-facing schemas and later observability needs.

- [ ] Add worker-orchestrator transport for remote tool catalog fetch and
  generic remote tool execution.
  Completion criteria:
  - the orchestrator exposes a hosted-visible catalog endpoint for active
    executable tools;
  - the worker can execute orchestrator-owned tools through one generic proxy
    path.
- [ ] Filter the hosted-visible catalog from the canonical `ToolRegistry`.
  Completion criteria:
  - only active MCP tools that are executable in hosted mode are advertised;
  - unavailable or approval-incompatible tools are excluded rather than
    described optimistically.
- [ ] Merge remote MCP tool definitions into the worker reasoning context.
  Completion criteria:
  - hosted large language model requests include the real tool descriptions and
    JSON Schemas;
  - worker-local tools and orchestrator-owned tools appear as one unified tool
    surface to the model.
- [ ] Add hosted-mode tests for schema fidelity and execution routing.
  Completion criteria:
  - tests fail if required MCP fields disappear or are rewritten incorrectly;
  - tests prove that advertised remote tools execute through the orchestrator
    rather than a local stub.

### Step 2: Proactive WASM schema publication

Objective: make proactive WASM schema advertisement the only normal contract
for active WASM tools.

Learning opportunity: verify how much provider-specific schema shaping can be
done without losing guest-defined semantics.

- [ ] Audit and fix WASM registration paths so every active tool publishes
  `ToolDefinition.parameters`.
  Completion criteria:
  - guest-exported metadata or explicit host overrides are applied during
    registration;
  - active WASM tools never rely on a failure path to teach the model their
    arguments.
- [ ] Extend the remote tool catalog to include orchestrator-owned WASM tools.
  Completion criteria:
  - hosted workers receive proactive WASM definitions through the same catalog
    path used for MCP tools;
  - hosted mode stops omitting orchestrator-owned WASM tools from the tool
    array.
- [ ] Demote schema-bearing retry hints to fallback diagnostics.
  Completion criteria:
  - wrapper comments and behaviour describe retry hints as supplemental help,
    not the primary contract;
  - validation or parse failures still surface actionable recovery guidance.
- [ ] Add end-to-end tests for first-call WASM schema exposure.
  Completion criteria:
  - tests prove that the first request includes the advertised schema;
  - hosted and non-hosted paths both fail if proactive schema publication
    regresses.

### Step 3: Multi-file skill bundles

Objective: replace the effective single-file skill model with a validated
bundle format and a narrow file-access surface.

Learning opportunity: measure whether progressive disclosure is sufficient for
multi-file skills without widening the generic filesystem surface.

- [ ] Implement `.skill` archive validation and extraction.
  Completion criteria:
  - the installer accepts only bundles with `SKILL.md` at the archive root;
  - unsupported top-level content and executable payloads are rejected.
- [ ] Extend skill installation flows for uploaded bundles and `.skill` URLs.
  Completion criteria:
  - install paths preserve `references/` and `assets/` when present;
  - installation failures report archive-shape errors explicitly.
- [ ] Persist canonical skill roots in the loaded skill model.
  Completion criteria:
  - runtime state records the installed skill root and `SKILL.md` entrypoint;
  - active-skill injection can refer to a stable bundle-relative file layout.
- [ ] Add a read-only `skill_read_file` interface for bundled resources.
  Completion criteria:
  - the model can read bundle-relative files without raw filesystem access;
  - oversized or disallowed files fail through a skill-scoped error path.
- [ ] Add installation and runtime tests for bundled skills.
  Completion criteria:
  - tests cover valid bundles, malformed bundles, and lazy bundled-file reads;
  - regression coverage proves that installation no longer drops ancillary
    files.

## Phase 2: Introduce controlled execution surfaces

Phase objective: add new programmable execution paths without weakening
capability mediation, redaction, or approval boundaries.

### Step 4: Delegated endpoint requests

Objective: let axinite use confidential service endpoints on behalf of WASM
tools without exposing raw URLs to the extension or the model.

Learning opportunity: validate whether endpoint confidentiality can coexist
with understandable approvals and useful diagnostics.

- [ ] Add typed setup fields and delegated endpoint binding persistence.
  Completion criteria:
  - extension setup can store non-secret endpoint configuration separately from
    secret material;
  - endpoint bindings are validated and stored through a dedicated service.
- [ ] Add delegated endpoint capability schema and WIT runtime plumbing.
  Completion criteria:
  - WASM capabilities can declare delegated endpoint use without naming the
    real host in a static allowlist;
  - the runtime exposes an `authorized-endpoint-request` path that resolves
    endpoint identities inside the host.
- [ ] Add endpoint-aware redaction, approval, and audit behaviour.
  Completion criteria:
  - logs, errors, and approval surfaces do not reveal configured endpoint URLs;
  - audit events retain enough structured detail to diagnose failures without
    leaking origin data.
- [ ] Deliver a pilot extension against the delegated request path.
  Completion criteria:
  - the pilot can operate end-to-end without guest-visible raw endpoint URLs;
  - test coverage proves that agent-visible output does not leak the endpoint.

### Step 5: Monty codemode runner

Objective: add a constrained Python execution environment for tool-oriented
automation without introducing a general-purpose runtime.

Learning opportunity: determine how much practical automation value axinite can
get from a JSON-only, host-brokered codemode before considering richer Python
surfaces.

- [ ] Add a helper subprocess wrapper for Monty and expose `exec_code`.
  Completion criteria:
  - Monty runs out of process so a panic does not terminate the parent runtime;
  - host callbacks remain constrained to an explicit per-run tool allowlist.
- [ ] Implement the JSON ABI for tool calls, parameters, results, and state.
  Completion criteria:
  - cross-boundary data is normalised to JSON-shaped values only;
  - host callback approval and attenuation rules are shared with existing tool
    execution paths.
- [ ] Add saved-script persistence with `save_script` and `run_script`.
  Completion criteria:
  - script source and manifest data are stored under a dedicated workspace
    scripts area;
  - optional per-script state is explicit rather than hidden in interpreter
    globals.
- [ ] Add run metadata and audit logging for script execution.
  Completion criteria:
  - each script run records version, inputs, outputs, and failure state;
  - reruns can distinguish code changes from parameter changes.
- [ ] Integrate saved scripts into higher-level automation paths.
  Completion criteria:
  - routines or job orchestration can invoke saved scripts without bypassing
    approval or policy checks;
  - review or rerun surfaces expose script identity and version clearly.

### Step 6: Provenance-enforced intent execution

Objective: replace plugin-controlled secret placement with host-assembled
intent execution and provenance-aware policy.

Learning opportunity: test whether a stable intent vocabulary can stay legible
to users while still being strict enough for enforceable policy decisions.

- [ ] Add `execution_model` plumbing and disable placeholder-based secret
  placement for zero-knowledge tools.
  Completion criteria:
  - capability loading and registry state distinguish legacy and
    provenance-enforced execution modes;
  - zero-knowledge tools reject `UrlPath`-style credential placement and other
    guest-controlled secret sinks.
- [ ] Introduce the intent WIT package, bindings, and wrapper selection.
  Completion criteria:
  - the runtime can load and instantiate intent-capable components alongside
    legacy WASM tools;
  - intent declarations are versioned independently from the existing
    `sandboxed-tool` world.
- [ ] Build the template registry and transport assembler.
  Completion criteria:
  - plugins declare semantic operations rather than raw HTTP requests;
  - the host can assemble a concrete request, inject credentials, and apply
    redaction obligations at send time.
- [ ] Add provenance token resources and policy-engine integration.
  Completion criteria:
  - the host can track provenance classes across intent execution and enforce
    allow or deny decisions through Rego;
  - policy outputs can require approval or redaction before a result reaches a
    public sink.
- [ ] Deliver one concrete service profile on the intent path.
  Completion criteria:
  - the pilot profile proves that authentication and templated transport can be
    handled without guest-visible secrets or endpoints;
  - integration tests cover both successful execution and blocked exfiltration
    attempts.
- [ ] Add fuzzing and differential tests for noninterference constraints.
  Completion criteria:
  - tests exercise derived-data exfiltration paths rather than only literal
    token leakage;
  - failures localise whether the break occurred in template assembly,
    provenance tracking, or policy evaluation.

## Phase 3: Move retrieval and conversation state onto durable boundaries

Phase objective: shift memory and long-running chat state onto components that
can be rolled out cautiously and observed directly.

### Step 7: Secure memory sidecar

Objective: replace the in-process memory path with a local sidecar that owns
extraction, recall, and structured memory storage.

Learning opportunity: compare shadow-mode recall and latency against the
current workspace search path before switching user-facing retrieval.

- [ ] Add transactional outbox support for memory-producing writes.
  Completion criteria:
  - conversation and workspace writes emit outbox events in the same database
    transaction;
  - memory side effects can be replayed without inventing state after the fact.
- [ ] Implement memoryd RPC over a Unix domain socket with capability tokens.
  Completion criteria:
  - memoryd exposes scoped read and write operations over a local-only socket;
  - invalid, expired, or over-scoped tokens are rejected deterministically.
- [ ] Add extraction and consolidation workers backed by local stores.
  Completion criteria:
  - the pipeline can extract facts and embeddings, write vectors to Qdrant, and
    persist structured facts in Oxigraph;
  - consolidation work runs through a queued worker model with retry and
    timeout limits.
- [ ] Run shadow-mode ingestion and recall alongside the existing search path.
  Completion criteria:
  - shadow mode records recall overlap, latency, and error metrics;
  - deletion propagation retracts facts and vectors when source content is
    removed.
- [ ] Switch retrieval to memoryd-first with fallback and kill switch support.
  Completion criteria:
  - user-facing recall prefers memoryd when active and falls back cleanly when
    unavailable;
  - one operator switch can disable the sidecar path without a schema rollback.

### Step 8: OpenAI Responses over WebSocket

Objective: add a stateful provider backend that supports multi-turn tool
calling and server-side compaction over a persistent WebSocket session.

Learning opportunity: determine whether a stateful provider session model fits
axinite's agent loop better than transcript replay for long-running tool-heavy
threads.

- [ ] Add a new provider protocol and configuration surface for Responses
  WebSocket mode.
  Completion criteria:
  - provider selection can opt into a Responses WebSocket backend without
    disturbing the existing `open_ai_completions` path;
  - configuration covers base URL, storage mode, and compaction strategy.
- [ ] Implement `ResponsesWsSession` connection management.
  Completion criteria:
  - the session enforces authenticated connection setup, sequential in-flight
    behaviour, reconnect handling, and connection rotation;
  - disconnects do not silently orphan per-thread provider state.
- [ ] Implement the streaming event parser and `response.create` builder.
  Completion criteria:
  - event handling reconstructs output text, function-call arguments, and final
    completion state correctly;
  - request construction maps axinite message and tool state into Responses
    input items and tool definitions.
- [ ] Preserve provider-native tool call state in thread persistence.
  Completion criteria:
  - tool turns store OpenAI `call_id` values and continuation identifiers;
  - continuation requests can emit `function_call_output` items without
    synthesising incompatible identifiers later.
- [ ] Integrate server-side compaction and retry controls.
  Completion criteria:
  - the delegate can enable Responses compaction without fighting the existing
    summarisation path;
  - retry and backoff rules handle rate limits, reconnects, and
    `previous_response_not_found` failures explicitly.
- [ ] Add mock WebSocket tests and feature-flagged rollout controls.
  Completion criteria:
  - automated tests cover long tool loops, compaction events, reconnects, and
    fallback behaviour;
  - rollout can be enabled per provider or model with dashboards for reconnect,
    compaction, and rate-limit failure rates.

## Completion criteria for the roadmap

The roadmap is complete when every step has shipped its headline tasks and the
resulting runtime satisfies the following product-level outcomes:

- hosted and local tool execution paths expose canonical machine-readable tool
  contracts before first use;
- extension packaging, delegated endpoints, codemode execution, and
  provenance-based intents all preserve explicit capability boundaries;
- memory and long-running provider state can be rolled out behind opt-in or
  shadow-mode controls rather than replacing current behaviour blindly.
