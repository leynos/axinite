# Add end-to-end tests for first-call WASM schema exposure

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`,
`Surprises & Discoveries`, `Decision Log`, and
`Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: COMPLETE

## Purpose / big picture

Roadmap item `1.2.4` exists to prove the contract established by `1.2.1`,
`1.2.2`, and `1.2.3`: active WebAssembly (WASM) tools must expose their
advertised schema to the large language model (LLM) before the first tool call
in both non-hosted and hosted flows. The failure mode this plan must prevent is
silent regression back to a failure-first contract where the model only learns
WASM arguments after a bad call and a retry hint.

After this work, the test suite must fail loudly if either of the following
regressions returns:

1. A first reasoning request containing an active WASM tool omits that tool's
   advertised `ToolDefinition.parameters` schema or falls back to the
   placeholder schema shape.
2. A hosted worker advertises or forwards orchestrator-owned WASM tools without
   their proactive schema, or can only recover after a schema-bearing failure
   hint.

Success is observable in four ways. First, targeted tests prove that the first
LLM request in the in-process path includes the advertised schema for an active
WASM tool before any execution attempt. Second, targeted hosted-path tests
prove that the worker's first proxied tool-capable completion request includes
the same advertised schema for orchestrator-owned WASM tools fetched from the
remote catalogue. Third, unhappy-path tests still show retry guidance as
supplemental diagnostics that point back to the already advertised schema.
Fourth, `make all` passes and the roadmap entry for `1.2.4` is marked done.

This plan uses the `hexagonal-architecture` guidance narrowly. Contract policy
belongs in inward-facing seams such as WASM registration, registry projection,
reasoning-context assembly, and the worker-orchestrator transport types.
Adapters such as the orchestrator HTTP handlers and worker runtime may expose
and verify that policy, but must not invent a second WASM-specific contract.

## Approval gates

- Plan approved
  Acceptance criteria: the implementation stays scoped to tests, narrowly
  supporting test harnesses, and documentation updates for roadmap item
  `1.2.4`. No new runtime contract, no new transport route, and no broadened
  provider-shaping work.
  Sign-off: human reviewer approves this ExecPlan before implementation
  begins.

- Implementation complete
  Acceptance criteria: non-hosted and hosted first-call schema exposure are
  both covered by observable regression tests, unhappy paths remain covered,
  and relevant documentation is synchronized.
  Sign-off: implementer marks all milestones complete before final validation.

- Validation passed
  Acceptance criteria: targeted tests, `make all`, Markdown linting for changed
  docs, and `git diff --check` all pass with retained logs.
  Sign-off: implementer records evidence immediately before commit.

- Docs synced
  Acceptance criteria: `docs/roadmap.md`, `docs/rfcs/0002...`,
  `docs/users-guide.md`, `docs/worker-orchestrator-contract.md`, and
  `docs/axinite-architecture-overview.md` describe the same first-call WASM
  contract, and `docs/contents.md` indexes this plan.
  Sign-off: implementer completes documentation updates as the final
  pre-commit checkpoint.

## Repository orientation

The files below are the authoritative orientation points for this feature.

- `docs/roadmap.md` defines `1.2.4`, its dependencies on `1.2.2` and `1.2.3`,
  and the success condition that both hosted and non-hosted paths fail if
  proactive schema publication regresses.
- `docs/rfcs/0002-expose-wasm-tool-definitions.md` is the design authority.
  The most important sections are `Goals`, `Detailed Interface`,
  `Testing Strategy`, and `Migration Plan` step 5.
- `docs/execplans/1-2-1-audit-and-fix-wasm-registration-paths.md`,
  `docs/execplans/1-2-2-orchestrator-owned-wasm-tools-in-tool-catalogue.md`,
  and `docs/execplans/1-2-3-demote-schema-bearing-retry-hints.md` capture the
  decisions already made for registration-time schema publication, hosted
  catalogue reuse, and fallback-only retry hints.
- `src/tools/wasm/loader.rs` already contains registration-path regression
  tests such as
  `load_from_files_publishes_guest_schema_in_tool_definitions` and
  `load_dev_tools_publishes_guest_schema_in_tool_definitions`. Those tests are
  the unit-level proof that active WASM registrations publish real schemas.
- `src/tools/wasm/wrapper.rs` and `src/tools/wasm/wrapper/metadata.rs` own the
  fallback-diagnostic path. The first-call tests must prove that this path is
  supplemental rather than primary.
- `src/tools/registry/hosted.rs` is the canonical hosted-visible policy seam.
  Hosted-path tests must keep this registry-owned filter as the source of truth
  instead of rebuilding selection logic in the orchestrator adapter or the
  worker runtime.
- `src/orchestrator/api/remote_tools.rs`,
  `src/orchestrator/api/handlers.rs`, and
  `src/orchestrator/api/tests/{remote_tools.rs,catalogue_fidelity.rs,transport_parity.rs}`
  own hosted catalogue construction, the shared transport boundary, and current
  fidelity tests.
- `src/worker/container.rs`, `src/worker/container/delegate.rs`, and
  `src/worker/container/tests/{remote_tools.rs,hosted_fidelity.rs}` own worker
  proxy registration, first reasoning-context assembly, later
  `available_tools` refresh, and the current hosted round-trip tests.
- `src/worker/api.rs` and `src/worker/api/proxy_types.rs` define the
  tool-capable proxied completion request. This is the precise place where the
  hosted first-call request can be observed without adding a new transport.
- `tests/e2e_traces/worker_coverage.rs` and the supporting trace rig under
  `tests/support/test_rig/` already provide an in-process end-to-end harness
  that can inspect captured first LLM requests through `TraceLlm`.
- `docs/users-guide.md` and `docs/worker-orchestrator-contract.md` already
  describe proactive schema publication as the primary contract.
- The user request referenced `docs/axinite-architecture-summary.md`, but that
  file does not exist in this checkout. Use
  `docs/axinite-architecture-overview.md` as the relevant architecture
  reference.

## Constraints

- Do not change the canonical LLM-facing tool shape. `ToolDefinition` must
  remain `name`, `description`, and `parameters`.

- Do not add a WASM-specific transport, a second hosted catalogue route, or a
  second schema source for hosted workers. The existing worker-orchestrator
  boundary must remain the only normal hosted contract.

- Keep policy inward. Tests may verify registry filtering, catalogue
  projection, reasoning-context assembly, and proxied request forwarding, but
  adapters must not gain new WASM-specific branching that duplicates existing
  policy.

- Preserve the `1.2.1` contract that active WASM tool schemas are recovered or
  overridden at registration time, and the `1.2.3` contract that retry hints
  are fallback diagnostics only.

- Use `rstest` fixtures for unit and integration coverage.

- Use `rstest-bdd` only if one focused behaviour-driven development (BDD)
  scenario can reuse an existing in-process harness with no disproportionate
  new scaffolding. If that threshold is exceeded, keep the behavioural proof in
  `rstest` integration or trace-backed tests and document the decision.

- Prefer existing in-process harnesses over live external services, Docker, or
  ambient environment mutation. Dependency injection and capturing stubs are the
  expected patterns.

- Keep new or modified files under repository size limits. If a test file would
  exceed 400 lines, extract helpers into a nearby fixture module instead.

- Update user-facing and maintainer-facing documentation in the same delivery
  pass, including the roadmap entry once the feature lands.

- Check `FEATURE_PARITY.md` during implementation and update it in the same
  branch if a tracked feature statement becomes stale.

## Tolerances (exception triggers)

- Scope: if the smallest credible implementation touches more than 14 files or
  roughly 650 net new lines before documentation, stop and verify that
  unrelated provider, worker-refresh, or extension-runtime work has not leaked
  into this slice.

- Interface: if proving hosted first-call behaviour requires changing
  `ToolDefinition`, `ToolCompletionRequest`, `ProxyToolCompletionRequest`, or
  the shared route constants, stop and document why the existing contract
  cannot express the test.

- Harness growth: if capturing the first hosted `complete_with_tools` request
  would require more than two new support files or a generic testing framework
  that reaches beyond this roadmap item, stop and choose the smallest
  in-process adapter-local capture seam instead.

- BDD proportionality: if adding `rstest-bdd` requires a new feature file
  family, a new runtime fixture stack, or more than one new scenario-support
  module, document that clearly and keep the behavioural proof in `rstest`.

- Ambiguity: if the current code can only prove schema presence in intermediate
  state and not in the actual first request sent to an LLM-facing seam, stop
  and decide explicitly whether to add a narrow capturing provider or to widen
  an existing trace harness.

- Documentation drift: if roadmap, RFC, user's guide, and internal contract
  docs disagree about whether `1.2.4` is complete or about what counts as the
  first-call contract, reconcile that before marking the work done.

## Risks

- Risk: Existing tests already cover schema publication and catalogue fidelity,
  so new tests may accidentally duplicate current coverage without proving the
  new first-call invariant.
  Severity: high
  Likelihood: medium
  Mitigation: every new test must name the exact regression it catches that is
  not already covered today, especially around observing the first LLM-facing
  request rather than intermediate registry state.

- Risk: Strict equality checks against full schemas may become brittle if
  provider-safe shaping changes while the underlying contract remains valid.
  Severity: medium
  Likelihood: medium
  Mitigation: use exact equality only where fidelity is itself the contract
  boundary, and otherwise assert the structural invariants that matter for
  first-call correctness.

- Risk: Hosted-path tests may prove only catalogue fetch or proxy registration,
  not the actual first proxied tool-capable completion request.
  Severity: high
  Likelihood: medium
  Mitigation: include one capturing hosted test that inspects the
  `ProxyToolCompletionRequest.tools` payload sent through
  `llm_complete_with_tools`.

- Risk: The non-hosted path may drift toward testing a fake native tool instead
  of a real WASM registration path, weakening the regression value.
  Severity: medium
  Likelihood: medium
  Mitigation: use the real GitHub WASM fixture or the existing WASM loader
  helpers wherever practical so the first-call proof remains tied to actual
  registration behaviour.

- Risk: Documentation drift is likely because the roadmap marks `1.2.2` done
  while its ExecPlan file still shows `IN PROGRESS`, and this feature touches
  the same document cluster.
  Severity: medium
  Likelihood: high
  Mitigation: treat documentation synchronization as its own milestone and
  explicitly reconcile status wording before marking `1.2.4` complete.

## Milestone 1: confirm the observable first-call seams

Start by deciding what counts as the first request for each path and how the
tests will observe it.

1. Re-read RFC 0002 sections `Goals`, `Detailed Interface`, `Testing Strategy`,
   and `Migration Plan` step 5 alongside the existing `1.2.1` through `1.2.3`
   ExecPlans.
2. For the non-hosted path, confirm that the trace-backed rig in
   `tests/support/test_rig/` can capture the first request via
   `TestRig::captured_llm_requests()` and that this route observes the actual
   `ToolCompletionRequest` seen by the reasoning engine.
3. For the hosted path, confirm the narrowest seam that can capture the first
   proxied request without new transport work. The preferred seam is a
   capturing orchestrator-side LLM stub exercised through
   `POST /worker/{job_id}/llm/complete_with_tools`, because that observes the
   worker's forwarded `tools` payload directly.
4. Record the exact assertions for each path: schema present, schema
   non-placeholder, and retry hints not required to learn the contract.

Expected result: the implementer can point to one concrete first-request seam
for non-hosted execution and one for hosted execution, with no ambiguity about
what the tests must observe.

## Milestone 2: extend non-hosted coverage from registration proof to first-call proof

Add the non-hosted tests that close the gap between registration and actual
request assembly.

1. Keep the existing registration-path tests in `src/tools/wasm/loader.rs` as
   the unit-level contract that real schemas enter the registry through file,
   storage-backed, and dev-build paths.
2. Add a trace-backed behavioural test under `tests/e2e_traces/` that builds a
   rig with an active WASM tool and inspects the first captured LLM request.
   The test should assert that the active WASM tool appears in the request's
   tool list with the advertised `parameters` schema before any tool execution
   occurs.
3. Prefer the real GitHub WASM fixture and existing helper functions used by
   the WASM loader tests, so the behavioural proof is tied to the actual guest
   metadata path rather than a native-tool stand-in.
4. Add one unhappy-path assertion showing that a malformed first call still
   yields fallback guidance that points back to the already advertised schema.

Expected result: the in-process agent path now has one observable proof that
the first LLM call already carries the active WASM schema and one proof that
the failure path is supplemental only.

## Milestone 3: extend hosted coverage from catalogue fidelity to first-call forwarding

Add the hosted tests that prove the worker forwards proactive WASM schema data
on its first tool-capable request.

1. Reuse the existing hosted catalogue and worker registration tests in
   `src/orchestrator/api/tests/remote_tools.rs`,
   `src/orchestrator/api/tests/catalogue_fidelity.rs`,
   `src/worker/container/tests/remote_tools.rs`, and
   `src/worker/container/tests/hosted_fidelity.rs` as the base matrix.
2. Introduce a capturing LLM stub or equivalent narrow helper in the
   orchestrator API test surface that records the incoming
   `ToolCompletionRequest` built by `llm_complete_with_tools`.
3. Drive a worker runtime through its first hosted reasoning call and assert
   that the forwarded `tools` vector contains the advertised orchestrator-owned
   WASM definition with the same schema already proven at catalogue time.
4. Keep the worker-orchestrator contract source-agnostic at the wire level.
   The test should prove that hosted-visible Model Context Protocol (MCP) tools
   and hosted-visible WASM tools share the same request boundary, not that WASM
   tools have a special hosted path.

Expected result: the hosted worker path now has a direct regression test for
the first forwarded `complete_with_tools` request, not only for catalogue fetch
and proxy registration.

## Milestone 4: harden fail-closed and unhappy-path regressions

Use focused tests to prevent regressions that would reintroduce failure-first
teaching or weaken hosted safety behaviour.

1. Add or extend tests that fail if a first-call path carries a placeholder
   schema instead of the advertised one.
2. Keep hosted visibility and execution rejection in sync with the canonical
   registry policy. Ineligible, protected, or approval-gated tools must remain
   hidden or rejected even though active hosted-visible WASM tools are present.
3. Keep the `1.2.3` contract intact by asserting that fallback guidance still
   references the advertised schema rather than becoming the primary contract.
4. Avoid duplicate assertions across orchestrator, worker, and trace-backed
   tests. Each new test should protect a distinct seam: registration,
   forwarding, or unhappy-path recovery.

Expected result: both happy and unhappy paths fail closed in the way RFC 0002
intends, and the test matrix distinguishes contract loss from normal tool
errors.

## Milestone 5: evaluate whether a focused `rstest-bdd` scenario is proportional

The user asked for unit and behavioural tests, using `rstest-bdd` where
applicable. Make the applicability decision explicit.

1. Attempt to identify one scenario that would add clarity beyond the
   trace-backed and integration tests, such as "active WASM tool is visible to
   the model before first use".
2. Only proceed if that scenario can reuse an existing in-process fixture stack
   and stay within the tolerances above.
3. If it cannot, record in `Decision Log` that `rstest-bdd` is not
   proportional for this slice because the existing `rstest` and trace-backed
   harnesses already provide more direct observability of the first request.

Expected result: the plan either includes one deliberately small BDD scenario
or documents why `rstest` provides the proportionate behavioural layer here.

## Milestone 6: synchronize documentation and close the roadmap item

Update the documents that define or summarize the contract once the tests land.

1. Update `docs/roadmap.md` to mark `1.2.4` done only after the test evidence
   exists.
2. Update `docs/rfcs/0002-expose-wasm-tool-definitions.md` implementation
   status so it says first-call end-to-end regression coverage is complete.
3. Review `docs/users-guide.md` and
   `docs/worker-orchestrator-contract.md` for wording about proactive schema
   publication and fallback diagnostics. Update only if the implementation or
   current wording has drifted.
4. Review `docs/axinite-architecture-overview.md` for any maintainer-facing
   statement that should mention the first-call contract more explicitly.
5. Check `FEATURE_PARITY.md` for any stale feature-tracking wording.

Expected result: the roadmap, RFC, user's guide, internal contract document,
and architecture overview all describe the same first-call WASM contract.

## Validation

Run focused tests first and keep logs in `/tmp` as required by repository
policy.

```plaintext
cargo test load_from_files_publishes_guest_schema_in_tool_definitions \
  | tee /tmp/test-axinite-feat-plan-wasm-schema-e2e-loader.out

cargo test remote_tool_catalog \
  | tee /tmp/test-axinite-feat-plan-wasm-schema-e2e-remote-tools.out

cargo test hosted_worker_proxy_definition_matches_orchestrator_canonical_definition \
  | tee /tmp/test-axinite-feat-plan-wasm-schema-e2e-hosted-fidelity.out

cargo test worker_ \
  | tee /tmp/test-axinite-feat-plan-wasm-schema-e2e-worker.out
```

Add one targeted command for the new non-hosted trace-backed test once the test
name is known.

```plaintext
cargo test <new_non_hosted_first_call_test_name> \
  | tee /tmp/test-axinite-feat-plan-wasm-schema-e2e-first-call.out
```

If a focused hosted capture test lands with a stable name, run it directly as
well.

```plaintext
cargo test <new_hosted_first_call_test_name> \
  | tee /tmp/test-axinite-feat-plan-wasm-schema-e2e-hosted-first-call.out
```

Run the required full gate before commit.

```plaintext
make all \
  | tee /tmp/make-all-axinite-feat-plan-wasm-schema-e2e.out
```

Run documentation hygiene checks for changed Markdown files.

```plaintext
bunx markdownlint-cli2 \
  docs/execplans/1-2-4-end-to-end-tests-for-first-call-wasm-schema-exposure.md \
  docs/contents.md \
  | tee /tmp/markdownlint-axinite-feat-plan-wasm-schema-e2e.out

git diff --check \
  | tee /tmp/diff-check-axinite-feat-plan-wasm-schema-e2e.out
```

If the implementation updates additional docs, add them to the Markdown lint
command before the final commit.

## Progress

- [x] 2026-04-14: Draft ExecPlan written and indexed in `docs/contents.md`.
- [x] 2026-04-15T17:57:00+02:00: Confirm non-hosted first-call capture seam
  and exact test location.
- [x] 2026-04-15T17:57:00+02:00: Confirm hosted first-call capture seam and
  exact test location.
- [x] 2026-04-15T18:10:00+02:00: Add non-hosted unit and behavioural
  regression coverage.
- [x] 2026-04-15T18:10:00+02:00: Add hosted first-call forwarding regression
  coverage.
- [x] 2026-04-15T18:10:00+02:00: Evaluate `rstest-bdd` proportionality and
  record the decision.
- [x] 2026-04-15T18:15:00+02:00: Synchronize roadmap, RFC, user-facing, and
  internal documentation.
- [x] 2026-04-15T18:19:14+02:00: Run validation gates, collect evidence, and
  prepare the commit.

## Surprises & Discoveries

- 2026-04-14: The user-requested file
  `docs/axinite-architecture-summary.md` does not exist in this checkout. The
  operative architecture reference is `docs/axinite-architecture-overview.md`.
- 2026-04-14: The repository already has a trace-backed end-to-end rig that can
  inspect captured LLM requests through `TestRig::captured_llm_requests()`,
  which makes the non-hosted first-call proof much more direct than a pure
  registry or reasoning-context assertion.
- 2026-04-14: `docs/roadmap.md` marks `1.2.2` complete, but the corresponding
  ExecPlan file still says `IN PROGRESS`. Treat adjacent documentation status
  carefully when closing `1.2.4`.
- 2026-04-15T17:57:00+02:00: `TraceLlm` currently captures only
  `ToolCompletionRequest.messages`, not the `tools` vector. A truthful
  first-call assertion therefore requires either widening that capture or
  using a narrow purpose-built capturing LLM in the behavioural test.
- 2026-04-15T17:57:00+02:00: The hosted path already has an exact first-call
  observation seam: `WorkerRuntime` uses `ProxyLlmProvider`, which forwards the
  worker's first `ToolCompletionRequest` into
  `WorkerHttpClient::llm_complete_with_tools()` and the shared
  `ProxyToolCompletionRequest` transport type.
- 2026-04-15T17:57:00+02:00: The repository currently has no `rstest-bdd`
  scenarios under `src/` or `tests/`, so adding one here would introduce a new
  feature-file and step-definition family rather than reusing an existing BDD
  harness.
- 2026-04-15T18:10:00+02:00: The smallest truthful non-hosted seam was a
  purpose-built capturing LLM wired through `TestRigBuilder::with_llm(...)`.
  That still exercises the real agent loop, real `Reasoning::respond_with_tools`
  path, and real GitHub WASM registration helper while recording the first
  `ToolCompletionRequest.tools` payload directly.
- 2026-04-15T18:10:00+02:00: The smallest truthful hosted seam was an Axum
  test server that served the remote-tool catalogue and captured the worker's
  first proxied `POST /worker/{job_id}/llm/complete_with_tools` payload.
- 2026-04-15T18:15:00+02:00: `FEATURE_PARITY.md` was reviewed during
  documentation sync. No parity row described this WASM-schema contract
  precisely enough to require an update.

## Decision Log

- 2026-04-14: Use `docs/axinite-architecture-overview.md` in place of the
  missing `docs/axinite-architecture-summary.md`.
  Rationale: it is the available maintainer-facing architecture reference in
  this checkout and is already named as the fallback in adjacent WASM
  ExecPlans.

- 2026-04-14: Treat the non-hosted behavioural layer as a trace-backed
  end-to-end request-capture test, not merely a reasoning-context assertion.
  Rationale: RFC 0002 explicitly requires proof that the first request includes
  the schema, and the trace rig can observe that request directly.
- 2026-04-15T17:57:00+02:00: Implement the non-hosted first-call proof with a
  narrow capturing LLM provider wired through `TestRigBuilder::with_llm(...)`
  while still exercising the real agent loop and real GitHub WASM registration
  helper.
  Rationale: this keeps the behavioural test end to end, avoids widening
  shared trace support beyond what this slice needs, and records the actual
  `ToolCompletionRequest.tools` payload that RFC 0002 cares about.

- 2026-04-14: Prefer a narrow hosted request-capture seam around
  `llm_complete_with_tools` instead of inventing a new hosted test framework.
  Rationale: this keeps the existing worker-orchestrator transport as the only
  contract boundary and follows the repository's dependency-injection testing
  guidance.
- 2026-04-15T17:57:00+02:00: Drive the hosted proof through
  `Reasoning::respond_with_tools(...)` plus `WorkerRuntime`'s existing proxied
  LLM field, backed by an Axum test server that captures the first
  `ProxyToolCompletionRequest`.
  Rationale: this observes the real first hosted request without introducing a
  second transport seam or reaching directly into intermediate worker state.

- 2026-04-14: `rstest-bdd` remains conditional rather than mandatory for this
  slice.
  Rationale: the repository already has strong `rstest` and trace-backed
  harnesses, and a BDD scenario is only justified if it adds new observable
  value without disproportionate scaffolding.
- 2026-04-15T17:57:00+02:00: Do not add `rstest-bdd` coverage for this slice.
  Rationale: there is no existing BDD harness to extend, the contract is more
  directly observable through `rstest`-driven captured request payloads, and
  adding feature files plus step definitions would exceed the proportionality
  bar set in Milestone 5.
- 2026-04-15T18:10:00+02:00: Keep the non-hosted first-call capture local to
  the new behavioural test instead of widening shared trace-support APIs.
  Rationale: only one roadmap item currently needs direct inspection of
  `ToolCompletionRequest.tools`, and a local capturing provider preserved a
  smaller blast radius than changing shared support used by many unrelated
  trace tests.

## Outcomes & Retrospective

Delivered the roadmap item with one new non-hosted end-to-end behavioural test,
one new hosted first-call forwarding test, and one transport round-trip test
for the shared proxied tool-completion request. The non-hosted proof uses the
real agent loop plus the real GitHub WASM registration helper and records the
first `ToolCompletionRequest.tools` payload before any tool executes. The
hosted proof uses the real worker proxy LLM path and records the first
`ProxyToolCompletionRequest` sent across the worker-orchestrator boundary.

Validation evidence:

- `cargo test first_llm_request_includes_advertised_schema_for_active_wasm_tool`
  passed and wrote `/tmp/test-axinite-feat-plan-wasm-schema-e2e-first-call.out`.
- `cargo test hosted_worker_first_llm_request_forwards_wasm_schema_on_first_call`
  passed and wrote
  `/tmp/test-axinite-feat-plan-wasm-schema-e2e-hosted-first-call.out`.
- `cargo test worker_tool_completion_request_round_trips_through_shared_types`
  passed and wrote
  `/tmp/test-axinite-feat-plan-wasm-schema-e2e-transport-parity.out`.
- `cargo test malformed_first_call_returns_fallback_guidance` passed and wrote
  `/tmp/test-axinite-feat-plan-wasm-schema-e2e-fallback-guidance.out`.
- `make all` passed and wrote
  `/tmp/make-all-axinite-feat-plan-wasm-schema-e2e.out`.
- `bunx markdownlint-cli2 ...` passed and wrote
  `/tmp/markdownlint-axinite-feat-plan-wasm-schema-e2e.out`.
- `git diff --check` passed and wrote
  `/tmp/diff-check-axinite-feat-plan-wasm-schema-e2e.out`.

Key lesson: the most reliable way to prove "first call already has the schema"
is to capture the actual tool-capable request object at the LLM boundary, not
to infer behaviour from registry state or reasoning-context assembly alone. In
this repository that meant using two different narrow seams: a custom
capturing provider for the in-process agent loop and an Axum capture stub for
the hosted worker boundary. That kept the change proportional while still
producing end-to-end evidence.
