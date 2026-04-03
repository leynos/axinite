# Audit and fix WASM registration paths for proactive schema publication

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`, `Surprises & Discoveries`,
`Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work
proceeds.

Status: COMPLETE

## Purpose / big picture

After this work, every active WebAssembly (WASM) tool in axinite publishes a
real `ToolDefinition.parameters` schema before the large language model (LLM)
makes its first call. No active WASM tool may rely on a failure-path retry hint
as the primary way the model learns the tool's argument contract.

The change is observable in three ways:

1. Running `make test` passes, and every new test added by this plan is present
   and green. Deliberately removing the schema from a registration path causes
   at least one test to fail.
2. A WASM tool registered through any of the three registration paths
   (file-loaded, storage-backed, dev-build) exposes a non-placeholder
   `parameters` value in `ToolRegistry::tool_definitions()`.
3. The inline comment in `src/tools/wasm/wrapper.rs` that describes
   schema-in-error-hint as the normal contract is updated to describe it as
   supplemental fallback guidance.

This plan delivers roadmap item `1.2.1` and addresses
[RFC 0002 SS Current State](../rfcs/0002-expose-wasm-tool-definitions.md#current-state)
and
[RFC 0002 SS Migration Plan](../rfcs/0002-expose-wasm-tool-definitions.md#migration-plan)
steps 1 and 2.

## Approval gates

- Plan approved
  Acceptance criteria: the plan is scoped to auditing and fixing WASM
  registration paths, adding tests, updating the retry-hint comment contract,
  and synchronising documentation. No changes to the hosted remote-tool
  catalogue (that is `1.2.2`), no changes to provider-specific schema shaping,
  and no changes to the WIT interface.
  Sign-off: human reviewer approves this ExecPlan before implementation begins.

- Implementation complete
  Acceptance criteria: all milestones are complete, existing tests are
  unbroken, and documentation is synchronised.
  Sign-off: implementer marks the plan in progress and then complete after
  final validation.

- Validation passed
  Acceptance criteria: `make all` passes with retained logs, Markdown linting
  passes for changed documentation, and the final plan notes record validation
  evidence.
  Sign-off: implementer records final evidence immediately before commit.

- Docs synced
  Acceptance criteria: `docs/roadmap.md` marks `1.2.1` done,
  `docs/rfcs/0002-expose-wasm-tool-definitions.md` implementation status
  reflects the completed audit, `docs/contents.md` indexes this ExecPlan,
  `docs/users-guide.md` is reviewed, and this ExecPlan status is updated.
  Sign-off: implementer completes documentation sync as the final pre-commit
  checkpoint.

## Context and orientation

The following subsections describe the current state of WASM tool registration,
schema publication, and the retry-hint path. All paths are relative to the
repository root.

### The three WASM registration paths

Axinite registers WASM tools through three entry points, all of which converge
on `ToolRegistry::register_wasm()` in `src/tools/registry/wasm.rs`:

1. **File-loaded tools.** `WasmToolLoader::load_from_files()` in
   `src/tools/wasm/loader.rs` reads a `.wasm` file and an optional
   `.capabilities.json` sidecar. It passes `description: None` and
   `schema: None` to `WasmToolRegistration`, which means the registration path
   must recover metadata from the guest or fall back to placeholders.

2. **Storage-backed tools.** `ToolRegistry::register_wasm_from_storage()` in
   `src/tools/registry/wasm.rs` loads a tool record from the database. It
   passes `description` and `schema` from the stored record through
   `normalized_description()` and `normalized_schema()`, which strip empty or
   null values. When the stored schema is present and non-null, it becomes an
   explicit override and the guest export is not consulted.

3. **Dev-build tools.** `load_dev_tools()` in `src/tools/wasm/loader.rs`
   discovers build artefacts in `tools-src/` and delegates to
   `load_from_files()` with the same `description: None`, `schema: None`
   pattern.

4. **Runtime activation.** `ExtensionManager::activate_wasm_tool()` in
   `src/extensions/manager.rs` (line 2730) delegates to
   `WasmToolLoader::load_from_files()` with the same `None`/`None` pattern.

All four paths funnel through `ToolRegistry::register_wasm()`, which calls
`resolve_metadata_overrides()`. That function (lines 83-119 of
`src/tools/registry/wasm.rs`) attempts `wrapper.exported_metadata()` only when
no explicit override is provided. If the export recovery fails, the wrapper
retains placeholder metadata: `"WASM sandboxed tool"` for description and
`{"type": "object", "properties": {}, "additionalProperties": true}` for
schema.

### The placeholder schema problem

The placeholder schema in `src/tools/wasm/wrapper/metadata.rs` (lines 20-27)
is:

```json
{
  "type": "object",
  "properties": {},
  "additionalProperties": true
}
```

This schema tells the LLM "pass any JSON object", which is effectively no
contract at all. If `exported_metadata()` silently fails, the tool appears in
`ToolRegistry::tool_definitions()` with this placeholder, and the model must
guess the argument structure or wait for a failure hint.

### The retry-hint path

When a WASM tool returns an error, `WasmToolWrapper::execute_sync()` (line 698
of `src/tools/wasm/wrapper.rs`) calls `metadata::build_tool_hint()` to
re-instantiate the guest and read its `description()` and `schema()` exports.
The hint is embedded in the `WasmError::ToolReturnedError` variant and
displayed to the model.

The inline comment at line 698-701 still describes this as the normal
contract:

```rust
// Check for tool-level error -- on failure, call the WASM module's
// description() and schema() exports so the LLM can retry with the
// correct parameters without us having to include the (large) schema
// in every request's tools array.
```

That comment contradicts RFC 0002's design principle that the model should see
the correct schema before the first call. The comment must be updated to
describe the hint as supplemental fallback guidance.

### The Tool trait and ToolDefinition

The `NativeTool` trait in `src/tools/tool/traits.rs` requires
`parameters_schema() -> serde_json::Value`. `WasmToolWrapper` implements this
by returning `self.schema.clone()` (line 721 of `src/tools/wasm/wrapper.rs`).
The schema is set during construction and frozen thereafter.

`ToolRegistry::tool_definitions()` in `src/tools/registry/loader.rs`
(lines 165-179) maps every registered tool to a `ToolDefinition` by calling
`tool.parameters_schema()`. This is the path through which WASM tool schemas
reach the LLM.

### Existing test coverage

The following tests already exercise parts of the WASM schema path:

- `test_exported_metadata_from_real_github_component` in
  `src/tools/wasm/wrapper/metadata.rs` (line 256): proves that the GitHub
  WASM tool's guest exports yield a real description and schema.
- `wasm_tool_wrapper_reports_wasm_catalog_source` in the same file (line 294):
  proves the wrapper reports `HostedToolCatalogSource::Wasm`.
- `test_tool_definitions` in `src/tools/registry/tests.rs`: proves that
  registered tools appear in `tool_definitions()` with their schemas.
- `test_hosted_registry` in the same file: proves hosted-visible filtering
  works for WASM sources.

The gap is that no test asserts the end-to-end invariant: a WASM tool
registered through any of the three paths publishes a non-placeholder schema in
`tool_definitions()`.

### Reference documents

- [RFC 0002: Expose WASM tool definitions to LLMs](../rfcs/0002-expose-wasm-tool-definitions.md)
- [Roadmap item 1.2.1](../roadmap.md)
- `docs/rust-testing-with-rstest-fixtures.md` for `rstest` fixture patterns.
- `docs/rstest-bdd-users-guide.md` for `rstest-bdd` behavioural test patterns.
- `docs/reliable-testing-in-rust-via-dependency-injection.md` for DI testing.
- `docs/complexity-antipatterns-and-refactoring-strategies.md` for complexity
  management.
- `AGENTS.md` for repository quality gates and commit conventions.

## Constraints

- Do not change the `ToolDefinition` struct or the `NativeTool` trait.

- Do not change the WIT interface (`wit/tool.wit`) or the `wasmtime`
  component-model bindings.

- Do not change the hosted remote-tool catalogue or the worker-orchestrator
  transport. That work belongs to roadmap items `1.2.2` and later.

- Do not change provider-specific schema shaping. That is out of scope per
  RFC 0002 non-goals.

- Do not remove the retry-hint path from `WasmToolWrapper::execute_sync()`.
  RFC 0002 explicitly says hints should remain available for tool-level errors.
  The change is contractual (comment and documentation), not behavioural.

- All new test files must remain under 400 lines per `AGENTS.md`.

- Follow en-GB-oxendict spelling in comments and documentation.

- Tests must use `rstest` fixtures for shared setup and `mockall` where ad hoc
  mocks are needed.

- New or modified source files must pass `make all` (format, lint across the
  full clippy matrix, nextest).

- The `FEATURE_PARITY.md` file must be checked for any entry affected by this
  change and updated in the same branch if needed.

## Tolerances (exception triggers)

- Scope: if implementation requires changes to more than 10 files or 400 net
  new lines of code (excluding comments and blank lines), stop and verify that
  scope has not crept beyond the audit-and-fix boundary.

- Interface: if any public type, trait, or function signature outside test
  modules must change, stop and document the interface pressure before
  proceeding.

- Fixture complexity: if a single test fixture function exceeds 50 lines or a
  single test function exceeds 80 lines, stop and extract helpers.

- Iterations: if a test remains red after three focused debugging attempts,
  stop and document the failure in `Surprises & Discoveries`.

- BDD harness: if adding `rstest-bdd` behavioural coverage requires a new
  feature-test harness, external services, or Docker orchestration, fall back
  to in-process `rstest` integration tests and document the decision.

- Schema recovery failure: if `exported_metadata()` cannot be made reliable
  for any registration path (for example because the guest component cannot
  be instantiated during registration), stop and document the limitation
  rather than hiding it behind an unconditional fallback.

## Risks

- Risk: `exported_metadata()` may fail for some real WASM components because
  the guest's `description()` or `schema()` export triggers a trap or produces
  invalid JSON.
  Severity: medium
  Likelihood: low
  Mitigation: the existing `resolve_metadata_overrides()` already catches
  `exported_metadata()` failures and falls back to placeholders. The fix is not
  to remove that fallback, but to add a diagnostic warning when a placeholder
  schema reaches `tool_definitions()` and to ensure the happy path is
  exercised by tests.

- Risk: storage-backed tools may have null or empty schemas in the database,
  causing `normalized_schema()` to return `None` and triggering the
  placeholder fallback even though the guest export might have real metadata.
  Severity: medium
  Likelihood: medium
  Mitigation: the storage-backed path already passes the stored schema through
  `normalized_schema()` and then delegates to `register_wasm()`. If the stored
  schema is null, `resolve_metadata_overrides()` will attempt
  `exported_metadata()`. The test for this path should verify that a null
  stored schema triggers guest export recovery.

- Risk: the `metadata_test_runtime()` and `github_wasm_artifact()` test helpers
  in `src/testing_wasm.rs` depend on building the GitHub WASM tool from source.
  If the `wasm32-wasip2` target is not installed in the test environment, these
  tests will fail.
  Severity: low
  Likelihood: low
  Mitigation: the existing test infrastructure already handles this. The
  `github_wasm_artifact()` helper checks for the build artefact and builds it
  if missing. New tests should reuse the same fixture.

- Risk: tests asserting "non-placeholder schema" may be brittle if the
  placeholder format changes.
  Severity: low
  Likelihood: low
  Mitigation: define a helper predicate `is_placeholder_schema(value) -> bool`
  that checks for the specific placeholder shape, and use it in assertions.
  If the placeholder changes, only the predicate needs updating.

## Plan of work

### Milestone 1: audit registration paths and document findings

Before writing any code, perform a structured audit of all WASM registration
paths to identify exactly where a placeholder schema could reach
`tool_definitions()` without the model seeing real metadata.

Audit each path:

1. **File-loaded path** (`load_from_files` -> `register_wasm` ->
   `resolve_metadata_overrides`): description and schema are both `None`, so
   `exported_metadata()` is always attempted. If it succeeds, real metadata is
   used. If it fails, the placeholder survives. The failure is logged at
   `debug` level.

2. **Storage-backed path** (`register_wasm_from_storage` -> `register_wasm` ->
   `resolve_metadata_overrides`): the stored description and schema are
   normalised. If the stored schema is non-null, it becomes an explicit
   override and `exported_metadata()` is not called. If the stored schema is
   null, `exported_metadata()` is attempted as in the file-loaded path.

3. **Dev-build path** (`load_dev_tools` -> `load_from_files`): identical to the
   file-loaded path.

4. **Runtime activation path** (`activate_wasm_tool` ->
   `load_from_files`): identical to the file-loaded path.

The key finding from this audit is that the placeholder schema can survive in
`tool_definitions()` in exactly one scenario: `exported_metadata()` fails for a
component. The existing code handles this with a `debug`-level log, which is
insufficient. RFC 0002 requires that active WASM tools "never rely on a failure
path to teach the model their arguments."

The fix must:

1. Escalate the log level from `debug` to `warn` when a WASM tool falls back
   to placeholder metadata during registration, so operators can see and fix
   the problem.
2. Add a diagnostic method or helper that can detect placeholder schemas in the
   registry, enabling tests to assert the invariant.
3. Update the inline comment in `execute_sync()` to describe the retry-hint
   path as supplemental fallback guidance.
4. Add tests that prove every registration path produces a non-placeholder
   schema for a valid WASM component.

Milestone 1 deliverable: this audit is recorded in `Progress` below. No code
changes in this milestone.

### Milestone 2: add a placeholder-detection helper and escalate the log

Add a small helper to detect whether a `serde_json::Value` matches the
placeholder schema shape. Then escalate the log level in
`resolve_metadata_overrides()` when a WASM tool falls back to placeholder
metadata.

Location: `src/tools/wasm/wrapper/metadata.rs`.

Add:

```rust
/// Whether the given schema is the registration-time placeholder that
/// carries no real tool contract.
pub(crate) fn is_placeholder_schema(schema: &serde_json::Value) -> bool {
    *schema == placeholder_schema()
}
```

Location: `src/tools/registry/wasm.rs`, inside `resolve_metadata_overrides()`.

Change: escalate the `tracing::debug!` at line 102 to `tracing::warn!` when
the wrapper ends up with a placeholder schema after the recovery attempt fails.
The warning must name the tool and explain that the tool will be advertised with
a placeholder schema.

Add a unit test for `is_placeholder_schema()` in the metadata module.

### Milestone 3: update the retry-hint comment contract

Location: `src/tools/wasm/wrapper.rs`, lines 698-701.

Replace the existing comment:

```rust
// Check for tool-level error -- on failure, call the WASM module's
// description() and schema() exports so the LLM can retry with the
// correct parameters without us having to include the (large) schema
// in every request's tools array.
```

With a comment that describes the hint as supplemental fallback guidance,
consistent with RFC 0002:

```rust
// Check for tool-level error.  On failure, rebuild a compact hint from
// the guest's description() and schema() exports so the LLM receives
// recovery guidance.  This hint is supplemental: the model should
// already have the tool's parameter schema from the proactive
// ToolDefinition published at registration time.  See RFC 0002.
```

No behavioural change. The `build_tool_hint()` call and
`WasmError::ToolReturnedError` variant remain unchanged.

### Milestone 4: add unit tests for schema publication invariants

Add tests that prove every registration path produces a non-placeholder schema
for a valid WASM component. These tests exercise the `register_wasm()` and
`resolve_metadata_overrides()` seam.

Location: `src/tools/registry/wasm.rs` (tests module, or a sibling test file if
the module would exceed 400 lines).

Test 4a: `file_loaded_wasm_tool_publishes_real_schema`

Register the GitHub WASM tool through the file-loaded path (using the existing
`github_wasm_artifact()` fixture). Assert that the resulting `ToolDefinition`
in `tool_definitions()` has a `parameters` value that is not the placeholder
schema. Assert that `parameters["type"]` is `"object"` and that
`parameters["required"]` contains `"action"`.

Test 4b: `file_loaded_wasm_tool_publishes_real_description`

Same registration path. Assert that the `description` in `tool_definitions()`
is not the placeholder `"WASM sandboxed tool"` and contains `"GitHub"`.

Test 4c: `explicit_schema_override_wins_over_guest_export`

Register a WASM tool with an explicit `schema` override. Assert that the
resulting `ToolDefinition.parameters` matches the override exactly, not the
guest export.

Test 4d: `explicit_description_override_wins_over_guest_export`

Register a WASM tool with an explicit `description` override. Assert that the
resulting `ToolDefinition.description` matches the override exactly.

Test 4e: `storage_backed_null_schema_falls_through_to_guest_export`

Register a WASM tool through the storage-backed path simulation by passing
`schema: None` (simulating a null stored schema). Assert that
`resolve_metadata_overrides()` attempts `exported_metadata()` and the resulting
schema is non-placeholder.

Test 4f: `placeholder_schema_detection_helper_works`

Assert that `is_placeholder_schema(placeholder_schema())` returns `true` and
that `is_placeholder_schema(real_github_schema)` returns `false`.

### Milestone 5: evaluate and add behavioural tests

Evaluate whether `rstest-bdd` scenarios add value beyond the unit tests in
milestone 4. The candidate scenario is:

```gherkin
Feature: Proactive WASM schema publication

  Scenario: File-loaded WASM tool publishes real schema before first call
    Given a WASM tool compiled from guest source with description and schema
      exports
    When the tool is registered through the file-loaded path
    Then the tool definition in the registry has a non-placeholder parameters
      schema
    And the tool definition description is not the placeholder

  Scenario: Schema override takes precedence over guest export
    Given a WASM tool with guest-exported schema
    And an explicit schema override provided at registration
    When the tool is registered
    Then the tool definition parameters match the override exactly
```

If `rstest-bdd` is feasible in-process without new infrastructure, implement
these scenarios. Otherwise, the unit tests from milestone 4 already provide the
same guarantees; document the decision and move on.

### Milestone 6: synchronise design and operator documentation

1. Update `docs/rfcs/0002-expose-wasm-tool-definitions.md` to note that
   `1.2.1` is complete: registration paths have been audited, placeholder
   fallback logging has been escalated, and tests are in place.

2. Review `docs/users-guide.md`. The current text (lines 46-48) says
   "orchestrator-owned WebAssembly (WASM) tools, remain outside the
   hosted-visible catalogue until their roadmap items land." That statement
   remains correct because `1.2.1` does not change the hosted catalogue.
   Confirm no other wording needs adjustment.

3. Review `docs/axinite-architecture-overview.md` section 4.4 for any wording
   about WASM schema publication that should now reference the proactive
   contract. Update if needed.

4. Mark roadmap item `1.2.1` done in `docs/roadmap.md` by changing
   `- [ ] 1.2.1.` to `- [x] 1.2.1.`.

5. Add this ExecPlan to the `docs/contents.md` index under the ExecPlans
   directory listing.

6. Check `FEATURE_PARITY.md` for any entry related to WASM tool schema
   publication and update if the status has changed.

### Milestone 7: validate, gate, and publish

Run the following command pattern during implementation for targeted tests:

```bash
set -o pipefail && CARGO_BUILD_JOBS=1 cargo test <test_name> --lib \
  -- --nocapture | tee /tmp/unit-axinite-1-2-1.out
```

Then run the broader gates:

```bash
set -o pipefail && CARGO_BUILD_JOBS=1 make check-fmt \
  | tee /tmp/check-fmt-axinite-1-2-1.out
set -o pipefail && CARGO_BUILD_JOBS=1 make lint \
  | tee /tmp/lint-axinite-1-2-1.out
set -o pipefail && CARGO_BUILD_JOBS=1 make test \
  | tee /tmp/test-axinite-1-2-1.out
```

If documentation changes are made, also run:

```bash
set -o pipefail && bunx markdownlint-cli2 \
  docs/roadmap.md \
  docs/users-guide.md \
  docs/axinite-architecture-overview.md \
  docs/rfcs/0002-expose-wasm-tool-definitions.md \
  docs/execplans/1-2-1-audit-and-fix-wasm-registration-paths.md \
  docs/contents.md \
  | tee /tmp/markdownlint-axinite-1-2-1.out
git diff --check
```

Only after the gates pass should the implementation be committed. The commit
message must describe that WASM registration paths have been audited and fixed
for proactive schema publication per roadmap item `1.2.1`. The commit summary
must reference the issue if one is tracked.

## Concrete steps

### Step 1: add `is_placeholder_schema()` helper

In `src/tools/wasm/wrapper/metadata.rs`, after the existing
`placeholder_schema()` function, add:

```rust
/// Whether the given schema is the registration-time placeholder that
/// carries no real tool contract.
pub(crate) fn is_placeholder_schema(schema: &serde_json::Value) -> bool {
    *schema == placeholder_schema()
}
```

Add a test in the same file's `#[cfg(test)] mod tests` block:

```rust
#[test]
fn placeholder_schema_detection_identifies_placeholder() {
    assert!(super::is_placeholder_schema(&super::placeholder_schema()));
}

#[test]
fn placeholder_schema_detection_rejects_real_schema() {
    let real = serde_json::json!({
        "type": "object",
        "properties": {
            "action": { "type": "string" }
        },
        "required": ["action"]
    });
    assert!(!super::is_placeholder_schema(&real));
}
```

### Step 2: escalate the log in `resolve_metadata_overrides()`

In `src/tools/registry/wasm.rs`, inside `resolve_metadata_overrides()`, change
the `tracing::debug!` (line 102) to `tracing::warn!` and add the tool name and
a clear message:

```rust
Err(error) => {
    tracing::warn!(
        name = name,
        %error,
        "Failed to recover exported WASM metadata; \
         tool will be advertised with placeholder schema. \
         The model may not receive accurate parameter \
         guidance before first call."
    );
}
```

### Step 3: update the retry-hint comment

In `src/tools/wasm/wrapper.rs`, replace the comment block at lines 698-701 as
described in milestone 3.

### Step 4: add registration-path tests

In `src/tools/registry/wasm.rs` or a sibling test file, add the tests
described in milestone 4. Use `rstest` fixtures:

```rust
#[fixture]
async fn github_wasm_artifact_path() -> PathBuf {
    github_wasm_artifact().expect("build or find github WASM artifact")
}

#[fixture]
fn test_wasm_runtime() -> Arc<WasmToolRuntime> {
    metadata_test_runtime().expect("create metadata test runtime")
}

#[fixture]
fn test_registry() -> Arc<ToolRegistry> {
    Arc::new(ToolRegistry::new())
}
```

### Step 5: evaluate BDD and decide

Check whether `.feature` files exist in the project. If not, evaluate the cost
of adding the first `rstest-bdd` harness for this narrow scope. Record the
decision.

### Step 6: update documentation

Edit the files listed in milestone 6.

### Step 7: run gates

Run the commands listed in milestone 7. Record evidence in `Outcomes &
Retrospective`.

## Validation and acceptance

Quality criteria (what "done" means):

- Tests: `make test` passes. Every test named in milestone 4 exists and is
  green. Deliberately removing the `exported_metadata()` call in
  `resolve_metadata_overrides()` causes at least one test to fail.
- Lint/typecheck: `make check-fmt` and `make lint` pass without new warnings.
- Documentation: `bunx markdownlint-cli2` passes for all changed Markdown
  files. `git diff --check` shows no whitespace errors.
- Roadmap: item `1.2.1` is marked done. RFC 0002 reflects the completed audit.
- Retry-hint comment: the comment in `src/tools/wasm/wrapper.rs` describes the
  hint as supplemental fallback guidance.

Quality method (how we check):

```bash
set -o pipefail && CARGO_BUILD_JOBS=1 make all \
  | tee /tmp/make-all-axinite-1-2-1.out
```

## Idempotence and recovery

All steps are re-runnable. The `is_placeholder_schema()` helper is a pure
function. The log-level change is idempotent. The comment update is a text
replacement. Tests are additive and do not modify global state.

If a step fails halfway, the working tree can be reset to the last clean commit
and the step re-attempted.

## Interfaces and dependencies

### New helpers

In `src/tools/wasm/wrapper/metadata.rs`:

```rust
pub(crate) fn is_placeholder_schema(schema: &serde_json::Value) -> bool;
```

### Existing interfaces consumed (no changes)

- `WasmToolWrapper::exported_metadata(&self) ->
  Result<(String, Value), WasmError>`
- `ToolRegistry::register_wasm(&self, reg: WasmToolRegistration<'_>) ->
  Result<(), WasmError>`
- `ToolRegistry::tool_definitions(&self) -> Vec<ToolDefinition>`
- `metadata::placeholder_schema() -> serde_json::Value`
- `metadata::placeholder_description() -> String`
- `metadata::build_tool_hint(...) -> String`

### Test fixtures consumed (no changes)

- `github_wasm_artifact() -> Result<PathBuf>` in `src/testing_wasm.rs`
- `metadata_test_runtime() -> Result<Arc<WasmToolRuntime>>` in
  `src/testing_wasm.rs`

### Dependencies (no new external crates)

- `rstest` (existing) for test fixtures
- `serde_json` (existing) for schema comparison
- `tracing` (existing) for log escalation

## Progress

- [x] Audit registration paths and document findings (milestone 1).
- [x] Add `is_placeholder_schema()` helper and escalate the log (milestone 2).
- [x] Update the retry-hint comment contract (milestone 3).
- [x] Add unit tests for schema publication invariants (milestone 4).
- [x] Evaluate and add behavioural tests (milestone 5).
- [x] Synchronise documentation (milestone 6).
- [x] Run full validation gates and publish (milestone 7).

## Surprises & Discoveries

- The compiled `ToolRegistry::register_wasm()` and
  `register_wasm_from_storage()` paths live in `src/tools/registry/loader.rs`,
  while `src/tools/registry/wasm.rs` contains a duplicate helper
  implementation that is not wired into `src/tools/registry.rs`.
- The live storage-backed path was still passing empty descriptions and
  `null` schemas through as explicit overrides, which suppressed guest
  metadata recovery until `normalized_description()` and
  `normalized_schema()` were added to the live registration path.
- No existing `rstest-bdd` harness was available for this seam, and adding a
  first behavioural harness would have expanded scope beyond this audit.

## Decision Log

- Decision: keep the new placeholder-schema helper test-only.
  Why: the helper exists to lock down the placeholder shape in regression
  tests, and compiling it only for tests avoids dead-code suppressions under
  the repository's strict clippy policy.
- Decision: do not add a new BDD harness for roadmap item `1.2.1`.
  Why: the path is already covered by in-process Rust registration tests for
  file-loaded, storage-backed, and dev-build flows, and the first `rstest-bdd`
  harness would not have improved confidence enough to justify the added
  surface area.
- Decision: review `docs/users-guide.md` without changing it.
  Why: the user guide currently documents hosted MCP catalogue behaviour, and
  this roadmap item only changes in-process WASM registration plus internal
  contract wording.

## Outcomes & Retrospective

- Implemented the live registration-path fix in
  `src/tools/registry/loader.rs`, including:
  - warning-level observability when a WASM tool remains on a placeholder
    schema after metadata recovery fails
  - storage-backed normalisation so empty descriptions and `null` schemas fall
    back to guest-exported metadata instead of suppressing it
- Updated the retry-hint contract comment in `src/tools/wasm/wrapper.rs` to
  describe the schema-bearing hint as supplemental recovery guidance.
- Added regression coverage for:
  - placeholder-schema detection in
    `src/tools/wasm/wrapper/metadata.rs`
  - file-loaded and dev-build schema publication in
    `src/tools/wasm/loader.rs`
  - storage-backed schema publication in
    `src/tools/registry/wasm_registration_tests.rs`
- Synchronized the documentation in:
  - `docs/roadmap.md`
  - `docs/rfcs/0002-expose-wasm-tool-definitions.md`
  - `docs/contents.md`
- Validation evidence:
  - `set -o pipefail && CARGO_BUILD_JOBS=1 make all 2>&1 | tee /tmp/axinite-1-2-1-make-all.log`
  - `set -o pipefail && CARGO_BUILD_JOBS=1 cargo test
    storage_publishes_guest_schema --lib -- --nocapture 2>&1 | tee
    /tmp/axinite-1-2-1-storage-schema.log`
  - previously targeted regression runs for placeholder detection, file-loaded
    publication, and dev-build publication completed successfully
