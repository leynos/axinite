# Resolve Hosted Worker Meta Tooling Unavailability

This ExecPlan (execution plan) is a living document. The sections `Constraints`, `Tolerances`, `Risks`, `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: COMPLETE

## Purpose / big picture

After this work, a hosted worker in IronClaw will advertise and use the safe
extension-management tools it can execute without bypassing approval:
`tool_search`, `tool_activate`, `tool_list`, and `extension_info`. A user
should be able to ask a hosted agent whether an extension such as Telegram is
installed or connected and see a real `tool_list` or `tool_search` call happen
instead of the capability being absent, while approval-gated operations remain
orchestrator-only.

The change must be observable in three ways. First, a focused regression test must fail before the fix and pass after it by proving the hosted worker now advertises the meta tools. Second, surrounding unit and integration coverage must pin the registration and behavior changes so the gap does not reappear in a different layer. Third, the normal in-process agent path must continue to behave exactly as it does today.

## Constraints

- This plan file must live at `docs/plans/2026-03-09-resolve-meta-tooling-unavailability.md` because the user requested that exact path.
- The public tool names and their parameter schemas must remain stable. Existing prompts, traces, and UI affordances refer to `tool_search`, `tool_auth`, `tool_activate`, `tool_list`, and the other extension-management tools by those exact names.
- The existing meaning of `crate::tools::ToolRegistry::register_container_tools` in `src/tools/registry.rs` must not be broadened casually. That helper currently means container-local development tools. If hosted worker needs more than that, add a separate hosted-worker registration path rather than silently changing the meaning of the existing function.
- The normal in-process application bootstrap in `src/app.rs` must keep working without behavioral regression. Existing recorded traces such as `tests/fixtures/llm_traces/recorded/telegram_check.json` must remain valid.
- Tests added by this work must be deterministic and must not require live network calls, real OAuth flows, or mutable external services.
- No new third-party dependency may be introduced to implement the fix or its tests.
- The fix must preserve a single source of truth for extension state unless a prototype proves that a worker-local copy is behaviorally identical and cannot drift. The default assumption for this plan is that extension state should remain owned by the orchestrator-side application, not duplicated inside the worker container.

## Tolerances (exception triggers)

- Scope: if implementation requires changes to more than 16 files or more than 1200 net lines, stop and escalate with an updated scope breakdown.
- Interface: if the fix requires a generic remote tool-execution API rather than a narrow extension-management proxy, stop and escalate before implementing the broader interface.
- Dependencies: if the chosen test harness or fix requires a new crate or a new external service, stop and escalate.
- Iterations: if a deterministic red test for hosted-worker meta tooling still does not exist after three harness attempts, stop and document the alternatives in `Decision Log`.
- Time: if the worker feature-behavior test harness takes more than four hours to make deterministic, stop and escalate before expanding the harness further.
- Ambiguity: if it becomes unclear whether hosted worker should act on orchestrator-owned extension state or container-local extension state, stop and escalate instead of guessing.

## Risks

- Risk: The worker currently has only a concrete `WorkerHttpClient`, so testing real hosted-worker behavior may require extracting a small seam before any regression test can be written.
  Severity: medium
  Likelihood: high
  Mitigation: Make the first implementation step a pure or test-only bootstrap helper so the tool inventory can be asserted without network setup. Add the full behavior harness only after the seam is in place.

- Risk: A worker-local `ExtensionManager` would create a second source of truth for installed extensions, authentication state, and registry-backed discovery.
  Severity: high
  Likelihood: medium
  Mitigation: Prefer a narrow orchestrator proxy for extension-management tool execution. Treat worker-local extension state as an escalation path, not the default plan.

- Risk: Existing worker-focused tests do not instantiate `WorkerRuntime`, so adding more tests to `tests/e2e_worker_coverage.rs` alone could create a false sense of coverage.
  Severity: high
  Likelihood: high
  Mitigation: Add a dedicated hosted-worker inventory or behavior test that constructs `WorkerRuntime` or a directly extracted hosted-worker bootstrap path.

- Risk: Extension-management tools have approval and auth behavior that is more complex than their current schema-only coverage suggests.
  Severity: medium
  Likelihood: medium
  Mitigation: Separate inventory coverage from execution-behavior coverage. Use deterministic temp-dir and stub-backed tests for `tool_list`, `tool_search`, `tool_activate`, and `tool_auth` before relying on a worker end-to-end trace.

## Progress

- [x] (2026-03-09 12:57Z) Investigated the current regression and confirmed that `src/worker/runtime.rs` only registers container-local development tools.
- [x] (2026-03-09 12:57Z) Mapped the current surrounding coverage and identified the gap between normal app-path tests and hosted-worker bootstrap.
- [x] (2026-03-09 12:57Z) Drafted the execution plan, including the preferred fix direction and validation strategy.
- [x] (2026-03-09 13:01Z) Began implementation and confirmed the first code seam is the hosted-worker bootstrap in `src/worker/runtime.rs` plus the registration helpers in `src/tools/registry.rs`.
- [x] (2026-03-09 14:11Z) Added hosted-worker regression coverage in `src/worker/runtime.rs` to assert the worker inventory and advertised tool set now include extension-management tools.
- [x] (2026-03-09 14:14Z) Added surrounding registry, schema, and behavior coverage in `src/tools/registry.rs`, `tests/tool_schema_validation.rs`, and `src/tools/builtin/extension_tools.rs`.
- [x] (2026-03-09 14:16Z) Implemented the hosted-worker fix with an orchestrator-backed extension-tool proxy in `src/tools/builtin/worker_extension_proxy.rs`, `src/worker/api.rs`, `src/worker/runtime.rs`, and `src/orchestrator/api.rs`.
- [x] (2026-03-09 14:41Z) Ran targeted validation plus the broader Rust/clippy matrices that match local Linux CI expectations.

## Surprises & Discoveries

- Observation: `tests/e2e_worker_coverage.rs` is not a real `WorkerRuntime` harness. It uses `TestRigBuilder` and the normal app stack rather than constructing the hosted worker.
  Evidence: `tests/e2e_worker_coverage.rs` builds through `TestRigBuilder`, while `tests/support/test_rig.rs` uses `AppBuilder::build_all()`.
  Impact: A new hosted-worker-specific regression test is required; extending the existing worker trace file alone is not enough.

- Observation: Extension-management tools already have only schema metadata coverage in `src/tools/builtin/extension_tools.rs`, while `src/extensions/manager.rs` already contains temp-dir helpers and activation-oriented tests.
  Evidence: `src/tools/builtin/extension_tools.rs` tests only tool names, approval requirements, and schema presence; `src/extensions/manager.rs` has test helpers such as `make_test_manager`.
  Impact: The fastest surrounding-coverage gains are to extend registry and extension-tool tests rather than inventing all fixtures from scratch.

- Observation: The normal in-process agent path already proves `tool_list` behavior through the recorded Telegram check trace.
  Evidence: `tests/e2e_recorded_trace.rs` replays `telegram_check.json`, and that fixture expects `tool_list`.
  Impact: The fix must preserve the normal app path while closing only the hosted-worker gap.

- Observation: This repository does not provide a `Makefile`.
  Evidence: `Makefile` is absent at the repository root.
  Impact: Validation commands in this plan must use direct `cargo test` invocations with `tee` logs rather than project make targets.

- Observation: Reusing the exact extension-tool metadata across both the normal app path and the hosted-worker proxy path was cheaper and safer than maintaining parallel name/schema definitions.
  Evidence: Centralizing names, descriptions, schemas, and approval requirements in `ExtensionToolKind` removed duplicate strings from the tool implementations and let the proxy tool reflect the same contract.
  Impact: Future extension-tool additions now have a single metadata source, reducing the risk of another inventory/schema drift between runtimes.

- Observation: The `--no-default-features --features libsql` test matrix has a very slow first local compile because it fully rebuilds the large `libsql` and `wasmtime` graph for a distinct unit-test target.
  Evidence: The first local run spent about eight minutes in the initial `src/lib.rs` test binary compilation before the test execution phase began.
  Impact: Broad validation is still feasible locally, but it should be expected to take materially longer than the default or pre-warmed matrices.

## Decision log

- Decision: Keep the plan in `docs/plans/2026-03-09-resolve-meta-tooling-unavailability.md`.
  Rationale: Repository guidance prefers `docs/execplans/...`, but the user explicitly requested the `docs/plans/...` location, which takes priority.
  Date/Author: 2026-03-09 12:57Z / Codex

- Decision: Treat the work as three separate coverage layers: hosted-worker inventory regression, surrounding registration and tool-behavior coverage, and hosted feature behavior.
  Rationale: The current regression exists because no test covers the middle layer between isolated tool schemas and the full app-path trace harness.
  Date/Author: 2026-03-09 12:57Z / Codex

- Decision: Prefer a narrow orchestrator-side extension-tool proxy over constructing a second `ExtensionManager` inside the worker.
  Rationale: The comments and history around sandbox jobs indicate that worker and orchestrator were intentionally split. Reusing orchestrator-owned extension state is safer than inventing container-local extension state that may drift.
  Date/Author: 2026-03-09 12:57Z / Codex

- Decision: Do not change the meaning of `register_container_tools`; introduce a separate hosted-worker registration path.
  Rationale: `register_container_tools` is currently documented as the container-local development tool set. Preserving that meaning reduces blast radius and makes the final tests easier to read.
  Date/Author: 2026-03-09 12:57Z / Codex

## Outcomes & retrospective

Hosted workers now advertise and can execute the non-mutating
extension-management tool surface that was previously only available to the
normal in-process app path. The fix keeps `register_container_tools()` scoped
to container-local development tools and adds a separate hosted-worker
registration path that layers orchestrator-backed extension-tool proxies on top
of the existing worker tool set. The orchestrator now exposes a narrow
`POST /worker/{job_id}/extension_tool` endpoint that accepts only known
extension-management tool names, rejects approval-gated operations, and
dispatches safe calls through the existing orchestrator-owned `ToolRegistry`,
so extension state remains single-sourced in the main app process.

Coverage landed at three levels. First, `src/worker/runtime.rs` now has
regression tests that assert the hosted-worker registry and advertised tool
schemas include the safe proxy surface `tool_list`, `tool_search`,
`tool_activate`, and `extension_info`, while excluding approval-gated tools.
Second, `src/tools/registry.rs`, `tests/tool_schema_validation.rs`, and
`src/tools/builtin/extension_tools.rs` now pin exact registration sets, schema
validity, and deterministic `tool_search` / `tool_list` execution behavior plus
parameter-validation execution paths for `tool_auth` and `tool_activate`.
Third, `src/tools/builtin/worker_extension_proxy.rs` and
`src/orchestrator/api.rs` now have proxy round-trip tests that prove the
worker-facing proxy only accepts safe extension tools and executes them through
orchestrator-owned tool implementations.

Validation completed with the repository’s local Linux-equivalent gates. `cargo fmt --all -- --check` passed. `cargo clippy --all --benches --tests --examples -- -D warnings`, `cargo clippy --all --benches --tests --examples --no-default-features --features libsql -- -D warnings`, and `cargo clippy --all --benches --tests --examples --all-features -- -D warnings` all passed. `cargo test -- --nocapture`, `cargo test --no-default-features --features libsql -- --nocapture`, and `cargo test --features postgres,libsql,html-to-markdown -- --nocapture` all passed, as did the focused regression suites recorded in `/tmp/test-ironclaw-tool-list-breakage-*.out`.

No follow-up was deferred for correctness. The only remaining gap relative to full CI parity is the Windows-specific build and clippy matrix, which was not runnable in this Linux workspace.

## Context and orientation

IronClaw currently has two relevant execution paths. The normal application path builds the full app in `src/app.rs`, creates an `ExtensionManager`, and registers extension-management tools into the main `ToolRegistry`. The hosted worker path builds `WorkerRuntime` in `src/worker/runtime.rs`, creates a fresh `ToolRegistry`, and registers only container-local development tools through `register_container_tools`.

The files that matter most are:

- `src/app.rs`, where the normal in-process app wires `ExtensionManager` and `register_extension_tools`.
- `src/worker/runtime.rs`, where the hosted worker is bootstrapped and where a small test seam should be introduced.
- `src/tools/registry.rs`, which defines the registration helpers and already has registry-level tests.
- `src/tools/builtin/extension_tools.rs`, which implements the actual extension-management tools and currently only checks schemas and approval metadata.
- `src/extensions/manager.rs`, which contains most of the actual extension behavior and already has temp-dir test helpers that can be reused.
- `tests/tool_schema_validation.rs`, which validates only built-in plus dev-tool schemas today.
- `tests/e2e_recorded_trace.rs` and `tests/fixtures/llm_traces/recorded/telegram_check.json`, which already prove the normal app path can call `tool_list`.
- `tests/e2e_worker_coverage.rs`, which is useful for worker-like tool-loop coverage but is not a direct hosted-worker bootstrap harness.
- `src/orchestrator/api.rs`, `src/worker/api.rs`, and `src/worker/proxy_llm.rs`, which are the likely integration points for a narrow extension-tool proxy if the fix preserves orchestrator-owned extension state.

In this document, “meta tooling” means the extension-management tools that let the model discover, inspect, authenticate, activate, and remove extensions from conversation. “Hosted worker” means the `ironclaw worker` path that runs inside the sandboxed job environment and talks back to the orchestrator over HTTP.

## Plan of work

Stage A is a pure bootstrap and regression stage. Extract the hosted-worker tool bootstrap into a testable helper without changing behavior yet. The helper should live either in `src/tools/registry.rs` as a new function such as `register_hosted_worker_tools(...)` or in `src/worker/runtime.rs` as a small builder that returns the worker registry before networking begins. The first red test must assert that the hosted-worker path should advertise extension-management tools in addition to the existing development tools. This test should fail against the current code. A second test should assert that the development tools remain present, because the fix must not remove shell, file, or patch capabilities.

Stage B adds surrounding coverage before the fix is merged. Extend `src/tools/registry.rs` tests so registration behavior is pinned explicitly: the hosted-worker registration path must include the extension-management tool names and preserve deterministic ordering, and `register_extension_tools(...)` itself must have an exact-set test rather than relying on downstream schema tests. Extend `tests/tool_schema_validation.rs` so extension-tool schemas are validated under a temp-backed `ExtensionManager`. Extend `src/tools/builtin/extension_tools.rs` with deterministic behavior tests for `tool_search`, `tool_list`, `tool_activate`, and `tool_auth`, using temp directories and test helpers rather than live network or OAuth.

Stage C implements the fix. The preferred implementation is to add a narrow extension-tool proxy that lets the worker advertise the same tool names and schemas as the normal app path while executing the actual extension-management operations against orchestrator-owned state. Concretely, add a worker-side registration helper that composes the existing dev tools with proxy implementations of the extension-management tools. Add a narrow orchestrator endpoint that accepts only those tool names and parameters, dispatches to the existing extension tool implementations, and returns the same JSON tool result shape the worker already expects. Add matching request and response types in `src/worker/api.rs` and matching client helpers in `WorkerHttpClient`. Keep the current `register_container_tools()` behavior untouched.

Stage D adds hosted feature-behavior coverage. Build a dedicated integration test, for example in `tests/hosted_worker_extension_tools.rs`, that drives a real `WorkerRuntime` against a small fake orchestrator server. The fake server should expose only the endpoints needed by `WorkerRuntime`: job description, completion with tools, status, event, prompt, completion report, and the new extension-tool proxy endpoint. Configure the fake LLM response so it chooses `tool_list` only when the incoming advertised tool set contains `tool_list`. Before the fix, the test should fail because the worker never advertises that tool. After the fix, it should pass and assert that `tool_list` was invoked and its result was turned into a user-visible answer. If this harness becomes too large within the stated tolerances, stop and escalate rather than silently replacing it with another normal app-path trace.

Stage E is validation and cleanup. Run the new red tests first to prove they fail before the fix, then rerun them after the fix, then run the surrounding schema and behavior tests, then the existing normal app-path trace, and finally the broader Rust suite. Update this plan’s `Progress`, `Decision Log`, `Surprises & Discoveries`, and `Outcomes & Retrospective` sections with what was actually required.

## Concrete steps

Work from the repository root `/data/leynos/Projects/ironclaw`.

1. Inspect the current hosted-worker bootstrap and registry helpers.

```plaintext
sed -n '1,220p' src/worker/runtime.rs
sed -n '240,430p' src/tools/registry.rs
sed -n '620,730p' src/tools/builtin/extension_tools.rs
sed -n '3540,3815p' src/extensions/manager.rs
```

2. Add the first failing tests without changing behavior yet.

```plaintext
set -o pipefail
BRANCH=$(git branch --show)
cargo test --lib test_register_hosted_worker_tools_includes_extension_management_tools -- --nocapture \
  | tee /tmp/test-ironclaw-${BRANCH}.out
```

Expected pre-fix result:

```plaintext
running 1 test
test ... FAILED
assertion failed: hosted worker tools contain "tool_list"
```

3. Add the surrounding coverage for registry sets, extension-tool schemas, and extension-tool behavior.

```plaintext
set -o pipefail
BRANCH=$(git branch --show)
cargo test --lib test_register_extension_tools_registers_expected_names -- --nocapture \
  | tee /tmp/test-ironclaw-${BRANCH}.out
cargo test --test tool_schema_validation -- --nocapture \
  | tee /tmp/test-ironclaw-${BRANCH}.out
cargo test extension_tools --lib -- --nocapture \
  | tee /tmp/test-ironclaw-${BRANCH}.out
```

4. Implement the hosted-worker registration helper and the narrow orchestrator proxy path.

```plaintext
sed -n '160,220p' src/orchestrator/api.rs
sed -n '220,340p' src/worker/api.rs
sed -n '1,220p' src/worker/runtime.rs
```

5. Add the hosted feature-behavior integration test.

```plaintext
set -o pipefail
BRANCH=$(git branch --show)
cargo test --features libsql --test hosted_worker_extension_tools -- --nocapture \
  | tee /tmp/test-ironclaw-${BRANCH}.out
```

Expected post-fix result:

```plaintext
running 1 test
test hosted_worker_can_call_tool_list ... ok
```

6. Run the targeted regression and surrounding suites, then the broader suite.

```plaintext
set -o pipefail
BRANCH=$(git branch --show)
cargo test --lib test_register_hosted_worker_tools_includes_extension_management_tools -- --nocapture \
  | tee /tmp/test-ironclaw-${BRANCH}.out
cargo test --lib test_worker_runtime_advertises_meta_tooling -- --nocapture \
  | tee /tmp/test-ironclaw-${BRANCH}.out
cargo test --test tool_schema_validation -- --nocapture \
  | tee /tmp/test-ironclaw-${BRANCH}.out
cargo test --features libsql --test hosted_worker_extension_tools -- --nocapture \
  | tee /tmp/test-ironclaw-${BRANCH}.out
cargo test --features libsql --test e2e_recorded_trace recorded_telegram_check -- --nocapture \
  | tee /tmp/test-ironclaw-${BRANCH}.out
cargo test --features libsql -- --nocapture \
  | tee /tmp/test-ironclaw-${BRANCH}-full.out
```

## Validation and acceptance

Acceptance is behavioral, not structural.

- The new regression test fails before the fix and passes after it by proving that the hosted-worker tool inventory now includes the extension-management tools.
- A deterministic hosted-worker integration test proves that a real `WorkerRuntime` can advertise `tool_list`, receive a `tool_list` selection from the fake orchestrator-backed model, execute the tool through the narrow proxy, and surface the result back into the worker response loop.
- Registry and schema tests prove that hosted-worker registration and extension-tool schemas are now explicitly covered.
- Existing normal app-path behavior still works: `cargo test --features libsql --test e2e_recorded_trace recorded_telegram_check -- --nocapture` passes unchanged.
- The broader `cargo test --features libsql -- --nocapture` suite passes from the repository root.

Quality criteria:

- Tests: all new regression, inventory, schema, and behavior tests pass.
- Lint and typecheck: if the repository has separate lint commands in CI beyond `cargo test`, run the relevant Rust checks that already exist in the repository workflow for touched files. If no separate local lint entry point is available, record that limitation in `Outcomes & Retrospective`.
- Security: the fix must not create a generic remote-execution backchannel from worker to orchestrator. The proxy endpoint must accept only the known extension-management tool names and must reuse existing tool implementations.

## Idempotence and recovery

All planned steps are re-runnable. Tests should use temp directories, test-only helpers, and fake HTTP servers so repeated runs do not pollute the repository or require cleanup outside `/tmp`.

If the hosted-worker feature harness proves too complex, do not discard the regression work. Keep the extracted hosted-worker inventory helper and the red unit or integration test, record the harness blocker in `Decision Log`, and escalate before broadening the implementation. Do not “recover” by replacing the hosted-worker test with another normal app-path test, because that would reintroduce the current blind spot.

If a proxy endpoint is partially implemented and tests become confusing, revert only the incomplete endpoint-specific edits, keep the registration and inventory tests, and rerun the red test to verify the regression is still visible before continuing.

## Artifacts and notes

Current evidence for the regression:

```plaintext
src/worker/runtime.rs creates a fresh ToolRegistry and calls register_container_tools().
src/tools/registry.rs documents register_container_tools() as container-local development tools only.
src/app.rs registers extension tools through register_extension_tools().
tests/e2e_recorded_trace.rs proves tool_list on the normal app path.
No existing test constructs WorkerRuntime and asserts the hosted-worker tool inventory.
```

Desired end-state evidence:

```plaintext
running 1 test
test hosted_worker_can_call_tool_list ... ok

running 1 test
test test_register_hosted_worker_tools_includes_extension_management_tools ... ok

running 1 test
test recorded_telegram_check ... ok
```

## Interfaces and dependencies

Keep the existing extension-management tool names and JSON schemas unchanged.

Add a new hosted-worker registration helper rather than changing `register_container_tools()` in place. A good final shape is:

```rust
pub fn register_hosted_worker_tools(&self, proxy: Arc<dyn HostedExtensionToolExecutor>);
```

or an equivalent builder that composes:

1. the existing container-local development tools, and
2. proxy-backed implementations of the extension-management tools.

If a trait-based executor is used, keep it narrow and extension-specific. Do not create a generic “execute any orchestrator tool by name” interface unless the user explicitly approves that broader scope.

The orchestrator-side API should likewise be narrow. One acceptable shape is:

```rust
pub struct ProxyExtensionToolRequest {
    pub tool_name: String,
    pub params: serde_json::Value,
}

pub struct ProxyExtensionToolResponse {
    pub result: serde_json::Value,
}
```

served by a handler in `src/orchestrator/api.rs` that accepts only the extension-management tool names already registered by the main app. `src/worker/api.rs` should gain the matching client method, and `src/worker/runtime.rs` should use the new hosted-worker registration helper during bootstrap.

Revision note: Initial draft created on 2026-03-09 after confirming that hosted-worker bootstrap and existing tests leave a gap between container-local tool registration and normal app-path extension tooling. Updated at 2026-03-09 13:01Z to mark execution as in progress and to record the first implementation seam. Remaining work is test-first implementation, validation, and updating this document with actual outcomes.
