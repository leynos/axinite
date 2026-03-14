# Repository layout

This document explains the current repository shape for axinite. It is for
contributors who need to find source code, tests, runtime extension artefacts,
operational documentation, and generated output quickly before making changes.

## Top-level tree

Listing 1. Simplified repository tree.

```plaintext
.
├── .cargo/
├── .claude/
├── .github/
├── channels-src/
├── deploy/
├── docker/
├── docs/
├── fuzz/
├── migrations/
├── registry/
├── scripts/
├── skills/
├── src/
├── tests/
├── tools-src/
├── wit/
├── wix/
├── Cargo.toml
├── Makefile
├── README.md
└── providers.json
```

The tree above is illustrative rather than exhaustive. It omits many leaf
files and collapses large subtrees so the main responsibility boundaries are
easy to scan.

## Directory responsibilities

Table 1. Major repository paths and their current responsibilities.

<!-- markdownlint-disable MD013 MD060 -->
| Path | Purpose | Notable conventions and constraints |
|------|---------|-------------------------------------|
| `.cargo/` | Checked-in Cargo configuration for local and Continuous Integration (CI) builds | Includes Linux linker configuration for `clang` and `mold` |
| `.claude/` | Local automation prompts and rules used by assistant-driven workflows | Contributor-facing only when working on those automation flows |
| `.github/` | GitHub Actions workflows, repository automation, and pull request templates | Workflow changes often need YAML-aware review because they affect release and CI behaviour |
| `channels-src/` | Standalone source crates for WebAssembly (WASM) channel integrations such as Telegram, Slack, Discord, and WhatsApp | These crates are intentionally excluded from the root workspace build and are packaged as runtime-loadable channel artefacts |
| `deploy/` | Service-unit examples, deployment environment templates, and setup scripts | Intended for operational setup rather than local development logic |
| `docker/` and `Dockerfile*` | Container build material for host, test, and worker runtimes | The worker image and sandbox image are part of the execution-isolation story, not just packaging |
| `docs/` | Long-lived maintainer and design documentation, including plans and RFCs | `plans/` and `rfcs/` are durable reference subtrees rather than scratch notes |
| `fuzz/` | Fuzzing harnesses, corpora, and fuzz-specific Cargo manifest | Kept outside the main workspace member set |
| `migrations/` | Database schema migrations and backend-specific schema baselines | Includes both numbered migrations and `libsql_schema.sql` |
| `registry/` | Embedded extension manifests, bundles, and catalogue metadata | Feeds runtime discovery and installation of tools and channels |
| `scripts/` | Repository utility scripts for builds, validation, coverage, and safety checks | Prefer these scripts and `Makefile` targets over ad hoc command sequences when an existing path exists |
| `skills/` | Project-local skill bundles and supporting assets for in-product workflows | Distinct from the global Codex skills installed outside the repository |
| `src/` | Main Rust host application code | Contains the agent runtime, channels, config, persistence, extensions, sandbox, worker, and web gateway subsystems |
| `tests/` | Integration, end-to-end, and support test suites | Includes Rust integration tests plus the `tests/e2e/` browser harness |
| `tools-src/` | Standalone source crates for WASM tools and related manifests | Like `channels-src/`, these live outwith the root workspace member build and are packaged for dynamic loading |
| `wit/` | Shared WIT contracts for WASM tools and channels | These contracts are authoritative for extension interface compatibility |
| `wix/` | Windows installer packaging assets | Relevant when changing release packaging on Windows |
| `image_out/` | Generated local image output | Treat as generated output rather than hand-maintained source material |
| `target/` | Cargo build output and shared generated artefacts such as cached WASM builds | Generated directory; do not treat contents as authoritative source |
<!-- markdownlint-enable MD013 MD060 -->

## Source tree guide

The `src/` tree is the main host application and is organized by subsystem
rather than by technical layer alone.

Table 2. Key `src/` paths and why they matter.

<!-- markdownlint-disable MD013 MD060 -->
| Path | Purpose |
|------|---------|
| `src/main.rs` | Process entry point, CLI dispatch, service bootstrap, channel wiring, and agent startup |
| `src/app.rs` | Mechanical bootstrap through `AppBuilder`, including database, secrets, LLM, tools, workspace, and extension initialization |
| `src/agent/` | Core agent runtime, scheduling, routines, heartbeats, session state, compaction, and self-repair |
| `src/channels/` | REPL, HTTP, Signal, web gateway, relay, webhook, and WASM-backed channel integrations |
| `src/cli/` | Public and internal command definitions, onboarding helpers, diagnostics, and subcommand execution |
| `src/config/` | Layered configuration loading from environment, optional TOML, database settings, and injected secrets |
| `src/db/` | Backend-agnostic persistence traits plus PostgreSQL and libSQL implementations |
| `src/extensions/` | Runtime discovery, installation, authentication, and activation for WASM and Model Context Protocol (MCP) extensions |
| `src/llm/` | Language model provider chain, routing, provider-specific integrations, and session handling |
| `src/orchestrator/` | Sandbox orchestrator API, job manager, token store, and reaper support |
| `src/registry/` | Runtime registry catalogue loading and installer support |
| `src/safety/` | Input validation, sanitization, policy enforcement, and secret leak detection |
| `src/sandbox/` | Docker-backed execution sandbox, network proxy, and resource-policy handling |
| `src/secrets/` | Encrypted secrets storage, cryptography helpers, and credential lookup interfaces |
| `src/tools/` | Built-in tools, MCP tool plumbing, WASM tool hosting, and builder flows |
| `src/worker/` | Container worker runtime and orchestrator-facing HTTP client logic |
| `src/workspace/` | Persistent memory documents, chunking, embeddings, and hybrid search support |
| `src/NETWORK_SECURITY.md` | Authoritative security reference for network-facing surfaces and trust boundaries |
<!-- markdownlint-enable MD013 MD060 -->

## Documentation and planning paths

The `docs/` tree mixes stable reference documents with ongoing planning
material. The important distinction is whether a file describes the implemented
system, a proposed design, or active execution work.

Table 3. Documentation subtrees and their roles.

<!-- markdownlint-disable MD013 MD060 -->
| Path | Role |
|------|------|
| `docs/*.md` | Stable guides, specifications, and overview documents for the current repository state |
| `docs/execplans/` | Approval-gated execution plans written in the Codex ExecPlan format |
| `docs/plans/` | Execution plans and implementation tracking documents that may describe in-flight work |
| `docs/rfcs/` | Request for Comments (RFC) documents for proposed or recently accepted changes |
<!-- markdownlint-enable MD013 MD060 -->

## Extension packaging paths

This repository has a split between the host runtime and extension source
trees. That split is important because not every extension crate is built as a
normal root-workspace member.

- `channels-src/` contains standalone channel crates that produce deployable
  WASM channel artefacts.
- `tools-src/` contains standalone tool crates and related manifests for
  runtime-loadable WASM tools.
- `registry/` contains the metadata that lets the host discover, install, and
  present those extensions.
- `wit/` contains the interface definitions that extension authors must target.

These paths should usually be considered together when changing extension
contracts, packaging, or runtime loading behaviour.

## Generated and unusual paths

Several paths need extra care because they are generated, environment-specific,
or operational rather than source-of-truth code.

- `target/` is generated build output and shared artefact cache content.
- `image_out/` is generated local output, not hand-maintained repository
  source.
- `tests/e2e/` uses a browser-driven test harness and therefore has different
  runtime prerequisites from the pure Rust integration tests.
- `channels-src/` and `tools-src/` are source trees that are intentionally
  packaged for dynamic runtime loading instead of normal static linking into the
  main crate.
