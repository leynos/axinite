# Fix Invalid Top-Level Tool Schema For GitHub WASM Tools

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`,
`Surprises & Discoveries`, `Decision Log`, and
`Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: IN PROGRESS

## Purpose / big picture

After this work, IronClaw must stop sending OpenAI function-tool schemas
whose root object still contains forbidden JSON Schema keywords such as
`oneOf`, `anyOf`, `allOf`, `enum`, or `not`. The concrete user-visible
outcome is that the `github` WASM tool can again be advertised to
`openai/gpt-5.4` without the provider rejecting the request with
HTTP 400 `invalid_function_parameters`.

Success is observable in three ways. First, a failing regression test
must demonstrate that the current GitHub WASM schema normalization path
still emits a root-level `oneOf`. Second, a focused behavioral test must
prove that the provider-bound schema keeps the GitHub tool’s action
variants while no longer exposing forbidden root-level combinators.
Third, the targeted Rust test suites and formatting checks must pass
with logs captured under `/tmp`.

## Repository orientation

The live failure path currently crosses four areas.

- `src/tools/registry.rs` publishes tool definitions from
  `tool.parameters_schema()` without provider-specific shaping.
- `src/tools/wasm/wrapper/metadata.rs` and
  `src/tools/wasm/wrapper.rs` recover real guest-exported metadata for
  file-loaded WASM tools. The real GitHub schema includes a root-level
  `oneOf`.
- `tests/tool_schema_validation.rs` currently asserts that the published
  GitHub tool definition contains that root-level `oneOf`, which
  reflects recovered guest metadata but not provider compatibility.
- `src/llm/rig_adapter.rs` converts IronClaw tool definitions into
  rig/OpenAI tool definitions with `convert_tools(...)` and
  `normalize_schema_strict(...)`. The current normalizer recursively
  adjusts nested objects but does not eliminate root-level
  `oneOf`/`anyOf`/`allOf`/`enum`/`not`, so OpenAI still receives an
  invalid top-level schema.

The reported provider error names the `github` tool and points at
`tools[15].parameters`, which matches the provider-bound tool list
created inside `RigAdapter::complete_with_tools(...)`.

## Change history and likely intention

Two earlier changes are relevant and must be kept distinct.

Commit `c18f6730f8b04ac79b15a0c9c611f2263fb84282`
(`fix: OpenAI tool calling — schema normalization, missing types, and
Responses API panic (#132)`) introduced `normalize_schema_strict(...)`
and routed `RigAdapter::convert_tools(...)` through it. The stated
purpose was to normalize tool schemas for OpenAI strict mode by forcing
object strictness, filling `required`, and making optional fields
nullable. Its semantic diff shows no handling for forbidden root-level
combinators; it assumes schemas are already rooted in an acceptable
object form.

Commit `91b18ea0caa6072049e506e9fc8ce7eaac619fe3`
(`Recover exported schemas for file-loaded WASM tools`) changed
file-loaded WASM tool publication so `register_wasm(...)` recovers real
guest-exported schemas when explicit overrides are absent. Its purpose
was to stop publishing placeholder empty schemas and instead expose the
real WASM schema to the model. The semantic diff and new regression
tests show that this intentionally surfaced the GitHub schema’s
root-level `oneOf`.

The likely regression is therefore an interaction between a newer
metadata recovery fix and an older OpenAI normalization pass that was
only designed for object-shaped schemas. The WASM fix itself appears
directionally correct; the provider-bound shaping layer is what failed
to keep pace with the richer real schema.

## Constraints

- This plan file must live at
  `docs/plans/2026-03-10-invalid-tool-schema.md`.
- Keep the recovered guest metadata path intact. The fix must not revert
  file-loaded WASM tools back to placeholder schemas.
- Do not special-case the `github` tool by name. The repair must apply
  generically to any tool schema that reaches the OpenAI provider
  boundary.
- Keep provider-facing tool names and descriptions stable.
- Do not change the external `ToolDefinition` Rust type or the WASM
  guest contract unless the investigation proves that provider-bound
  normalization cannot be solved locally.
- No new third-party dependency may be introduced.

## Tolerances (exception triggers)

- Scope: if the smallest credible fix requires touching more than
  8 files or more than 350 net lines, stop and reassess before
  proceeding.
- Interface: if fixing the provider-bound schema requires changing WIT
  exports, the `Tool` trait, or the registry’s stored schema format,
  stop and escalate.
- Ambiguity: if more than one provider path independently serializes
  tool schemas to OpenAI-format requests and the active failing path
  cannot be proven from code/tests, stop and record the competing paths
  with evidence.
- Coverage: if a deterministic failing regression cannot be written
  against the current normalization path after three targeted attempts,
  stop and document why the path is not observable.

## Risks

- Risk: Flattening or wrapping a top-level combinator could accidentally
  erase real constraints from the GitHub schema and let the model emit
  malformed arguments.
  Severity: high
  Likelihood: medium
  Mitigation: Preserve the action-discriminated variants inside nested
  property schemas and assert on representative fields like `action`,
  `owner`, and `repo` in tests.

- Risk: Tightening CI validation without aligning the provider normalizer
  would create noisy failures without fixing runtime behavior.
  Severity: medium
  Likelihood: medium
  Mitigation: Land red tests for the provider-bound transformation and
  validator changes in the same change as the normalization fix.

- Risk: The same root-level schema issue may exist for MCP or future
  externally sourced tools, not just file-loaded WASM tools.
  Severity: medium
  Likelihood: medium
  Mitigation: Make the normalization and strict validation generic for
  any root-level combinator-bearing tool schema.

## Milestone 1: Reproduce the provider-bound invalid schema

Write a focused red test around `src/llm/rig_adapter.rs` that feeds
`convert_tools(...)` or its normalization helper a GitHub-style schema
with root-level `oneOf`. The failing condition should mirror the
provider complaint: the normalized schema still has a root-level
forbidden keyword or lacks a rooted object-shaped parameter contract
acceptable to OpenAI.

Also add a complementary validator regression that proves our current
strict schema checks do not catch this case even though OpenAI rejects
it. This closes the current blind spot where
`tests/tool_schema_validation.rs` can pass while the provider request
still fails.

## Milestone 2: Fix the provider-bound schema shaping

Implement the narrowest repair in `src/llm/rig_adapter.rs`. The likely
repair point is a root-level schema rewrite after recursive
normalization. The output must remain an object schema with no top-level
`oneOf`/`anyOf`/`allOf`/`enum`/`not`, while retaining meaningful variant
information inside nested fields so the model still sees the GitHub
action contract.

If the normalizer needs a helper for “OpenAI-compatible root object
wrapping”, keep it local to the rig/OpenAI adapter unless tests prove
another provider path shares the same requirement.

## Milestone 3: Guard the blast radius

Add tests at two layers.

1. Unit-level tests in `src/llm/rig_adapter.rs` for raw schema
   normalization, including a GitHub-style root `oneOf` input and at
   least one non-WASM control schema that must remain stable.
2. Behavioral coverage in `tests/tool_schema_validation.rs` or another
   integration-style Rust test that loads the real GitHub WASM tool,
   passes its published schema through the provider normalization path,
   and verifies that the provider-bound result is root-safe while still
   exposing GitHub-specific fields.

## Validation

Run the smallest targeted commands first, then the broader gates needed
for the commit.

```bash
set -o pipefail
BRANCH=$(git branch --show)
cargo test rig_adapter --lib -- --nocapture \
  2>&1 | tee /tmp/test-rig-adapter-ironclaw-${BRANCH}.out
```

```bash
set -o pipefail
BRANCH=$(git branch --show)
cargo test --test tool_schema_validation -- --nocapture \
  2>&1 | tee /tmp/test-tool-schema-validation-ironclaw-${BRANCH}.out
```

```bash
set -o pipefail
BRANCH=$(git branch --show)
cargo fmt --all --check \
  2>&1 | tee /tmp/fmt-check-ironclaw-${BRANCH}.out
```

## Progress

- [x] 2026-03-10 09:32Z: Verified repo scope, current branch
  `secret-blocking-overzealous`, and relevant AGENTS instructions.
- [x] 2026-03-10 09:34Z: Loaded the `leta`, `grepai`, `execplans`,
  `sem`, `rust-router`, and `rust-types-and-apis` skill entrypoints
  needed for this investigation.
- [x] 2026-03-10 09:35Z: Confirmed memory points at yesterday’s
  hosted-worker and WASM schema work and opened the hosted-worker
  rollout summary for nearby context.
- [x] 2026-03-10 09:38Z: Created shared context pack `pk_qies5rer` for
  the agent-team investigation.
- [x] 2026-03-10 09:39Z: Attempted primary `grepai` exploration; qdrant
  on `127.0.0.1:6334` refused connections, so the repo fallback path is
  exact-text search plus `leta`.
- [x] 2026-03-10 09:44Z: Traced the likely failure seam from
  `tests/tool_schema_validation.rs` and
  `src/tools/wasm/wrapper/metadata.rs` to `src/llm/rig_adapter.rs`; the
  GitHub schema is now intentionally published with root-level `oneOf`,
  while the provider normalizer still forwards that root-level
  combinator.
- [x] 2026-03-10 09:46Z: Used `git blame`, `git show`, and `sem diff`
  to identify the interaction between `c18f673` (OpenAI strict
  normalization) and `91b18ea` (real WASM metadata recovery).
- [ ] Add failing normalization and validator regression tests.
- [ ] Implement the provider-bound root-schema fix.
- [ ] Run targeted gates, update this plan with actual results, and
  commit.

## Surprises & Discoveries

- The current behavioral test added for the previous WASM fix explicitly
  asserts that the real GitHub tool definition still has a top-level
  `oneOf`. That is valid evidence for “real metadata recovered” but
  invalid evidence for “OpenAI-compatible”.
- The older OpenAI normalizer in `src/llm/rig_adapter.rs` is not dead
  code. It is still the last transformation before tool definitions are
  handed to rig/OpenAI, so the defect is in the live request path, not
  just offline validation.
- `src/tools/schema_validator.rs` currently calls its representative
  WASM schemas “strict-mode” coverage, but it does not test or reject
  forbidden root-level combinators. That explains why CI could stay
  green while OpenAI rejects the request.

## Decision Log

- 2026-03-10 09:38Z: Chose to treat this as a provider-bound
  schema-shaping bug first, not an MCP transport bug. Rationale: the
  provider error names the function schema itself and points at
  serialized tool parameters.
- 2026-03-10 09:45Z: Chose not to revert `91b18ea` pre-emptively.
  Rationale: that commit fixed a real bug by exposing guest-exported
  schemas; the incompatibility appears to be in the OpenAI compatibility
  layer, not the WASM metadata recovery itself.
- 2026-03-10 09:46Z: Recorded both `c18f673` and `91b18ea` as
  provenance. Rationale: the regression only exists because those two
  correct-in-isolation changes now interact.

## Outcomes & Retrospective

Pending implementation.
