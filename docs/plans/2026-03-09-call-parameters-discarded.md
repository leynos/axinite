# Fix Discarded Call Parameters For File-Loaded WASM Tools

This ExecPlan (execution plan) is a living document. The sections `Constraints`, `Tolerances`, `Risks`, `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: COMPLETE

## Purpose / big picture

After this work, when IronClaw advertises a file-loaded WebAssembly (WASM) tool
such as `GitHub`, the external tool definition must expose the same parameter
schema that the tool itself exports from its WASM `schema()` function. The
user-visible outcome is that the model sees a non-empty function definition,
includes the correct arguments in tool calls, and those arguments arrive at the
WASM guest intact instead of being collapsed to `{}` or omitted.

Success is observable in two complementary ways. First, a focused regression test must prove that a file-loaded WASM tool registered through `WasmToolLoader::load_from_files(...)` publishes the exported schema in `ToolRegistry::tool_definitions()` rather than the current placeholder schema with empty `properties`. Second, a behavioural test must prove that the externally visible tool definition for a representative WASM tool includes required fields such as `action`, so the tool call path no longer fails with messages like `missing field action at line 1 column 2` when the model attempted to send arguments.

## Repository orientation

The relevant code paths are:

- `src/tools/wasm/runtime.rs`, which now prepares WASM components without
  caching tool metadata and leaves metadata recovery to the wrapper-side
  registration path.
- `src/tools/registry.rs`, which builds `ToolDefinition` objects from `tool.parameters_schema()` and also has two different WASM registration paths: direct registration and storage-backed registration.
- `src/tools/wasm/loader.rs`, which loads tools from `.wasm` files and sidecar capabilities files, then calls `register_wasm(...)` with `schema: None`.
- `src/tools/wasm/storage.rs`, which persists `parameters_schema` and lets `register_wasm_from_storage(...)` override the wrapper schema from storage.
- `tools-src/github/src/lib.rs`, which exports the real JSON schema string for the GitHub tool.
- `tests/tool_schema_validation.rs` and `src/tools/schema_validator.rs`, which validate schema shapes today but do not currently prove that file-loaded real WASM tools surface their exported schema through the external tool-definition path.

The key asymmetry was initially visible in code. Storage-backed activation used
`register_wasm_from_storage(...)` and passed
`schema: Some(tool_with_binary.tool.parameters_schema.clone())`, so that path
could override placeholder metadata. File-based loading through
`WasmToolLoader::load_from_files(...)` passed `schema: None`, which left the
`WasmToolWrapper` publishing placeholder metadata until registration-side
recovery was added.

## Change history and likely intention

The placeholder schema extractor was introduced when `src/tools/wasm/runtime.rs` itself was created in commit `aea3f47f8b24e7274dd128a14b8a283fd38a9d00` (`Implementing WASM runtime`). A semantic diff of that newly added file shows `extract_tool_description(...)` and `extract_tool_schema(...)` being introduced together with explicit comments that they are temporary scaffolding: they do not call the WASM guest exports yet and instead return `"WASM sandboxed tool"` plus a minimal object schema with empty `properties` and `additionalProperties: true`. The likely purpose of that change was initial runtime bring-up: get compilation, caching, and execution working before proper metadata extraction existed.

The user-facing bug appears when that scaffolding leaks into the file-loader path. `src/tools/wasm/loader.rs` was introduced in commit `4ae59ef52c3a23977e333d0182af418095b4bc1f` (`Fix TuiChannel integration and enable in main.rs`), and its semantic diff shows `WasmToolLoader::load_from_files(...)` registering tools with `description: None` and `schema: None`. That was reasonable if runtime metadata extraction were trustworthy, but combined with the placeholder extractor from `aea3f47`, it means file-loaded tools publish an empty external schema even though the guest still exports a real `schema()` string internally. The likely intention of the loader change was to add runtime file loading of WASM tools, not to discard tool parameters.

There is also a later comparison point that narrows blast radius. Storage-backed registration in `ToolRegistry::register_wasm_from_storage(...)` now explicitly passes stored `parameters_schema` into `register_wasm(...)`, which suggests the maintainers already recognized that schema overrides may be needed at registration time. That raises confidence that fixing the file-loader path to use real exported schema is a correction toward the intended contract rather than a broad redesign.

## Constraints

- This plan file must live at `docs/plans/2026-03-09-call-parameters-discarded.md`.
- The public tool-calling interface must remain stable. Tool names, descriptions, and JSON parameter schemas should become more accurate, not structurally redefined beyond what the WASM guest already exports.
- The fix must preserve both file-loaded and storage-loaded WASM tool paths. Do not repair one path by regressing the other.
- Avoid changing the WIT host contract unless the investigation proves that proper metadata extraction is impossible without it. The current evidence points to registration/metadata plumbing, not WIT surface mismatch.
- Do not weaken strict schema validation or hide the problem by making the caller more permissive. The correct outcome is that external tool definitions publish the real schema so arguments flow correctly.
- Keep the fix narrow. The issue is about metadata extraction and registration, not tool execution semantics, auth, or extension activation policy.
- No new third-party dependency may be introduced.

## Tolerances (exception triggers)

- Scope: if the smallest credible fix requires touching more than 10 files or more than 400 net lines, stop and escalate with a breakdown. The expected fix is much smaller.
- Interface: if fixing metadata extraction requires changing the WIT world, the `Tool` trait, or the external `ToolDefinition` shape, stop and escalate before implementation.
- Runtime: if proper schema extraction from a compiled component requires instantiating full tools with live network/auth side effects during registration, stop and document the trade-offs before proceeding.
- Coverage: if a deterministic failing test cannot be written for the file-loader path after three targeted attempts, stop and record what makes the path hard to observe.
- Ambiguity: if there are multiple active registration paths for the same curated tools and it is unclear which one the user-facing system actually uses in this branch, stop and present the exact paths with evidence.

## Risks

- Risk: Implementing real schema extraction too early inside `WasmToolRuntime::prepare(...)` may require a lightweight instantiation path that could accidentally introduce side effects or dependency on runtime-only host state.
  Severity: high
  Likelihood: medium
  Mitigation: Keep extraction scoped to calling pure metadata exports such as `description()` and `schema()` only, with minimal host scaffolding and no tool `execute(...)` invocation.

- Risk: Fixing only the runtime placeholder extractor may still leave some loader or storage path using stale overrides or inconsistent precedence.
  Severity: medium
  Likelihood: medium
  Mitigation: Add tests that cover both direct file loading and storage-backed registration precedence, and document the intended order explicitly.

- Risk: Existing schema validation tests use representative schemas or already-overridden paths, so they may stay green even while real file-loaded tools continue to publish `{}` externally.
  Severity: high
  Likelihood: high
  Mitigation: Add a failing test that exercises a real file-loaded WASM tool and inspects `ToolRegistry::tool_definitions()` directly.

- Risk: The GitHub tool is only one representative. A fix that special-cases GitHub would miss other file-loaded WASM tools.
  Severity: high
  Likelihood: low
  Mitigation: Use GitHub as the regression reproducer because it is already reported, but implement and validate the fix generically at the runtime/loader layer.

## Milestone 1: Reproduce the exact registration failure with a real file-loaded WASM tool

Begin with the user-visible failure mode, not a synthetic schema object.

1. Load a real WASM tool through the file-loader path, ideally the in-tree
   `GitHub` tool artifact or a minimal fixture WASM tool that exports a
   required `action` field.
2. Call `ToolRegistry::tool_definitions()` and inspect the published `parameters` for that tool.
3. Assert that the current code exposes the placeholder shape from `extract_tool_schema(...)`:

    ```json
    {
      "type": "object",
      "properties": {},
      "additionalProperties": true
    }
    ```

4. Assert that this differs materially from the guest-exported schema, which should include required fields such as `action`.

This must be a failing test before the fix. It should demonstrate the exact symptom the model reported: the external definition is empty even though the guest knows its schema.

Suggested command:

```bash
set -o pipefail
BRANCH=$(git branch --show-current | tr '/' '-')
cargo test file_loaded_wasm_tool_exposes_exported_schema -- --nocapture \
  2>&1 | tee /tmp/test-wasm-file-schema-red-ironclaw-${BRANCH}.out
```

Expected pre-fix evidence:

```plaintext
assertion failed: published schema contains required field "action"
published schema was {"type":"object","properties":{},"additionalProperties":true}
```

## Milestone 2: Identify and implement the narrow metadata-plumbing fix

Once the failing test exists, fix the path that produces the empty external definition.

There are two plausible repair points:

1. Teach `WasmToolRuntime::prepare(...)` to call the guest’s exported `description()` and `schema()` functions instead of using placeholder metadata.
2. If runtime extraction still needs to remain partial for some reason, make registration recover the guest-exported metadata after wrapper creation, where the full host linker and runtime limits already exist, and then apply those values before the tool is published externally.

The preferred solution is the one that makes file-loaded and storage-loaded
registration consistent around real metadata while keeping precedence clear.
Implementation evidence in this branch showed that forcing metadata extraction
inside `prepare(...)` against a minimal host was brittle on the real GitHub
component, while wrapper-side extraction after full host wiring worked. The
chosen fix therefore leaves runtime preparation side-effect-free and recovers
real metadata in the registration path before publication.

## Milestone 3: Guard the blast radius with unit and behavioural tests

The fix must land with both unit and behavioural coverage, and the failing testcase from Milestone 1 must stay as the permanent regression.

Required coverage:

1. A failing-then-passing regression proving that a real file-loaded WASM tool publishes its exported schema through `ToolRegistry::tool_definitions()`.
2. Unit coverage for the metadata extraction path itself, proving that `extract_tool_schema(...)` or its replacement returns the guest-exported JSON rather than the placeholder object.
3. Unit coverage for registration precedence, proving that storage-backed registration still respects explicit stored `parameters_schema` overrides where those are intentionally provided.
4. Behavioural coverage proving that a model-facing tool definition for a real WASM tool includes required fields such as `action` and `owner`/`repo` for GitHub-style actions, so arguments are not discarded at the external interface.
5. If the fix changes description extraction too, mirrored tests for descriptions so the same bug does not remain on that metadata surface.

Current coverage gap to close explicitly:

- `src/tools/schema_validator.rs` validates representative WASM schema shapes, not real schemas extracted from actual file-loaded WASM tools.
- `tests/tool_schema_validation.rs` validates built-in and extension schema validity, but it does not currently prove that `WasmToolLoader::load_from_files(...)` surfaces the guest’s exported schema through `ToolRegistry::tool_definitions()`.
- There is no current behavioural test that links real WASM schema export to the external function definition visible to the model.

## Milestone 4: Validate both registration paths and document precedence

After the fix and tests land:

1. Re-run the new file-loader regression and behavioural tests.
2. Re-run schema validation suites that cover built-in, extension, and representative WASM tool schemas.
3. Add or update an exact-set test showing how file-loaded versus storage-loaded registration determines the final wrapper schema.
4. Update this plan’s living sections with the actual precedence rule and any discovered trade-offs.

Suggested commands:

```bash
set -o pipefail
BRANCH=$(git branch --show-current | tr '/' '-')
cargo test --test tool_schema_validation -- --nocapture \
  2>&1 | tee /tmp/test-tool-schema-ironclaw-${BRANCH}.out
set -o pipefail
BRANCH=$(git branch --show-current | tr '/' '-')
cargo test wasm_tool_schemas --lib -- --nocapture \
  2>&1 | tee /tmp/test-wasm-schema-validator-ironclaw-${BRANCH}.out
```

## Concrete steps

Work from the repository root `/data/leynos/Projects/ironclaw`.

1. Confirm the current placeholder path and override asymmetry:

    ```plaintext
    nl -ba src/tools/wasm/runtime.rs | sed -n '260,355p'
    nl -ba src/tools/wasm/loader.rs | sed -n '145,170p'
    nl -ba src/tools/registry.rs | sed -n '643,675p'
    ```

1. Add the first failing test showing that a file-loaded real WASM tool publishes the placeholder schema externally.

1. Implement the narrow metadata extraction or override fix.

1. Add the remaining unit and behavioural coverage from Milestone 3.

1. Run the targeted suites with `tee`, review the logs, and update this plan with the actual outcome.

## Progress

- [x] 2026-03-09 21:35Z: Confirmed the current branch is `secret-blocking-overzealous` and collected the relevant planning and semantic-diff guidance.
- [x] 2026-03-09 21:37Z: Verified that `ToolRegistry::tool_definitions()` publishes `tool.parameters_schema()` directly, so the issue must originate before the registry emits function definitions.
- [x] 2026-03-09 21:39Z: Identified the likely root cause: `WasmToolLoader::load_from_files(...)` passes `schema: None`, which causes file-loaded tools to inherit the placeholder schema returned by `extract_tool_schema(...)` in `src/tools/wasm/runtime.rs`.
- [x] 2026-03-09 21:41Z: Verified that storage-backed registration is different: `register_wasm_from_storage(...)` passes the persisted `parameters_schema` as an explicit override, which explains why the file-loader path is the likely regression surface.
- [x] 2026-03-09 21:44Z: Used Git history, blame, and `sem` to trace the placeholder extractor to commit `aea3f47` (`Implementing WASM runtime`) and the file-loader registration path to commit `4ae59ef` (`Fix TuiChannel integration and enable in main.rs`).
- [x] 2026-03-09 21:47Z: Drafted this ExecPlan with the root-cause hypothesis and the required failing, unit, and behavioural coverage.
- [x] Add the failing real-WASM file-loader regression.
- [x] Implement the metadata-plumbing fix.
- [x] Add the remaining unit and behavioural tests.
- [x] Run targeted validation and update outcomes.
- [x] 2026-03-09 22:06Z: Added the first real regression in `tests/tool_schema_validation.rs` for `WasmToolLoader::load_from_files(...)` plus a real GitHub WASM artifact. With the old code, registration failed to expose the guest schema and the test reproduced the “no parameters” symptom.
- [x] 2026-03-09 22:15Z: Tried the most direct fix first by extracting `description()` and `schema()` during `WasmToolRuntime::prepare(...)`. This turned out to be the wrong repair point for the real GitHub component: direct metadata calls against a minimal metadata host were brittle and did not provide a stable fix.
- [x] 2026-03-09 22:24Z: Pivoted to the narrower registration-side fix. `ToolRegistry::register_wasm(...)` now asks the newly created `WasmToolWrapper` for exported metadata when explicit overrides are absent, then applies the recovered description/schema before publishing the tool.
- [x] 2026-03-09 22:29Z: Added wrapper-level unit coverage for real metadata extraction from the GitHub component and a parser unit test for the fallback hint path.
- [x] 2026-03-09 22:41Z: Added registry precedence coverage proving explicit schema/description overrides still win over recovered guest metadata.
- [x] 2026-03-09 22:43Z: Validated the wrapper suite, behavioural schema suite, override precedence regression, and formatting.

## Surprises & Discoveries

- The most suspicious code is not in the WASM execution wrapper. The arguments appear to be discarded earlier because the model-facing function definition is assembled from `tool.parameters_schema()`, and that schema is already empty before any tool call is executed.
- The runtime file still contains explicit temporary comments saying description and schema extraction are placeholders “for now.” That temporary path has survived long enough to become externally visible through file-based tool loading.
- The storage-backed path already has a schema override hook, so the codebase is not uniformly wrong; the bug is likely a path-precedence mismatch rather than a universal inability to carry WASM schemas.
- Current schema validator coverage is misleadingly reassuring here because it exercises representative schema shapes, not the actual metadata emitted by real file-loaded WASM tools.
- The GitHub schema is not shaped as top-level `properties.action`; it is an object schema with top-level `required: ["action"]` plus a `oneOf` over action-specific property sets. The regression tests needed to check for non-empty real structure rather than assume a flattened schema.
- `WasmRuntimeConfig::for_testing()` uses a 1 MiB default memory limit, which is too small for the real GitHub component’s declared memory minimum. Real-WASM metadata tests need an explicit higher memory ceiling or they fail on an artificial limiter error before hitting the metadata path.
- The direct “call `description()` then `schema()` on a fresh minimal metadata host” approach was not robust enough on the real GitHub component. The registration-side wrapper path, which uses the normal host linker and runtime limits, was stable.

## Decision Log

- 2026-03-09 21:36Z: Chose to trace the external tool-definition path first rather than the guest execution path. Rationale: the user-reported symptom is that the function definition is empty while the guest still emits schema-aware retry hints.
- 2026-03-09 21:40Z: Recorded both `aea3f47` and `4ae59ef` as relevant provenance. Rationale: the placeholder extractor was introduced intentionally as temporary bring-up scaffolding, while the file-loader later allowed that scaffolding to leak into the live external interface by passing `schema: None`.
- 2026-03-09 21:42Z: Chose not to assume the fix belongs only in `runtime.rs`. Rationale: the right repair could be either real runtime extraction or a safer registration override path, and the tests should decide which narrow fix is most defensible.
- 2026-03-09 22:16Z: Rejected the `prepare(...)`-time metadata extraction approach after testing it against the real GitHub component. Rationale: it added complexity in the wrong layer and was brittle on a minimal metadata-only host.
- 2026-03-09 22:24Z: Chose wrapper-side metadata recovery in `register_wasm(...)`. Rationale: it uses the same linker/runtime limits as real execution, fixes the file-loader path generically, and keeps explicit registration overrides as the final authority.
- 2026-03-09 22:31Z: Initially kept runtime placeholder metadata in
  `PreparedModule` because the bug appeared to be only in the publication path.
- 2026-03-09 23:58Z: Removed placeholder metadata fields from
  `PreparedModule` and moved metadata recovery into a dedicated
  `src/tools/wasm/wrapper/metadata.rs` module. Rationale: this made the
  wrapper-side path the only authoritative metadata source and eliminated the
  stale cached placeholder state flagged in review.

## Outcomes & Retrospective

Call parameters were not being discarded during guest execution. They were
being lost earlier because file-loaded WASM tools were published with
placeholder metadata instead of the guest-exported schema, so the model saw an
effectively empty function definition and called the tool with `{}`.

The implemented fix is in the registration path, not the compile path. `ToolRegistry::register_wasm(...)` now creates the wrapper, asks it for exported metadata when no explicit overrides were provided, and then applies the recovered description/schema before the tool is registered. `WasmToolWrapper::exported_metadata()` first tries direct export calls on a fully wired wrapper instance and falls back to parsing the guest’s retry hint if direct export calls are unavailable on the real component. This keeps the fix generic for file-loaded WASM tools while preserving the existing override precedence for storage-backed or explicitly registered tools.

Validation evidence:

- `cargo component build --release --target wasm32-wasip2 --manifest-path tools-src/github/Cargo.toml`
- `cargo fmt --all`
- `cargo test tools::wasm::wrapper --lib -- --nocapture`
- `cargo test --test tool_schema_validation -- --nocapture`
- `cargo test test_explicit_wasm_schema_override_wins_over_exported_metadata --lib -- --nocapture`

All passed on `secret-blocking-overzealous`.

Final precedence rule after the fix:

1. If `register_wasm(...)` receives explicit `description`/`schema` overrides, those win.
2. Otherwise, the wrapper recovers the guest-exported metadata and publishes that externally.
3. Runtime placeholder metadata was removed from the normal registration path; `PreparedModule` no longer carries it as a fallback for file-loaded registration. The authoritative source for normal registration is the wrapper-side recovery flow that reads guest-exported metadata during `register_wasm(...)`.
