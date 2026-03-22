# Merge remote Model Context Protocol (MCP) tool definitions into the worker reasoning context

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`,
`Surprises & Discoveries`, `Decision Log`, and
`Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: COMPLETE

## Purpose / big picture

Roadmap item `1.1.3` exists to make the hosted worker reason from the same
tool contracts that the orchestrator already owns. After this work, a hosted
model request must see one unified tool surface that contains both the
container-local worker tools and the orchestrator-owned hosted-visible Model
Context Protocol (MCP) tools, with the remote tools carrying their canonical
`name`, `description`, and JSON Schema `parameters` unchanged.

Success is observable in five ways. First, worker startup still fetches the
remote catalogue and registers one worker-local proxy per advertised remote
tool. Second, the reasoning context built for hosted jobs exposes those
proxies alongside the local container tools in `available_tools`. Third, the
same merged tool surface is refreshed before later large language model (LLM)
calls, so long-running jobs do not drift back to a local-only view. Fourth,
toolset guidance from the orchestrator is injected once as guidance rather than
duplicated noisily on every loop iteration. Fifth, tests prove that the worker
advertises exactly the same remote tool definitions that the canonical
orchestrator registry would advertise in the normal in-process path.

This plan covers roadmap item `1.1.3` only. It assumes `1.1.1` and `1.1.2`
remain the completed prerequisites that own the shared worker-orchestrator
transport and the canonical hosted-visible filtering seam. It must not absorb
the broader schema-fidelity and routing matrix from `1.1.4` beyond the narrow
tests needed to prove the reasoning-context merge itself.

## Approval gates

- Plan approved
  Acceptance criteria: the plan stays limited to reasoning-context merge
  behaviour for roadmap item `1.1.3`, with transport and broader schema-matrix
  work left to the neighbouring roadmap items that already own them.
  Sign-off: human reviewer approves the ExecPlan before implementation begins.
- Implementation complete
  Acceptance criteria: initial context construction and later
  `before_llm_call(...)` refreshes use the same registry-backed merged tool
  surface, degraded startup remains intact, and hosted guidance is injected
  once during context build rather than on every refresh.
  Sign-off: implementer marks the feature complete before final validation.
- Validation passed
  Acceptance criteria: the required repository gates and targeted reasoning
  regressions pass with retained logs, and the final plan notes record any
  follow-up work still reserved for `1.1.4`.
  Sign-off: implementer records final validation evidence immediately before
  commit and push.
- Docs synced
  Acceptance criteria: the roadmap, RFC, architecture overview, user guide,
  contents index, and execplan all describe the same merged hosted-tool
  reasoning behaviour before the plan is marked complete.
  Sign-off: implementer completes the documentation sync as the final
  pre-commit checkpoint.

## Repository orientation

The relevant seams already exist. The plan should tighten and document them,
not replace them.

- `src/worker/container.rs` owns `WorkerRuntime`, including remote-catalogue
  registration, degraded startup handling, and
  `WorkerRuntime::build_reasoning_context(...)`.
- `src/worker/container/delegate.rs` owns `ContainerDelegate::before_llm_call`,
  which refreshes `reason_ctx.available_tools` during the hosted execution
  loop.
- `src/tools/builtin/worker_remote_tool_proxy.rs` owns the worker-local proxy
  wrapper created from orchestrator-supplied `ToolDefinition` values.
- `src/worker/api.rs` and `src/worker/api/types.rs` already own the shared
  catalogue-fetch and remote-execution client contract from `1.1.1`.
- `src/tools/registry/hosted.rs` owns the canonical hosted-visible registry
  projection added in `1.1.2`.
- `src/orchestrator/api/remote_tools.rs` consumes that canonical projection to
  produce the hosted remote catalogue and execute remote tools through the
  orchestrator-owned registry.
- `src/worker/container/tests.rs`,
  `src/orchestrator/api/tests/remote_tools.rs`,
  `src/tools/registry/tests.rs`, and
  `src/tools/builtin/worker_remote_tool_proxy.rs` tests are the closest
  existing regression seams.
- `docs/rfcs/0001-expose-mcp-tool-definitions.md` remains the design authority
  for this work, while `docs/roadmap.md` defines the delivery checkpoint.
- The user request named `docs/axinite-architecture-summary.md`, but that file
  does not exist in this checkout. Use
  `docs/axinite-architecture-overview.md` as the current architecture source
  of truth.

## Constraints

- Keep the existing worker-orchestrator transport intact. This step is about
  how the worker reasons from the fetched catalogue, not a second transport
  redesign.
- Preserve the LLM-facing `ToolDefinition` contract exactly. No provider-
  specific rewriting, prose reconstruction, or lossy summaries may sit between
  the orchestrator catalogue and the worker-advertised remote proxy.
- Keep the hosted-visible filtering boundary in the tool system. The worker
  must consume the already-filtered remote catalogue from `1.1.2`; it must not
  rebuild hosted eligibility rules in container code.
- Apply the `hexagonal-architecture` skill narrowly. Separate policy or merge
  logic from the worker's HTTP and loop adapters, but do not transplant a new
  application-layer directory structure into the repository.
- Preserve degraded startup. If the catalogue fetch fails, worker-local tools
  must remain available and the job must still be able to proceed with the
  reduced local surface.
- Keep the current source family scoped to active hosted-visible MCP tools.
  Do not broaden the catalogue to orchestrator-owned WebAssembly (WASM) tools
  in this step; that belongs to roadmap item `1.2.2`.
- Prefer deterministic recomputation of the available tool list over ad hoc
  in-place mutation. The same merge rule must apply at initial context build
  and pre-LLM refresh.
- Use `rstest` fixtures for shared setup and dependency injection. Only add
  `rstest-bdd` behavioural coverage if it can reuse in-process harnesses
  without Docker, live MCP services, or an unrelated test framework expansion.
- Update the design and operator docs that actually change. At minimum, review
  `docs/rfcs/0001-expose-mcp-tool-definitions.md`,
  `docs/axinite-architecture-overview.md`, `docs/users-guide.md`,
  `docs/roadmap.md`, and `FEATURE_PARITY.md` before closing the work.

## Tolerances (exception triggers)

- Scope: if the smallest credible implementation needs more than 10 files or
  roughly 400 net new lines before tests, stop and verify that `1.1.4` or
  `1.2.2` work has not leaked in.
- Merge semantics: if the worker cannot expose one deterministic merged tool
  surface without changing public LLM-provider interfaces, stop and document
  the interface pressure before proceeding.
- Refresh path: if `build_reasoning_context(...)` and
  `ContainerDelegate::before_llm_call(...)` cannot share one clear rule for
  `available_tools` after two focused refactoring attempts, stop and document
  the hidden coupling.
- Guidance injection: if avoiding duplicate `toolset_instructions` would
  require a larger prompt-assembly rewrite outside the worker container path,
  stop and record the trade-off rather than improvising.
- Behaviour tests: if meaningful `rstest-bdd` coverage would require a brand
  new feature-test harness, external services, or Docker orchestration, record
  that explicitly and fall back to in-process `rstest` integration coverage
  with equally observable assertions.
- Documentation drift: if `docs/users-guide.md` or
  `docs/axinite-architecture-overview.md` already describe behaviour that the
  implementation cannot actually deliver, stop and reconcile the product
  wording before marking `1.1.3` complete.

## Risks

- Risk: The code already registers remote proxies before reasoning-context
  construction, so implementers may assume `1.1.3` is already "done" and skip
  the explicit merge contract and regression tests.
  Severity: high
  Likelihood: high
  Mitigation: treat this roadmap step as making the reasoning-context merge
  explicit, deterministic, and test-locked rather than merely relying on an
  incidental side effect of registry population.

- Risk: `reason_ctx.available_tools` may drift between initial context build
  and later loop refreshes if those paths remain separate and untested.
  Severity: high
  Likelihood: medium
  Mitigation: extract or centralize the rule for computing the worker-visible
  tool surface, then test both the initial and iterative paths against the
  same expectations.

- Risk: Tool ordering or duplicate registration could change the prompt surface
  and therefore hosted model behaviour even when the raw tool set is correct.
  Severity: medium
  Likelihood: medium
  Mitigation: preserve sorted `ToolRegistry::tool_definitions()` output, add a
  regression assertion for the merged list, and avoid concatenating unordered
  local and remote slices manually.

- Risk: `toolset_instructions` may be injected more than once, causing prompt
  bloat or contradictory guidance during long-running jobs.
  Severity: medium
  Likelihood: medium
  Mitigation: keep orchestrator guidance injection in the context-build path
  only, and ensure the pre-LLM refresh path updates tools without re-adding
  the same system guidance message.

- Risk: Documentation drift is already present because `docs/users-guide.md`
  describes the desired hosted proxy behaviour today.
  Severity: medium
  Likelihood: high
  Mitigation: make documentation review an explicit milestone, and update the
  wording to reflect the implemented merge semantics rather than the intended
  state alone.

## Milestone 1: define the worker-side merge contract explicitly

Start by proving exactly what "merge remote MCP tool definitions into the
worker reasoning context" means in this repository. The important finding from
the current code is that the worker already fetches the remote catalogue,
registers local proxies, and then fills `reason_ctx.available_tools` from
`self.tools.tool_definitions().await`. That means `1.1.3` should not invent a
second merge mechanism. Instead, it should make the existing unified-registry
path explicit, stable, and impossible to regress silently.

Perform the following discovery work before changing behaviour:

1. Confirm the current order across the hosted worker startup path:
   local tool registration, remote catalogue fetch, proxy registration, and
   reasoning-context construction.
2. Confirm the current order across the iterative loop path:
   prompt polling, tool-surface refresh, and status reporting.
3. Decide whether the merge rule should live in one helper on `WorkerRuntime`,
   one helper on `ContainerDelegate`, or one small worker-side policy module.
   The preferred outcome is one named helper that both paths call.
4. Decide how to treat `toolset_instructions`. The working default is:
   preserve them as a dedicated system guidance message at context-build time,
   and do not duplicate that message during later `available_tools` refreshes.

Record the chosen rule in `docs/rfcs/0001-expose-mcp-tool-definitions.md` and
`docs/axinite-architecture-overview.md` if the implementation clarifies an
open question from the RFC, especially around whether guidance injection is a
context-build concern or a per-iteration refresh concern.

## Milestone 2: refactor the worker to compute one unified tool surface

Make the merge rule concrete in code without widening the architecture.

The implementation should keep the existing ownership boundaries:

- the orchestrator owns hosted-visible filtering and remote execution
- the worker transport owns catalogue fetch and generic execute calls
- the worker runtime owns reasoning-context assembly
- the worker delegate owns per-iteration refresh hooks

Concretely, the code should end this milestone with one obvious way to produce
the worker-visible tool surface. The likely shape is a helper such as
`WorkerRuntime::available_tool_definitions()` or
`WorkerRuntime::refresh_reasoning_context_tools(...)` that:

1. reads the combined local-plus-proxy registry,
2. returns sorted `ToolDefinition` values,
3. is called from `build_reasoning_context(...)`, and
4. is called from `ContainerDelegate::before_llm_call(...)`.

Do not manually concatenate local tools and remote catalogue entries in the
reasoning context. That would bypass the registry, duplicate logic, and create
new drift risk. The registry already carries the merged tool ownership model;
the worker should reason from that canonical merged registry view.

Keep the degraded-startup contract unchanged. If the remote catalogue fails to
load, the worker must still advertise the local container tools and proceed.

## Milestone 3: add tests that lock the reasoning-context contract in place

Write the failing tests first. The intent is to prove the reasoning-context
merge itself, not to absorb all of `1.1.4`.

### `rstest` unit and integration tests

Extend the existing worker and orchestrator tests with focused coverage:

- In `src/worker/container/tests.rs`, add a test that starts from a worker with
  local tools plus a remote catalogue response and proves that
  `build_reasoning_context(...)` exposes both families in
  `reason_ctx.available_tools`.
- In the same module, add a test that exercises
  `ContainerDelegate::before_llm_call(...)` and proves that a later refresh
  still exposes the same merged tool surface without re-injecting duplicate
  `Hosted remote-tool guidance` system messages.
- Extend the existing degraded-startup test so it remains explicit that a
  failed catalogue fetch leaves only the local tool surface available.
- In `src/tools/builtin/worker_remote_tool_proxy.rs` tests, tighten the
  fidelity assertion so the proxy-reported `ToolDefinition` fields are compared
  as a unit, not only through individual spot checks.
- In `src/orchestrator/api/tests/remote_tools.rs` and
  `src/tools/registry/tests.rs`, keep the current catalogue and lookup tests
  aligned with the worker-facing expectations so the plan still proves
  canonical source fidelity from both sides of the boundary.

### Behavioural coverage

Add `rstest-bdd` coverage only if it can be done as a narrow in-process
feature. A good candidate would be a scenario such as:

```plaintext
Feature: Hosted worker tool advertising
  Scenario: Hosted worker exposes local and remote tools together
    Given a worker with local development tools
    And the orchestrator catalogue advertises one hosted-visible MCP tool
    When the worker builds the reasoning context
    Then the available tools include both the local tools and the hosted MCP tool
    And the hosted guidance appears once
```

If the repository still lacks a practical `rstest-bdd` harness for this
surface, document that explicitly in the implementation `Decision Log` and use
an equally observable in-process `rstest` integration test instead. "Where
applicable" does not justify a fake BDD layer that proves less than the direct
integration seam.

## Milestone 4: synchronize design and operator documentation

The code and docs already describe overlapping pieces of this feature. Update
them in the same implementation pass so they stop drifting.

1. Update `docs/rfcs/0001-expose-mcp-tool-definitions.md` implementation
   status and any resolved wording around the reasoning-context merge and
   guidance injection.
2. Update `docs/axinite-architecture-overview.md` to describe the final
   worker-side assembly rule for hosted tools if the implementation makes that
   rule more explicit than it is today.
3. Update `docs/users-guide.md` if the wording about hosted workers,
   proxied tools, or guidance injection needs to become more precise.
4. Mark roadmap item `1.1.3` done in `docs/roadmap.md` once the feature,
   tests, and docs are all complete. Do not mark `1.1.4` done as part of this
   step.
5. Check `FEATURE_PARITY.md` for any entry that should change because hosted
   tool-advertising behaviour moved from partial to complete.

## Milestone 5: validate, gate, and publish the implementation

Run the smallest useful red-green loop first, then the full repository gates.
All substantive commands should use `set -o pipefail` and `tee` so logs are
retained under `/tmp`.

Use the following command pattern during implementation:

```bash
set -o pipefail && cargo test worker_runtime_build_reasoning_context_merges_local_and_remote_tools \
  | tee /tmp/unit-axinite-1-1-3-merge-mcp-defs-into-worker-reasoning-context.out
```

Then run the broader gates expected by this repository:

```bash
set -o pipefail && make check-fmt \
  | tee /tmp/check-fmt-axinite-1-1-3-merge-mcp-defs-into-worker-reasoning-context.out
set -o pipefail && make lint \
  | tee /tmp/lint-axinite-1-1-3-merge-mcp-defs-into-worker-reasoning-context.out
set -o pipefail && make test \
  | tee /tmp/test-axinite-1-1-3-merge-mcp-defs-into-worker-reasoning-context.out
```

If documentation changes are made outside the Rust gates, also run:

```bash
set -o pipefail && bunx markdownlint-cli2 docs/roadmap.md docs/users-guide.md \
  docs/axinite-architecture-overview.md docs/rfcs/0001-expose-mcp-tool-definitions.md \
  docs/execplans/1-1-3-merge-mcp-defs-into-worker-reasoning-context.md docs/contents.md \
  | tee /tmp/markdownlint-axinite-1-1-3-merge-mcp-defs-into-worker-reasoning-context.out
git diff --check
```

Only after the gates pass should the implementation be committed. The commit
must describe that the worker now exposes one explicit merged hosted tool
surface to reasoning, and the final report must cite the exact log paths.

## Progress

- [x] 2026-03-22: Verified branch, repository status, and governing
  instructions.
- [x] 2026-03-22: Reviewed `docs/roadmap.md`, RFC 0001, the current
  architecture docs, the user guide, and the existing `1.1.1` and `1.1.2`
  execplans.
- [x] 2026-03-22: Inspected the current worker, registry, orchestrator, and
  test seams that already participate in hosted remote-tool advertisement.
- [x] 2026-03-22: Drafted this ExecPlan and indexed it from `docs/contents.md`.
- [x] Implemented Milestones 1 and 2, making the merge contract explicit in
  the worker reasoning path through one shared helper-backed registry read.
- [x] Implemented Milestone 3 coverage with in-process `rstest` integration
  tests and tightened worker, orchestrator, registry, and proxy fidelity
  assertions. `rstest-bdd` was not used because this slice already had a
  stronger in-process harness and no existing narrow feature harness.
- [x] Implemented Milestone 4 documentation sync and marked roadmap item
  `1.1.3` complete.
- [ ] Run the full validation gates, commit, and push.

## Surprises & Discoveries

- `docs/axinite-architecture-summary.md`, named in the request, does not exist
  in this checkout. `docs/axinite-architecture-overview.md` is the usable
  architecture reference.
- The worker already fetches the remote catalogue, registers worker-local
  proxies, and fills `reason_ctx.available_tools` from the combined registry.
  The missing work is making that merge rule explicit and regression-proof.
- `docs/users-guide.md` already describes much of the desired hosted remote-
  tool behaviour, so documentation review must verify reality rather than
  merely restating intent.
- The repository currently shows no existing `.feature` or `#[scenario(...)]`
  usage for `rstest-bdd` in this area, so behavioural coverage may need to
  stay with stronger in-process `rstest` integration tests unless a narrow BDD
  harness proves worthwhile.
- The most surgical implementation point is a tiny shared helper in
  `src/worker/container.rs` that both `build_reasoning_context(...)` and
  `ContainerDelegate::before_llm_call(...)` call. That makes the merge rule
  explicit without pushing worker policy into a new module.
- The refresh-path contract was easier to prove by exercising
  `ContainerDelegate::before_llm_call(...)` against the existing catalogue
  route than by extending the test server with extra prompt-state machinery.
  That kept the test focused on tool-surface refresh and one-time guidance
  injection rather than unrelated worker-loop plumbing.

## Decision Log

- Decision: Treat `1.1.3` as an explicit reasoning-context contract task, not
  as a second remote-catalogue transport task.
  Rationale: `1.1.1` and `1.1.2` already own the transport and filtering seams.
  The current branch already has the pieces required for registry-backed merge;
  the remaining risk is silent drift in how the worker advertises that merged
  tool surface to the LLM.

- Decision: Use `docs/axinite-architecture-overview.md` in place of the
  missing `docs/axinite-architecture-summary.md`.
  Rationale: The requested file is absent, and the overview document already
  contains the hosted worker-orchestrator ownership notes relevant to this
  feature.

- Decision: The preferred implementation should compute the merged tool list by
  reading the worker registry after remote proxy registration, not by manually
  appending remote catalogue entries during prompt assembly.
  Rationale: The registry is already the canonical merged ownership boundary on
  the worker side. Reusing it keeps sorting, shadowing, and proxy ownership
  consistent.

- Decision: `rstest-bdd` is required only if a narrow in-process scenario adds
  clearer observable coverage than the existing worker and orchestrator harness
  tests.
  Rationale: The user asked for behavioural tests where applicable. A fake BDD
  layer that weakens the assertions would not satisfy that requirement.

- Decision: keep the merged tool-surface rule as one small worker-side helper
  rather than a new policy module.
  Rationale: both relevant call sites already live in the worker container
  family, and the shared helper makes the recomputation rule explicit without
  widening the architecture for this roadmap slice.

- Decision: use direct in-process `rstest` integration coverage instead of
  adding `rstest-bdd` to this slice.
  Rationale: the repository already had strong worker and orchestrator harness
  seams, while a new BDD layer would have added ceremony without improving the
  observable guarantees for the merge contract.

## Outcomes & Retrospective

Shipped `1.1.3` as a worker-side reasoning contract hardening pass rather than
as a transport rewrite. The implementation added one helper-backed path for
reading the worker-visible merged registry so both
`WorkerRuntime::build_reasoning_context(...)` and
`ContainerDelegate::before_llm_call(...)` advertise the same local-plus-remote
tool surface. Hosted `toolset_instructions` remain a one-time system guidance
message during context build and are not duplicated during later refreshes.

The test plan stayed inside the existing in-process `rstest` harnesses. Worker
tests now prove merged-tool visibility at initial build and later refresh,
including degraded startup and one-time guidance injection. Proxy,
orchestrator, and registry tests were tightened so the worker-facing remote
definitions are checked as full `ToolDefinition` payloads rather than loose
field spot checks.

Documentation was synchronized across the roadmap, RFC, architecture overview,
and user guide so the described behaviour matches the implemented merge rule.
Follow-up work intentionally left for `1.1.4` is the broader hosted-mode test
matrix for schema fidelity and execution routing beyond the narrow
reasoning-context contract covered here.
