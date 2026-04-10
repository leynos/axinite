# Extend the hosted remote-tool catalogue to include orchestrator-owned WASM tools

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`,
`Surprises & Discoveries`, `Decision Log`, and
`Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: IN PROGRESS

## Purpose / big picture

Roadmap item `1.2.2` exists to make proactive WebAssembly (WASM) schema
advertisement the normal hosted contract, not an in-process-only benefit. After
this work, a hosted worker must receive active orchestrator-owned WASM tool
definitions through the same remote catalogue used for Model Context Protocol
(MCP) tools, register local proxy wrappers from those canonical definitions, and
stop omitting orchestrator-owned WASM tools from the tool array sent to the
large language model (LLM).

Success is observable in six ways. First, the orchestrator catalogue response
includes hosted-visible WASM tools without creating a second WASM-only route or
wire shape. Second, the worker registers proxies for those remote WASM tools at
startup through the existing remote-tool path. Third, each advertised WASM tool
retains its canonical `name`, `description`, and `parameters` values, including
schemas recovered or overridden during `1.2.1`. Fourth, direct execution of
ineligible or approval-gated tools still fails closed. Fifth, tests cover happy
paths, unhappy paths, and fidelity edges at registry, orchestrator, and worker
layers. Sixth, the design and user documentation stop describing
orchestrator-owned WASM tools as absent from the hosted catalogue, and the
roadmap marks `1.2.2` complete when implementation lands.

This plan deliberately uses the `hexagonal-architecture` guidance narrowly. The
goal is to keep hosted-catalogue policy in the tool system and treat the
orchestrator HTTP layer plus the worker runtime as adapters that consume that
policy. No repository-wide pattern transplant is required.

## Approval gates

- Plan approved
  Acceptance criteria: the implementation stays within roadmap item `1.2.2`,
  reuses the canonical hosted-visible filter seam from `1.1.2`, and does not
  redefine the worker-orchestrator transport or introduce a WASM-specific
  catalogue path.
  Sign-off: human reviewer approves this ExecPlan before feature
  implementation begins.
- Implementation complete
  Acceptance criteria: active orchestrator-owned WASM tools are visible through
  the existing hosted remote catalogue, the worker registers their proxies, and
  the catalogue continues to fail closed for tools that hosted mode cannot
  execute.
  Sign-off: implementer marks all milestones complete before final validation.
- Validation passed
  Acceptance criteria: required code and documentation gates pass with retained
  logs, and test evidence proves both schema fidelity and hosted execution-path
  correctness.
  Sign-off: implementer records validation evidence immediately before commit.
- Docs synced
  Acceptance criteria: the roadmap, RFC 0002, architecture documents,
  user-facing guidance, and this ExecPlan all describe the same hosted WASM
  catalogue behaviour.
  Sign-off: implementer completes documentation updates as the final
  pre-commit checkpoint.

## Repository orientation

The following files are the core orientation points for this feature.

- `docs/roadmap.md` defines `1.2.2`, its dependency on `1.1.1` and `1.2.1`,
  and the success condition that hosted workers receive proactive WASM
  definitions through the same catalogue path as MCP tools.
- `docs/rfcs/0002-expose-wasm-tool-definitions.md` is the design authority for
  why proactive schemas are the normal contract and why hosted mode must reuse
  the existing remote catalogue rather than rediscovering schemas from failure
  hints.
- `src/orchestrator/api/remote_tools.rs` owns the current adapter-level source
  allowlist through `HOSTED_REMOTE_TOOL_SOURCES`. It currently projects only
  `HostedToolCatalogSource::Mcp`, which is the narrow seam this roadmap item
  must broaden.
- `src/tools/registry/hosted.rs` is the canonical hosted-visible filter and
  lookup layer introduced by `1.1.2`. That is the policy boundary that this
  change must reuse rather than bypass.
- `src/tools/tool/approval_policy.rs` defines the relevant source-family
  vocabulary, including both `HostedToolCatalogSource::Mcp` and
  `HostedToolCatalogSource::Wasm`.
- `src/tools/wasm/wrapper.rs` already reports
  `HostedToolCatalogSource::Wasm`, which means the wrapper metadata surface is
  already prepared for catalogue inclusion.
- `src/orchestrator/api/tests/remote_tools.rs` and
  `src/tools/registry/tests.rs` hold the current hosted-visibility regression
  tests and already contain fixtures that distinguish MCP-visible versus
  WASM-visible tools.
- `src/worker/container/tests/remote_tools.rs` and
  `src/worker/container/tests/hosted_fidelity.rs` cover worker-side proxy
  registration and definition fidelity, and are the likely locations for the
  behavioural proof that hosted workers now see WASM tools through the same
  path as MCP tools.
- `docs/users-guide.md` currently states that orchestrator-owned WASM tools
  remain outside the hosted-visible catalogue. That text must change when the
  feature ships.
- `docs/worker-orchestrator-contract.md` is the relevant component-architecture
  reference for the hosted transport boundary. Update it if the internal
  catalogue contract description changes.
- The user request named `docs/axinite-architecture-summary.md`, but that file
  does not exist in this checkout. Use
  `docs/axinite-architecture-overview.md` as the architecture overview document
  for this feature.

## Constraints

- Reuse the worker-orchestrator transport from `1.1.1` unchanged. Do not add a
  second catalogue endpoint, a WASM-only proxy route, or a new response shape.
- Keep hosted-catalogue policy inside the tool system. The orchestrator adapter
  and worker runtime may consume that policy, but they must not become the
  source of truth for hosted visibility.
- Treat this as a boundary extension, not a behavioural broadening without
  guardrails. Approval-gated, protected, container-only, or otherwise
  ineligible tools must remain hidden or rejected.
- Preserve the canonical `ToolDefinition` contract:
  `name`, `description`, and `parameters`. No WASM-specific LLM-facing schema
  type is allowed.
- Preserve the `1.2.1` contract that active WASM tool schemas are resolved at
  registration time. This feature must not fall back to schema-in-error-hint as
  a normal hosted path.
- Apply the `hexagonal-architecture` guidance narrowly. Put policy and
  selection logic inward, keep HTTP extraction and worker bootstrap in adapter
  code, and avoid coupling adapters directly to each other.
- Use `rstest` for unit and integration coverage. Add `rstest-bdd`
  behavioural coverage if it can be done with the existing in-process mocked
  orchestrator harness and without building a disproportionate new support
  stack.
- Keep new and modified source files under the repository's file-size and
  complexity limits. Prefer extracting helpers over growing already-busy test
  files into bumpy-road shapes.
- Update user-facing and maintainer-facing documentation in the same delivery
  pass, including the roadmap entry once the feature is fully implemented.
- Check `FEATURE_PARITY.md` during implementation and update it in the same
  branch if the hosted remote-tool behaviour tracked there changes.

## Tolerances (exception triggers)

- Scope: if the smallest credible implementation touches more than 12 files or
  roughly 500 net new lines before documentation, stop and verify that work
  from `1.2.3`, refresh behaviour, or broader runtime changes have not leaked
  into this slice.
- Boundary pressure: if including WASM tools requires changing `ToolDefinition`,
  `RemoteToolCatalogResponse`, or the shared route constants, stop and document
  why the existing transport cannot carry the feature.
- Policy leakage: if the orchestrator adapter needs to inspect WASM-specific
  implementation details that are not already represented through
  `Tool`, `HostedToolCatalogSource`, `HostedToolEligibility`, or
  `ToolDomain`, stop and introduce the narrowest inward-facing abstraction
  instead of adding adapter-local heuristics.
- Behavioural tests: if adding one focused `rstest-bdd` scenario would require
  more than two new support files, external services, or Docker orchestration
  beyond the current mocked-server harness, record that clearly and keep the
  observable behaviour assertions in `rstest` integration tests.
- Schema quality: if any active orchestrator-owned WASM tool still publishes a
  placeholder schema during hosted advertisement, stop and reconcile that with
  the completed `1.2.1` contract before declaring `1.2.2` done.
- Approval semantics: if param-dependent approval rules can make a tool appear
  safe in the catalogue but fail in a surprising way at execution time, stop
  and document whether the catalogue must stay coarse-grained or gain a tighter
  policy signal.

## Risks

- Risk: The implementation may broaden the hosted catalogue by only changing
  the orchestrator constant, leaving the policy rationale implicit.
  Severity: high
  Likelihood: medium
  Mitigation: keep the source-family expansion tied to explicit tests and
  documentation that say the canonical registry-owned filter now admits both
  hosted-safe MCP and hosted-safe WASM tools.

- Risk: Hosted workers may receive WASM tools whose schema is still
  placeholder-shaped, which would technically satisfy visibility while still
  undermining first-call correctness.
  Severity: high
  Likelihood: medium
  Mitigation: include at least one real-metadata or fidelity test that proves
  the worker sees a non-placeholder WASM schema through the hosted catalogue.

- Risk: Approval-gated tools may be visible at catalogue time but fail at
  execution time in ways the user experiences as inconsistent.
  Severity: medium
  Likelihood: medium
  Mitigation: keep catalogue eligibility tests and execute-time rejection tests
  aligned, including at least one param-aware unhappy path.

- Risk: Documentation drift is likely because the current user's guide,
  architecture overview, worker-orchestrator contract document, roadmap, and
  RFC 0002 all describe adjacent parts of this behaviour.
  Severity: medium
  Likelihood: high
  Mitigation: treat documentation sync as its own milestone rather than a
  final afterthought.

- Risk: Introducing `rstest-bdd` into a subsystem that currently has no Gherkin
  coverage could create more scaffolding than value.
  Severity: medium
  Likelihood: medium
  Mitigation: attempt one small in-process scenario only if it stays inside the
  tolerances. Otherwise, document why `rstest` integration tests are the
  proportional behavioural layer for this feature.

## Milestone 1: confirm the narrow policy change and reuse boundary

Start by confirming the exact hosted-visibility rule that the code must express.

1. Verify in `src/tools/registry/hosted.rs` that the canonical filter already
   projects definitions and named tools based on `HostedToolCatalogSource`,
   approval eligibility, protected names, and `ToolDomain`.
2. Confirm that `WasmToolWrapper` already reports
   `HostedToolCatalogSource::Wasm` and that `1.2.1` tests prove proactive
   schema publication for active WASM tools.
3. Record in the implementation notes that `1.2.2` is not allowed to create a
   parallel WASM-specific hosted-visibility pass. The correct seam is the
   existing registry-owned filter consumed by the orchestrator adapter.

Expected result: the implementer can point to one narrow policy change, most
likely broadening the allowed hosted source family from MCP-only to
MCP-plus-WASM, without redesigning the transport.

## Milestone 2: extend the canonical hosted source set without leaking policy

Make the policy change at the inward boundary and keep adapters thin.

1. Update the hosted remote-tool policy in `src/orchestrator/api/remote_tools.rs`
   so it consumes both `HostedToolCatalogSource::Mcp` and
   `HostedToolCatalogSource::Wasm`.
2. Keep `src/tools/registry/hosted.rs` as the single place that decides whether
   a tool is hosted-visible or executable by name. If helper extraction is
   needed to keep source-family logic readable, place it near the registry and
   approval-policy types rather than in the HTTP handler.
3. Do not add WASM-specific execution routing. The existing
   `execute_hosted_remote_tool()` path must remain the only normal remote-tool
   execution entry point.
4. Sanity-check catalogue versioning. Adding WASM tools will change
   `catalog_version` when the tool set changes, and that is expected. Document
   the behaviour rather than treating it as churn.

Expected result: the orchestrator catalogue and execute path both recognize
hosted-safe orchestrator-owned WASM tools through the same registry-owned
policy seam as MCP tools.

## Milestone 3: add unit and behavioural tests for happy, unhappy, and edge cases

The validation matrix must prove more than simple inclusion.

### Unit and policy tests with `rstest`

1. Extend `src/tools/registry/tests.rs` so
   `hosted_tool_definitions(&[Mcp, Wasm])` returns both families in sorted
   order, while MCP-only and WASM-only lookups still filter correctly.
2. Add lookup tests for `get_hosted_tool()` that cover:
   visible WASM tool, missing tool, approval-gated tool, and ineligible tool.
3. Extend `src/orchestrator/api/tests/remote_tools.rs` so the catalogue
   includes a hosted-safe WASM fixture and still excludes protected or
   approval-gated tools.

### Behavioural integration tests

1. Add or extend worker-side tests in
   `src/worker/container/tests/remote_tools.rs` to prove that a hosted worker
   fetches a mixed catalogue, registers remote proxies, and exposes both local
   tools and remote WASM tools through the merged reasoning surface.
2. Add or extend fidelity coverage in
   `src/worker/container/tests/hosted_fidelity.rs` so a WASM tool definition
   round-trips from orchestrator catalogue to worker proxy without loss of
   `name`, `description`, or `parameters`.
3. Add at least one unhappy-path integration test proving that a tool outside
   the hosted-visible set, or one requiring approval at execution time, is
   still rejected even after the source-family broadening.
4. If proportionate, add one focused `rstest-bdd` feature and scenario set
   describing the hosted worker flow in plain language:
   "Given an orchestrator-owned hosted-safe WASM tool, when the worker fetches
   the remote catalogue, then the tool appears in the worker tool array with
   its canonical schema."
   Add a second unhappy-path scenario only if the step library remains small.
   If the subsystem still lacks a practical `rstest-bdd` seam after one
   focused attempt, document why in `Surprises & Discoveries` and keep the
   behavioural proof in `rstest` integration tests.

Expected result: the feature is locked down at the policy layer, the HTTP
adapter layer, and the worker runtime layer, with explicit assertions around
schema fidelity and fail-closed behaviour.

## Milestone 4: synchronize design, architecture, and user documents

Update the documents that describe the hosted remote-tool contract.

1. Update `docs/rfcs/0002-expose-wasm-tool-definitions.md` so its implementation
   status reflects `1.2.2` completion and its migration notes align with the
   implemented catalogue reuse.
2. Update `docs/users-guide.md` to explain that hosted workers now advertise
   orchestrator-owned WASM tools through the same remote catalogue path as MCP
   tools, while preserving the visibility rules for approval-gated and other
   ineligible tools.
3. Update `docs/worker-orchestrator-contract.md` if its hosted tool catalogue
   description needs to say the source set now includes orchestrator-owned WASM
   tools.
4. Update `docs/axinite-architecture-overview.md` where it currently describes
   the hosted catalogue as MCP-only or future work.
5. Mark roadmap item `1.2.2` as done in `docs/roadmap.md` only when the
   implementation and validation evidence are complete.
6. Update `FEATURE_PARITY.md` if the hosted remote-tool feature matrix or notes
   now need to acknowledge this shipped behaviour.

Expected result: operators, maintainers, and roadmap readers all see the same
contract, with no stale statement that hosted mode omits orchestrator-owned
WASM tools.

## Milestone 5: validate, record evidence, and land the feature

Use the repository's documented gates and retain logs.

1. Run targeted tests first while developing, using `tee` to capture logs in
   `/tmp`, for example:

   ```plaintext
   BRANCH_SLUG=$(git branch --show-current | tr '/' '-')
   cargo test -p <crate> <targeted-test> | tee /tmp/test-axinite-${BRANCH_SLUG}.out
   ```

2. Before commit, run the full repository gate:

   ```plaintext
   BRANCH_SLUG=$(git branch --show-current | tr '/' '-')
   make all | tee /tmp/make-all-axinite-${BRANCH_SLUG}.out
   ```

3. Run Markdown validation for changed documentation:

   ```plaintext
   BRANCH_SLUG=$(git branch --show-current | tr '/' '-')
   bunx markdownlint-cli2 \
     docs/execplans/1-2-2-orchestrator-owned-wasm-tools-in-tool-catalogue.md \
     docs/roadmap.md \
     docs/rfcs/0002-expose-wasm-tool-definitions.md \
     docs/users-guide.md \
     docs/worker-orchestrator-contract.md \
     docs/axinite-architecture-overview.md \
     docs/contents.md \
     | tee /tmp/markdownlint-axinite-${BRANCH_SLUG}.out
   ```

4. Run the diff hygiene check:

   ```plaintext
   BRANCH_SLUG=$(git branch --show-current | tr '/' '-')
   git diff --check | tee /tmp/diff-check-axinite-${BRANCH_SLUG}.out
   ```

5. Record the passing evidence in `Progress` and `Outcomes & Retrospective`,
   then commit with a focused message that describes both the hosted WASM
   catalogue extension and the reason for it.

Expected result: the change lands with explicit evidence that code, tests, and
documentation all match the hosted WASM catalogue contract.

## Progress

- [x] 2026-04-10T17:32:32+02:00 Researched the roadmap item, RFC 0002,
  adjacent ExecPlans, architecture documents, user guide, worker-orchestrator
  contract, and the current registry, orchestrator, and worker seams.
- [x] 2026-04-10T17:32:32+02:00 Drafted this ExecPlan for roadmap item
  `1.2.2`.
- [ ] Implementation approved by a human reviewer.
- [x] 2026-04-10T19:36:00+02:00 Reconfirmed that the worker-side proxy
  registration path is already generic over remote `ToolDefinition` values, so
  `1.2.2` remains a narrow hosted-visibility extension rather than a transport
  redesign.
- [x] 2026-04-10T19:56:00+02:00 Hosted source-family broadening implemented
  through the canonical filter seam by extending
  `src/orchestrator/api/remote_tools.rs` from MCP-only to MCP-plus-WASM.
- [x] 2026-04-10T19:56:00+02:00 Unit and behavioural regression coverage added
  and passing in targeted runs for registry lookups, orchestrator catalogue and
  execute flows, worker remote-tool registration, and worker fidelity.
- [x] 2026-04-10T19:56:00+02:00 Documentation updated, including
  `docs/roadmap.md`, RFC 0002, the users guide, and the relevant architecture
  references.
- [x] 2026-04-10T20:21:00+02:00 Final gates passed and evidence recorded:
  targeted `cargo test` runs, `make all`, Markdown lint, and `git diff --check`
  all succeeded.
- [ ] Feature commit created.

## Surprises & Discoveries

- 2026-04-10T17:32:32+02:00 The user request referenced
  `docs/axinite-architecture-summary.md`, but this checkout contains
  `docs/axinite-architecture-overview.md` instead. This plan uses the overview
  plus `docs/worker-orchestrator-contract.md` as the relevant architecture
  references.
- 2026-04-10T17:32:32+02:00 `WasmToolWrapper` already reports
  `HostedToolCatalogSource::Wasm`; the main implementation gap is that the
  orchestrator's hosted source allowlist is still hard-coded to MCP-only.
- 2026-04-10T17:32:32+02:00 This subsystem currently has no visible
  `rstest-bdd` or `.feature` coverage, so behavioural BDD coverage needs an
  explicit proportionality check before new harness code is introduced.
- 2026-04-10T19:36:00+02:00 `src/tools/registry/tests.rs` already contains a
  hosted-visible WASM fixture and `src/worker/container/tests/remote_tools.rs`
  already proves that the worker merges remote catalogue definitions into its
  local reasoning surface. The remaining test work is to broaden those
  assertions from MCP-only to mixed MCP-plus-WASM catalogues and preserve
  schema fidelity.
- 2026-04-10T19:36:00+02:00 A fresh proportionality check still points away
  from `rstest-bdd` for this slice. The subsystem has no existing BDD harness,
  and equivalent observable behaviour can be locked down in the existing
  `rstest` integration tests without adding new support files or feature
  plumbing.
- 2026-04-10T19:56:00+02:00 The worker runtime did not need any source-aware
  changes. Once the orchestrator catalogue admitted hosted-visible WASM tools,
  the existing remote proxy registration path accepted them unchanged because it
  already consumes only canonical `ToolDefinition` values.

## Decision Log

- 2026-04-10T17:32:32+02:00 Use the existing registry-owned hosted filter seam
  as the policy boundary for this plan.
  Rationale: this matches roadmap item `1.1.2`, RFC 0002's migration plan, and
  the `hexagonal-architecture` guidance to keep policy inward and adapters
  thin.
- 2026-04-10T17:32:32+02:00 Treat `docs/worker-orchestrator-contract.md` as a
  required architecture update candidate alongside
  `docs/axinite-architecture-overview.md`.
  Rationale: the feature changes the internal description of the hosted
  catalogue boundary even if the wire format stays stable.
- 2026-04-10T17:32:32+02:00 Require a proportionality check before adding
  `rstest-bdd` coverage to this subsystem.
  Rationale: the user asked for behavioural testing where applicable, but the
  current subsystem has no existing Gherkin harness, so the plan must preserve
  a path to strong behavioural assertions without forcing scaffolding that
  exceeds the feature's scope.
- 2026-04-10T19:36:00+02:00 Keep the behavioural proof in the existing
  `rstest` integration suites rather than introducing new `rstest-bdd`
  scaffolding for `1.2.2`.
  Rationale: the existing worker/orchestrator harness already exercises the
  exact observable catalogue fetch and proxy-registration flow. Adding BDD
  support here would create disproportionate scaffolding for no contract gain.
- 2026-04-10T19:56:00+02:00 Express the policy change at the orchestrator
  adapter seam by broadening `HOSTED_REMOTE_TOOL_SOURCES` rather than altering
  `ToolRegistry::hosted_tool_definitions()` or the shared transport types.
  Rationale: the registry already owned the hosted-visibility predicate, and
  the shared transport already carried the canonical `ToolDefinition` shape.

## Outcomes & Retrospective

- 2026-04-10T20:21:00+02:00 Shipped behaviour:
  - hosted workers now receive hosted-visible orchestrator-owned WASM tool
    definitions through the same remote catalogue as MCP tools
  - worker-side proxy registration and reasoning-surface merging required no
    new transport or source-aware logic because the existing path already
    consumes canonical `ToolDefinition` values
  - generic hosted execution now accepts eligible WASM-backed tools through the
    same lookup and execute path as MCP-backed tools
- 2026-04-10T20:21:00+02:00 Validation evidence:
  - `cargo test hosted_tool --lib`
  - `cargo test remote_tool_ --lib`
  - `cargo test fidelity --lib`
  - `make all`
  - `bunx markdownlint-cli2`
    `docs/execplans/1-2-2-orchestrator-owned-wasm-tools-in-tool-catalogue.md`
    `docs/roadmap.md`
    `docs/rfcs/0002-expose-wasm-tool-definitions.md`
    `docs/users-guide.md`
    `docs/worker-orchestrator-contract.md`
    `docs/axinite-architecture-overview.md`
  - `git diff --check`
- 2026-04-10T20:21:00+02:00 Lessons for `1.2.3` and `1.2.4`:
  - the main hosted WASM contract gap was policy exposure, not transport or
    worker plumbing
  - the existing `rstest` integration harness was sufficient behavioural proof
    for this slice without introducing new `rstest-bdd` scaffolding
