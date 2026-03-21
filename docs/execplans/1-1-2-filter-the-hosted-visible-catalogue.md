# Filter the hosted-visible catalogue from the canonical `ToolRegistry`

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`,
`Surprises & Discoveries`, `Decision Log`, and
`Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: DRAFT

## Purpose / big picture

Roadmap item `1.1.2` exists to stop the hosted worker from advertising an
optimistic remote-tool surface. After this work, the hosted-visible catalogue
must come from one canonical `ToolRegistry`-owned filter that includes only
tools the orchestrator can really execute for hosted jobs. In practice, that
means active Model Context Protocol (MCP) tools whose live wrapper metadata is
still available and whose approval semantics are compatible with hosted mode.

Success is observable in five ways. First, the canonical hosted-visible
selection logic lives with the tool registry and policy code rather than in the
`axum` handler layer. Second, the orchestrator catalogue endpoint consumes that
canonical projection instead of walking `ToolRegistry::all()` directly. Third,
approval-gated, inactive, protected, container-only, or otherwise ineligible
tools are omitted from the catalogue rather than described to the model. Fourth,
execution continues to reject direct calls to tools outside that hosted-visible
set. Fifth, documentation and tests explain the rule precisely enough that later
WASM catalogue work (`1.2.2`) can reuse the same boundary instead of inventing a
parallel filter.

This plan is intentionally plan-only. It prepares implementation for roadmap
item `1.1.2` and must be approved before code changes begin.

## Repository orientation

The current transport from `1.1.1` is already in place, so this step should not
re-open the worker-orchestrator contract unless the existing seam proves
insufficient.

- `src/tools/registry/loader.rs` owns `ToolRegistry`, including
  `tool_definitions()`, `all()`, and `get()`. It is the current canonical
  source of active registered tools, but it does not yet expose a hosted
  catalogue projection.
- `src/tools/tool/approval_policy.rs` and `src/tools/tool/traits.rs` define the
  current policy vocabulary:
  `ApprovalRequirement`, `HostedToolEligibility`, `ToolDomain`, and the `Tool`
  trait hooks that wrappers override.
- `src/tools/mcp/client.rs` already marks approval-gated MCP wrappers as
  `HostedToolEligibility::ApprovalGated`, which is the strongest existing hint
  that hosted visibility policy belongs near tool metadata rather than in HTTP
  handlers.
- `src/orchestrator/api/remote_tools.rs` currently builds the hosted catalogue
  by iterating `ToolRegistry::all()` and filtering with a helper local to the
  orchestrator adapter. That is the core `1.1.2` gap: the filter is real, but
  it is not yet canonical.
- `src/worker/api/types.rs`, `src/worker/api.rs`, and `src/worker/container.rs`
  already own the shared transport, the worker HTTP adapter, and startup
  registration of remote proxies. Those files should consume the canonical
  filtered catalogue, not implement filtering themselves.
- `src/orchestrator/api/tests/remote_tools.rs`,
  `src/orchestrator/api/tests/remote_tools_param_aware.rs`, and
  `src/worker/container/tests.rs` already cover most of the current hosted
  catalogue path and should receive the first regression updates.
- `docs/rfcs/0001-expose-mcp-tool-definitions.md` is the design authority for
  this task. Its "Filter the catalogue to hosted-executable tools" section and
  migration plan must stay aligned with implementation.
- `docs/axinite-architecture-summary.md`, named in the user request, does not
  exist in this checkout. Use `docs/axinite-architecture-overview.md` as the
  current component-architecture reference instead.

## Constraints

- Keep the worker-orchestrator transport contract from `1.1.1` intact. This
  step is about canonical filtering, not a second transport redesign.
- Apply the `hexagonal-architecture` skill narrowly. The goal is to pull
  hosted-catalogue policy inward, away from HTTP adapters, without transplanting
  a new directory architecture across the whole repository.
- Treat the canonical hosted-visible selection as domain or policy logic owned
  by the tool system. `axum` handlers, router tests, and worker startup code may
  consume that policy, but must not become the source of truth for it.
- Preserve the existing `ToolDefinition` and `ToolOutput` contracts used by the
  worker, orchestrator, and language model provider code.
- Do not advertise tools that hosted mode cannot execute. Failing closed is
  mandatory; "described optimistically, then rejected later" is specifically the
  behaviour this roadmap item exists to remove.
- Keep future reuse in mind for roadmap item `1.2.2`. The canonical filter may
  start with MCP-focused rules, but it must not hard-code assumptions that make
  orchestrator-owned WebAssembly (WASM) tools impossible to add through the same
  seam later. The companion WASM schema-advertising design in
  `docs/rfcs/0002-expose-wasm-tool-definitions.md` must explicitly reference
  this canonical `1.1.2` seam so later work is required to reuse it rather than
  rebuilding hosted visibility logic elsewhere.
- Avoid widening approval semantics in hosted mode. If a tool currently needs an
  interactive approval flow, it must remain hidden until the product has a real
  hosted approval-grant path.
- Prefer small, explicit helper types or methods over embedding more branches in
  `src/orchestrator/api/remote_tools.rs`. File size and cognitive complexity
  must stay manageable.
- Use `rstest` for focused unit and adapter tests. Add `rstest-bdd` behavioural
  coverage only where it gives a clearer user-visible contract than another
  in-process `rstest` integration test.
- Update the relevant design and user documentation in the same implementation
  pass. At minimum, that means checking `docs/rfcs/0001-expose-mcp-tool-definitions.md`,
  `docs/axinite-architecture-overview.md`, `docs/users-guide.md`, and
  `docs/roadmap.md`.

## Tolerances (exception triggers)

- Scope: if the smallest credible implementation needs more than 10 files or
  roughly 400 net new lines before tests, stop and verify that `1.1.3` or
  `1.2.2` work has not been pulled in accidentally.
- Metadata: if `ToolRegistry` cannot distinguish the hosted-visible MCP subset
  without adding new source metadata to `Tool`, stop and document the minimum
  additional trait or registry metadata needed before proceeding.
- Contract: if the canonical registry filter needs to change the wire shape of
  `RemoteToolCatalogResponse`, stop and justify the transport change before
  touching worker startup.
- Behaviour tests: if adding `rstest-bdd` would require new external services,
  Docker orchestration, or a broad test harness unrelated to this feature,
  record that clearly and fall back to in-process `rstest` integration tests
  with equivalent observable assertions.
- Product mismatch: if the code or existing docs show that hosted workers must
  still advertise non-MCP orchestrator tools for supported workflows, stop and
  reconcile that with the roadmap wording before merging an implementation that
  narrows the catalogue.

## Risks

- Risk: Moving the filter into `ToolRegistry` may tempt the implementation to
  add HTTP-specific concepts to the registry.
  Severity: high
  Likelihood: medium
  Mitigation: keep the canonical output expressed in tool-system terms such as
  `ToolDefinition`, `HostedToolEligibility`, and "hosted executable", then let
  the orchestrator adapter translate that directly into the existing response
  type.

- Risk: The registry may not yet carry enough provenance to distinguish active
  MCP tools from other orchestrator-owned tools cleanly.
  Severity: high
  Likelihood: medium
  Mitigation: start by inventorying how MCP, extension-management, and other
  orchestrator tools are registered today. Prefer a minimal metadata addition
  owned by the tool layer over `Any` downcasts or name-prefix heuristics in the
  adapter.

- Risk: Approval-dependent tools with parameter-sensitive rules may still look
  globally eligible unless the filter and execution guard stay aligned.
  Severity: medium
  Likelihood: medium
  Mitigation: keep the catalogue predicate and execution-time approval check as
  two explicit layers: one coarse-grained visibility decision, one
  params-aware execution guard. Extend existing param-aware tests to prove the
  relationship.

- Risk: Documentation may drift because user-visible behaviour already appears
  in `docs/users-guide.md`, `docs/welcome-to-axinite.md`, the roadmap, and
  RFC 0001.
  Severity: medium
  Likelihood: high
  Mitigation: include a documentation sync pass in the implementation milestone
  rather than treating docs as a final clean-up step.

- Risk: The future `1.2.2` WASM catalogue work could be boxed in if `1.1.2`
  hard-codes "MCP only" too deeply into public method names.
  Severity: medium
  Likelihood: medium
  Mitigation: name internal helpers around "hosted visible" or "hosted remote
  catalogue" and put any MCP-specific rule inside the policy predicate or
  metadata lookup, not in transport names.

## Milestone 1: inventory the current registry and choose the canonical seam

Start by confirming what the registry knows today and what is missing.

1. List the current sources of orchestrator-owned tools that can appear in the
   remote catalogue:
   MCP wrappers, extension-management tools, other built-ins, and future WASM
   wrappers.
2. Verify which properties are already available without adapter reach-through:
   `ToolDomain`, `HostedToolEligibility`, `requires_approval(params)`, protected
   names, and any existing source metadata.
3. Decide the narrowest canonical API that lets the orchestrator answer both
   questions it needs:
   "What tool definitions may hosted workers advertise?" and
   "May this named tool execute for hosted mode?"

The preferred result is a `ToolRegistry`-owned method or small policy helper
near the registry, for example:

- `ToolRegistry::hosted_remote_catalog()` returning sorted
  `ToolDefinition` values plus any later metadata needed for versioning, or
- `ToolRegistry::iter_hosted_visible_tools()` paired with a
  `ToolRegistry::get_hosted_executable(name)` helper.

Do not commit to the method names above blindly. Use the smallest surface that
fits both the catalogue endpoint and the direct execute guard.

## Milestone 2: move hosted-visibility policy out of the adapter

Implement the canonical projection in the tool layer and have the orchestrator
consume it.

The intended boundary is:

- tool layer owns filtering rules
- orchestrator API owns HTTP extraction, authentication, and status mapping
- worker runtime remains a consumer of the wire response only

Concrete implementation expectations:

1. Introduce a dedicated tool-layer helper or registry method in
   `src/tools/registry/` rather than growing `src/orchestrator/api/remote_tools.rs`
   further.
2. Keep protected-name checks, `ToolDomain`, and hosted-eligibility decisions in
   one place.
3. If the filter must distinguish MCP-backed tools from other orchestrator tools,
   add that distinction through the tool layer, not through handler-local type
   tests or tool-name heuristics.
4. Keep execution-time guards aligned with catalogue visibility. A tool omitted
   from the hosted-visible catalogue must also fail closed if called directly.

This is where the hexagonal-architecture guidance matters most: a small inward
policy seam is enough. The repository does not need a full "application layer"
rewrite to gain the boundary we care about here.

## Milestone 3: keep the worker and transport stable while switching the source

Once the registry owns the hosted-visible projection, update the orchestrator
remote-tool adapter to consume it.

1. Replace the current `ToolRegistry::all()` walk in
   `src/orchestrator/api/remote_tools.rs` with the canonical registry method or
   policy helper.
2. Preserve response ordering, `catalog_version` stability, and
   `toolset_instructions` handling unless the chosen registry seam provides a
   better canonical source for those values.
3. Keep `src/worker/api/types.rs`, `src/worker/api.rs`, and
   `src/worker/container.rs` transport-compatible. `1.1.2` should change which
   tools appear in the catalogue, not how the worker fetches or registers them.
4. Preserve degraded startup behaviour in the worker: a catalogue-fetch failure
   still leaves container-local tools available.

If the canonical filter causes a visible change in which tools the worker sees,
document that precisely and use the same examples in tests and docs.

## Milestone 4: lock down the behaviour with tests

Write failing tests before code changes. Use the smallest test shapes that prove
the contract.

### Unit and focused integration tests with `rstest`

Update or extend:

- `src/orchestrator/api/tests/remote_tools.rs`
- `src/orchestrator/api/tests/remote_tools_param_aware.rs`
- `src/worker/container/tests.rs`
- `src/tools/registry/tests.rs` or a new neighbouring registry test module

Minimum cases:

1. Happy path: an active hosted-visible MCP tool appears in the canonical
   registry projection with unchanged `name`, `description`, and `parameters`.
2. Hidden path: approval-gated, protected, container-only, and otherwise
   ineligible tools are absent from the canonical projection.
3. Unhappy path: direct execution of a tool outside the hosted-visible set still
   fails closed with the correct status.
4. Param-aware path: a tool whose approval depends on params may remain
   catalogue-visible, but a dangerous invocation still fails at execution time.
5. Stability path: catalogue ordering and `catalog_version` remain deterministic
   across registration order changes.

### Behavioural coverage with `rstest-bdd` where it adds value

Add one focused in-process feature if it can be kept local to this behaviour.
The clearest candidate is:

- Feature: Hosted worker receives only executable remote tools
  - Scenario: active hosted-safe MCP tools are advertised, while approval-gated
    or unavailable tools are hidden

That scenario should:

1. Stand up the in-process orchestrator router with mixed tool fixtures.
2. Let a worker runtime fetch and register the remote catalogue.
3. Assert on the worker-visible tool list rather than on internal helper state.

If adding this one feature introduces disproportionate harness cost, record that
in the implementation's `Decision Log` and keep the observable assertions in a
plain `rstest` integration test instead.

## Milestone 5: synchronize design, architecture, user docs, and roadmap

Implementation is not complete until the docs say the same thing as the code.

1. Update `docs/rfcs/0001-expose-mcp-tool-definitions.md` if the final
   canonical-filter seam or MCP-specific rule is narrower or more explicit than
   the current RFC text.
2. Update `docs/rfcs/0002-expose-wasm-tool-definitions.md` so roadmap item
   `1.2.2` explicitly references the canonical hosted-visible filter introduced
   by `1.1.2`, not only the shared worker-orchestrator transport.
3. Update `docs/axinite-architecture-overview.md` to say that hosted-catalogue
   filtering now comes from the canonical tool registry or policy layer rather
   than from the orchestrator adapter.
4. Review `docs/users-guide.md` and update the visibility-rules section if the
   set of hosted-visible tools or the rationale for exclusions changed from the
   current text.
5. Add or refresh the `docs/contents.md` entry if new plan or architecture
   documents need to be indexed.
6. When the feature implementation is complete and validated, mark roadmap item
   `1.1.2` as done in `docs/roadmap.md`.

Do not mark the roadmap entry done during the plan phase.

## Validation and evidence

For the eventual implementation, capture gate output with `tee` and
`set -o pipefail` as required by repository guidance.

Expected validation sequence for code changes:

```bash
set -o pipefail
make check-fmt 2>&1 | tee /tmp/check-fmt-axinite-$(git branch --show).out
set -o pipefail
make test 2>&1 | tee /tmp/test-axinite-$(git branch --show).out
set -o pipefail
make typecheck 2>&1 | tee /tmp/typecheck-axinite-$(git branch --show).out
set -o pipefail
make lint 2>&1 | tee /tmp/lint-axinite-$(git branch --show).out
```

Expected validation sequence for docs changed during implementation:

```bash
set -o pipefail
bunx markdownlint-cli2 \
  docs/execplans/1-1-2-filter-the-hosted-visible-catalogue.md \
  docs/rfcs/0001-expose-mcp-tool-definitions.md \
  docs/axinite-architecture-overview.md \
  docs/users-guide.md \
  docs/roadmap.md \
  docs/contents.md \
  2>&1 | tee /tmp/markdownlint-axinite-$(git branch --show).out

git diff --check
```

When the implementation is ready, the final evidence bundle should include:

- the exact log paths for every gate run
- the final hosted-visible catalogue test names that prove pass/fail behaviour
- the roadmap diff showing `1.1.2` marked done
- the push output, including any remote-returned URLs if present

## Progress

- [x] 2026-03-21 09:27Z: Read roadmap item `1.1.2`, RFC 0001, the current
  architecture and testing references, and the completed `1.1.1` ExecPlan.
- [x] 2026-03-21 09:27Z: Confirmed that the current hosted catalogue filter
  still lives in `src/orchestrator/api/remote_tools.rs` and walks
  `ToolRegistry::all()` rather than consuming a registry-owned projection.
- [x] 2026-03-21 09:27Z: Confirmed that
  `docs/axinite-architecture-summary.md` is absent in this checkout and that
  `docs/axinite-architecture-overview.md` is the current architecture document
  to update instead.
- [x] 2026-03-21 09:27Z: Drafted this ExecPlan and indexed it from
  `docs/contents.md`.
- [ ] Await user approval before implementation.
- [ ] Implement the canonical hosted-visible registry filter, tests, and
  documentation updates.
- [ ] Run full gates for the implementation, commit, push, and mark roadmap item
  `1.1.2` done.

## Surprises & Discoveries

- The user request referenced `docs/axinite-architecture-summary.md`, but that
  file does not exist on this branch. The live architecture document is
  `docs/axinite-architecture-overview.md`.
- The current code already has two useful policy hooks on `Tool`:
  `hosted_tool_eligibility()` for coarse hosted visibility and
  `requires_approval(params)` for params-aware execution blocking. The gap is
  not missing policy vocabulary; it is missing a canonical registry-owned place
  to apply it.
- Worker startup already preserves degraded behaviour when catalogue fetch
  fails, so `1.1.2` can stay focused on selection correctness rather than
  bootstrap resilience.

## Decision Log

- 2026-03-21 09:27Z: Use `docs/axinite-architecture-overview.md` as the
  architecture-update target because the requested summary document is not
  present in this checkout.
- 2026-03-21 09:27Z: Keep the `1.1.1` transport seam intact and move only the
  hosted-catalogue policy inward. This is the smallest change that satisfies
  the roadmap while respecting the repository's existing boundaries.
- 2026-03-21 09:27Z: Treat catalogue visibility and execution-time approval as
  separate checks. The catalogue must fail closed by default, but params-aware
  approval still belongs in the execution path for tools whose safety depends on
  invocation data.

## Outcomes & Retrospective

Not started. Populate this section after implementation and validation,
including what shipped, what stayed deferred to `1.1.3` or `1.2.2`, and what
future maintainers should watch for when extending the hosted-visible
catalogue.
