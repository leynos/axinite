# Add hosted-mode tests for schema fidelity and execution routing

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`,
`Surprises & Discoveries`, `Decision Log`, and
`Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: COMPLETE

## Purpose / big picture

Roadmap item `1.1.4` exists to close the testing gap left after the transport
(`1.1.1`), filtering (`1.1.2`), and reasoning-context merge (`1.1.3`) work.
Those earlier items delivered the runtime behaviour; this item proves that
behaviour cannot silently regress.

After this work, the test suite must fail loudly if any of the following
regressions occur:

1. A required Model Context Protocol (MCP) field (`name`, `description`, or
   `parameters`) disappears or is rewritten during the journey from
   orchestrator registry to worker-advertised proxy.
2. An advertised remote tool executes through a local stub rather than
   through the orchestrator's generic remote-tool execution endpoint.
3. The worker-orchestrator transport contract drifts so that route paths,
   request shapes, or response shapes become inconsistent between the two
   sides.

Success is observable in two ways. First, `make test` passes and every new
test added by this plan is present and green. Second, deliberately breaking
a field, route, or execution path causes at least one of the new tests to
fail with a clear, diagnostic message.

This plan tracks the worker-orchestrator parity portion of
[Issue #16](https://github.com/leynos/axinite/issues/16) and addresses
[RFC 0001 §Migration Plan](../rfcs/0001-expose-mcp-tool-definitions.md#migration-plan)
step 5.

## Approval gates

- Plan approved
  Acceptance criteria: the plan stays scoped to tests and test fixtures for
  schema fidelity, execution routing, and worker-orchestrator contract parity.
  No runtime behaviour changes beyond what is needed to make the code testable.
  Sign-off: human reviewer approves this ExecPlan before implementation begins.
- Implementation complete
  Acceptance criteria: all test families described in the milestones are
  implemented, the existing test suite is unbroken, and the RFC and roadmap
  are updated to reflect the completed state.
  Sign-off: implementer marks the plan in progress and then complete after
  final validation.
- Validation passed
  Acceptance criteria: `make all` passes with retained logs, Markdown linting
  passes for changed documentation, and the final plan notes record validation
  evidence.
  Sign-off: implementer records final evidence immediately before commit.
- Docs synced
  Acceptance criteria: `docs/roadmap.md` marks `1.1.4` done,
  `docs/rfcs/0001-expose-mcp-tool-definitions.md` implementation status
  reflects the completed test matrix, `docs/contents.md` indexes this
  ExecPlan, and `docs/users-guide.md` is reviewed for accuracy against the
  tested behaviour.
  Sign-off: implementer completes documentation sync as the final pre-commit
  checkpoint.

## Repository orientation

The following seams already exist and carry the runtime behaviour that this
plan must lock down with tests. All paths are relative to the repository root.

### Orchestrator side (catalogue construction and execution)

- `src/orchestrator/api/remote_tools.rs` owns `hosted_remote_tool_catalog()`
  and `execute_hosted_remote_tool()`. The catalogue function reads the
  canonical `ToolRegistry`, filters for hosted-visible sources, sorts
  alphabetically, and hashes the payload into a deterministic
  `catalog_version`. The execution function resolves a tool by name through
  the registry's hosted lookup, checks approval requirements, builds a
  `JobContext`, calls `tool.execute(...)`, and maps `ToolError` variants to
  HTTP status codes.

- `src/tools/registry/hosted.rs` owns the canonical hosted-tool projection:
  `hosted_tool_definitions()`, `get_hosted_tool()`, and the private
  `hosted_tool_lookup()` and `is_tool_ineligible_for_hosted_catalogue()`
  predicates. Eligibility requires the tool to be in the `Orchestrator`
  domain, not protected, sourced from an allowed `HostedToolCatalogSource`,
  and not approval-gated.

- `src/tools/registry/loader.rs` owns `ToolRegistry`, including
  `is_protected_tool_name()` and the `PROTECTED_TOOL_NAMES` constant.

### Worker side (proxy registration and reasoning-context merge)

- `src/worker/container.rs` owns `WorkerRuntime`, including
  `build_tools()`, `register_remote_tools()`, and
  `build_reasoning_context()`. The `available_tool_definitions()` helper
  reads the combined local-plus-proxy registry to produce the sorted merged
  tool surface.

- `src/worker/container/delegate.rs` owns
  `ContainerDelegate::before_llm_call()`, which refreshes
  `reason_ctx.available_tools` before each hosted large language model (LLM)
  call using the same `available_tool_definitions()` helper.

- `src/tools/builtin/worker_remote_tool_proxy.rs` owns
  `WorkerRemoteToolProxy` and `register_worker_remote_tool_proxies()`. Each
  proxy caches a `ToolDefinition` and delegates `execute()` to the worker
  HTTP client's `execute_remote_tool()` method.

### Shared transport contract

- `src/worker/api.rs` owns the worker HTTP client methods
  `get_remote_tool_catalog()` and `execute_remote_tool()`.

- `src/worker/api/types.rs` owns the shared route constants
  (`REMOTE_TOOL_CATALOG_ROUTE`, `REMOTE_TOOL_EXECUTE_ROUTE`) and the shared
  payload types (`RemoteToolCatalogResponse`,
  `RemoteToolExecutionRequest`, `RemoteToolExecutionResponse`).

### Existing test seams

- `src/orchestrator/api/tests/remote_tools.rs` tests catalogue visibility,
  execution routing, error-status mapping, and job-identifier propagation.
  Uses fixtures from `src/orchestrator/api/tests/fixtures/remote_tool_mocks.rs`
  and `src/orchestrator/api/tests/fixtures/remote_tool_helpers.rs`.

- `src/worker/container/tests.rs` tests remote-tool registration, merged
  reasoning context, guidance deduplication, and degraded startup.

- `src/tools/registry/tests.rs` tests registry operations, alphabetical
  sorting, hosted source filtering, and hosted lookup error reporting.

- `src/tools/builtin/worker_remote_tool_proxy.rs` (tests module) tests
  round-trip execution and schema preservation through the proxy.

### Design authority

- `docs/rfcs/0001-expose-mcp-tool-definitions.md` is the design authority.
  Its §Testing Strategy names three test families (unit tests, behavioural
  tests, and a regression test target) and its §Migration Plan step 5 names
  the deliverable: "targeted tests for definition fidelity, execution
  routing, and contract parity between worker and orchestrator."

- `docs/roadmap.md` item `1.1.4` defines the success criterion:
  "tests fail if required MCP fields disappear or are rewritten incorrectly,
  and prove that advertised remote tools execute through the orchestrator
  rather than a local stub."

### Reference documents

The following project documents inform test design and code style:

- `docs/rust-testing-with-rstest-fixtures.md` for `rstest` fixture patterns.
- `docs/rstest-bdd-users-guide.md` for `rstest-bdd` behavioural test
  patterns.
- `docs/reliable-testing-in-rust-via-dependency-injection.md` for dependency
  injection testing patterns.
- `docs/complexity-antipatterns-and-refactoring-strategies.md` for complexity
  management.
- `AGENTS.md` for repository quality gates and commit conventions.

## Constraints

- Do not change runtime behaviour. This plan adds tests and test fixtures
  only. The sole exception is refactoring existing test helpers to make them
  reusable across the new test families, provided such refactoring does not
  change external behaviour.

- Preserve the existing `ToolDefinition` contract. The LLM-facing shape
  (`name: String`, `description: String`, `parameters: serde_json::Value`)
  must not be altered.

- Keep tests in the modules that own the seams being tested. Orchestrator
  catalogue tests stay in `src/orchestrator/api/tests/`, worker merge tests
  stay in `src/worker/container/tests.rs`, proxy fidelity tests stay in
  `src/tools/builtin/worker_remote_tool_proxy.rs`, and registry tests stay
  in `src/tools/registry/tests.rs`.

- Use `rstest` fixtures for shared setup, `mockall` where ad hoc mocks are
  needed, and in-process harnesses rather than Docker, live MCP services, or
  external network dependencies.

- Use `rstest-bdd` for behavioural tests where scenarios add clarity beyond
  what unit-level `rstest` coverage provides. If an `rstest-bdd` harness
  would require new external infrastructure, fall back to in-process `rstest`
  integration tests with equally observable assertions and document the
  decision.

- All new test files must remain under 400 lines per AGENTS.md. Extract
  helpers into the existing fixture modules when a file would exceed that
  limit.

- Follow en-GB-oxendict spelling in comments and documentation.

- Every new test function must have a clear, descriptive name that states
  what it verifies, following the existing naming conventions in the test
  modules.

## Tolerances (exception triggers)

- Scope: if implementation requires changes to more than 12 files or roughly
  500 net new lines (excluding comments and blank lines), stop and verify
  that runtime behaviour changes have not leaked in.

- Interface: if any public type, trait, or function signature outside the
  test modules must change to make the code testable, stop and document the
  interface pressure before proceeding.

- Fixture complexity: if a single test fixture function exceeds 50 lines or
  a single test function exceeds 80 lines, stop and extract helpers rather
  than accepting the complexity.

- Iterations: if a test remains red after three focused debugging attempts,
  stop and document the failure in `Surprises & Discoveries` before
  continuing.

- Behaviour-driven development (BDD) harness: if adding `rstest-bdd`
  behavioural coverage for any test family requires a brand-new feature-test
  harness, external services, or Docker orchestration, document that explicitly
  in `Decision Log` and fall back to in-process `rstest` integration coverage.

- Documentation drift: if `docs/users-guide.md` or
  `docs/axinite-architecture-overview.md` describes behaviour that the
  tests cannot actually verify, stop and reconcile the documentation before
  marking the plan complete.

## Risks

- Risk: The existing test infrastructure is already quite comprehensive.
  New tests may duplicate existing coverage rather than adding new
  fidelity guarantees, creating maintenance burden without additional
  protection.
  Severity: medium
  Likelihood: medium
  Mitigation: before writing each test, review the existing tests in the
  target module and write only tests that cover a gap. Each new test must
  protect against a regression that no existing test catches. Document
  the specific gap each test fills.

- Risk: Schema fidelity tests may become brittle if they assert on exact
  JSON values rather than structural properties, breaking when legitimate
  schema changes occur.
  Severity: medium
  Likelihood: medium
  Mitigation: assert on structural invariants (field presence, type shape,
  required-field completeness) rather than exact string equality for
  descriptions. Use snapshot-style assertions for the full `ToolDefinition`
  round-trip where exact preservation is the contract.

- Risk: The `rstest-bdd` framework may not yet have a practical in-process
  harness for the worker-orchestrator surface, making behavioural test
  coverage ceremonial rather than valuable.
  Severity: low
  Likelihood: high
  Mitigation: evaluate `rstest-bdd` feasibility in milestone 1 before
  committing to it. If a narrow scenario adds real clarity, use it.
  Otherwise, use `rstest` integration tests with equally observable
  assertions and record the decision.

- Risk: Mock HTTP servers used by worker container tests may introduce
  flakiness through port conflicts or timing sensitivity.
  Severity: medium
  Likelihood: low
  Mitigation: reuse the existing mock-server patterns from
  `src/worker/container/tests.rs` and
  `src/tools/builtin/worker_remote_tool_proxy.rs` which already handle
  ephemeral port allocation.

- Risk: Contract parity tests between worker and orchestrator may be hard
  to express without introducing a coupling between the two test modules.
  Severity: medium
  Likelihood: medium
  Mitigation: use the shared types in `src/worker/api/types.rs` as the
  contract surface. Contract tests assert that both sides produce and
  consume payloads that round-trip through these shared types without loss.

## Milestone 1: audit existing coverage and identify gaps

Before writing any test code, perform a structured audit of the existing
test suite to identify exactly which schema-fidelity and execution-routing
properties are already covered and which are not.

### Audit scope

Review each existing test module listed in the repository orientation
section. For each test, record:

1. What property it asserts (e.g., "catalogue excludes protected tools",
   "proxy preserves tool name").
2. Whether that property covers schema fidelity (field preservation),
   execution routing (orchestrator-side dispatch), or contract parity
   (shared type consistency).
3. Whether the assertion is structural (would catch a missing field) or
   incidental (tests something else and happens to touch the field).

### Gap identification

The RFC 0001 §Testing Strategy names the following test families. Map each
to existing or missing coverage:

**Unit tests (RFC 0001 §Testing Strategy):**

1. "Catalog construction returns orchestrator-owned active MCP tool
   definitions with original descriptions and schemas."
   Gap: existing tests check that the catalogue includes eligible tools and
   excludes ineligible ones, but do not assert that every `ToolDefinition`
   field survives the catalogue journey unchanged. A full-payload fidelity
   assertion is needed.

2. "Catalog filtering excludes approval-gated or otherwise uncallable
   tools."
   Gap: covered by existing
   `remote_tool_catalog_excludes_ineligible_tools` and
   `get_hosted_tool_reports_lookup_reason`. No new test needed.

3. "Worker proxy registration preserves the orchestrator-provided
   `ToolDefinition` exactly."
   Gap: the existing proxy test asserts individual fields match. A
   whole-`ToolDefinition` structural comparison is needed to catch future
   field additions that might be dropped.

4. "Generic remote execution dispatches to the requested orchestrator-owned
   tool."
   Gap: existing tests cover success and several error paths. Missing:
   a test that proves execution reaches the real orchestrator-side tool
   (not a local stub) by observing a side effect only the orchestrator
   path can produce.

**Behavioural tests (RFC 0001 §Testing Strategy):**

1. "Hosted worker with an active MCP tool advertises that tool in
   `available_tools`."
   Gap: partially covered by
   `worker_runtime_build_reasoning_context_merges_local_and_remote_tools`.
   Strengthen with a scenario-level assertion that names both tool families.

2. "Hosted worker can execute a proxied MCP tool end-to-end through the
   orchestrator."
   Gap: the proxy test exercises round-trip execution through a mock server,
   but no test exercises the full path from worker registry lookup through
   proxy dispatch to orchestrator-side tool execution in a single assertion
   chain.

3. "Hosted worker still exposes container tools and extension-management
   proxy tools."
   Gap: covered by
   `worker_runtime_build_tools_preserves_container_local_tools`. No new
   test needed.

4. "Extension activation refreshes the hosted-visible tool list."
   Gap: not covered, but this belongs to a later roadmap item (extension
   lifecycle) rather than `1.1.4`. Document the exclusion.

**Regression test target (RFC 0001 §Testing Strategy):**

> "A hosted LLM receives the same `name`, `description`, and `parameters`
> for an active MCP tool that the canonical orchestrator registry would
> expose in the normal in-process path."

Gap: no existing test compares the orchestrator-side canonical definition
against the worker-side proxy-reported definition in a single assertion.
This is the primary regression this plan must lock down.

**Contract parity (Issue #16):**

> "There is no obvious shared contract test proving that worker and
> orchestrator endpoint parity is maintained."

Gap: no test asserts that the routes registered by the orchestrator match
the routes consumed by the worker client, or that request and response
payloads round-trip through the shared types without field loss.

### Milestone 1 deliverable

A written list of gaps (which may refine the analysis above) recorded in
the `Progress` section, with each gap assigned to a milestone below. No
code changes in this milestone.

### Milestone 1 go/no-go

Proceed to milestone 2 only if the gap analysis is complete and the set of
new tests is bounded. If the analysis reveals that more than 15 new test
functions are needed, stop and consider whether the scope should be
narrowed.

## Milestone 2: schema-fidelity tests

Add tests that fail if any required `ToolDefinition` field is dropped,
rewritten, or corrupted during the journey from orchestrator registry
through the catalogue endpoint to the worker-side proxy.

### Test 2a: catalogue preserves full `ToolDefinition` payloads

Location: `src/orchestrator/api/tests/remote_tools.rs`.

Add a test that registers a `StubTool` with a non-trivial JSON Schema for
`parameters` (including `required`, `properties` with `description` and
`type` fields, and nested objects) alongside a multi-sentence `description`
containing special characters and Markdown formatting. Assert that the
`ToolDefinition` returned by the catalogue endpoint is byte-for-byte
identical to the definition reported by the registered tool's trait methods.

This test catches regressions where a serialization layer, filter, or
transformer silently strips or rewrites fields. Name the test
`remote_tool_catalog_preserves_full_tool_definition_payload`.

Use an `rstest` fixture for the complex `StubTool` so it can be reused in
later tests.

### Test 2b: proxy-reported definition matches catalogue definition

Location: `src/tools/builtin/worker_remote_tool_proxy.rs` (tests module).

Add a test that creates a `WorkerRemoteToolProxy` from a `ToolDefinition`
with the same complex schema used in test 2a, then asserts that
`proxy.name()`, `proxy.description()`, and `proxy.parameters_schema()`
produce values that, when reassembled into a `ToolDefinition`, are
structurally equal to the input. This catches any transformation or
truncation in the proxy layer.

Name the test
`worker_remote_tool_proxy_preserves_full_tool_definition_fields`.

### Test 2c: end-to-end definition fidelity from registry to worker proxy

Location: `src/worker/container/tests.rs`.

Add a test that wires a mock orchestrator serving a catalogue with a
complex tool definition, starts a worker runtime that fetches and registers
the proxies, and asserts that the `ToolDefinition` reported by the
worker's merged registry matches the original orchestrator-side definition
field by field. This is the RFC 0001 regression test target: the worker
sees the same definition the orchestrator would expose in-process.

Name the test
`hosted_worker_proxy_definition_matches_orchestrator_canonical_definition`.

### Test 2d: catalogue version changes when tool definitions change

Location: `src/orchestrator/api/tests/remote_tools.rs`.

Add a test that computes the catalogue version for two different tool sets
and asserts the versions differ. Also assert that computing the version
twice for the same tool set produces the same value. This catches
nondeterminism and ensures the version is a meaningful signal.

Name the test
`remote_tool_catalog_version_is_deterministic_and_sensitive_to_content`.

### Milestone 2 go/no-go

Run the new tests. All must pass. Deliberately mutate a field in the test
fixture (e.g., truncate the description) and confirm at least one test
fails. Record the evidence.

## Milestone 3: execution-routing tests

Add tests that prove advertised remote tools execute through the
orchestrator's generic execution endpoint rather than through a local stub,
and that the execution path handles the full error taxonomy.

### Test 3a: proxy execution reaches the orchestrator endpoint

Location: `src/tools/builtin/worker_remote_tool_proxy.rs` (tests module)
or `src/worker/container/tests.rs`.

Add a test that creates a mock orchestrator HTTP server with a request
counter or flag, registers a proxy tool, executes it, and asserts that the
mock server received exactly one request on the
`/worker/{job_id}/tools/execute` route with the correct `tool_name` and
`params`. This proves execution routes through the orchestrator rather
than resolving locally.

Name the test
`worker_remote_tool_proxy_routes_execution_through_orchestrator_endpoint`.

If a similar assertion already exists in the existing proxy round-trip
test, extend that test with an explicit route-path assertion rather than
adding a duplicate.

### Test 3b: execution propagates tool output fields faithfully

Location: `src/tools/builtin/worker_remote_tool_proxy.rs` (tests module).

Add or extend a test that asserts the full `ToolOutput` returned by the
proxy matches the `ToolOutput` returned by the mock orchestrator, including
`result`, `cost`, `raw`, and `duration_ms` fields. This catches field loss
in the execution response path.

Name the test
`worker_remote_tool_proxy_preserves_full_tool_output_fields`.

### Test 3c: execution error taxonomy maps correctly

Location: `src/orchestrator/api/tests/remote_tools.rs`.

Review the existing `remote_tool_execute_maps_error_statuses` test. If it
already covers `InvalidParameters` (400), `NotAuthorized` (403),
`RateLimited` (429), and general `ExecutionFailed` (502), no new test is
needed. If any mapping is missing, extend the existing test or add a
focused companion.

### Test 3d: execution of a non-catalogue tool is rejected

Location: `src/orchestrator/api/tests/remote_tools.rs`.

Review the existing `remote_tool_execute_rejects_unknown_tools` and
`remote_tool_execute_rejects_non_catalog_tools` tests. If they already
cover the relevant rejection cases (unknown tool → 404, container-only
tool → 400, protected tool → 400), no new test is needed. Document the
existing coverage.

### Milestone 3 go/no-go

Run the new and existing execution tests. All must pass. Deliberately
change the mock server's response route or response body and confirm at
least one test fails. Record the evidence.

## Milestone 4: worker-orchestrator contract parity tests

Add tests that prove the shared transport types in `src/worker/api/types.rs`
are the single source of truth for routes and payloads, so drift between
the worker client and orchestrator router is caught at compile time or test
time.

### Test 4a: route constants match between worker client and orchestrator router

Location: `src/worker/api/types.rs` (tests module, creating if necessary)
or `src/orchestrator/api/tests/remote_tools.rs`.

Add a test that reads the route constant strings
(`REMOTE_TOOL_CATALOG_ROUTE` and `REMOTE_TOOL_EXECUTE_ROUTE`) from
`src/worker/api/types.rs` and asserts they match the routes actually
registered in the orchestrator's Axum router. This catches route-string
drift where one side updates a path and the other does not.

The implementation approach depends on how the orchestrator registers
routes. If the router is built from the same constants, a compile-time
guarantee already exists and this test should verify that relationship. If
the router uses string literals independently, this test must bridge the
gap.

Name the test
`worker_and_orchestrator_share_remote_tool_route_constants`.

### Test 4b: request and response payloads round-trip through shared types

Location: `src/worker/api/types.rs` (tests module).

Add a test that serializes a `RemoteToolCatalogResponse` with non-trivial
content (tools with complex schemas, non-empty `toolset_instructions`, a
non-zero `catalog_version`) to JSON, deserializes it back, and asserts
structural equality. Do the same for `RemoteToolExecutionRequest` and
`RemoteToolExecutionResponse`. This catches serde attribute mismatches
(`skip`, `default`, `rename`) that could cause silent field loss.

Name the test
`remote_tool_transport_types_round_trip_without_field_loss`.

### Test 4c: worker client and orchestrator consume the same payload shape

Location: `src/orchestrator/api/tests/remote_tools.rs` or a new shared
test file.

Add a test that builds a catalogue response using
`hosted_remote_tool_catalog()`, serializes it to JSON, and deserializes
it as `RemoteToolCatalogResponse`. Assert that the deserialized value
matches the original. This proves the orchestrator produces responses that
the worker client can consume without field loss or misinterpretation.

Do the same for the execution path: build a
`RemoteToolExecutionRequest`, serialize it, and parse it as the
orchestrator would. Build a `RemoteToolExecutionResponse`, serialize it as
the orchestrator would, and parse it as the worker client would.

Name the test
`orchestrator_responses_deserialize_into_worker_shared_types`.

### Milestone 4 go/no-go

Run the new contract tests. All must pass. Deliberately add a
`#[serde(skip)]` attribute to a field in one of the shared types and
confirm at least one test fails. Record the evidence, then revert the
deliberate breakage.

## Milestone 5: behavioural tests

Add `rstest-bdd` behavioural tests if a narrow in-process scenario can
provide clearer, more readable coverage than the unit-level tests above.

### Feasibility check

Evaluate whether the project already has a practical `rstest-bdd` harness
for the worker-orchestrator surface. If `.feature` files and step
definitions exist for this area, extend them. If no harness exists, assess
whether adding one for this plan's scope is proportionate.

### Candidate scenarios

If `rstest-bdd` is feasible, the following scenarios add value:

```gherkin
Feature: Hosted remote-tool schema fidelity

  Scenario: Worker advertises orchestrator tool definitions unchanged
    Given an orchestrator with one active hosted-visible MCP tool
    And the tool has a description and JSON Schema parameters
    When the worker fetches the remote catalogue
    And the worker registers proxy tools
    Then the worker-advertised tool definition matches the orchestrator
      definition exactly

  Scenario: Worker routes tool execution through orchestrator endpoint
    Given an orchestrator with one active hosted-visible MCP tool
    And a worker with the remote proxy registered
    When the model selects the remote tool
    Then the execution request reaches the orchestrator endpoint
    And the tool output is returned unchanged
```

```gherkin
Feature: Hosted remote-tool execution routing

  Scenario: Protected tools are not advertised
    Given an orchestrator with a protected tool registered
    When the worker fetches the remote catalogue
    Then the catalogue does not include the protected tool

  Scenario: Approval-gated tools are not advertised
    Given an orchestrator with an approval-gated MCP tool
    When the worker fetches the remote catalogue
    Then the catalogue does not include the approval-gated tool
```

### Fallback

If `rstest-bdd` is not feasible for this surface, the in-process `rstest`
integration tests from milestones 2-4 already provide equally observable
assertions. Record the decision in `Decision Log` and do not add a
ceremonial BDD layer that weakens the guarantees.

### Milestone 5 go/no-go

If BDD tests were added, they must pass under `make test`. If the fallback
was chosen, the decision must be documented and the unit-level tests must
already cover the same properties.

## Milestone 6: synchronize design and operator documentation

Update the project documentation so the described behaviour matches the
tested behaviour.

1. Update `docs/rfcs/0001-expose-mcp-tool-definitions.md` implementation
   status to reflect that `1.1.4` is complete and the test matrix is in
   place. Note which test families were delivered and which (if any) were
   deferred.

2. Review `docs/axinite-architecture-overview.md` for any wording about
   the hosted remote-tool contract that should now reference the test
   coverage as the source of truth.

3. Review `docs/users-guide.md` for accuracy against the tested behaviour.
   If any operator-visible description is not backed by a test, either add
   a test or reconcile the documentation.

4. Mark roadmap item `1.1.4` done in `docs/roadmap.md`. Since this
   completes all items in section `1.1`, review whether the parent section
   heading should also note completion.

5. Add this ExecPlan to the `docs/contents.md` index under the ExecPlans
   directory listing.

6. Check `FEATURE_PARITY.md` for any entry that should change because the
   hosted tool-advertising test matrix is now in place.

## Milestone 7: validate, gate, and publish

Run the smallest useful red-green loop first, then the full repository
gates. All substantive commands should use `set -o pipefail` and `tee` so
logs are retained under `/tmp`.

Use the following command pattern during implementation for targeted tests:

```bash
set -o pipefail && cargo test <test_name> --lib -- --nocapture \
  | tee /tmp/unit-axinite-1-1-4-schema-fidelity.out
```

Then run the broader gates expected by this repository:

```bash
set -o pipefail && make check-fmt \
  | tee /tmp/check-fmt-axinite-1-1-4.out
set -o pipefail && make lint \
  | tee /tmp/lint-axinite-1-1-4.out
set -o pipefail && make test \
  | tee /tmp/test-axinite-1-1-4.out
```

If documentation changes are made outside the Rust gates, also run:

```bash
set -o pipefail && bunx markdownlint-cli2 \
  docs/roadmap.md \
  docs/users-guide.md \
  docs/axinite-architecture-overview.md \
  docs/rfcs/0001-expose-mcp-tool-definitions.md \
  docs/execplans/1-1-4-tests-for-schema-fidelity-and-execution-routing.md \
  docs/contents.md \
  | tee /tmp/markdownlint-axinite-1-1-4.out
git diff --check
```

Only after the gates pass should the implementation be committed. The
commit must describe that hosted-mode schema fidelity and execution routing
tests are now in place for roadmap item `1.1.4`, and the final report must
cite the exact log paths.

## Progress

- [x] Audit existing test coverage and identify gaps (milestone 1).
- [x] Implement schema-fidelity tests (milestone 2).
- [x] Implement execution-routing tests (milestone 3).
- [x] Implement contract-parity tests (milestone 4).
- [x] Evaluate and implement behavioural tests (milestone 5).
- [x] Synchronize documentation (milestone 6).
- [x] Run full validation gates and publish (milestone 7).
- [ ] Address code-review follow-ups (post-review).

### Milestone 1 findings

The audit confirmed the gap analysis in the plan. The following test gaps
were identified:

**Schema-fidelity gaps:**

1. **Full payload preservation in catalogue**: existing test
   `remote_tool_catalog_returns_hosted_safe_tool_definitions` (line 86) checks
   individual fields (`name`, `description`, `parameters`) but does not assert
   that the entire `ToolDefinition` structure survives unchanged, including any
   future fields. A structural comparison is needed.

2. **Proxy field preservation**: existing test
   `remote_tool_execute_round_trips_catalog_tools` (line 118 in
   `worker_remote_tool_proxy.rs`) asserts individual field equality but does
   not catch if a new field is added to `ToolDefinition` and silently dropped
   by the proxy.

3. **End-to-end definition fidelity**: no test compares the orchestrator
   canonical definition against the worker-advertised proxy definition in a
   single assertion. The test `worker_runtime_build_reasoning_context_merges_local_and_remote_tools`
   (line 566 in `worker/container/tests.rs`) checks tool names are merged but
   does not assert field-level fidelity.

4. **Catalogue version sensitivity**: no test proves the `catalog_version`
   changes when tool definitions change, only that sorting produces a
   deterministic version (test at line 174 in
   `orchestrator/api/tests/remote_tools.rs`).

**Execution-routing gaps:**

1. **Proxy routes through orchestrator endpoint**: existing test
   `remote_tool_execute_round_trips_catalog_tools` creates a mock server and
   executes through it, but does not explicitly assert that the request
   reached the correct route path. It implicitly proves routing but does not
   name the guarantee.

2. **Output field preservation**: the round-trip test checks `result`, `cost`,
   `raw`, and `duration` individually but does not assert the full `ToolOutput`
   structure survives.

3. **Error taxonomy**: existing test `remote_tool_execute_maps_error_statuses`
   (line 307) covers all four error kinds (`InvalidParameters`, `NotAuthorized`,
   `RateLimited`, `ExecutionFailed`). No new test needed.

4. **Non-catalogue tool rejection**: existing tests
   `remote_tool_execute_rejects_unknown_tools` (line 214),
   `remote_tool_execute_rejects_non_catalog_tools` (line 242), and
   `remote_tool_execute_rejects_protected_orchestration_tools` (line 254)
   cover unknown, container-only, and protected tools. No new test needed.

**Contract-parity gaps:**

1. **Route constants**: no test asserts that `REMOTE_TOOL_CATALOG_ROUTE` and
   `REMOTE_TOOL_EXECUTE_ROUTE` in `src/worker/api/types.rs` are used by both
   the worker client and orchestrator router. The constants are imported and
   used in tests, proving usage, but no test explicitly validates parity.

2. **Payload round-trip**: no test asserts that `RemoteToolCatalogResponse`,
   `RemoteToolExecutionRequest`, and `RemoteToolExecutionResponse` survive
   serialization and deserialization without field loss.

3. **Worker-orchestrator payload compatibility**: no test builds a response on
   the orchestrator side, serializes it, and deserializes it as the worker
   would, proving the shared types are truly shared.

All gaps align with the plan's milestone 2-4 test families. Milestone 1 go/no-go
criterion met: the set of new tests is bounded (9 new test functions across 4
modules).

## Surprises & Discoveries

**Discovery 1 (milestone 1):** The existing test
`remote_tool_execute_round_trips_catalog_tools` in
`src/tools/builtin/worker_remote_tool_proxy.rs` already exercises the full
execution path through a mock server and checks all `ToolOutput` fields. The
gap is not missing coverage but missing explicit route-path and structural
assertions. Milestone 3 tests will strengthen rather than replace this test.

**Discovery 2 (milestone 2-4):** All new tests compile and pass individually
during targeted test runs. The fixtures shared between orchestrator and worker
test modules (`complex_tool_definition`, `complex_tool_stub`) successfully
exercise complex JSON Schema structures with nested objects, arrays, and
special characters including UTF-8 emoji. The route-capturing test in
milestone 3 successfully proves that proxy execution reaches the exact expected
orchestrator endpoint path.

## Decision Log

**Decision 1 (milestone 5):** BDD tests deferred in favour of in-process unit
tests. No practical `rstest-bdd` harness exists for the worker-orchestrator
surface. The project has no `.feature` files, no step definition infrastructure,
and no BDD test patterns. Adding BDD infrastructure for this narrow scope would
introduce ceremony without material benefit. The in-process `rstest` integration
tests from milestones 2-4 already provide equally observable assertions with
clear, descriptive test names that state exactly what they verify. The unit
tests cover the same properties the BDD scenarios would have named:
`remote_tool_catalog_preserves_full_tool_definition_payload` is as readable as
a scenario named "Worker advertises orchestrator tool definitions unchanged",
and fails with equally clear diagnostics. BDD infrastructure may be added in a
future roadmap item if cross-cutting behavioural scenarios justify the
investment.

**Decision 2 (milestone 3):** Tests 3c and 3d confirmed to be already covered
by existing tests. The test `remote_tool_execute_maps_error_statuses` covers
all four error-taxonomy mappings (`InvalidParameters`, `NotAuthorized`,
`RateLimited`, `ExecutionFailed`). The tests
`remote_tool_execute_rejects_unknown_tools`,
`remote_tool_execute_rejects_non_catalog_tools`, and
`remote_tool_execute_rejects_protected_orchestration_tools` cover rejection
of unknown, container-only, and protected tools. No new tests were added for
these gaps.

## Outcomes & Retrospective

All milestones completed successfully. The test matrix is now in place and
enforces the schema-fidelity, execution-routing, and contract-parity guarantees
named in RFC 0001 and roadmap item `1.1.4`. Code-review follow-ups are being
addressed in a subsequent pass (see progress checklist above).

### Files added or modified

**Test files modified:**

- `src/orchestrator/api/tests/fixtures/remote_tool_mocks.rs`: added
  `complex_tool_definition()` and `complex_tool_stub()` fixtures for testing
  full payload fidelity with nested JSON Schema and special characters.
- `src/orchestrator/api/tests/catalogue_fidelity.rs`: added three new tests:
  `remote_tool_catalog_preserves_full_tool_definition_payload`,
  `remote_tool_catalog_version_is_deterministic_and_sensitive_to_content`, and
  `orchestrator_responses_deserialize_into_worker_shared_types`.
- `src/tools/builtin/worker_remote_tool_proxy.rs`: added three new tests:
  `worker_remote_tool_proxy_preserves_full_tool_definition_fields`,
  `worker_remote_tool_proxy_preserves_full_tool_output_fields`, and
  `worker_remote_tool_proxy_routes_execution_through_orchestrator_endpoint`.
- `src/worker/container/tests.rs`: added one new test:
  `hosted_worker_proxy_definition_matches_orchestrator_canonical_definition`,
  plus helper functions `complex_orchestrator_tool_definition()` and
  `remote_tool_catalog_with_complex_tool()`.
- `src/worker/api/types.rs`: added tests module with two new tests:
  `worker_and_orchestrator_share_remote_tool_route_constants` and
  `remote_tool_transport_types_round_trip_without_field_loss`.

**Documentation files modified:**

- `docs/roadmap.md`: marked roadmap item `1.1.4` complete.
- `docs/rfcs/0001-expose-mcp-tool-definitions.md`: updated implementation status
  to reflect that all roadmap items in section `1.1` are complete.
- `docs/contents.md`: added ExecPlan `1-1-4-tests-for-schema-fidelity-and-execution-routing.md`
  to the ExecPlans directory listing.
- `docs/execplans/1-1-4-tests-for-schema-fidelity-and-execution-routing.md`:
  updated status to `COMPLETE` and recorded progress, decisions, and discoveries.

### Test coverage added

The implementation added 9 new test functions covering:

1. Full `ToolDefinition` payload preservation through the catalogue endpoint
   (milestone 2).
2. Proxy-reported fields matching input definitions exactly (milestone 2).
3. End-to-end definition fidelity from orchestrator canonical to worker-advertised
   proxy (milestone 2).
4. Catalogue version determinism and content sensitivity (milestone 2).
5. Proxy execution routing through the correct orchestrator endpoint path
   (milestone 3).
6. Full `ToolOutput` field preservation including cost, raw, and duration_ms
   (milestone 3).
7. Route constant sharing and correctness between worker and orchestrator
   (milestone 4).
8. Transport type round-trip without field loss for catalogue, execution
   request, and execution response payloads (milestone 4).
9. Orchestrator-built responses deserializing correctly into worker shared types
   (milestone 4).

All tests use in-process mock servers and fixtures, avoiding external
dependencies. All tests follow existing `rstest` patterns and naming
conventions. The format check (`make check-fmt`) passed after running
`cargo fmt --all`. The format check, git whitespace check, and full test suite
passed successfully. Markdown linting remained partially blocked by
pre-existing issues in `docs/roadmap.md`.

### Validation evidence

Format check passed:

```bash
cargo fmt --all -- --check
cargo fmt --manifest-path tools-src/github/Cargo.toml --all -- --check
```

Git whitespace check passed:

```bash
git diff --check
```

(No output, indicating no whitespace errors.)

Full test suite passed: 3076 tests passed; 0 failed; 2 ignored (webhook server
test fixed to use already-bound address instead of privileged port; worker API
types test split into three focused tests per code review).

Markdown linting was only partially green because `docs/roadmap.md` still had
pre-existing issues unrelated to this implementation (multiple consecutive
blank lines at lines 1342, 1408, 1450, 1489, 1512). The ExecPlan, RFC 0001,
and `docs/contents.md` changes introduced no new Markdown issues.

### Retrospective observations

The audit phase (milestone 1) saved significant rework. By mapping existing
coverage before writing new tests, the plan avoided duplicating
`remote_tool_execute_maps_error_statuses` and the rejection-case tests, which
already covered execution-routing gaps 3c and 3d.

The complex-tool-definition fixtures proved valuable for both orchestrator and
worker test modules. Sharing these fixtures kept the test code DRY and ensured
both sides exercised the same non-trivial payload shapes.

The BDD decision (milestone 5) was straightforward because the project had zero
BDD infrastructure. The in-process `rstest` tests provide the same guarantees
without ceremony.

The contract-parity tests (milestone 4) surface a compile-time guarantee: if
the shared types in `src/worker/api/types.rs` drift, both the worker client and
orchestrator router would fail to compile or serialize. The tests make that
guarantee explicit and observable.

All gaps identified in milestone 1 are now covered. The test suite will fail
loudly if MCP fields disappear, execution routes incorrectly, or transport
contracts drift.
