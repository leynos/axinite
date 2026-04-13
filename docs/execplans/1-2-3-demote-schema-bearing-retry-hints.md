# Demote schema-bearing retry hints to fallback diagnostics

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`,
`Surprises & Discoveries`, `Decision Log`, and
`Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: COMPLETED

## Purpose / big picture

Roadmap item `1.2.3` exists to finish the contract reversal described in
[RFC 0002](../rfcs/0002-expose-wasm-tool-definitions.md). After this work,
Axinite must treat proactive `ToolDefinition.parameters` publication as the
normal contract for active WebAssembly (WASM) tools, while schema-bearing retry
hints remain only supplemental recovery guidance when a call already failed.

The change is observable in five ways.

1. A first reasoning request that includes an active WASM tool already carries
   that tool's advertised parameter schema before any tool call fails.
2. A malformed first call still produces actionable recovery guidance, but the
   message points back to the advertised contract instead of teaching the
   contract for the first time.
3. Wrapper comments, helper names, and error wording stop describing
   schema-bearing hints as the primary interface.
4. Unit tests fail if proactive schema publication regresses or if the failure
   path stops giving useful recovery guidance.
5. Documentation and roadmap status all describe the same contract:
   proactive schema publication is primary, retry hints are fallback-only.

This plan uses the `hexagonal-architecture` guidance narrowly. Policy about
what the contract is belongs in inward-facing tool logic and shared error
semantics. WASM execution, hosted transport, and user-facing adapters should
consume that policy rather than each inventing their own rule.

## Approval gates

- Plan approved
  Acceptance criteria: the implementation stays within roadmap item `1.2.3`,
  treats this as a contract-semantics change rather than a new transport
  feature, and does not alter the canonical `ToolDefinition` shape.
  Sign-off: human reviewer approves this ExecPlan before implementation begins.

- Implementation complete
  Acceptance criteria: retry hints are demoted to fallback diagnostics in code,
  tests, and documentation, while parse and validation failures still produce
  actionable recovery guidance.
  Sign-off: implementer marks all milestones complete before final validation.

- Validation passed
  Acceptance criteria: targeted tests, `make all`, Markdown linting, and
  `git diff --check` all pass with retained logs.
  Sign-off: implementer records evidence immediately before commit.

- Docs synced
  Acceptance criteria: `docs/roadmap.md`, RFC 0002, the user's guide, and the
  relevant internal architecture document describe the same fallback-only retry
  hint contract.
  Sign-off: implementer completes documentation updates as the final
  pre-commit checkpoint.

## Repository orientation

The following files are the important orientation points for this feature.

- `docs/roadmap.md` defines `1.2.3`, its dependency on `1.2.1`, and the
  success condition that retry hints become supplemental help rather than the
  primary contract.
- `docs/rfcs/0002-expose-wasm-tool-definitions.md` is the design authority for
  this change. The most important sections are `Summary`, `Proposal` step 2
  ("Treat retry hints as fallback diagnostics only"), `Unit tests`,
  `Behavioural tests`, and `Migration Plan` step 4.
- `src/tools/wasm/wrapper.rs` owns the tool-level error branch in
  `WasmToolWrapper::execute_sync()`. That branch already says the hint is only
  supplemental guidance, but the surrounding behaviour still emits a
  schema-bearing hint and needs to be reframed consistently.
- `src/tools/wasm/wrapper/metadata.rs` owns the helper that currently builds a
  retry hint directly from guest `description()` and `schema()` exports. This
  is the most likely place to centralize the "fallback diagnostic" policy and
  keep formatting logic out of the execution path.
- `src/tools/wasm/error.rs` still documents `ToolReturnedError.hint` as the
  place the large language model (LLM) learns the correct arguments. That text
  conflicts with RFC 0002 and must be updated along with any outward-facing
  label such as `Tool usage hint`.
- `src/tools/wasm/loader.rs` and `src/tools/registry/loader.rs` already prove
  that proactive schema publication exists. `1.2.3` must preserve those
  invariants and use them as the reason retry hints can now be demoted.
- `src/orchestrator/api/tests/remote_tools.rs` and
  `src/worker/container/tests/remote_tools.rs` already cover hosted tool
  advertisement and remote execution. They are the best existing behavioural
  evidence that hosted flows do not need a failure-first schema path.
- `docs/users-guide.md` currently describes hosted advertisement of
  Model Context Protocol (MCP) and WASM tools but does not yet explicitly say
  that retry hints are fallback diagnostics only.
- `docs/worker-orchestrator-contract.md` is the relevant internally facing
  architecture document if the hosted-contract description needs a concise note
  that error hints are supplemental to the advertised `ToolDefinition`.
- The user request referenced `docs/axinite-architecture-summary.md`, but this
  checkout does not contain that file. Use
  `docs/axinite-architecture-overview.md` as the high-level architecture
  reference for this work.

## Constraints

- Do not change the canonical LLM-facing tool shape. `ToolDefinition` must
  remain `name`, `description`, and `parameters`.
- Do not add a WASM-specific schema transport or a second hosted catalogue
  path. This roadmap item is about contract wording and fallback behaviour, not
  transport.
- Do not remove retry hints entirely. RFC 0002 treats them as retained
  recovery guidance for tool-level failures.
- Keep provider-specific schema shaping, if any, as a separate adaptation
  concern. This plan must not move contract ownership away from guest-exported
  or explicitly overridden metadata.
- Preserve the `1.2.1` guarantee that active WASM tools publish their schema
  through `ToolDefinition.parameters` before first use.
- Preserve parse and validation failure recoverability. A malformed first call
  must still tell the caller how to recover, even if the message becomes
  shorter and points back to the advertised contract.
- Keep policy inward and adapters thin. The wrapper or a closely related helper
  should own the fallback-diagnostic rule; the worker and orchestrator adapters
  should only consume already decided contract data.
- New or modified Rust tests must use `rstest` fixtures for shared setup.
- Add `rstest-bdd` coverage only if one focused behaviour-driven development
  (BDD) scenario can be introduced without a disproportionate new harness. If
  that threshold is exceeded, document why behavioural BDD coverage is not
  applicable for this slice and keep the observable proof in the existing
  `rstest` integration suites.
- Update user-facing and maintainers-facing documentation in the same delivery
  pass, and mark roadmap item `1.2.3` done only after the implementation lands.
- Check `FEATURE_PARITY.md` during implementation and update it in the same
  branch if a tracked feature entry is affected.

## Tolerances (exception triggers)

- Scope: if the smallest credible implementation touches more than 11 files or
  roughly 450 net new lines before documentation, stop and verify that work
  from roadmap item `1.2.4` or broader provider-shaping changes has not leaked
  into this slice.
- Interface: if demoting retry hints requires changing `ToolDefinition`,
  `NativeTool`, the WebAssembly Interface Types (WIT) interface, or the
  worker-orchestrator transport types, stop and document why the existing
  contract cannot express the feature.
- Behavioural drift: if preserving actionable guidance requires making retry
  hints longer or more schema-heavy than today for common failures, stop and
  document the trade-off instead of silently broadening the payload.
- Test scaffolding: if adding `rstest-bdd` requires a new workspace dependency,
  more than one feature file, or more than two new support files, stop and
  confirm whether behavioural assertions should remain in the existing `rstest`
  suites for this roadmap item.
- Regression ambiguity: if a failure can no longer clearly distinguish between
  parse/validation guidance and normal contract publication, stop and define
  the new rule explicitly before continuing.

## Risks

- Risk: comments and error strings may be updated while helper behaviour still
  embeds the full schema indiscriminately, leaving code and docs inconsistent.
  Severity: high
  Likelihood: medium
  Mitigation: centralize the fallback-diagnostic wording in one helper and
  cover it with direct tests.

- Risk: the implementation may over-correct and strip too much detail from
  parse or validation failures, making retry guidance vague.
  Severity: high
  Likelihood: medium
  Mitigation: add unhappy-path tests that assert both the original failure
  reason and concrete recovery guidance remain present.

- Risk: hosted-path tests may be skipped because this roadmap item looks local
  to the WASM wrapper, which would miss the cross-boundary contract guarantee.
  Severity: medium
  Likelihood: medium
  Mitigation: keep at least one worker or orchestrator behavioural assertion in
  scope to prove hosted flows still rely on proactive advertisement.

- Risk: introducing `rstest-bdd` for the first time in this subsystem may add
  more scaffolding than signal.
  Severity: medium
  Likelihood: high
  Mitigation: perform an explicit proportionality check and document the
  outcome in `Decision Log` before adding new BDD harness code.

- Risk: documentation drift is likely because roadmap, RFC, user's guide, and
  architecture references all speak about adjacent parts of the same contract.
  Severity: medium
  Likelihood: high
  Mitigation: treat documentation synchronization as its own milestone, not a
  final cleanup task.

## Milestone 1: confirm the contract boundary and desired failure language

Start by making the intended rule explicit in code-facing terms.

1. Re-read RFC 0002 sections `Summary`, `Proposal` step 2, and `Migration
   Plan` step 4 while examining the current `build_tool_hint()` and
   `ToolReturnedError` wording.
2. Record the exact before/after contract:
   proactive schema advertisement remains canonical, and the failure path only
   helps recovery after something already went wrong.
3. Identify which failures are in scope for actionable fallback diagnostics.
   The minimum set is malformed parameters, validation failure, and
   tool-reported request errors that the caller can immediately retry.
4. Confirm the hosted behavioural proof points that already exist in
   `src/orchestrator/api/tests/remote_tools.rs` and
   `src/worker/container/tests/remote_tools.rs` so `1.2.3` does not reinvent a
   hosted-specific test harness.

Expected result: the implementer has a precise rule for when a retry hint is
shown, what it should say, and which existing seams already prove proactive
advertisement.

## Milestone 2: refactor the WASM fallback diagnostic helper around policy

Express the fallback-only rule in one inward-facing place.

1. Replace the "tool usage hint" framing in
   `src/tools/wasm/wrapper/metadata.rs` with helper names and comments that
   describe fallback diagnostics rather than the primary contract.
2. Keep guest-export recovery local to the helper, but make the generated text
   prefer language such as "retry using the advertised schema" before deciding
   whether to append truncated description or schema detail.
3. Update `src/tools/wasm/wrapper.rs` so the error branch comment and call site
   clearly state that the hint is recovery-only and assumes the schema was
   already advertised through registration.
4. Update `src/tools/wasm/error.rs` so the variant documentation and display
   label match the same policy. If the label changes from `Tool usage hint` to
   something like `Fallback guidance`, change tests accordingly.
5. Keep formatting decisions centralized. Do not spread schema-truncation rules
   or contract wording across multiple adapter layers.

Expected result: the codebase has one coherent fallback-diagnostic policy for
WASM tool failures, and no nearby comment still describes failure-time schema
delivery as the normal interface.

## Milestone 3: add tests for happy paths, unhappy paths, and fallback-only semantics

Lock the new rule down with tests before wider refactoring.

### Unit and integration coverage with `rstest`

1. Extend `src/tools/wasm/wrapper/metadata.rs` tests with direct assertions for
   the fallback helper:
   empty export case, truncated export case, and recovery wording that points
   back to the advertised schema.
2. Extend `src/tools/wasm/error.rs` tests so the display string proves the
   label and wording now describe fallback guidance rather than the primary
   contract.
3. Add or adjust a focused execution-path test in `src/tools/wasm/wrapper.rs`
   or a nearby WASM test module using the GitHub WASM fixture so a malformed
   first call still returns actionable recovery guidance.
4. Re-run or extend proactive-publication tests in
   `src/tools/wasm/loader.rs` and any registry tests needed to prove the
   advertised schema remains visible before failure.
5. Keep one hosted behavioural assertion in either
   `src/orchestrator/api/tests/remote_tools.rs` or
   `src/worker/container/tests/remote_tools.rs` that proves hosted flows still
   receive the canonical schema before any fallback path is exercised.

### Behavioural coverage with `rstest-bdd` where behaviour-driven development (BDD) is applicable

Attempt one narrow, in-process scenario only if it stays within the tolerances.
The preferred scenario is:

```plaintext
Feature: WASM retry hints are fallback diagnostics

  Scenario: malformed first call still receives recovery guidance
    Given an active WASM tool with an advertised schema
    When the caller omits a required field on the first call
    Then the failure tells the caller to retry using the advertised schema
    And any appended schema detail matches the advertised contract
```

If that scenario can be implemented with one feature file and one Rust test
module that reuse existing fixtures, keep it. If it requires broader harness
construction or a new workspace dependency, record the non-applicability in
`Decision Log` and keep the behavioural proof in the existing `rstest`
integration suites instead.

Expected result: tests prove both sides of the contract at once. Proactive
advertisement remains the first-class interface, and failures still give useful
recovery guidance without redefining the contract.

## Milestone 4: synchronize design, user, and architecture documents

The implementation is not done until the documents stop contradicting each
other.

1. Update `docs/rfcs/0002-expose-wasm-tool-definitions.md` implementation
   status to show `1.2.3` complete and leave `1.2.4` open.
2. Update RFC 0002 wording if needed so the fallback-diagnostic behaviour and
   chosen error phrasing are reflected in the migration notes.
3. Update `docs/users-guide.md` so operators understand that proactive schema
   advertisement is the normal contract for WASM tools and that failure hints
   are supplemental recovery guidance.
4. Update the relevant internal architecture document. Use
   `docs/worker-orchestrator-contract.md` for the internal boundary note and
   `docs/axinite-architecture-overview.md` if the high-level extension/runtime
   narrative needs a brief contract update.
5. Mark roadmap item `1.2.3` done in `docs/roadmap.md`.
6. Review `FEATURE_PARITY.md` and either update the affected entry or record
   why no parity row changes.

Expected result: every affected design and operator document describes the same
steady-state rule and the roadmap accurately shows the task complete.

## Milestone 5: validate, record evidence, and commit

Run the full validation sequence sequentially and retain logs with `tee` as
required by the repository instructions.

1. Run targeted Rust tests for the WASM helper, error, and hosted behavioural
   surfaces. Use stable log filenames keyed by the branch name.

   ```plaintext
   BRANCH_SLUG=$(git branch --show | tr '/' '-')
   cargo test wasm_tool --lib \
     | tee /tmp/test-wasm-contract-axinite-${BRANCH_SLUG}.out
   cargo test remote_tool_ --lib \
     | tee /tmp/test-remote-tools-axinite-${BRANCH_SLUG}.out
   cargo test hosted_worker_remote_tool_catalog --lib \
     | tee /tmp/test-hosted-worker-axinite-${BRANCH_SLUG}.out
   ```

2. If a proportional `rstest-bdd` scenario was added, run its filtered test
   target and retain the log in `/tmp`.
3. Run the full repository gate.

   ```plaintext
   BRANCH_SLUG=$(git branch --show | tr '/' '-')
   make all | tee /tmp/make-all-axinite-${BRANCH_SLUG}.out
   ```

4. Run Markdown validation for every changed document.

   ```plaintext
   BRANCH_SLUG=$(git branch --show | tr '/' '-')
   bunx markdownlint-cli2 \
     docs/execplans/1-2-3-demote-schema-bearing-retry-hints.md \
     docs/roadmap.md \
     docs/rfcs/0002-expose-wasm-tool-definitions.md \
     docs/users-guide.md \
     docs/worker-orchestrator-contract.md \
     docs/axinite-architecture-overview.md \
     docs/contents.md \
     | tee /tmp/markdownlint-axinite-${BRANCH_SLUG}.out
   ```

5. Run the diff hygiene check.

   ```plaintext
   BRANCH_SLUG=$(git branch --show | tr '/' '-')
   git diff --check | tee /tmp/diff-check-axinite-${BRANCH_SLUG}.out
   ```

6. Record passing evidence in `Progress` and `Outcomes & Retrospective`, then
   create one focused commit that describes demoting schema-bearing retry hints
   to fallback diagnostics and explains why proactive schema publication
   remains canonical.

Expected result: the feature lands with evidence that code, tests, and
documentation all match the intended contract.

## Progress

- [x] 2026-04-10T22:02:24+02:00 Reviewed roadmap item `1.2.3`, RFC 0002, the
  earlier `1.2.1` and `1.2.2` ExecPlans, and the current WASM wrapper,
  metadata, error, registry, orchestrator, and worker seams.
- [x] 2026-04-10T22:02:24+02:00 Drafted this ExecPlan for roadmap item
  `1.2.3`.
- [x] Implementation approved by a human reviewer.
- [x] Implementation started.
- [x] Fallback-diagnostic policy implemented in code.
- [x] Unit and behavioural regression coverage added and passing.
- [x] Documentation synchronized and roadmap item `1.2.3` marked done.
- [x] Final validation evidence recorded.
- [ ] Feature commit created.

## Surprises & Discoveries

- 2026-04-10T22:02:24+02:00 The user request referenced
  `docs/axinite-architecture-summary.md`, but this checkout contains
  `docs/axinite-architecture-overview.md` instead. This plan therefore uses
  the overview plus `docs/worker-orchestrator-contract.md` as the relevant
  architecture references.
- 2026-04-10T22:02:24+02:00 `src/tools/wasm/wrapper.rs` already contains a
  comment stating the hint is supplemental recovery guidance, but
  `src/tools/wasm/error.rs` still documents the hint as the place the LLM
  learns the correct arguments. The implementation must reconcile those two
  surfaces, not just one.
- 2026-04-10T22:02:24+02:00 This subsystem currently shows no checked-in
  `.feature` files or `#[scenario]` usage, so any `rstest-bdd` coverage for
  this roadmap item needs an explicit proportionality check rather than a
  default assumption.
- 2026-04-13T11:12:00+02:00 The workspace does not currently include an
  `rstest-bdd` dependency, checked-in feature files, or `#[scenario]` tests.
  Adding them for this slice would require new harness scaffolding rather than
  extending an existing path.
- 2026-04-13T11:12:00+02:00 Existing hosted behavioural tests already assert
  that orchestrator and worker remote-tool catalogues expose hosted-safe WASM
  `ToolDefinition.parameters` before execution. That evidence can be retained
  without inventing a hosted-specific failure-path harness for `1.2.3`.
- 2026-04-13T12:54:00+02:00 The implementation fit inside the planned seam:
  wrapper metadata helper, wrapper error wording, one focused malformed-call
  regression, and synchronized documentation updates. No transport, WIT, or
  hosted-catalogue shape changes were required.

## Decision Log

- 2026-04-10T22:02:24+02:00 Use the existing WASM wrapper plus metadata helper
  as the inward policy boundary for fallback diagnostics.
  Rationale: this keeps contract semantics close to the domain rule, avoids
  adapter drift, and follows the narrow `hexagonal-architecture` guidance
  requested for this task.
- 2026-04-10T22:02:24+02:00 Treat `docs/worker-orchestrator-contract.md` as the
  primary internally facing document for this change, with
  `docs/axinite-architecture-overview.md` as the higher-level companion.
  Rationale: the transport does not change, but the document that explains the
  boundary should say that failure hints are supplemental to advertised tool
  definitions.
- 2026-04-10T22:02:24+02:00 Require an explicit proportionality check before
  adding `rstest-bdd` scaffolding.
  Rationale: the user asked for behavioural tests where applicable, but the
  current subsystem has no visible BDD harness. The plan must preserve quality
  without forcing a broader test-framework rollout into a narrow contract task.
- 2026-04-13T11:12:00+02:00 Keep behavioural coverage in the existing `rstest`
  suites and do not add `rstest-bdd` scaffolding for this roadmap item.
  Rationale: the workspace has no existing BDD harness, so introducing one
  would exceed the plan's proportionality tolerance for a narrow contract
  semantics change.
- 2026-04-13T12:54:00+02:00 Implement fallback guidance as a short imperative
  that points back to the advertised schema, then append truncated guest
  metadata only as optional diagnostic context.
  Rationale: this preserves actionable recovery for malformed first calls while
  making the pre-advertised `ToolDefinition.parameters` contract unmistakably
  primary.
- 2026-04-13T13:18:00+02:00 Keep the existing hosted behavioural assertions as
  the cross-boundary proof point and add one focused wrapper regression for the
  malformed first-call path.
  Rationale: the hosted catalogue tests already prove that workers and the
  orchestrator advertise the canonical schema before execution, so this slice
  only needed one local failure-path test to lock the fallback wording down.

## Outcomes & Retrospective

- 2026-04-13T12:54:00+02:00 Focused regression coverage passed with
  `cargo test fallback_guidance --lib`, including the new malformed first-call
  wrapper test and direct helper wording tests.
- 2026-04-13T13:18:00+02:00 Final validation passed with
  `bunx markdownlint-cli2` on the touched Markdown files, `make all`, and
  `git diff --check`. The aggregate Rust gate completed successfully after one
  formatting-only retry through `cargo fmt --all`.
