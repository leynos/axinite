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

## Operator and integration references

- [Configuration guide](configuration-guide.md) is the reference for command
  line options, environment variables, defaults, and configuration precedence.
- [LLM providers](LLM_PROVIDERS.md) summarizes supported model backends and
  provider-specific setup notes.
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
  - [E2E infrastructure design](plans/2026-02-24-e2e-infrastructure-design.md)
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

## RFCs

- [RFC directory](rfcs/) stores proposed and in-flight architectural changes
  that need technical review before acceptance.
  - [RFC 0001: Expose MCP tool definitions](rfcs/0001-expose-mcp-tool-definitions.md)
    proposes how MCP tool schemas should be surfaced to the runtime and model.
  - [RFC 0002: Expose WASM tool definitions](rfcs/0002-expose-wasm-tool-definitions.md)
    proposes how WebAssembly tool schemas should be exported and consumed.
  - [RFC 0003: Skill bundle installation](rfcs/0003-skill-bundle-installation.md)
    proposes the packaging and installation model for skill bundles.
  - [RFC 0006: Provenance-based zero-knowledge intent plugins](rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md)
    proposes intent-plugin boundaries driven by provenance-aware controls.
  - [RFC 0007: Secure memory sidecar design](rfcs/0007-secure-memory-sidecar-design.md)
    proposes a sidecar-based design for protected memory handling.
  - [RFC 0008: WebSocket Responses API](rfcs/0008-websocket-responses-api.md)
    proposes a WebSocket-based Responses API surface for axinite.
  - [Monty code execution environment](rfcs/2026-03-11-monty-code-execution-environment.md)
    captures a pending RFC for the Monty execution environment that has not yet
    been renumbered.
  - [RFC 0009: Feature flags for the web front end](rfcs/0009-feature-flags-frontend.md)
    proposes a mechanism for passing feature flags from the backend to the
    browser front end.
  - [Tokenized delegated authorized endpoint requests](rfcs/2026-03-11-tokenized-delegated-authorized-endpoint-requests.md)
    captures a pending RFC for delegated endpoint requests that has not yet
    been renumbered.
