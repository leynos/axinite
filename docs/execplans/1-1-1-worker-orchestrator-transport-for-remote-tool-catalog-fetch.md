# Implement hosted remote tool transport for the worker and orchestrator

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`,
`Surprises & Discoveries`, `Decision Log`, and
`Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: DRAFT

## Purpose / big picture

After this work, a hosted worker must be able to ask the orchestrator for the
real hosted-visible tool catalogue and execute orchestrator-owned tools through
one generic proxy path. The concrete user-visible outcome is that hosted jobs
stop advertising only the worker-local container tools and the small
extension-management proxy subset; they instead advertise the real
orchestrator-owned tool definitions that the hosted runtime can actually call.

Success is observable in four ways. First, the orchestrator serves a
worker-authenticated catalogue endpoint for active executable remote tools.
Second, the worker fetches that catalogue during startup without hard-coding
new route fragments in more than one place. Third, the worker registers remote
tool proxies whose `name`, `description`, and `parameters` match the
orchestrator-supplied `ToolDefinition` values unchanged. Fourth, executing one
of those proxies calls one generic orchestrator execution endpoint rather than
the current extension-only special case.

This plan covers roadmap item `1.1.1`, tracks Issue `#12`, and deliberately
stops short of the broader canonical filtering and reasoning-context merge work
reserved for `1.1.2` and `1.1.3`.

## Repository orientation

The current transport seam already exists, but it is incomplete and still too
special-cased for this roadmap step.

- `src/worker/api/types.rs` already owns the serialized worker-orchestrator
  request and response types for completions, status, credentials, and the
  extension-only proxy path. This file is the best existing seed for the new
  shared boundary, although it may be renamed or extracted if a more neutral
  module name improves clarity.
- `src/worker/api.rs` is the worker-side HTTP adapter. It constructs URLs by
  joining a free-form path string onto `/worker/{job_id}/...`, and it exposes
  the current special-case `execute_extension_tool(...)` call.
- `src/orchestrator/api.rs` owns the worker-facing routes. The current router
  exposes `/worker/{job_id}/extension_tool`, but there is no catalogue route
  and no generic remote tool execution route.
- `src/orchestrator/api/handlers.rs` contains the current extension-only proxy
  policy and execution logic. It hard-codes `ExtensionToolKind` as the only
  remotely executable orchestrator-owned tool family.
- `src/worker/container.rs` constructs `WorkerHttpClient` with
  `WorkerHttpClient::from_env(...)` inside `WorkerRuntime::new(...)`, builds
  the worker tool registry, and loads `reason_ctx.available_tools` from that
  registry. This is the main startup coupling point that must become
  injectable.
- `src/tools/builtin/worker_extension_proxy.rs` demonstrates the current
  worker-local proxy pattern. It is useful as a reference for local `Tool`
  wrappers, but the feature must not deepen this extension-only special case.
- `src/orchestrator/api/tests/extension_tool.rs`,
  `src/worker/api/tests.rs`, and `src/worker/container/tests.rs` are the
  nearest existing unit-test seams for the new transport and startup changes.

Two documentation gaps matter before implementation begins.

- `docs/axinite-architecture-summary.md`, named in the request, does not exist
  in this checkout. Use `docs/axinite-architecture-overview.md` as the current
  architecture reference instead.
- `docs/users-guide.md` also does not exist yet, even though the documentation
  style guide treats it as canonical. This feature should create that guide if
  it still does not exist when implementation lands, then document the new
  hosted-tool behaviour there.

## Constraints

- Keep the worker-orchestrator transport contract owned in one shared module.
  Route fragments, request payloads, and response payloads must not be added as
  duplicated stringly typed conventions in separate worker and orchestrator
  files.
- Keep the worker startup path injectable at the transport boundary. Reading
  `IRONCLAW_WORKER_TOKEN` from the environment may remain at the composition
  root, but the runtime and tests must be able to supply a prebuilt transport
  adapter directly.
- Protect boundaries using the existing codebase shape rather than forcing a
  large hexagonal-architecture transplant. The important split is between
  policy and contract logic on one side and the `axum` or `reqwest` adapters on
  the other.
- Do not change the LLM-facing `ToolDefinition` contract. This step changes
  who supplies tool definitions, not the schema format itself.
- Do not widen hosted execution to approval-gated or otherwise unsafe tools as
  an incidental side effect of making the transport generic.
- Keep `1.1.1` scoped to transport, proxy execution, and startup injection.
  Do not absorb the full canonical `ToolRegistry` filtering work from `1.1.2`
  unless the code proves there is no stable seam between the two steps.
- Prefer `rstest` fixtures for shared setup. Any new behaviour-level tests must
  use `rstest-bdd` only where they add clear value over simpler unit coverage.
- Avoid environment mutation in tests. Use dependency injection and explicit
  constructors instead of `std::env::set_var`.

## Tolerances (exception triggers)

- Scope: if the smallest credible implementation requires touching more than
  12 files or more than 500 net lines before tests, stop and reassess whether
  `1.1.2` or `1.1.3` work has been pulled in accidentally.
- Contract: if the new transport needs a second shared boundary module because
  catalogue fetch and generic execution cannot fit one coherent contract, stop
  and document why the contract split is unavoidable.
- Startup coupling: if `WorkerRuntime` still has to read environment variables
  directly after three focused refactoring attempts, stop and record the
  hidden coupling that blocked injection.
- Filtering: if exposing a real MCP tool safely requires the full hosted
  visibility policy from `1.1.2`, stop and document the exact predicate or
  data that is missing.
- Behaviour tests: if a meaningful `rstest-bdd` scenario cannot be expressed
  without Docker or live MCP infrastructure, document that limitation and fall
  back to in-process `rstest` integration coverage rather than faking the
  behaviour poorly.

## Risks

- Risk: The generic execution route could become a second ad hoc policy layer
  that diverges from later hosted-tool filtering work.
  Severity: high
  Likelihood: medium
  Mitigation: put the hosted-executable predicate in one helper that both the
  catalogue and execution handlers call, even if the predicate is intentionally
  narrower in `1.1.1` than the later canonical filter.

- Risk: Replacing the extension-only proxy path could accidentally remove the
  currently safe extension-management tools from hosted workers before remote
  catalogue registration is complete.
  Severity: high
  Likelihood: medium
  Mitigation: stage the worker refactor so catalogue-backed remote registration
  lands before the extension-only path is removed, and preserve regression
  coverage for `tool_list`, `tool_search`, `tool_activate`, and
  `extension_info`.

- Risk: Making startup injectable could leak transport abstractions too far
  into unrelated worker logic.
  Severity: medium
  Likelihood: medium
  Mitigation: keep the injected port narrow, limited to the existing
  orchestrator interactions plus the new catalogue and execution calls.

- Risk: Behaviour tests may become brittle if they rely on route strings rather
  than the shared transport contract.
  Severity: medium
  Likelihood: medium
  Mitigation: build tests around the typed client and router helpers from the
  shared boundary module instead of asserting raw path fragments in multiple
  places.

## Milestone 1: define one shared worker-orchestrator boundary

Start by making the contract explicit in one place. Extend the existing shared
transport types module or extract it into a more neutral module such as
`src/worker_orchestrator/transport.rs`. The exact filename is less important
than the rule that both worker and orchestrator import the same route and
payload definitions.

The shared boundary must own:

- the catalogue route shape for `GET /worker/{job_id}/tools/catalog`
- the generic execution route shape for `POST /worker/{job_id}/tools/execute`
- typed request and response payloads for both routes
- any path-builder helpers or route constants needed to stop duplicating
  string fragments

Use the existing `ToolDefinition` and `ToolOutput` types in the shared payloads
instead of inventing a parallel schema format. The likely transport shapes are:

- `RemoteToolCatalogResponse { tools, toolset_instructions, catalog_version }`
- `RemoteToolExecutionRequest { tool_name, params }`
- `RemoteToolExecutionResponse { output }`

At the worker boundary, introduce a narrow port for the orchestrator transport.
This can be a trait or a very small adapter interface, but it must let the
worker runtime receive a prebuilt transport during tests and composition.
`run_worker(...)` may keep the `from_env(...)` composition step; the runtime
itself should not own that concern.

## Milestone 2: add orchestrator catalogue and generic execution support

Add the new routes in `src/orchestrator/api.rs`, then keep the handler logic in
`src/orchestrator/api/handlers.rs` or a small adjacent support module focused
on policy rather than `axum` extraction.

The orchestrator must do three things.

1. Build a hosted-visible remote catalogue from the canonical
   `ToolRegistry`. For `1.1.1`, keep this predicate intentionally narrow and
   explicit: a remote tool is catalogue-visible only if the orchestrator can
   execute it through the new generic path without interactive approval or a
   worker-local dependency. Do not promise the full `1.1.2` policy yet.
2. Return those definitions unchanged as `ToolDefinition` values, together with
   any `toolset_instructions` available today and a `catalog_version` value
   suitable for later refresh or observability work.
3. Execute any catalogue-visible remote tool through the canonical
   `ToolRegistry`, using the request `job_id` in the `JobContext`, and reject
   unknown, missing, approval-gated, or otherwise ineligible tools with clear
   status codes.

The existing `execute_extension_tool(...)` path should either be removed or
reduced to a thin compatibility wrapper that delegates into the new generic
execution service. Leaving two independent remote execution paths would violate
the roadmap objective.

Keep the hosted-executable predicate separate from HTTP concerns. A small
helper such as `HostedRemoteToolPolicy` or `is_hosted_remote_tool(...)` is
enough; no larger architecture transplant is needed.

## Milestone 3: register remote proxies during worker startup

Refactor worker startup so the transport is injected before tool registration.
The likely shape is:

1. `run_worker(...)` builds `WorkerHttpClient::from_env(...)`.
2. `WorkerRuntime::new(...)` or a replacement constructor receives that client
   or a narrow transport port directly.
3. `WorkerRuntime` registers container-local tools first.
4. `WorkerRuntime` fetches the remote catalogue through the injected
   transport.
5. `WorkerRuntime` registers one local `RemoteToolProxy` per catalogue entry.
6. `reason_ctx.available_tools` comes from the combined registry.

Model the worker-side proxy after the current `worker_extension_proxy` wrapper,
but make it generic over the shared transport contract rather than hard-coding
`ExtensionToolKind`. The proxy must report the orchestrator-supplied
`ToolDefinition` data unchanged and execute by calling the shared generic
remote execution endpoint.

Preserve the worker’s existing local container tools. This milestone should
replace the extension-only remote registration path, not remove local file,
patch, or shell tools.

## Milestone 4: add focused unit and behavioural coverage

Write the failing tests first. The feature should be observable at three
layers.

### Shared-boundary and adapter tests

Add `rstest` coverage for the shared boundary module and the worker HTTP
adapter. Cover:

- URL or route construction for the new catalogue and execute paths
- JSON round-trips for the new request and response payloads
- unhappy-path client behaviour when the orchestrator returns non-success
  status codes

### Orchestrator API tests

Extend `src/orchestrator/api/tests/` with `rstest`-based endpoint tests that
prove:

- the catalogue returns active hosted-executable tool definitions and preserves
  their `description` and `parameters`
- unknown or non-catalogue tools are rejected by generic execution
- approval-gated tools are rejected by generic execution
- successful generic execution propagates the request `job_id` into the
  `JobContext`

Use fake `Tool` implementations registered into `ToolRegistry` rather than live
MCP infrastructure.

### Worker startup and proxy tests

Extend `src/worker/container/tests.rs` and, if needed,
`src/tools/builtin/worker_extension_proxy.rs` tests so the worker runtime proves
that:

- a fake remote catalogue adds remote tools to the worker-visible definitions
- remote tool order and metadata are stable enough for reasoning-context use
- executing a remote proxy hits the generic execution route
- the runtime can be constructed with an explicit transport without relying on
  `IRONCLAW_WORKER_TOKEN`

### Behaviour tests

If an in-process behaviour harness is practical, add one `rstest-bdd` feature
under `tests/` that exercises the end-to-end hosted-remote-tool flow without
Docker:

- Scenario: a worker starts against an orchestrator that exposes a hosted-safe
  MCP-like tool, fetches the catalogue, and advertises that tool unchanged.
- Scenario: the worker executes that advertised remote tool and the
  orchestrator receives the generic execution request with the expected job ID
  and parameters.

If `rstest-bdd` adds no clarity beyond the unit and in-process integration
tests, document that in the `Decision Log` and keep the coverage in `rstest`
tests instead. The important requirement is behavioural proof, not framework
maximalism.

## Milestone 5: document the new contract and complete the roadmap step

Update the design and user-facing documents in the same change.

- Update `docs/rfcs/0001-expose-mcp-tool-definitions.md` to record the chosen
  shared boundary module, the generic execution path, and any decision about
  how much catalogue policy belongs in `1.1.1` versus `1.1.2`.
- Update `docs/axinite-architecture-overview.md` so the orchestrator and
  worker sections describe the new catalogue and generic execution routes.
- Update `docs/users-guide.md`. If the file still does not exist, create it as
  the canonical user guide and include a section explaining that hosted workers
  now advertise hosted-visible orchestrator-owned tools, while approval-gated
  or unavailable tools may still be hidden.
- Update `docs/contents.md` for any new documentation files or directories.
- If the repository keeps `docs/execplans/` after this change, update
  `docs/repository-layout.md` so the documentation subtree description remains
  accurate.
- Mark roadmap item `1.1.1` as done in `docs/roadmap.md` only after code,
  tests, and documentation all land.

## Validation

Run the focused suites first, then the full repository gate. Keep all output in
`/tmp` with `tee` so truncated terminal output does not hide failures.

Name the new tests with stable `remote_tool_catalog`, `remote_tool_execute`,
and `hosted_worker_remote_tool` prefixes so these commands remain useful.

```bash
set -o pipefail
BRANCH=$(git branch --show-current | tr '/' '-')
cargo test remote_tool_catalog --lib -- --nocapture \
  2>&1 | tee /tmp/test-remote-tool-catalog-axinite-${BRANCH}.out
```

```bash
set -o pipefail
BRANCH=$(git branch --show-current | tr '/' '-')
cargo test remote_tool_execute --lib -- --nocapture \
  2>&1 | tee /tmp/test-remote-tool-execute-axinite-${BRANCH}.out
```

```bash
set -o pipefail
BRANCH=$(git branch --show-current | tr '/' '-')
cargo test hosted_worker_remote_tool --lib -- --nocapture \
  2>&1 | tee /tmp/test-hosted-worker-remote-tool-axinite-${BRANCH}.out
```

```bash
set -o pipefail
BRANCH=$(git branch --show-current | tr '/' '-')
make all 2>&1 | tee /tmp/make-all-axinite-${BRANCH}.out
```

```bash
set -o pipefail
BRANCH=$(git branch --show-current | tr '/' '-')
bunx markdownlint-cli2 \
  docs/execplans/1-1-1-worker-orchestrator-transport-for-remote-tool-catalog-fetch.md \
  docs/rfcs/0001-expose-mcp-tool-definitions.md \
  docs/axinite-architecture-overview.md \
  docs/users-guide.md \
  docs/roadmap.md \
  docs/contents.md \
  docs/repository-layout.md \
  2>&1 | tee /tmp/markdownlint-axinite-${BRANCH}.out
```

```bash
set -o pipefail
git diff --check 2>&1 | tee /tmp/git-diff-check-axinite.out
```

Expected evidence:

- the focused test commands end with `test result: ok.`
- `make all` completes without `clippy` warnings or failing tests
- `markdownlint-cli2` exits cleanly for the changed documentation
- `git diff --check` reports no whitespace or merge-marker problems

## Progress

- [x] 2026-03-14 11:27Z: Reviewed the roadmap entry, RFC `0001`, the
  architecture overview, testing guidance, and the existing ExecPlan format.
- [x] 2026-03-14 11:27Z: Traced the current transport seam through
  `src/worker/api/types.rs`, `src/worker/api.rs`, `src/orchestrator/api.rs`,
  `src/orchestrator/api/handlers.rs`, and `src/worker/container.rs`.
- [x] 2026-03-14 11:27Z: Confirmed that the current hosted path still depends
  on extension-only worker proxies in
  `src/tools/builtin/worker_extension_proxy.rs`.
- [x] 2026-03-14 11:27Z: Confirmed that `docs/axinite-architecture-summary.md`
  and `docs/users-guide.md` are absent in this checkout and must be handled
  explicitly by the implementation.
- [x] 2026-03-14 11:27Z: Drafted this ExecPlan and recorded the expected code,
  test, and documentation touch points for roadmap item `1.1.1`.
- [ ] Await user approval before implementation begins.

## Surprises & Discoveries

- The repository already has one shared worker transport type module in
  `src/worker/api/types.rs`. The missing piece is not the absence of any shared
  contract; it is that hosted catalogue fetch and generic remote execution have
  not been added to that contract.
- The worker runtime still creates its transport adapter from the environment
  inside `WorkerRuntime::new(...)`. That makes startup injection the main
  refactoring seam, not route registration.
- The request named `docs/axinite-architecture-summary.md`, but this branch
  uses `docs/axinite-architecture-overview.md` as the architecture document.
- The canonical `docs/users-guide.md` does not exist yet, so this feature has
  a documentation bootstrap task as well as a runtime task.
- Project-memory retrieval through the qdrant notes tool failed during plan
  drafting with `Unexpected response type`, so this plan relies on repository
  source and documentation rather than stored notes.

## Decision Log

- Decision: keep this plan scoped to roadmap item `1.1.1` rather than folding
  in full canonical hosted-tool filtering from `1.1.2`.
  Rationale: the roadmap explicitly separates transport hardening from later
  filtering and reasoning-context work. A narrow shared contract and generic
  execution path are the prerequisites that reduce risk for the later steps.

- Decision: treat the shared transport contract as the primary architecture
  boundary and keep HTTP libraries on the adapter side of that boundary.
  Rationale: this applies the useful part of hexagonal architecture here
  without forcing a large repository-wide reorganization.

- Decision: create or update `docs/users-guide.md` as part of implementation if
  it is still missing.
  Rationale: the request explicitly calls for that update, and the
  documentation style guide treats the file as canonical for user-facing
  behaviour.

- Decision: prefer in-process `rstest` integration tests first, then add
  `rstest-bdd` only if it gives clearer behavioural evidence.
  Rationale: the feature is an internal transport change, so behaviour tests
  must remain lightweight and deterministic.

## Outcomes & Retrospective

This section is intentionally blank until implementation begins. On completion,
record what landed, what changed from the draft, which risks materialized, and
what `1.1.2` can now build on safely.
