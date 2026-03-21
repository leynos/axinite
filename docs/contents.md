# Documentation contents

- [Documentation contents](contents.md) explains how to navigate the
  documentation set and serves as the index for the material below.

## Start here

- [Welcome to axinite](welcome-to-axinite.md) introduces the project,
  its current direction, and the main documents to read next.
- [Developer's guide](developers-guide.md) is the maintainer-facing operating
  manual for building, testing, debugging, and extending axinite.
- [Testing strategy](testing-strategy.md) explains how the repository designs
  tests, runs them locally, exercises them in CI, and supports periodic or ad
  hoc validation.
- [User's guide](users-guide.md) explains current operator-visible behaviour
  and day-to-day usage expectations for the shipped runtime.
- [Repository layout](repository-layout.md) maps the repository structure and
  shows where major subsystems, assets, and support files live.
- [Documentation style guide](documentation-style-guide.md) defines the
  repository's conventions for document types, formatting, RFCs, ADRs, and
  roadmap structure.
- [Roadmap](roadmap.md) lays out the implementation workstreams, tasks,
  dependencies, and design checkpoints for planned delivery.

## Architecture and subsystem design

- [axinite architecture overview](axinite-architecture-overview.md) explains
  the top-level runtime shape, major subsystems, and how the pieces fit
  together.
- [Webhook server design](webhook-server-design.md) describes the unified
  webhook listener, route composition model, and rollback-focused restart
  behaviour.
- [Front-end architecture](front-end-architecture.md) explains how the web
  gateway serves the browser UI, generates the interface, and connects browser
  actions to the runtime subsystems.
- [Chat model](chat-model.md) traces the chat pipeline from ingress through
  context assembly, tool execution, approvals, and outbound sinks.
- [Database integrations](database-integrations.md) explains the PostgreSQL,
  `pgvector`, and libSQL persistence backends and the differences between them.
- [Embedding integrations](embedding-integrations.md) documents the embedding
  provider interfaces, adapters, and the places embeddings are used.
- [Jobs and routines](jobs-and-routines.md) covers the scheduler, background
  jobs, routines engine, touchpoints, and extension seams.
- [Agent skills support](agent-skills-support.md) explains how skills are
  discovered, installed, selected, and injected into model context.
- [Smart routing spec](smart-routing-spec.md) captures the current design for
  routing requests across models and providers.

## Implementation and testing references

- [Navigating code complexity](complexity-antipatterns-and-refactoring-strategies.md)
  explains complexity metrics, the bumpy-road antipattern, and practical
  refactoring strategies for maintainers.
- [Reliable testing in Rust via dependency injection](reliable-testing-in-rust-via-dependency-injection.md)
  explains how to avoid global-state coupling in tests by injecting
  environment, clock, and other system dependencies.
- [`rstest-bdd` user's guide](rstest-bdd-users-guide.md) documents how to use
  the current `rstest-bdd` implementation from Gherkin features through step
  definitions and scenario execution.
- [A systematic guide to effective, ergonomic, and DRY doctests in Rust](rust-doctest-dry-guide.md)
  explains the `rustdoc` compilation model and practical doctest patterns for
  public API documentation.
- [Mastering test fixtures in Rust with `rstest`](rust-testing-with-rstest-fixtures.md)
  explains fixture-based and parameterized testing with `rstest` for Rust
  contributors.

## Operator and integration references

- [Configuration guide](configuration-guide.md) is the reference for command
  line options, environment variables, defaults, and configuration precedence.
- [Large language model (LLM) providers](LLM_PROVIDERS.md) summarizes
  supported model backends and provider-specific setup notes.
- [Telegram setup](TELEGRAM_SETUP.md) explains how to configure and run the
  Telegram channel integration.
- [Building channels](BUILDING_CHANNELS.md) describes how to implement and
  wire new channels into the application.
- [Writing WebAssembly tools for ironclaw](writing-web-assembly-tools-for-ironclaw.md)
  explains how extension authors build and package WebAssembly tools for the
  existing runtime and tool contract.

## Plans

- [Plans directory](plans/) collects execution plans, investigations, and
  implementation working notes for discrete streams of work.
  - [Automated QA](plans/2026-02-24-automated-qa.md) captures the plan for
    automated quality assurance coverage and supporting workflow changes.
  - [End-to-end (E2E) infrastructure design](plans/2026-02-24-e2e-infrastructure-design.md)
    explores the design for end-to-end test infrastructure.
  - [E2E infrastructure](plans/2026-02-24-e2e-infrastructure.md) tracks the
    delivery work for the end-to-end test environment.
  - [Call parameters discarded](plans/2026-03-09-call-parameters-discarded.md)
    investigates and plans remediation for dropped call parameters.
  - [Resolve meta tooling unavailability](plans/2026-03-09-resolve-meta-tooling-unavailability.md)
    captures the plan for restoring missing meta-tooling behaviour.
  - [Secret blocking overzealous](plans/2026-03-09-secret-blocking-overzealous.md)
    tracks the work to tighten secret-blocking behaviour without breaking
    valid flows.
  - [Use WIT v3 in extensions](plans/2026-03-09-use-wit-v3-in-extensions.md)
    plans the migration of extension interfaces to WIT v3.
  - [Invalid tool schema](plans/2026-03-10-invalid-tool-schema.md) records the
    investigation and fix plan for invalid tool-schema handling.
  - [Compile-time reduction](plans/2026-03-12-compile-time-reduction.md)
    tracks work to reduce build times and related tooling overhead.
- [ExecPlans directory](execplans/) collects approval-gated execution plans
  written in the Codex ExecPlan format for roadmap-scoped work.
  - [Migrate from async-trait to native async traits](execplans/migrate-async-trait.md)
    plans the staged migration away from `async-trait`, including the ADR 006
    follow-on for dyn-backed traits.
  - [Worker-orchestrator transport for hosted remote tool catalogue fetch](execplans/1-1-1-worker-orchestrator-transport-for-remote-tool-catalog-fetch.md)
    plans roadmap item `1.1.1` for the shared hosted remote-tool transport.

## RFCs

- [RFC directory](rfcs/) stores proposed and in-flight architectural changes
  that need technical review before acceptance.
  - [RFC 0001: Expose Model Context Protocol (MCP) tool definitions](rfcs/0001-expose-mcp-tool-definitions.md)
    describes how MCP tool schemas should be surfaced to the runtime and model.
  - [RFC 0002: Expose WebAssembly (WASM) tool definitions](rfcs/0002-expose-wasm-tool-definitions.md)
    proposes how WebAssembly tool schemas should be exported and consumed.
  - [RFC 0003: Skill bundle installation](rfcs/0003-skill-bundle-installation.md)
    proposes the packaging and installation model for skill bundles.
  - [RFC 0004: Tokenized delegated authorized endpoint requests](rfcs/0004-tokenized-delegated-authorized-endpoint-requests.md)
    proposes a delegated endpoint model that keeps configured service URLs out
    of agent-visible and extension-visible surfaces.
  - [RFC 0005: Monty code execution environment](rfcs/0005-monty-code-execution-environment.md)
    proposes a Monty-backed Python automation environment for saved scripts and
    ephemeral code execution.
  - [RFC 0006: Provenance-based zero-knowledge intent plugins](rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md)
    proposes intent-plugin boundaries driven by provenance-aware controls.
  - [RFC 0007: Secure memory sidecar design](rfcs/0007-secure-memory-sidecar-design.md)
    proposes a sidecar-based design for protected memory handling.
  - [RFC 0008: WebSocket Responses API](rfcs/0008-websocket-responses-api.md)
    proposes a WebSocket-based Responses API surface for axinite.
  - [RFC 0009: Feature flags for the web front end](rfcs/0009-feature-flags-frontend.md)
    proposes a mechanism for passing feature flags from the backend to the
    browser front end.
  - [RFC 0010: Intent contracts and fail-closed runtime gates](rfcs/0010-intent-contracts-and-fail-closed-runtime-gates.md)
    proposes explicit intent contracts and fail-closed runtime policy gates.
  - [RFC 0011: Execution truth ledger and action provenance](rfcs/0011-execution-truth-ledger-and-action-provenance.md)
    proposes an append-only ledger for approvals, tool calls, and system
    actions.
  - [RFC 0012: Delegated child jobs with isolated context](rfcs/0012-delegated-child-jobs-with-isolated-context.md)
    proposes bounded delegation with isolated context, budgets, and evidence
    bundles.
  - [RFC 0013: Auxiliary provider profiles and stable-prefix prompt assembly](rfcs/0013-auxiliary-provider-profiles-and-stable-prefix-prompt-assembly.md)
    proposes named provider profiles and stable prompt-prefix construction.
  - [RFC 0014: Memory projection tiers and promotion rules](rfcs/0014-memory-projection-tiers-and-promotion-rules.md)
    proposes projection classes, epistemic status, and promotion rules for
    memory.
  - [RFC 0015: Hierarchical memory materialization for memoryd](rfcs/0015-hierarchical-memory-materialization-for-memoryd.md)
    outlines how `memoryd` can materialize episode, semantic-carrier, and
    theme structures without replacing RFC 0014's projection taxonomy.
  - [RFC 0016: Theme detection and sparsity rebalancing for memoryd](rfcs/0016-theme-detection-and-sparsity-rebalancing-for-memoryd.md)
    defines how `memoryd` maintains stable theme identities, balancing, and
    lineage over semantic carriers without promoting themes into a new truth
    class.
  - [RFC 0017: Hierarchical recall for memoryd](rfcs/0017-hierarchical-recall-for-memoryd.md)
    proposes the theme-aware, budget-aware read path that expands to episodes
    and messages only when the extra evidence is worth the token cost.

## ADRs

- [ADR 001: OPA Rego as the policy engine for intent enforcement](adr-001-rego-policy-engine-for-intent-enforcement.md)
  records the proposed policy-engine choice for deterministic,
  machine-auditable intent gates.
- [ADR 002: Authoritative intent state must remain human-auditable](adr-002-authoritative-intent-state-must-remain-human-auditable.md)
  records that provider-owned continuation state may optimize execution but
  must never become the sole source of truth for intent or decision history.
- [ADR 003: Theme management belongs in memoryd](adr-003-theme-management-belongs-in-memoryd.md)
  records that stable theme IDs, balancing policy, and lineage belong in the
  memory sidecar rather than in the clustering substrate.
- [ADR 004: Dual-path semantic extraction with validated provenance](adr-004-dual-path-semantic-extraction-with-validated-provenance.md)
  records that `memoryd` should support both structured LLM extraction and
  encoder-only extraction behind one provenance-validated schema.
- [ADR 005: Dual-mode uncertainty gating for hierarchical recall](adr-005-dual-mode-uncertainty-gating-for-hierarchical-recall.md)
  records that hierarchical recall should support both proxy-based and
  model-assisted gain estimation behind one expansion-gating interface.
- [ADR 006: Dual-trait pattern for dyn-backed async interfaces](adr-006-dual-trait-pattern-for-dyn-backed-async-interfaces.md)
  records the proposed migration pattern for dyn-backed async traits,
  balancing compilation speed against implementation maintainability.
