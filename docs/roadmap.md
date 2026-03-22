# Axinite Roadmap

This roadmap turns the current axinite design set into a sequence of
implementation activities. It is derived from the welcome guide, RFCs 0001-0017,
ADRs 001-005, and the formal-verification design document in this branch.

The roadmap follows the current documentation style guidance:

- phases are strategic milestones;
- steps are GIST-style workstreams with one delivery objective, one explicit
  learning opportunity, and clear sequencing value;
- headline tasks are atomic implementation activities written in dotted
  notation;
- dependencies are called out explicitly where work is not strictly linear;
- headline tasks include signposts to the RFC sections that justify them.

## Source documents

- [welcome-to-axinite.md](./welcome-to-axinite.md)
- [formal-verification-methods-in-axinite.md](./formal-verification-methods-in-axinite.md)
- [FEATURE_PARITY.md](../FEATURE_PARITY.md)
- [RFC 0001](./rfcs/0001-expose-mcp-tool-definitions.md)
- [RFC 0002](./rfcs/0002-expose-wasm-tool-definitions.md)
- [RFC 0003](./rfcs/0003-skill-bundle-installation.md)
- [RFC 0004](./rfcs/0004-tokenized-delegated-authorized-endpoint-requests.md)
- [RFC 0005](./rfcs/0005-monty-code-execution-environment.md)
- [RFC 0006](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md)
- [RFC 0007](./rfcs/0007-secure-memory-sidecar-design.md)
- [RFC 0008](./rfcs/0008-websocket-responses-api.md)
- [RFC 0009](./rfcs/0009-feature-flags-frontend.md)
- [RFC 0010](./rfcs/0010-intent-contracts-and-fail-closed-runtime-gates.md)
- [ADR 001](./adr-001-rego-policy-engine-for-intent-enforcement.md)
- [RFC 0011](./rfcs/0011-execution-truth-ledger-and-action-provenance.md)
- [RFC 0012](./rfcs/0012-delegated-child-jobs-with-isolated-context.md)
- [RFC 0013](./rfcs/0013-auxiliary-provider-profiles-and-stable-prefix-prompt-assembly.md)
- [RFC 0014](./rfcs/0014-memory-projection-tiers-and-promotion-rules.md)
- [RFC 0015](./rfcs/0015-hierarchical-memory-materialization-for-memoryd.md)
- [RFC 0016](./rfcs/0016-theme-detection-and-sparsity-rebalancing-for-memoryd.md)
- [RFC 0017](./rfcs/0017-hierarchical-recall-for-memoryd.md)
- [ADR 002](./adr-002-authoritative-intent-state-must-remain-human-auditable.md)
- [ADR 003](./adr-003-theme-management-belongs-in-memoryd.md)
- [ADR 004](./adr-004-dual-path-semantic-extraction-with-validated-provenance.md)
- [ADR 005](./adr-005-dual-mode-uncertainty-gating-for-hierarchical-recall.md)

## 1. Make tool contracts explicit

Phase objective: ensure axinite advertises accurate tool interfaces before it
widens the runtime surface.

### 1.1. Hosted MCP tool catalogue parity

Objective: make hosted workers advertise the real orchestrator-owned Model
Context Protocol (MCP) tools instead of only local proxy tools.

Learning opportunity: determine whether one remote catalogue contract can
support both model-facing schema fidelity and later observability needs.

Dependencies: unlocks 1.2 and reduces integration risk for 3.2. No separate
architecture-prerequisite stream is required before this step; the
worker-orchestrator contract hardening belongs inside 1.1.1.

- [x] 1.1.1. Add worker-orchestrator transport for remote tool catalogue fetch
  and generic remote tool execution.
  - See [RFC 0001 §Migration Plan](./rfcs/0001-expose-mcp-tool-definitions.md#migration-plan).
  - Tracks [Issue #12](https://github.com/leynos/axinite/issues/12).
  - Define the catalogue and generic execution transport through one shared
    worker-orchestrator boundary module or equivalent typed contract, rather
    than duplicating route fragments and payload shapes independently in the
    worker and orchestrator.
  - Keep worker startup injectable at the transport boundary so the hosted
    catalogue path does not deepen the current env-only client coupling.
  - Success: the orchestrator exposes a hosted-visible catalogue endpoint for
    active executable tools, the worker can execute orchestrator-owned tools
    through one generic proxy path, and the transport shape is owned in one
    place rather than mirrored as stringly typed route assembly on both sides.
- [x] 1.1.2. Filter the hosted-visible catalogue from the canonical
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
  - Tracks the worker-orchestrator parity portion of
    [Issue #16](https://github.com/leynos/axinite/issues/16).
  - Success: tests fail if required MCP fields disappear or are rewritten
    incorrectly, and prove that advertised remote tools execute through the
    orchestrator rather than a local stub.

### 1.2. Proactive WebAssembly (WASM) schema publication

Objective: make proactive WebAssembly (WASM) schema advertisement the only
normal contract for active WASM tools.

Learning opportunity: verify how much provider-specific schema shaping can be
done without losing guest-defined semantics.

Dependencies: depends on 1.1 for the shared remote-catalog shape and informs 2.3
by tightening the contract around active WASM tools.

- [ ] 1.2.1. Audit and fix WASM registration paths so every active tool
      publishes `ToolDefinition.parameters`.
  - See
    [RFC 0002 §Current State](./rfcs/0002-expose-wasm-tool-definitions.md#current-state)
    and
    [RFC 0002 §Migration Plan](./rfcs/0002-expose-wasm-tool-definitions.md#migration-plan).
  - Success: guest-exported metadata or explicit host overrides are applied
    during registration, and active WASM tools never rely on a failure path to
    teach the model their arguments.
- [ ] 1.2.2. Extend the remote tool catalog to include orchestrator-owned WASM
      tools. Requires 1.1.1 and 1.2.1.
  - See [RFC 0002 §Problem](./rfcs/0002-expose-wasm-tool-definitions.md#problem)
    and
    [RFC 0002 §Migration Plan](./rfcs/0002-expose-wasm-tool-definitions.md#migration-plan).
  - Success: hosted workers receive proactive WASM definitions through the same
    catalog path used for MCP tools, and hosted mode stops omitting
    orchestrator-owned WASM tools from the tool array.
- [ ] 1.2.3. Demote schema-bearing retry hints to fallback diagnostics. Requires
      1.2.1.
  - See [RFC 0002 §Summary](./rfcs/0002-expose-wasm-tool-definitions.md#summary)
    and
    [RFC 0002 §Migration Plan](./rfcs/0002-expose-wasm-tool-definitions.md#migration-plan).
  - Success: wrapper comments and behaviour describe retry hints as supplemental
    help rather than the primary contract, while parse and validation failures
    still surface actionable recovery guidance.
- [ ] 1.2.4. Add end-to-end tests for first-call WASM schema exposure. Requires
      1.2.2 and 1.2.3.
  - See [RFC 0002 §Goals](./rfcs/0002-expose-wasm-tool-definitions.md#goals) and
    [RFC 0002 §Migration Plan](./rfcs/0002-expose-wasm-tool-definitions.md#migration-plan).
  - Success: tests prove that the first request includes the advertised schema,
    and hosted plus non-hosted paths both fail if proactive schema publication
    regresses.

### 1.3. Multi-file skill bundles

Objective: replace the effective single-file skill model with a validated bundle
format and a narrow file-access surface.

Learning opportunity: measure whether progressive disclosure is sufficient for
multi-file skills without widening the generic filesystem surface.

Dependencies: independent of 1.1 and 1.2 at the transport layer, but should land
before 2.2 so codemode and later automation can rely on richer skill content
packaging.

- [ ] 1.3.1. Implement `.skill` archive validation and extraction.
  - See
    [RFC 0003 §Proposed Bundle Format](./rfcs/0003-skill-bundle-installation.md#proposed-bundle-format)
    and
    [RFC 0003 §Rollout Plan](./rfcs/0003-skill-bundle-installation.md#rollout-plan).
  - Success: the installer accepts only bundles with `SKILL.md` at the archive
    root and rejects unsupported top-level content or executable payloads.
- [ ] 1.3.2. Extend skill installation flows for uploaded bundles and `.skill`
      URLs. Requires 1.3.1.
  - See [RFC 0003 §Summary](./rfcs/0003-skill-bundle-installation.md#summary)
    and
    [RFC 0003 §Rollout Plan](./rfcs/0003-skill-bundle-installation.md#rollout-plan).
  - Success: install paths preserve `references/` and `assets/` when present,
    and installation failures report archive-shape errors explicitly.
- [ ] 1.3.3. Persist canonical skill roots in the loaded skill model. Requires
      1.3.1.
  - See
    [RFC 0003 §Reference Model](./rfcs/0003-skill-bundle-installation.md#reference-model)
    and
    [RFC 0003 §Rollout Plan](./rfcs/0003-skill-bundle-installation.md#rollout-plan).
  - Success: runtime state records the installed skill root and `SKILL.md`
    entrypoint, and active-skill injection can refer to a stable bundle-relative
    file layout.
- [ ] 1.3.4. Add a read-only `skill_read_file` interface for bundled resources.
      Requires 1.3.2 and 1.3.3.
  - See [RFC 0003 §Problem](./rfcs/0003-skill-bundle-installation.md#problem)
    and
    [RFC 0003 §Rollout Plan](./rfcs/0003-skill-bundle-installation.md#rollout-plan).
  - Success: the model can read bundle-relative files without raw filesystem
    access, and oversized or disallowed files fail through a skill-scoped error
    path.
- [ ] 1.3.5. Add installation and runtime tests for bundled skills. Requires
      1.3.2, 1.3.3, and 1.3.4.
  - See [RFC 0003 §Goals](./rfcs/0003-skill-bundle-installation.md#goals) and
    [RFC 0003 §Rollout Plan](./rfcs/0003-skill-bundle-installation.md#rollout-plan).
  - Success: tests cover valid bundles, malformed bundles, and lazy bundled-file
    reads, and prove that installation no longer drops ancillary files.

## 2. Introduce controlled execution surfaces

Phase objective: add new programmable execution paths without weakening
capability mediation, redaction, or approval boundaries.

### 2.1. Delegated endpoint requests

Objective: let axinite use confidential service endpoints on behalf of WASM
tools without exposing raw URLs to the extension or the model.

Learning opportunity: validate whether endpoint confidentiality can coexist with
understandable approvals and useful diagnostics.

Dependencies: depends on 1.2 for the stricter WASM contract and informs 2.3 by
establishing host-owned transport assembly for sensitive requests.

- [ ] 2.1.1. Add typed setup fields and delegated endpoint binding persistence.
  - [RFC 0004 §Current Surface](./rfcs/0004-tokenized-delegated-authorized-endpoint-requests.md#current-surface)
    and
    [RFC 0004 §Rollout Plan](./rfcs/0004-tokenized-delegated-authorized-endpoint-requests.md#rollout-plan).
  - Success: extension setup can store non-secret endpoint configuration
    separately from secret material, and endpoint bindings are validated and
    stored through a dedicated service.
- [ ] 2.1.2. Add delegated endpoint capability schema and WIT runtime plumbing.
      Requires 2.1.1.
  - [RFC 0004 §Goals](./rfcs/0004-tokenized-delegated-authorized-endpoint-requests.md#goals)
    and
    [RFC 0004 §Rollout Plan](./rfcs/0004-tokenized-delegated-authorized-endpoint-requests.md#rollout-plan).
  - Success: WASM capabilities can declare delegated endpoint use without naming
    the real host in a static allowlist, and the runtime exposes an
    `authorized-endpoint-request` path that resolves endpoint identities inside
    the host.
- [ ] 2.1.3. Add endpoint-aware redaction, approval, and audit behaviour.
      Requires 2.1.2.
  - [RFC 0004 §Summary](./rfcs/0004-tokenized-delegated-authorized-endpoint-requests.md#summary)
    and
    [RFC 0004 §Rollout Plan](./rfcs/0004-tokenized-delegated-authorized-endpoint-requests.md#rollout-plan).
  - Success: logs, errors, and approval surfaces do not reveal configured
    endpoint URLs, while audit events retain enough structure to diagnose
    failures without leaking origin data.
- [ ] 2.1.4. Deliver a pilot extension against the delegated request path.
      Requires 2.1.3.
  - [RFC 0004 §Problem](./rfcs/0004-tokenized-delegated-authorized-endpoint-requests.md#problem)
    and
    [RFC 0004 §Rollout Plan](./rfcs/0004-tokenized-delegated-authorized-endpoint-requests.md#rollout-plan).
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
  - [RFC 0005 §Summary](./rfcs/0005-monty-code-execution-environment.md#summary)
    and
    [RFC 0005 §Rollout Plan](./rfcs/0005-monty-code-execution-environment.md#rollout-plan).
  - Success: Monty runs out of process so a panic does not terminate the parent
    runtime, and host callbacks remain constrained to an explicit per-run tool
    allowlist.
- [ ] 2.2.2. Implement the JSON ABI for tool calls, parameters, results, and
      state. Requires 2.2.1.
  - [RFC 0005 §Goals](./rfcs/0005-monty-code-execution-environment.md#goals) and
    [RFC 0005 §Rollout Plan](./rfcs/0005-monty-code-execution-environment.md#rollout-plan).
  - Success: cross-boundary data is normalized to JSON-shaped values only, and
    host callback approval plus attenuation rules are shared with existing tool
    execution paths.
- [ ] 2.2.3. Add saved-script persistence with `save_script` and `run_script`.
      Requires 2.2.2.
  - [RFC 0005 §Problem](./rfcs/0005-monty-code-execution-environment.md#problem)
    and
    [RFC 0005 §Rollout Plan](./rfcs/0005-monty-code-execution-environment.md#rollout-plan).
  - Success: script source and manifest data are stored under a dedicated
    workspace scripts area, and per-script state is explicit rather than hidden
    in interpreter globals.
- [ ] 2.2.4. Add run metadata and audit logging for script execution. Requires
      2.2.3.
  - [RFC 0005 §Goals](./rfcs/0005-monty-code-execution-environment.md#goals) and
    [RFC 0005 §Rollout Plan](./rfcs/0005-monty-code-execution-environment.md#rollout-plan).
  - Success: each script run records version, inputs, outputs, and failure
    state, and reruns can distinguish code changes from parameter changes.
- [ ] 2.2.5. Integrate saved scripts into higher-level automation paths.
      Requires 2.2.3 and 2.2.4.
  - [RFC 0005 §Rollout Plan](./rfcs/0005-monty-code-execution-environment.md#rollout-plan).
  - Success: routines or job orchestration can invoke saved scripts without
    bypassing approval or policy checks, and review or rerun surfaces expose
    script identity plus version clearly.

### 2.3. Provenance-enforced intent execution

Objective: replace plugin-controlled secret placement with host-assembled intent
execution and provenance-aware policy.

Learning opportunity: test whether a stable intent vocabulary can stay legible
to users while still being strict enough for enforceable policy decisions.

Dependencies: depends on 1.2 for WASM contract hygiene and on 2.1 for the
host-owned request model; it should land before 3.2 if Responses sessions are
expected to use these tools safely at scale.

- [ ] 2.3.1. Add `execution_model` plumbing and disable placeholder-based secret
      placement for zero-knowledge tools.
  - See
    [RFC 0006 §Migration checklist and prioritized plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritized-plan)
    and
    [RFC 0006 §Current IronClaw components and APIs relevant to an intent model](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#current-ironclaw-components-and-apis-relevant-to-an-intent-model).
  - Success: capability loading and registry state distinguish legacy and
    provenance-enforced execution modes, and zero-knowledge tools reject
    `UrlPath`-style credential placement plus other guest-controlled secret
    sinks.
- [ ] 2.3.2. Introduce the intent WIT package, bindings, and wrapper selection.
      Requires 2.3.1.
  - See
    [RFC 0006 §Design target: WIT-based intent ABI with provenance tokens](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#design-target-wit-based-intent-abi-with-provenance-tokens)
    and
    [RFC 0006 §Migration checklist and prioritized plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritized-plan).
  - Tracks the WIT migration coordination in
    [Issue #8](https://github.com/leynos/axinite/issues/8).
  - Success: the runtime can load and instantiate intent-capable components
    alongside legacy WASM tools, and intent declarations are versioned
    independently from the existing `sandboxed-tool` world.
- [ ] 2.3.3. Build the template registry and transport assembler. Requires 2.1
      and 2.3.2.
  - See
    [RFC 0006 §Design target: WIT-based intent ABI with provenance tokens](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#design-target-wit-based-intent-abi-with-provenance-tokens)
    and
    [RFC 0006 §Migration checklist and prioritized plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritized-plan).
  - Success: plugins declare semantic operations rather than raw HTTP requests,
    and the host can assemble a concrete request, inject credentials, and apply
    redaction obligations at send time.
- [ ] 2.3.4. Add provenance token resources and policy-engine integration.
      Requires 2.3.2 and 2.3.3.
  - See
    [RFC 0006 §Executive summary](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#executive-summary)
    and
    [RFC 0006 §Migration checklist and prioritized plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritized-plan).
  - Success: the host can track provenance classes across intent execution and
    enforce allow or deny decisions through Rego, while policy outputs can
    require approval or redaction before a result reaches a public sink.
- [ ] 2.3.5. Deliver one concrete service profile on the intent path. Requires
      2.3.3 and 2.3.4.
  - See
    [RFC 0006 §Migration checklist and prioritized plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritized-plan).
  - Success: the pilot profile proves that authentication and templated
    transport can be handled without guest-visible secrets or endpoints, and
    integration tests cover both successful execution and blocked exfiltration
    attempts.
- [ ] 2.3.6. Add fuzzing and differential tests for noninterference constraints.
      Requires 2.3.4 and 2.3.5.
  - See
    [RFC 0006 §Migration checklist and prioritized plan](./rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md#migration-checklist-and-prioritized-plan).
  - Success: tests exercise derived-data exfiltration paths rather than only
    literal token leakage, and failures localize whether the break occurred in
    template assembly, provenance tracking, or policy evaluation.

### 2.4. Intent contracts and fail-closed enforcement

Objective: replace the current distributed constraint model with a single
inspectable intent contract and deterministic policy evaluation at every gate
point.

Learning opportunity: determine whether shadow-mode contract validation can run
at acceptable latency on the tool-execution critical path, and whether Regorus
built-in coverage gaps affect any realistic policy patterns.

Dependencies: can be developed in parallel with 2.3 but should inform 2.3.4
(policy-engine integration); informs 3.3 by defining the trust labels that
memory promotion checks against; informs 4.4 by defining the decision artefact
shape that the execution ledger stores.

- [ ] 2.4.1. Define the intent contract schema, storage format, and scope
      precedence rules.
  - See
    [RFC 0010 §Intent Contract Schema](./rfcs/0010-intent-contracts-and-fail-closed-runtime-gates.md#1-intent-contract-schema)
    and
    [RFC 0010 §Contract Scoping and Precedence](./rfcs/0010-intent-contracts-and-fail-closed-runtime-gates.md#2-contract-scoping-and-precedence).
  - Success: contracts can be authored in YAML, stored at workspace, thread, and
    job scope, and composed with narrowing-only semantics where child scopes
    restrict but never widen parent constraints.
- [ ] 2.4.2. Add trust labelling to all content entering model context. Requires
      2.4.1.
  - See
    [RFC 0010 §Trust Labelling for Retrieved Content](./rfcs/0010-intent-contracts-and-fail-closed-runtime-gates.md#3-trust-labelling-for-retrieved-content).
  - Success: identity files, workspace documents, tool outputs, and MCP
    resources carry explicit trust labels (trusted control-plane, curated
    data-plane, untrusted data-plane), and the runtime distinguishes
    instruction-bearing artefacts from reference material.
- [ ] 2.4.3. Integrate the Rego policy engine for structured input evaluation
      and decision output. Requires 2.4.1.
  - See
    [ADR 001 §Integration Design](./adr-001-rego-policy-engine-for-intent-enforcement.md#integration-design)
    and
    [ADR 001 §Decision Outcome / Proposed Direction](./adr-001-rego-policy-engine-for-intent-enforcement.md#decision-outcome--proposed-direction).
  - Success: policy evaluation accepts the structured input schema (contract,
    action, provenance, approval, workspace, sink), produces machine-readable
    decision records, and denies by default on evaluation failure, absent
    policy, or malformed input.
- [ ] 2.4.4. Add gate evaluation at pre-tool-execution, pre-memory-promotion,
      pre-delegation, and pre-sink-write points. Requires 2.4.2 and 2.4.3.
  - See
    [RFC 0010 §Gate Evaluation](./rfcs/0010-intent-contracts-and-fail-closed-runtime-gates.md#4-gate-evaluation)
    and
    [RFC 0010 §Failure Mode](./rfcs/0010-intent-contracts-and-fail-closed-runtime-gates.md#5-failure-mode-fail-closed-with-optional-downgrade).
  - Success: every gate point evaluates the effective contract, produces a
    decision artefact, and fails closed by default, and the `escalate` mode
    routes blocked actions to operator approval without silently widening
    authority.
- [ ] 2.4.5. Run shadow-mode contract enforcement alongside existing safety
      mechanisms. Requires 2.4.4.
  - See
    [RFC 0010 §Compatibility and Migration](./rfcs/0010-intent-contracts-and-fail-closed-runtime-gates.md#compatibility-and-migration).
  - Success: shadow mode logs contract violations without blocking actions,
    operators can validate the effective policy before switching to enforcement,
    and latency metrics confirm that critical-path evaluation stays within
    acceptable bounds.

### 2.5. Delegated child jobs with isolated context

Objective: provide a model-callable delegation primitive that creates bounded
child jobs with isolated context, explicit tool allowlists, and budget
enforcement.

Learning opportunity: determine whether worktree-based workspace isolation adds
sufficient safety for parallel code modifications, and whether summary-only
result distillation preserves enough information for the parent context.

Dependencies: depends on 2.3 for the host-owned request model and on 2.4 for
contract narrowing semantics; informs 4.4 by producing delegation ledger
entries.

- [ ] 2.5.1. Add the delegation contract schema and child-job dispatch through
      the existing scheduler.
  - See
    [RFC 0012 §Delegation Contract](./rfcs/0012-delegated-child-jobs-with-isolated-context.md#1-delegation-contract)
    and
    [RFC 0012 §Execution Path](./rfcs/0012-delegated-child-jobs-with-isolated-context.md#3-execution-path).
  - Success: the `delegate_task` tool creates child jobs with explicit goals,
    tool allowlists, and budget parameters, and dispatches them through the
    existing scheduler's `dispatch_job_with_context()` path.
- [ ] 2.5.2. Implement budget enforcement at the child-job boundary. Requires
      2.5.1.
  - See
    [RFC 0012 §Budget Enforcement](./rfcs/0012-delegated-child-jobs-with-isolated-context.md#6-budget-enforcement).
  - Success: iteration, token, cost, and time caps are enforced
    deterministically at the worker level, the scheduler enforces a wall-clock
    timeout, and budget exhaustion produces a partial result with an explicit
    status.
- [ ] 2.5.3. Add approval context handling with inherit, narrow, and fresh
      modes. Requires 2.5.1.
  - See
    [RFC 0012 §Approval Context Handling](./rfcs/0012-delegated-child-jobs-with-isolated-context.md#4-approval-context-handling).
  - Success: child jobs respect the selected approval inheritance mode, the
    `narrow` default prevents escalation beyond parent-approved tools, and new
    approval requests from child jobs route to the parent's operator.
- [ ] 2.5.4. Implement workspace isolation for `none` and `worktree` modes.
      Requires 2.5.1.
  - See
    [RFC 0012 §Workspace Isolation](./rfcs/0012-delegated-child-jobs-with-isolated-context.md#5-workspace-isolation).
  - Success: `worktree` mode creates a git worktree for the child job with
    changes isolated until explicitly merged, and `none` mode shares the
    parent's working directory for read-only analysis tasks.
- [ ] 2.5.5. Add result distillation and delegation ledger entries. Requires
      2.5.2, 2.5.3, and 2.5.4.
  - See
    [RFC 0012 §Execution Path](./rfcs/0012-delegated-child-jobs-with-isolated-context.md#3-execution-path)
    and
    [RFC 0011 §Summary](./rfcs/0011-execution-truth-ledger-and-action-provenance.md#summary).
  - Success: the parent receives only a distilled summary with optional evidence
    references, full child context is stored out-of-band, and delegation events
    produce execution ledger entries recording the child job ID, delegation
    contract, and budget parameters.

## 3. Move retrieval and conversation state onto durable boundaries

Phase objective: shift memory and long-running chat state onto components that
can be rolled out cautiously and observed directly.

### 3.1. Secure memory sidecar

Objective: replace the in-process memory path with a local sidecar that owns
extraction, recall, and structured memory storage.

Learning opportunity: compare shadow-mode recall and latency against the current
workspace search path before switching user-facing retrieval.

Dependencies: independent from 2.2, but should land before 3.2 if the provider
backend is expected to rely on richer memory recall during long-running
sessions.

- [ ] 3.1.1. Add transactional outbox support for memory-producing writes.
  - See
    [RFC 0007 §Executive summary](./rfcs/0007-secure-memory-sidecar-design.md#executive-summary)
    and
    [RFC 0007 §Rollout plan](./rfcs/0007-secure-memory-sidecar-design.md#rollout-plan).
  - Success: conversation and workspace writes emit outbox events in the same
    database transaction, and memory side effects can be replayed without
    inventing state after the fact.
- [ ] 3.1.2. Implement memoryd RPC over a Unix domain socket with capability
      tokens. Requires 3.1.1.
  - See
    [RFC 0007 §Security considerations, rollout plan, tests, monitoring](./rfcs/0007-secure-memory-sidecar-design.md#security-considerations-rollout-plan-tests-monitoring)
    and
    [RFC 0007 §Rollout plan](./rfcs/0007-secure-memory-sidecar-design.md#rollout-plan).
  - Success: memoryd exposes scoped read and write operations over a local-only
    socket, and invalid, expired, or over-scoped tokens are rejected
    deterministically.
- [ ] 3.1.3. Add extraction and consolidation workers backed by local stores.
      Requires 3.1.1 and 3.1.2.
  - See
    [RFC 0007 §Executive summary](./rfcs/0007-secure-memory-sidecar-design.md#executive-summary)
    and
    [RFC 0007 §Test plan](./rfcs/0007-secure-memory-sidecar-design.md#test-plan).
  - Success: the pipeline can extract facts and embeddings, write vectors to
    Qdrant, persist structured facts in Oxigraph, and run consolidation through
    queued workers with retry and timeout limits.
- [ ] 3.1.4. Run shadow-mode ingestion and recall alongside the existing search
      path. Requires 3.1.3.
  - See
    [RFC 0007 §Rollout plan](./rfcs/0007-secure-memory-sidecar-design.md#rollout-plan)
    and
    [RFC 0007 §Test plan](./rfcs/0007-secure-memory-sidecar-design.md#test-plan).
  - Tracks the current libSQL search regression in
    [Issue #5](https://github.com/leynos/axinite/issues/5), which must be
    understood before shadow-mode comparisons are treated as trustworthy.
  - Success: shadow mode records recall overlap, latency, and error metrics, and
    deletion propagation retracts facts plus vectors when source content is
    removed.
- [ ] 3.1.5. Switch retrieval to memoryd-first with fallback and kill switch
      support. Requires 3.1.4.
  - See
    [RFC 0007 §Rollout plan](./rfcs/0007-secure-memory-sidecar-design.md#rollout-plan).
  - Success: user-facing recall prefers memoryd when active and falls back
    cleanly when unavailable, and one operator switch can disable the sidecar
    path without a schema rollback.

### 3.2. OpenAI Responses over WebSocket

Objective: add a stateful provider backend that supports multi-turn tool calling
and server-side compaction over a persistent WebSocket session.

Learning opportunity: determine whether a stateful provider session model fits
axinite's agent loop better than transcript replay for long-running tool-heavy
threads.

Dependencies: depends on 1.1 and 1.2 for canonical tool definitions, benefits
from 2.3 for safer tool execution at runtime, and should integrate with 3.1
rather than bypassing the memory path it introduces.

- [ ] 3.2.1. Add a new provider protocol and configuration surface for Responses
      WebSocket mode.
  - See
    [RFC 0008 §Requirements](./rfcs/0008-websocket-responses-api.md#requirements)
    and
    [RFC 0008 §Implementation plan](./rfcs/0008-websocket-responses-api.md#implementation-plan).
  - Success: provider selection can opt into a Responses WebSocket backend
    without disturbing the existing `open_ai_completions` path, and
    configuration covers base URL, storage mode, and compaction strategy.
- [ ] 3.2.2. Implement `ResponsesWsSession` connection management. Requires
      3.2.1.
  - See
    [RFC 0008 §Requirements](./rfcs/0008-websocket-responses-api.md#requirements)
    and
    [RFC 0008 §Stepwise tasks](./rfcs/0008-websocket-responses-api.md#stepwise-tasks).
  - Success: the session enforces authenticated connection setup, sequential
    in-flight behaviour, reconnect handling, and connection rotation, and
    disconnects do not silently orphan per-thread provider state.
- [ ] 3.2.3. Implement the streaming event parser and `response.create` builder.
      Requires 3.2.1 and 3.2.2.
  - See
    [RFC 0008 §Requirements](./rfcs/0008-websocket-responses-api.md#requirements)
    and
    [RFC 0008 §Stepwise tasks](./rfcs/0008-websocket-responses-api.md#stepwise-tasks).
  - Success: event handling reconstructs output text, function-call arguments,
    and final completion state correctly, and request construction maps axinite
    message plus tool state into Responses input items and tool definitions.
- [ ] 3.2.4. Preserve provider-native tool call state in thread persistence.
      Requires 3.2.2 and 3.2.3.
  - See
    [RFC 0008 §Feature gap analysis](./rfcs/0008-websocket-responses-api.md#feature-gap-analysis)
    and
    [RFC 0008 §Stepwise tasks](./rfcs/0008-websocket-responses-api.md#stepwise-tasks).
  - Success: tool turns store OpenAI `call_id` values and continuation
    identifiers, and continuation requests can emit `function_call_output` items
    without synthesizing incompatible identifiers later.
- [ ] 3.2.5. Integrate server-side compaction and retry controls. Requires
      3.1.5, 3.2.2, and 3.2.4.
  - See
    [RFC 0008 §Requirements](./rfcs/0008-websocket-responses-api.md#requirements)
    and
    [RFC 0008 §CI checks and rollout checklist](./rfcs/0008-websocket-responses-api.md#ci-checks-and-rollout-checklist).
  - Success: the delegate can enable Responses compaction without fighting the
    existing summarization path, and retry plus backoff rules handle rate
    limits, reconnects, and `previous_response_not_found` failures explicitly.
- [ ] 3.2.6. Add mock WebSocket tests and feature-flagged rollout controls.
      Requires 3.2.3, 3.2.4, and 3.2.5.
  - See
    [RFC 0008 §Stepwise tasks](./rfcs/0008-websocket-responses-api.md#stepwise-tasks)
    and
    [RFC 0008 §CI checks and rollout checklist](./rfcs/0008-websocket-responses-api.md#ci-checks-and-rollout-checklist).
  - Success: automated tests cover long tool loops, compaction events,
    reconnects, and fallback behaviour, and rollout can be enabled per provider
    or model with dashboards for reconnect, compaction, and rate-limit failure
    rates.

### 3.3. Memory projection tiers and promotion rules

Objective: extend the memory sidecar with explicit projection classes, epistemic
status, observer/subject scope, and promotion rules so recall distinguishes
trusted facts from hypothesized inferences.

Learning opportunity: determine whether layered recall (profile, fact, summary,
episode) produces meaningfully better context than single-tier vector retrieval,
and whether batched extraction materially reduces per-message extraction
overhead.

Dependencies: depends on 3.1 for the sidecar architecture, extraction pipeline,
and Qdrant/Oxigraph storage; informs 2.4 by defining the promotion boundaries
that intent contracts gate.

- [ ] 3.3.1. Add projection class and epistemic status fields to memory
      artefacts in Qdrant payloads and Oxigraph triples.
  - See
    [RFC 0014 §Projection Classes](./rfcs/0014-memory-projection-tiers-and-promotion-rules.md#1-projection-classes)
    and
    [RFC 0014 §Epistemic Status](./rfcs/0014-memory-projection-tiers-and-promotion-rules.md#2-epistemic-status).
  - Success: every memory artefact carries a projection class (episode, summary,
    concept, fact, profile) and epistemic status (explicit, curated, deduced,
    hypothesized, retracted), and the taxonomy is extensible without schema
    migration for existing artefacts.
- [ ] 3.3.2. Add observer and subject scope to memory artefacts. Requires 3.3.1.
  - See
    [RFC 0014 §Observer and Subject Scope](./rfcs/0014-memory-projection-tiers-and-promotion-rules.md#3-observer-and-subject-scope).
  - Success: every artefact carries observer_id, subject_id, scope (private,
    workspace, shared), and optional audience, and recall enforces scope
    constraints at the query boundary so only artefacts within the caller's
    scope are returned.
- [ ] 3.3.3. Implement promotion rules governing epistemic level transitions.
      Requires 3.3.1.
  - See
    [RFC 0014 §Promotion Rules](./rfcs/0014-memory-projection-tiers-and-promotion-rules.md#4-promotion-rules).
  - Success: hypothesized facts promote to deduced when corroboration thresholds
    are met, deduced facts promote to curated only by explicit operator action,
    and profile promotion requires explicit or curated status plus configurable
    stability duration.
- [ ] 3.3.4. Implement contradiction detection, recording, and resolution.
      Requires 3.3.1 and 3.3.3.
  - See
    [RFC 0014 §Contradiction Handling](./rfcs/0014-memory-projection-tiers-and-promotion-rules.md#5-contradiction-handling).
  - Success: new evidence that contradicts an existing fact triggers the
    appropriate resolution strategy (automatic retraction for lower-trust versus
    higher-trust, operator escalation for same-trust conflicts), and
    contradiction records are stored as Oxigraph relations linking the
    conflicting facts.
- [ ] 3.3.5. Add reconciliation metadata and sync state tracking per projection
      target. Requires 3.3.1.
  - See
    [RFC 0014 §Reconciliation Metadata](./rfcs/0014-memory-projection-tiers-and-promotion-rules.md#6-reconciliation-metadata).
  - Success: every projection target carries sync metadata (status, retry count,
    last error, last synced), memoryd marks projections as pending when targets
    are unavailable, and health and lag metrics are exposed.
- [ ] 3.3.6. Restructure recall to query projection layers separately before
      synthesis. Requires 3.3.1, 3.3.2, and 3.3.5.
  - See
    [RFC 0014 §Recall Across Projection Layers](./rfcs/0014-memory-projection-tiers-and-promotion-rules.md#7-recall-across-projection-layers).
  - Success: recall queries profile, fact, summary, and episode layers
    separately, results are annotated with projection class and epistemic
    status, and hypotheses are included only when explicitly requested.
- [ ] 3.3.7. Add batching and windowing between outbox consumption and
      extraction. Requires 3.1.1.
  - See
    [RFC 0014 §Batching and Windowing](./rfcs/0014-memory-projection-tiers-and-promotion-rules.md#8-batching-and-windowing).
  - Success: extraction triggers respect min_context_tokens, max_batch_delay_ms,
    flush_on_idle, and flush_on_explicit_write thresholds, batching is scoped by
    workspace, conversation, and entity, and configuration is per-workspace.

### 3.4. Hierarchical memory materialization and recall

Objective: materialize the post-0014 memory hierarchy, maintain stable theme
structure over semantic carriers, and use that structure for budget-aware
hierarchical recall.

Learning opportunity: determine whether the theme layer and hierarchical recall
deliver better evidence packs than projection-layer recall alone, and whether
dual-path extraction plus dual-mode expansion gating are worth their added
operational complexity.

Dependencies: depends on 3.1 for the sidecar architecture and stores; depends on
3.3 for the normative projection classes, epistemic states, and projection-layer
filtering rules.

- [ ] 3.4.1. Materialize episode nodes, semantic carriers, and theme nodes with
      durable provenance and temporal edges.
  - See
    [RFC 0015 §Materialized Node Families](./rfcs/0015-hierarchical-memory-materialization-for-memoryd.md#2-materialized-node-families),
    [RFC 0015 §Temporal Model](./rfcs/0015-hierarchical-memory-materialization-for-memoryd.md#5-temporal-model),
    and
    [RFC 0015 §Provenance Model](./rfcs/0015-hierarchical-memory-materialization-for-memoryd.md#6-provenance-model).
  - Success: `memoryd` writes durable episode, semantic-carrier, and theme
    structures; all retrievable semantic carriers resolve to concrete evidence;
    and curated-document projection plus retraction propagation preserve the RFC
    0015 lineage rules.
- [ ] 3.4.2. Implement the shared extraction schema, provenance validator, and
      dual-path semantic extraction contract. Requires 3.4.1.
  - See
    [ADR 004 §Decision Outcome / Proposed Direction](./adr-004-dual-path-semantic-extraction-with-validated-provenance.md#decision-outcome--proposed-direction)
    and
    [ADR 004 §Migration Plan](./adr-004-dual-path-semantic-extraction-with-validated-provenance.md#migration-plan).
  - Success: `memoryd` supports `encoder_extractive` and `llm_structured`
    extraction behind one schema, unresolved support references are rejected,
    and shadow mode can compare accepted versus rejected outputs per path.
- [ ] 3.4.3. Add the workspace-local `ThemeManager` and stable theme IDs in
      `memoryd`. Requires 3.4.1.
  - See
    [ADR 003 §Decision Outcome / Proposed Direction](./adr-003-theme-management-belongs-in-memoryd.md#decision-outcome--proposed-direction)
    and
    [RFC 0016 §Theme Manager Responsibilities](./rfcs/0016-theme-detection-and-sparsity-rebalancing-for-memoryd.md#theme-manager-responsibilities).
  - Success: theme identity, lineage, and balancing policy are sidecar-owned;
    Chutoro remains the clustering substrate; and the manager can rebuild from
    stored semantic carriers when a clustering checkpoint is lost.
- [ ] 3.4.4. Implement theme attach, split, merge, summary refresh, and sparse
      kNN maintenance with shadow-mode safety rails. Requires 3.4.3.
  - See
    [RFC 0016 §Incremental Attach Path](./rfcs/0016-theme-detection-and-sparsity-rebalancing-for-memoryd.md#incremental-attach-path),
    [RFC 0016 §Split Proposals](./rfcs/0016-theme-detection-and-sparsity-rebalancing-for-memoryd.md#split-proposals),
    [RFC 0016 §Merge Proposals](./rfcs/0016-theme-detection-and-sparsity-rebalancing-for-memoryd.md#merge-proposals),
    and
    [RFC 0016 §Theme and Semantic-Carrier kNN Graph](./rfcs/0016-theme-detection-and-sparsity-rebalancing-for-memoryd.md#theme-and-semantic-carrier-knn-graph).
  - Success: theme maintenance is bounded, lineage is auditable, summary refresh
    stays asynchronous, and shadow metrics expose theme-size drift, split rate,
    merge rate, and rebuild rate before live balancing is enabled.
- [ ] 3.4.5. Extend `Recall` with projection-aware hierarchical profiles and
      structured context assembly. Requires 3.3.6, 3.4.2, and 3.4.4.
  - See
    [RFC 0017 §Projection-Aware Candidate Generation](./rfcs/0017-hierarchical-recall-for-memoryd.md#stage-0-projection-aware-candidate-generation),
    [RFC 0017 §Representative Selection Over the High-Level Graph](./rfcs/0017-hierarchical-recall-for-memoryd.md#stage-i-representative-selection-over-the-high-level-graph),
    and
    [RFC 0017 §Context Assembly](./rfcs/0017-hierarchical-recall-for-memoryd.md#context-assembly).
  - Success: `Recall` supports `flat_v1`, `hierarchical_v2`, `cheap_v2`, and
    `evidence_v2`; returned blocks stay annotated with projection class and
    epistemic status; and fallback to `flat_v1` remains explicit and
    inspectable.
- [ ] 3.4.6. Implement proxy and model-assisted stage-II expansion gating plus
      shadow comparison. Requires 3.4.5.
  - See
    [ADR 005 §Decision Outcome / Proposed Direction](./adr-005-dual-mode-uncertainty-gating-for-hierarchical-recall.md#decision-outcome--proposed-direction)
    and
    [RFC 0017 §Uncertainty and Gain Estimation](./rfcs/0017-hierarchical-recall-for-memoryd.md#uncertainty-and-gain-estimation).
  - Success: stage-II gain estimation exposes `estimated_gain`,
    `estimated_token_cost`, and `reason_code`; cheap and evidence-heavy modes
    can be shadow-compared; and hierarchical recall never depends on one fragile
    uncertainty surface.

### 3.5. Auxiliary provider profiles and stable-prefix prompt assembly

Objective: generalize the provider chain into named profiles with independent
fallback chains and capability metadata, and restructure prompt assembly so the
stable prefix maximizes cache hits across providers.

Learning opportunity: measure prompt cache hit rates before and after the
stable-prefix restructuring to validate the cost and latency benefit, and
determine whether per-job prefix freeze scope is the right default for WebSocket
Responses sessions.

Dependencies: depends on 3.2 for the Responses session model that
per-provider-session freeze scope would align with; informs 3.1 by routing
memory extraction to the auxiliary profile. ADR 002 requires that provider-side
continuation state remain a cache, not the authoritative source; this step must
treat provider profiles accordingly.

- [ ] 3.5.1. Add named provider profile configuration and per-profile decorated
      provider chains.
  - See
    [RFC 0013 §Named Provider Profiles](./rfcs/0013-auxiliary-provider-profiles-and-stable-prefix-prompt-assembly.md#1-named-provider-profiles)
    and
    [RFC 0013 §Requirements](./rfcs/0013-auxiliary-provider-profiles-and-stable-prefix-prompt-assembly.md#requirements).
  - Success: the provider configuration supports named profiles (at minimum main
    and auxiliary), each profile receives its own
    retry/failover/circuit-breaker/cache chain, and existing configurations
    without explicit profiles continue to function unchanged.
- [ ] 3.5.2. Implement the profile dispatch table and route auxiliary workloads
      to the appropriate profile. Requires 3.5.1.
  - See
    [RFC 0013 §Profile Dispatch Table](./rfcs/0013-auxiliary-provider-profiles-and-stable-prefix-prompt-assembly.md#2-profile-dispatch-table).
  - Success: summarization, classification, heartbeat, and memory extraction
    default to the auxiliary profile without per-call configuration, and the
    dispatch table is operator-configurable.
- [ ] 3.5.3. Add provider capability and privacy metadata to profile
      definitions. Requires 3.5.1.
  - See
    [RFC 0013 §Provider Capability and Privacy Metadata](./rfcs/0013-auxiliary-provider-profiles-and-stable-prefix-prompt-assembly.md#3-provider-capability-and-privacy-metadata).
  - Success: provider definitions can carry optional metadata (supports_vision,
    supports_function_calling, data_retention, data_collection), and profile
    dispatch uses metadata to avoid selecting providers likely to fail or
    violate operator intent.
- [ ] 3.5.4. Restructure prompt assembly into stable prefix and volatile suffix
      segments. Requires 3.5.1.
  - See
    [RFC 0013 §Stable-Prefix Prompt Assembly](./rfcs/0013-auxiliary-provider-profiles-and-stable-prefix-prompt-assembly.md#4-stable-prefix-prompt-assembly)
    and
    [RFC 0013 §Prefix Freeze Scope](./rfcs/0013-auxiliary-provider-profiles-and-stable-prefix-prompt-assembly.md#5-prefix-freeze-scope).
  - Success: system instructions, identity files, skill definitions, and intent
    contracts form a byte-stable prefix, conversation turns and tool results sit
    after the cache break, and the prefix remains byte-identical across
    consecutive requests within the freeze scope.
- [ ] 3.5.5. Add provider-specific cache control breakpoints and per-job prefix
      freeze scope. Requires 3.5.4.
  - See
    [RFC 0013 §Stable-Prefix Prompt Assembly](./rfcs/0013-auxiliary-provider-profiles-and-stable-prefix-prompt-assembly.md#4-stable-prefix-prompt-assembly).
  - Success: Anthropic requests include `cache_control` breakpoints after the
    stable prefix, OpenAI requests benefit from automatic prefix caching, and
    per-job freeze scope is the configurable default.
- [ ] 3.5.6. Extend chaos tests to cover auxiliary-profile failure modes.
      Requires 3.5.1 and 3.5.2.
  - See
    [RFC 0013 §Requirements](./rfcs/0013-auxiliary-provider-profiles-and-stable-prefix-prompt-assembly.md#requirements).
  - Success: tests cover fallback from auxiliary to main, circuit breaking on
    auxiliary provider, and profile-specific failure isolation, and existing
    provider chaos tests continue to pass.

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
  - See future RFC: health monitoring RFC §Problem, §Requirements, and §Rollout
    Plan.
  - Success: health state distinguishes degraded vs failed subsystems, channel
    health monitoring can restart configured components on policy, and restart
    storms are throttled with bounded backoff.
- [ ] 4.1.3. Expose service state, readiness, and restart history through the
      gateway and operator-facing surfaces. Requires 4.1.1 and 4.1.2.
  - See future RFC: health monitoring RFC §Compatibility and Migration.
  - Success: operators can inspect service health, readiness reasons, and
    restart history through stable APIs and CLI output rather than only logs.
- [ ] 4.1.4. Extract config bootstrap and hot-reload orchestration into explicit
      runtime services. Requires 4.1.1.
  - See
    [Axinite architecture overview §3.1 Boot sequence](./axinite-architecture-overview.md#31-boot-sequence),
    [Axinite architecture overview §4.3 Persistence, configuration, and memory](./axinite-architecture-overview.md#43-persistence-configuration-and-memory),
    and
    [Webhook server design §6 Relationship to hot reload](./webhook-server-design.md#6-relationship-to-hot-reload).
  - Tracks [Issue #9](https://github.com/leynos/axinite/issues/9),
    [Issue #13](https://github.com/leynos/axinite/issues/13), and the SIGHUP
    test gap in [Issue #16](https://github.com/leynos/axinite/issues/16).
  - Success: bootstrap consumes an explicit startup context rather than ambient
    process state, reload policy can be exercised without booting the full
    runtime, and `WebhookServer` keeps its rollback-focused restart semantics
    while caller-side reload logic becomes simpler and more observable.
- [ ] 4.1.5. Separate runtime assembly from activation side effects and narrow
      extension lifecycle orchestration. Requires 4.1.4.
  - See
    [Axinite architecture overview §3.2 AppBuilder phases](./axinite-architecture-overview.md#32-appbuilder-phases)
    and
    [Axinite architecture overview §4.4 Extensions and tooling](./axinite-architecture-overview.md#44-extensions-and-tooling).
  - Tracks [Issue #10](https://github.com/leynos/axinite/issues/10) and
    [Issue #11](https://github.com/leynos/axinite/issues/11).
  - Success: `AppBuilder` composition can be exercised without starting
    unrelated background work, and extension discovery plus activation policy
    stop accumulating unrelated adapters in one manager-level choke point.
- [ ] 4.1.6. Make job lifecycle persistence and self-repair policy durable,
      observable, and testable. Requires 4.1.2 and 4.1.4.
  - See
    [Axinite architecture overview §3.3 Long-running services](./axinite-architecture-overview.md#33-long-running-services)
    and
    [Axinite architecture overview §4.2 Agent runtime](./axinite-architecture-overview.md#42-agent-runtime).
  - Tracks [Issue #14](https://github.com/leynos/axinite/issues/14),
    [Issue #15](https://github.com/leynos/axinite/issues/15), and the terminal
    lifecycle coverage gap in
    [Issue #16](https://github.com/leynos/axinite/issues/16).
  - Success: terminal job transitions are durably persisted before they are
    treated as complete, self-repair thresholds affect real behaviour rather
    than inert configuration, and automated tests cover repair policy plus
    terminal-state persistence.

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

- [ ] 4.2.1. Implement the missing `before_agent_start`, `before_message_write`,
      `llm_input`, and `llm_output` hook points across the conversational
      dispatcher, routines, and Responses session path.
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
  - Success: operators can manage gateway lifecycle, channels, agents, sessions,
    nodes, and plugins through the CLI with parity-grade inspection output and
    non-interactive scripting support.
- [ ] 4.3.3. Implement the missing automation and inspection command families:
      `cron`, `webhooks`, `logs`, improved `doctor`, and richer `models` output.
      Requires 4.3.1, 4.1.3, and 4.2.3.
  - See future RFC: enhanced CLI RFC §Requirements, §Compatibility and
    Migration, and §Rollout Plan. See FEATURE_PARITY.md §CLI Commands.
  - Success: operators can manage scheduled jobs, webhook config, log queries,
    health diagnostics, and model inventory from the CLI without falling back to
    direct database or config-file edits.
- [ ] 4.3.4. Implement the missing action workflows: `message send`, `browser`,
      `backup`, `update`, `/subagents spawn`, and `/export-session`. Requires
      4.3.1 and 4.3.3.
  - See future RFC: enhanced CLI RFC §Requirements and §Open Questions. See
    FEATURE_PARITY.md §CLI Commands.
  - Success: the CLI can trigger outbound channel sends, browser automation,
    local backups, self-update flows, subagent spawn, and session export with
    stable flags and audit-friendly output.

### 4.4. Execution truth ledger and action provenance

Objective: add an append-only execution ledger that records system-level actions
independently of the conversation transcript, and expose the ledger as a
first-class operator surface.

Learning opportunity: determine which action categories produce the most useful
audit signal for operators, and whether completion-claim verification adds
genuine trust value or produces excessive noise.

Dependencies: informs 2.4 by storing policy decision artefacts; informs 2.5 by
storing delegation events; builds on 3.2 for provider-event recording; builds on
4.2 for hook-inspection seams.

- [ ] 4.4.1. Add the append-only ledger table to both PostgreSQL and libSQL
      backends.
  - See
    [RFC 0011 §Ledger Entry Schema](./rfcs/0011-execution-truth-ledger-and-action-provenance.md#1-ledger-entry-schema)
    and
    [RFC 0011 §Storage](./rfcs/0011-execution-truth-ledger-and-action-provenance.md#3-storage).
  - Success: the ledger schema supports the defined entry fields (id, timestamp,
    scope IDs, entry_type, actor, detail JSON, redacted flag, content_hash,
    contract_version), rows are never updated or deleted during normal
    operation, and the schema is versioned and migration-safe.
- [ ] 4.4.2. Add ledger writes for tool invocation, approval decision, and
      policy decision entry types. Requires 4.4.1.
  - See
    [RFC 0011 §Entry Types and Payloads](./rfcs/0011-execution-truth-ledger-and-action-provenance.md#2-entry-types-and-payloads).
  - Success: every tool invocation, approval gate decision, and policy
    evaluation produces a ledger entry with sanitized parameters and execution
    duration, writes do not block the critical path of tool execution, and
    sensitive content is redactable with SHA-256 content hashes.
- [ ] 4.4.3. Add ledger writes for file write, workspace mutation, delegation,
      and provider event entry types. Requires 4.4.1 and 4.4.2.
  - See
    [RFC 0011 §Entry Types and Payloads](./rfcs/0011-execution-truth-ledger-and-action-provenance.md#2-entry-types-and-payloads)
    and
    [ADR 002 §Decision Outcome / Proposed Direction](./adr-002-authoritative-intent-state-must-remain-human-auditable.md#decision-outcome--proposed-direction).
  - Success: file writes record path and content hash, workspace mutations
    record document changes, delegation entries record child-job contracts and
    budgets, and provider events record `previous_response_id` transitions,
    compaction events, and connection lifecycle changes.
- [ ] 4.4.4. Add the bypass event entry type for actions that bypass normal
      persistence. Requires 4.4.1.
  - See
    [RFC 0011 §Entry Types and Payloads](./rfcs/0011-execution-truth-ledger-and-action-provenance.md#2-entry-types-and-payloads).
  - Success: auth token submissions, provider-native compaction events, and
    other non-standard persistence paths produce redacted ledger entries that
    preserve the fact of the action without leaking sensitive content.
- [ ] 4.4.5. Add ledger query API to the web gateway with filtering by
      workspace, thread, job, entry type, actor, and time range. Requires 4.4.2
      and 4.4.3.
  - See
    [RFC 0011 §UI Surface](./rfcs/0011-execution-truth-ledger-and-action-provenance.md#4-ui-surface).
  - Success: operators can query the ledger programmatically, results are
    filterable by all indexed fields, and the API serves the ledger UI panel.
- [ ] 4.4.6. Add the execution ledger UI panel with correlation to conversation
      turns and mismatch highlighting. Requires 4.4.5.
  - See
    [RFC 0011 §UI Surface](./rfcs/0011-execution-truth-ledger-and-action-provenance.md#4-ui-surface)
    and
    [RFC 0011 §Completion Claim Verification](./rfcs/0011-execution-truth-ledger-and-action-provenance.md#5-completion-claim-verification).
  - Success: the ledger panel displays entries in chronological order with
    filtering, correlates tool invocation entries with assistant messages where
    possible, and highlights mismatches where assistant completion claims lack
    matching ledger entries.

### 4.5. Feature flags for progressive front-end rollout

Objective: add a lightweight feature-flag delivery mechanism so the backend can
declare which front-end capabilities are enabled, the browser can gate rendering
accordingly, and operators can toggle flags at runtime without restarting the
gateway.

Learning opportunity: validate whether per-flag environment variables, operator
overrides, and subsystem-derived defaults provide sufficient control for
progressive feature rollout without introducing complex targeting or
percentage-based activation.

Dependencies: independent of other Phase 4 tasks, but provides a foundation for
Phase 6 front-end features (canvas hosting, advanced media handling) to roll out
behind flags.

- [ ] 4.5.1. Add per-flag environment variable parsing and registry
      initialization in `GatewayChannel` or a dedicated config module.
  - See
    [RFC 0009 §Configuration inputs](./rfcs/0009-feature-flags-frontend.md#1-configuration-inputs).
  - Success: `FEATURE_FLAG_<NAME>` environment variables are parsed into a
    `FeatureFlagRegistry` with plain `HashMap<String, bool>`, invalid flag names
    are silently discarded with a warning, and compiled defaults are applied for
    flags without environment overrides.
- [ ] 4.5.2. Add the `FeatureFlagRegistry` struct and `GatewayState`
      integration. Requires 4.5.1.
  - See
    [RFC 0009 §Data shape](./rfcs/0009-feature-flags-frontend.md#2-data-shape)
    and
    [RFC 0009 §GatewayState integration](./rfcs/0009-feature-flags-frontend.md#3-gatewaystate-integration).
  - Success: `GatewayState` holds an `Arc<RwLock<FeatureFlagRegistry>>` that
    supports runtime re-resolution when deployment-scoped operator overrides
    change, resolves flags per deployment while ignoring user-scoped
    `feature_flag:` rows, and applies subsystem availability defaults during
    initialization based on `GatewayState` field presence.
- [ ] 4.5.3. Extend the settings handler to detect `feature_flag:` keys and
      apply runtime overrides to the registry. Requires 4.5.2.
  - See
    [RFC 0009 §Configuration inputs](./rfcs/0009-feature-flags-frontend.md#1-configuration-inputs).
  - Success: `PUT /api/settings/feature_flag:<name>` requires a deployment
    identifier, persists to the database as a deployment-scoped `settings`
    entry, rejects writes that lack a deployment identifier, and immediately
    updates the `FeatureFlagRegistry` for that deployment so subsequent
    `GET /api/features` requests reflect the updated flag state without a
    gateway restart.
- [ ] 4.5.4. Implement the `GET /api/features` endpoint. Requires 4.5.2.
  - See
    [RFC 0009 §API endpoint](./rfcs/0009-feature-flags-frontend.md#4-api-endpoint).
  - Success: the endpoint returns a JSON object mapping flag names to booleans
    (e.g. `{ "experimental_chat_ui": true, "dark_mode": false }`) by serializing
    the resolved registry state from `GatewayState` for the requested
    deployment, requires a deployment identifier in the request, and performs no
    database query on the hot path.
- [ ] 4.5.5. Add front-end `loadFeatureFlags()` integration in `app.js`.
      Requires 4.5.4.
  - See
    [RFC 0009 §Front-end consumption](./rfcs/0009-feature-flags-frontend.md#5-front-end-consumption).
  - Success: the browser fetches `GET /api/features` after authentication,
    includes the deployment identifier required by the 4.5.4 API contract,
    stores the result in a plain `featureFlags` object, and uses
    `featureFlags.experimental_chat_ui` checks for gating UI rendering and
    behaviour.
- [ ] 4.5.6. Add integration tests for per-flag resolution, operator overrides,
      subsystem defaults, mutability, and endpoint contract. Requires 4.5.3,
      4.5.4, and 4.5.5.
  - See
    [RFC 0009 §Requirements](./rfcs/0009-feature-flags-frontend.md#requirements).
  - Success: tests cover per-flag environment variable parsing (including
    `FEATURE_FLAG_<NAME>` pattern matching), operator override persistence and
    immediate registry updates, subsystem/default fallback behaviour, concurrent
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

Outstanding design decisions: requires a companion RFC for `llms.txt` discovery
trust, cache invalidation, override precedence, and provider-capacity mapping
before 5.1.2 and 5.1.3.

- [ ] 5.1.1. Implement `llms.txt` discovery, caching, and operator controls, and
      record the trust model in a companion RFC.
  - See future RFC: `llms.txt` discovery RFC §Summary, §Requirements, and
    §Rollout Plan. See FEATURE_PARITY.md §Agent System and §Model & Provider
    Support.
  - Success: axinite can fetch and cache `llms.txt` metadata, operators can
    inspect or disable discovered metadata, and discovery failures do not
    silently corrupt the provider registry.
- [ ] 5.1.2. Integrate discovered metadata into the provider registry and model
      capability surface. Requires 5.1.1.
  - See future RFC: `llms.txt` discovery RFC §Compatibility and Migration.
  - Success: discovered metadata can augment provider capability views and model
    selection without overriding explicit local configuration by accident.
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
  - Success: operators can inspect, set, and clear model overrides from the UI,
    API, and CLI without guessing which layer currently wins.

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
- [ ] 5.3.3. Implement citation capture across provider responses, tool outputs,
      and workspace-backed context. Requires 3.1.5 and 5.3.1.
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

Objective: add agent-driven canvas hosting with placement, resizing, and runtime
mutation support inside the existing web control surface.

Learning opportunity: validate how much agent-controlled UI can be exposed
without giving runtime code ambient authority over the host page.

Dependencies: benefits from 5.3.4 if citations and reasoning traces are later
rendered inside canvas surfaces, but can land independently of provider
selection work.

Outstanding design decisions: requires a companion RFC for canvas authority,
widget transport, placement persistence, asset loading, and isolation boundaries
before 6.1.2-6.1.4.

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
- [ ] 6.1.4. Add end-to-end canvas tests and asset-resolution handling. Requires
      6.1.2 and 6.1.3.
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
provenance, transformation policy, and per-channel capability negotiation before
6.2.2-6.2.4.

- [ ] 6.2.1. Implement a unified media ingest and caching pipeline covering
      images, PDFs, forwarded attachment downloads, and rich-text embedded
      media, and record the contract in a companion RFC.
  - See future RFC: advanced media handling RFC §Summary, §Requirements, and
    §Rollout Plan. See FEATURE_PARITY.md §Media handling, §Channel Features, and
    §Automation.
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
  - See future RFC: advanced media handling RFC §Rollout Plan and §Known Risks.
  - Success: per-channel limits and capability negotiation are enforced in one
    place, unsupported media falls back predictably, and automated tests cover
    preview, cache reuse, transcription, TTS, and transformed-media flows.


## 7. Raise assurance for safety and lifecycle invariants

Phase objective: add proof-oriented and generated verification where Axinite's
highest-risk behaviour lives in lifecycle interleavings, allowlist semantics,
and layered configuration rules rather than in one more example-based test.

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
  implicit or surface-specific behaviour;
- intent contracts declare inspectable, diffable constraints at every scope, and
  fail-closed gate evaluation blocks rather than permits unauthorized actions;
- an append-only execution ledger records system-level truth independently of
  the model's conversational narrative;
- delegated child jobs run with isolated context, explicit budgets, and
  narrowing-only contract inheritance;
- memory recall distinguishes facts from hypotheses through explicit projection
  tiers and epistemic status;
- auxiliary provider profiles route non-critical workloads to cost-appropriate
  models, and stable-prefix prompt assembly maximizes cache hits.

### 7.2. Generated properties for configuration and installer semantics

Objective: add generated property coverage where Axinite's real bug surface
comes from partial overlays, precedence rules, and hostile manifest or archive
combinations.

Learning opportunity: determine which invariants in configuration loading and
registry installation deserve stable generated properties before any bounded
model checking is added.

Dependencies: depends on 7.1.1; informs 7.3 by clarifying the shared
host-matching contract used by installer- and allowlist-adjacent code.

- [ ] 7.2.1. Add Proptest coverage for `src/config/mod.rs` and
      `src/settings.rs`, including precedence, merge identity, and database
      round-trip properties.
  - See
    [Formal verification methods in Axinite: configuration layering](./formal-verification-methods-in-axinite.md#fourth-priority-configuration-layering-in-srcconfigmodrs-and-srcsettingsrs).
  - Success: tests prove that default overlays are identity operations,
    documented precedence rules hold, explicit environment variables win over
    lower layers, and representable settings survive `to_db_map()` /
    `from_db_map()` round-trips.
- [ ] 7.2.2. Add Proptest coverage for `src/registry/installer.rs`, focusing on
      HTTPS enforcement, hostile paths, archive extraction limits, and checksum
      or host-validation combinations. Requires 7.2.1.
  - See
    [Formal verification methods in Axinite §Fifth priority: `src/registry/installer.rs`](./formal-verification-methods-in-axinite.md#fifth-priority-srcregistryinstallerrs).
  - Success: tests explore near-valid hostile manifests and archives, reject
    traversal and host-spoofing shapes, preserve documented entry-size limits,
    and make installer regressions fail on structured generated input instead of
    one-off examples only.


### 7.3. Bounded checking for allowlist and host-matcher semantics

Objective: use Kani where the highest-value invariants are small, deterministic,
security-critical, and close to crate-internal helpers.

Learning opportunity: determine whether Axinite can freeze one shared
host-matching contract for both the WASM and sandbox allowlists before adding a
deeper proof system.

Dependencies: depends on 7.1.2; builds on 7.2.2 for hostile-host input shapes;
feeds 7.5 if the extracted matcher proves stable enough for a later full proof.

- [ ] 7.3.1. Add Kani smoke harnesses for `src/tools/wasm/allowlist.rs`,
      covering userinfo rejection, path normalization safety, empty-allowlist
      denial, and allowed-implies-predicate satisfaction.
  - See
    [Formal verification methods in Axinite §Second priority: `src/tools/wasm/allowlist.rs`](./formal-verification-methods-in-axinite.md#second-priority-srctoolswasmallowlistrs)
    and
    [Formal verification methods in Axinite §Recommended Kani targets](./formal-verification-methods-in-axinite.md#recommended-kani-targets).
  - Success: the fast Kani target covers the chosen smoke harnesses in pull
    request CI and fails if allowlist normalization or rejection rules drift.
- [ ] 7.3.2. Decide wildcard-host semantics, extract a shared matcher, and move
      both allowlist call sites onto that contract. Requires 7.3.1.
  - See
    [Formal verification methods in Axinite: shared host matcher](./formal-verification-methods-in-axinite.md#third-priority-shared-hostdomain-matching-semantics)
    and
    [Formal verification methods in Axinite: decisions before proofs](./formal-verification-methods-in-axinite.md#decisions-axinite-should-make-before-writing-proofs).
  - Success: one explicit wildcard rule governs both the WASM and sandbox
    allowlists, the shared matcher rejects suffix-spoofing shapes, and semantic
    drift between the two call sites becomes mechanically impossible.
- [ ] 7.3.3. Add Kani equivalence and full-suite harnesses for the extracted
      matcher, and keep the smoke versus full split in Makefile and CI targets.
      Requires 7.3.2.
  - See
    [Formal verification methods in Axinite §Smoke versus full targets](./formal-verification-methods-in-axinite.md#smoke-versus-full-targets)
    and
    [Formal verification methods in Axinite §Unwinding policy](./formal-verification-methods-in-axinite.md#unwinding-policy).
  - Success: the fast path remains practical for pull requests, the deeper path
    explores larger host and path combinations, and unwind bounds stay close to
    the harnesses they justify.


### 7.4. Model-check the job lifecycle with Stateright

Objective: model the scheduler, worker, token, reaper, and retained-result
semantics as one explicit state machine so Axinite can explore the races that
ordinary tests are least likely to cover systematically.

Learning opportunity: determine which lifecycle predicates should be made
explicit in production code once the model highlights where semantics are
currently split across helper methods.

Dependencies: depends on 7.1.1 and 7.1.2; should follow 7.3.2 where shared
contract work exposes adjacent semantic cleanup patterns.

- [ ] 7.4.1. Make the lifecycle contract explicit for worker-stop, reapability,
      and retained-result semantics before modelling the full state space.
  - See
    [Formal verification methods in Axinite: decisions before proofs](./formal-verification-methods-in-axinite.md#decisions-axinite-should-make-before-writing-proofs).
  - Success: Axinite records the authoritative lifecycle predicates, clarifies
    post-completion retention behaviour, and removes ambiguity about which
    states keep workers, tokens, and handles alive.
- [ ] 7.4.2. Add `JobLifecycleModel` and its safety or reachability properties
      in `crates/axinite-verification`. Requires 7.4.1.
  - See
    [Formal verification methods in Axinite §First priority: the job-lifecycle cluster](./formal-verification-methods-in-axinite.md#first-priority-the-job-lifecycle-cluster)
    and
    [Formal verification methods in Axinite §Properties to encode](./formal-verification-methods-in-axinite.md#properties-to-encode).
  - Success: the model represents scheduler entries, worker state, container
    state, token state, retained completion results, and cleanup or reaper
    actions, and encodes both safety properties and required reachability
    traces.
- [ ] 7.4.3. Gate a bounded breadth-first pull-request checker and a deeper
      scheduled run for the Stateright model. Requires 7.4.2 and 7.1.3.
  - See
    [Formal verification methods in Axinite §Shared checker harness](./formal-verification-methods-in-axinite.md#shared-checker-harness).
  - Success: pull requests run the agreed bounded checker budget, deeper
    scheduled runs can extend depth and state count, and any safety-property
    counterexample is surfaced as a dedicated formal-verification failure.


### 7.5. Add a narrow later-stage Verus proof path

Objective: keep a proof-only path available for the few invariants that remain
worth proving after Axinite has extracted a tiny stable kernel.

Learning opportunity: determine whether the shared host matcher remains stable
enough after 7.3 to justify full proof maintenance instead of bounded checking
alone.

Dependencies: depends on 7.1.2 and 7.3.2; should follow 7.3.3 once Kani and
production code have already converged on one matcher contract.

- [ ] 7.5.1. Add the proof-only host-matcher model under `verus/` and wire it
      into `make verus`, without folding Verus into the default Rust gate stack.
  - See
    [Formal verification methods in Axinite §Verus integration](./formal-verification-methods-in-axinite.md#verus-integration)
    and
    [Formal verification methods in Axinite §What Verus should prove in Axinite](./formal-verification-methods-in-axinite.md#what-verus-should-prove-in-axinite).
  - Success: Verus proves the chosen exact-host and wildcard-host semantics,
    suffix spoofing remains impossible in the proof model, and proof execution
    stays isolated from the normal Cargo-driven test path.


### 7.1. Formal-verification infrastructure and workflow split

Objective: add the repository structure, tool runners, and CI split needed for
Kani, Stateright, Proptest, and later Verus without weakening the existing test,
coverage, or end-to-end workflows.

Learning opportunity: determine how much proof-oriented coverage Axinite can add
while keeping local execution predictable and keeping non-Cargo proof tools out
of the default Rust path.

Dependencies: independent of the earlier roadmap phases; unlocks 7.2-7.5.

- [ ] 7.1.1. Extend the existing Cargo workspace with
      `crates/axinite-verification`, keep `fuzz/` excluded, and add `proptest`
      as a root `dev-dependency`.
  - See
    [Formal verification methods in Axinite §Root `Cargo.toml` changes](./formal-verification-methods-in-axinite.md#root-cargotoml-changes).
  - Success: the workspace keeps the main `ironclaw` package as the default
    member, the verification crate can build independently, and generated
    property tests can run through normal Rust test entry points.
- [ ] 7.1.2. Add pinned tool metadata, install scripts, and Makefile targets for
      `test-verification`, `proptest-properties`, `kani`, `kani-full`, and
      `verus`. Requires 7.1.1.
  - See
    [Formal verification methods in Axinite §Recommended Makefile changes](./formal-verification-methods-in-axinite.md#recommended-makefile-changes).
  - Success: Kani and Verus use explicit install wrappers and pinned version
    files, Stateright models run through one dedicated test target, and the
    repository exposes a fast `formal-pr` path plus a deeper scheduled
    `formal-nightly` path.
- [ ] 7.1.3. Add a dedicated `formal.yml` workflow with pull-request jobs for
      Stateright and Kani, and a scheduled or manually dispatched Verus job.
      Requires 7.1.2.
  - See
    [Formal verification methods in Axinite §Recommended CI changes](./formal-verification-methods-in-axinite.md#recommended-ci-changes).
  - Success: formal checks are independently cacheable and diagnosable, Kani and
    Stateright failures do not disappear into unrelated test output, and Verus
    remains opt-in until Axinite has a proof-worthy kernel.
