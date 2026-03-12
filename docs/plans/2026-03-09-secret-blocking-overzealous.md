# Fix Overzealous Secret Blocking For Host-Injected GitHub Credentials

This ExecPlan (execution plan) is a living document. The sections `Constraints`, `Tolerances`, `Risks`, `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: COMPLETE

## Purpose / big picture

After this work, a hosted or in-process IronClaw agent should be able to call the curated `github` WASM tool with a real `github_token` secret stored in IronClaw and have the host inject that credential into the outbound request without the request being blocked as a secret leak. The observable success case is simple: a call such as `{"action":"get_repo","owner":"leynos","repo":"mxd"}` reaches `api.github.com` and either returns real GitHub data or a real GitHub API error, but it must not fail with `Potential secret leak blocked` merely because IronClaw injected its own bearer token.

The current failure is specific and already reproducible. The `github` tool declares `github_token` as a bearer credential for `api.github.com` in `tools-src/github/github-tool.capabilities.json`, the tool implementation relies on host-side automatic injection rather than constructing the `Authorization` header itself in `tools-src/github/src/lib.rs`, and the tool-side WASM wrapper currently performs leak scanning after host credential injection in `src/tools/wasm/wrapper.rs`. The leak detector in `src/safety/leak_detector.rs` correctly blocks real GitHub PAT patterns, so scanning the post-injection request treats the legitimate host-managed credential as if the WASM module were exfiltrating it. The channel-side WASM wrapper in `src/channels/wasm/wrapper.rs` already has the correct ordering and comments explaining why leak scanning must happen before host credential injection. This plan exists to align the tool-side path with that already-correct channel-side behavior, add regression coverage, and verify that the fix remains narrow.

## Repository orientation

The important paths for this work are:

- `tools-src/github/github-tool.capabilities.json`, which declares that requests to `api.github.com` should receive bearer auth from the `github_token` secret.
- `tools-src/github/src/lib.rs`, which intentionally omits the `Authorization` header and expects the host to inject it.
- `src/tools/wasm/wrapper.rs`, which hosts WASM tool HTTP requests and currently injects host credentials before running `LeakDetector::scan_http_request(...)`.
- `src/channels/wasm/wrapper.rs`, which hosts WASM channel HTTP requests and already runs leak scanning before host credential injection.
- `src/safety/leak_detector.rs`, which defines the blocking GitHub token and fine-grained PAT patterns plus the outbound request scan flow.

The prior plans that matter only as context are:

- `docs/plans/2026-03-09-resolve-meta-tooling-unavailability.md`, which made hosted workers advertise and proxy extension-management tools, so the `github` tool is now reachable through the hosted path.
- `docs/plans/2026-03-09-use-wit-v3-in-extensions.md`, which confirmed this is not a WIT-version incompatibility issue.

## Change history and likely intention

The change that introduced the current tool-side ordering appears to be commit `a53b2c10b569de297ef3178e029b86df256c3763` (`fix: Fix wasm tool schemas and runtime (#42)`). `git blame` on `src/tools/wasm/wrapper.rs` shows the `inject_host_credentials(...)` call at the start of the request pipeline coming from that commit, while the surrounding leak-scan call predates it. A focused semantic diff of `src/tools/wasm/wrapper.rs` across `a53b2c10^..a53b2c10` shows that the commit added the host-credential machinery to the tool wrapper: the `WasmToolWrapper` and `StoreData` structures gained secrets-store and `host_credentials` state, helper methods for resolving and injecting host credentials were added, and the `near::agent::host::Host for StoreData::http_request` implementation was modified to inject those credentials before the existing outbound leak scan.

The commit message for `a53b2c10` strongly suggests the intention was to add host-based credential injection and runtime reuse for WASM tools, not to weaken or redefine the leak-scanning model. The relevant description is about fixing schemas, making WASM HTTP requests reliable during startup, adding OAuth refresh support, and resolving capability-declared credentials before synchronous host calls. There is no stated intention to treat host-injected credentials as exfiltration candidates; the ordering change looks like an integration detail that was not revisited when host-based injection was added to a request path that already had leak scanning.

There is also a strong control case. Commit `dc7d9cce34868f5f1083dbf8c0acf78640fc9ab1` (`fix(channels): add host-based credential injection to WASM channel wrapper (#421)`) added the same broad capability to the channel wrapper later, but its commit message explicitly calls out the correct leak-scan ordering: “scan runs on WASM-provided values BEFORE host credential injection, preventing false-positive blocks on injected Bearer tokens.” That makes the safe intention clearer. The channel-side author had the same feature goal, recognized the false-positive risk, and fixed the order there. Taken together, the most likely interpretation is that the tool-side regression was an unintended consequence of adding host-based credential injection into an existing request pipeline, not a deliberate security-policy choice. That increases confidence that aligning the tool wrapper to the later channel behavior is a correction back to intended semantics rather than a new behavioral expansion.

## Constraints

- This plan file must remain at `docs/plans/2026-03-09-secret-blocking-overzealous.md` because the user requested that exact path.
- The security goal of outbound leak scanning must remain intact. This work must not create a blanket bypass that allows WASM-provided secrets to flow into URLs, headers, or bodies undetected.
- The `github` tool contract must remain unchanged. It should continue relying on host-managed credential injection rather than reading the secret value directly inside WASM.
- The fix must preserve the existing curated capability model in `tools-src/github/github-tool.capabilities.json` and other tool capabilities files. Do not special-case GitHub in the registry when the real problem sits in the generic WASM tool host.
- The tool-side and channel-side wrappers should not drift further apart on this behavior. If the correct fix is a shared helper or near-identical ordering, prefer that over duplicating divergent logic again.
- No new third-party dependency may be introduced for the fix or its tests.
- The work must use repository-native command patterns with `set -o pipefail` and `tee` logs under `/tmp/...` for any validation run.

## Tolerances (exception triggers)

- Scope: if the smallest credible fix requires changing more than 8 files or more than 300 net lines, stop and escalate with a scope breakdown. The expected fix surface is much smaller.
- Interface: if the fix appears to require changing the WIT host interface, extension capability schema, or public tool schema, stop and escalate. The current evidence says it should not.
- Security model: if aligning the tool wrapper with the channel wrapper would leave any demonstrated exfiltration path unscanned, stop and document the exact path before proceeding.
- Divergence: if the tool wrapper and channel wrapper cannot be aligned without first extracting a shared abstraction, keep the abstraction small; if it grows beyond a local helper and starts touching unrelated execution paths, stop and escalate.
- Reproduction: if a deterministic regression test cannot be written after three targeted attempts, stop and document why the existing test seams are insufficient.

## Risks

- Risk: A naive “skip Authorization header” exception in the leak detector would mask real exfiltration attempts from WASM code and weaken the security model.
  Severity: high
  Likelihood: medium
  Mitigation: Fix the ordering in the host wrapper so leak scanning sees only WASM-provided request data, not host-injected credentials. Avoid token-pattern exemptions based on header name alone.

- Risk: The tool-side wrapper may have intentionally diverged from the channel-side wrapper for an older reason that is no longer documented.
  Severity: medium
  Likelihood: low
  Mitigation: Compare the exact request pipelines and add a regression test that proves the channel-side ordering rationale applies equally to tools.

- Risk: Error redaction may still leak partial information if the early leak-scan failure path returns before wrapper-level redaction logic runs.
  Severity: medium
  Likelihood: medium
  Mitigation: Verify the current error chain and ensure tests assert safe error previews in both failure and success-adjacent paths.

- Risk: Hosted-worker proxying added in the meta-tooling work could obscure whether the bug is in hosted-only code or the generic tool runtime.
  Severity: medium
  Likelihood: medium
  Mitigation: Prove the bug at the generic `src/tools/wasm/wrapper.rs` layer with a focused unit or integration test that does not depend on the worker proxy.

- Risk: Existing tests cover helper pieces such as credential resolution, direct leak-detector matching, and redaction, but do not cover the full tool-wrapper request pipeline where the regression actually lives.
  Severity: high
  Likelihood: high
  Mitigation: Add both a failing request-pipeline test and a behavioural test that exercise the wrapper’s request preparation and outbound execution path instead of only helper functions.

## Milestone 1: Reproduce the failure and pin the exact generic cause

Start by proving the bug in the generic tool host rather than through a broad end-to-end hosted-worker flow.

Inspect and capture the current order in:

- `src/tools/wasm/wrapper.rs`
- `src/channels/wasm/wrapper.rs`
- `src/safety/leak_detector.rs`
- `tools-src/github/src/lib.rs`
- `tools-src/github/github-tool.capabilities.json`

Then add a focused failing regression test at the tool-wrapper level that demonstrates the bug shape:

1. Prepare a `Capabilities` instance with an HTTP credential mapping that injects a bearer token for a specific host.
2. Resolve that host credential into the wrapper’s `host_credentials`.
3. Exercise the actual tool-side request-preparation path, not only `LeakDetector::scan_http_request(...)` in isolation. If necessary, extract a small helper from `StoreData::http_request(...)` so the test can run the same ordering as production code without needing a full component invocation.
4. Use a PAT-shaped secret value such as a synthetic `github_pat_...` token so the test matches the real failure mode, with a clean WASM-provided URL, headers, and body.
5. Assert that the current code fails with `Potential secret leak blocked` and a nested `header:Authorization` / GitHub PAT match.

This test must fail against the current code if it executes the existing ordering. If the current helper seams are too narrow, first extract the smallest possible helper that makes the request-preparation order testable without changing behavior. The failing testcase from this milestone becomes the permanent regression test used to verify the fix.

Suggested commands:

```bash
set -o pipefail
BRANCH=$(git branch --show)
cargo test tools::wasm::wrapper --lib -- --nocapture \
  | tee /tmp/test-tools-wrapper-order-ironclaw-${BRANCH}.out
```

Expected pre-fix evidence:

```plaintext
Potential secret leak blocked: Secret leak blocked: pattern 'header:Authorization' matched ...
```

## Milestone 2: Align the tool-side request pipeline with the channel-side security model

Implement the smallest fix in `src/tools/wasm/wrapper.rs`.

The current tool wrapper does this:

1. Inject placeholder credentials into URL and headers.
2. Inject pre-resolved host credentials based on the request host.
3. Run `LeakDetector::scan_http_request(...)`.

The channel wrapper does this instead:

1. Inject placeholder credentials into URL and headers.
2. Run `LeakDetector::scan_http_request(...)` on the WASM-visible request.
3. Inject pre-resolved host credentials after the scan.

Unless new evidence disproves it, the tool wrapper should adopt the channel wrapper’s ordering and explanatory comments. The core invariant is that leak scanning should inspect only data the WASM module could have supplied. Host-managed credentials that the module never sees should still be redacted on error paths, but they should not cause an outbound request to be rejected as exfiltration.

If the final implementation leaves the tool and channel wrappers with near-identical request preparation logic, consider extracting a small shared helper only if it reduces future drift without widening scope. Do not do a large refactor.

## Milestone 3: Add regression coverage for both the fixed case and the still-blocked case

Add tests that prove the fix is narrow, security-preserving, and adequately covered in both unit and behavioural terms.

Required coverage:

1. Unit coverage in `src/tools/wasm/wrapper.rs` for the fixed request pipeline, using the failing testcase from Milestone 1 and keeping it as a post-fix passing regression.
2. Unit coverage in `src/tools/wasm/wrapper.rs` showing that a host-injected GitHub-style PAT or bearer token does not trip the outbound leak detector when the original WASM request is clean.
3. Unit coverage in `src/tools/wasm/wrapper.rs` or `src/safety/leak_detector.rs` showing that if WASM itself provides a GitHub token pattern in a header, URL, or body, the request is still blocked.
4. Unit coverage for any helper extracted while making the pipeline testable, especially if request preparation is split into a new method or struct.
5. Behavioural coverage for the wrapper execution path: a deterministic local HTTP test using a loopback server or request-capture seam that proves a clean request with host-injected credentials progresses far enough to attempt the outbound request instead of failing locally in IronClaw. This does not need live GitHub access; it must only demonstrate that the wrapper now passes the host-injected credential through the request execution path.
6. If practical within scope, a parity test or mirrored assertion showing that both the tool and channel wrappers follow the same ordering rule for host credential injection versus leak scanning.

Current coverage gap to close explicitly:

- `src/tools/wasm/wrapper.rs` already tests `inject_host_credentials`, `redact_credentials`, and `resolve_host_credentials`, but it does not test the end-to-end request pipeline inside `StoreData::http_request(...)`.
- `src/safety/leak_detector.rs` already tests direct blocking for secrets in URL, headers, and body, but it does not distinguish between WASM-provided values and host-injected values.
- `src/channels/wasm/wrapper.rs` contains redaction coverage and comments for the correct ordering, but there is no equivalent tool-side behavioural regression covering the false-positive case.

These tests should live as close to the wrappers and detector as possible. Favor deterministic unit or narrow integration tests over full worker orchestration.

Suggested commands:

```bash
set -o pipefail
BRANCH=$(git branch --show)
cargo test tools::wasm::wrapper --lib -- --nocapture \
  | tee /tmp/test-tools-wrapper-ironclaw-${BRANCH}.out
set -o pipefail
BRANCH=$(git branch --show)
cargo test safety::leak_detector --lib -- --nocapture \
  | tee /tmp/test-leak-detector-ironclaw-${BRANCH}.out
```

## Milestone 4: Validate the GitHub tool path and adjacent extension surfaces

Once the narrow wrapper fix passes, validate that the `github` extension path now behaves like a real API client instead of failing locally inside IronClaw.

Validation should include:

1. Re-running the focused regression tests.
2. Running any existing schema or extension-tool tests that cover curated tool capabilities.
3. If there is an existing deterministic GitHub tool test harness, re-running it.
4. If there is no such harness, using the wrapper-level proof plus capability-file inspection as the primary evidence and calling out the remaining end-to-end gap honestly.

If an end-to-end hosted-worker test already exists or can be extended cheaply, use it. If not, do not balloon scope just to prove what the narrow wrapper tests already demonstrate.

Suggested commands:

```bash
set -o pipefail
BRANCH=$(git branch --show)
cargo test --test tool_schema_validation -- --nocapture \
  | tee /tmp/test-tool-schema-ironclaw-${BRANCH}.out
```

## Concrete steps

Work from the repository root `/data/leynos/Projects/ironclaw`.

1. Confirm the current bug shape in the source:

```plaintext
nl -ba src/tools/wasm/wrapper.rs | sed -n '270,320p'
nl -ba src/channels/wasm/wrapper.rs | sed -n '350,385p'
nl -ba src/safety/leak_detector.rs | sed -n '300,325p'
nl -ba src/safety/leak_detector.rs | sed -n '438,520p'
nl -ba tools-src/github/src/lib.rs | sed -n '274,300p'
nl -ba tools-src/github/github-tool.capabilities.json | sed -n '1,30p'
```

2. Add the first failing regression test around the tool wrapper ordering.

3. Make the minimal ordering fix in `src/tools/wasm/wrapper.rs`, using the channel wrapper as the behavioral reference.

4. Add the unit and behavioural coverage required in Milestone 3, including the negative security regression test proving WASM-provided GitHub secrets still block.

5. Run the targeted suites with `tee`, review the logs, and update this plan’s living sections with actual outcomes.

## Progress

- [x] 2026-03-09 20:47Z: Confirmed branch is `secret-blocking-overzealous` and gathered the governing repo instructions plus relevant skill guidance.
- [x] 2026-03-09 20:48Z: Investigated the current failure path and confirmed that `grepai` was unavailable locally, so exact search and symbol lookup were used instead.
- [x] 2026-03-09 20:50Z: Confirmed that `tools-src/github/github-tool.capabilities.json` injects `github_token` as bearer auth for `api.github.com` and that `tools-src/github/src/lib.rs` intentionally relies on host injection rather than setting `Authorization` itself.
- [x] 2026-03-09 20:52Z: Confirmed that `src/tools/wasm/wrapper.rs` injects host credentials before `LeakDetector::scan_http_request(...)`, while `src/channels/wasm/wrapper.rs` performs the scan before host credential injection with explicit false-positive rationale.
- [x] 2026-03-09 21:04Z: Used commit history, blame, and semantic diffs to trace the regression to `a53b2c10` adding tool-side host credential injection, and compared it with `dc7d9cce`, which later added the same feature to channels but explicitly fixed the leak-scan ordering.
- [x] 2026-03-09 20:55Z: Drafted this ExecPlan to drive a narrow, test-first fix.
- [x] 2026-03-09 21:18Z: Added `StoreData::prepare_http_request(...)` in `src/tools/wasm/wrapper.rs` to make the production request-ordering path directly testable and moved leak scanning before host credential injection in the tool wrapper.
- [x] 2026-03-09 21:19Z: Added the permanent regression `test_prepare_http_request_allows_host_injected_github_pat`, the negative test `test_prepare_http_request_blocks_wasm_supplied_github_pat`, and the higher-level host-function test `test_http_request_progresses_past_leak_scan_for_host_injected_github_pat`.
- [x] 2026-03-09 21:22Z: Ran `cargo fmt --all`.
- [x] 2026-03-09 21:24Z: Ran `cargo test tools::wasm::wrapper --lib -- --nocapture`; all 26 wrapper tests passed, including the new regression and behavioural tests.
- [x] 2026-03-09 21:25Z: Re-ran `cargo test test_scan_http_request_blocks_secret_in_header --lib -- --nocapture`; the direct leak-detector block case still passed.
- [x] 2026-03-09 21:27Z: Ran `cargo test --test tool_schema_validation -- --nocapture`; all 9 schema/registration tests passed.
- [x] 2026-03-12 16:50Z: Verified that `src/channels/web/server.rs` still carried live inline handler implementations despite existing `handlers/` modules, then moved the source-of-truth chat, extension, static/log/status, OAuth, and pairing handlers into `src/channels/web/handlers/*` and reduced `server.rs` to router composition plus shared state. This branch note keeps the later web-gateway refactor discoverable even though it is outside the original secret-blocking fix scope.

## Surprises & Discoveries

- `grepai` could not be used for this turn because the local Qdrant service on `127.0.0.1:6334` was unavailable. That did not block the investigation because the relevant paths were easy to locate with exact search.
- The channel-side WASM wrapper already documents the correct ordering and its security rationale, which makes the tool-side behavior look like an inconsistency rather than a deliberate GitHub-specific safeguard.
- The user-visible error chain includes both `header:Authorization` and the matched pattern name such as `github_fine_grained_pat`, which is consistent with `LeakDetector::scan_http_request(...)` wrapping lower-level pattern matches while scanning individual header values.
- Semantic history narrows the regression to a specific integration point: the leak scan in the tool wrapper already existed, and commit `a53b2c10` inserted host-based credential injection ahead of it while adding a larger host-credential feature set. The later channel commit `dc7d9cce` repeated the feature work but explicitly placed leak scanning first to avoid false positives.
- Existing tool-wrapper tests are helper-centric. They validate host credential injection, credential resolution, and redaction separately, but there is no current test that executes the production request-ordering path where this regression lives.
- The new behavioural test can avoid live GitHub access by targeting a reserved invalid public hostname. After the fix, the host function now gets past leak scanning and fails later with DNS resolution, which is the exact evidence needed to prove the false positive is gone without introducing an external network dependency.
- `src/channels/web/server.rs` had already accumulated extracted handler modules, but several of them were stale copies. The safe migration path was to treat `server.rs` as the source of truth, sync those modules from the live implementations, and then switch routing over; simply wiring the existing modules would have regressed image uploads, OAuth callback behavior, extension install flows, and gateway status responses.

## Decision Log

- 2026-03-09 20:49Z: Chose a narrow wrapper-level investigation rather than a full hosted-worker trace first. Rationale: the reported error string already points to outbound request scanning, and the hosted-worker meta-tooling and WIT plans show that the `github` tool is now reachable and WIT-aligned.
- 2026-03-09 20:53Z: Chose the channel-side wrapper as the behavioral reference for the intended fix. Rationale: it already explains why host-injected credentials must be added after leak scanning to avoid false positives.
- 2026-03-09 20:54Z: Chose not to propose secret-pattern exemptions for `Authorization` headers. Rationale: the security model should continue blocking WASM-provided secrets; the safe fix is to stop scanning host-injected values as if they were WASM output.
- 2026-03-09 21:04Z: Recorded `a53b2c10` as the introducing change and `dc7d9cce` as the corrective comparison point. Rationale: this gives a stronger basis for judging intent and fallout than line diff alone. The evidence indicates the intention was to add host-based credential injection and runtime reliability, while the later channel work shows the intended safe ordering for leak scanning.

## Outcomes & Retrospective

IronClaw’s GitHub PAT was not being blocked because GitHub auth was unsupported; it was being blocked because the generic WASM tool wrapper scanned the already-injected outbound request and therefore mistook host-managed credentials for exfiltrated secrets. Historical evidence strengthened confidence in a narrow fix: the introducing tool-side commit was trying to add host-based credential injection and related runtime support, and the later analogous channel-side commit explicitly adopted the opposite ordering to avoid false positives.

The implementation is now complete. `src/tools/wasm/wrapper.rs` has been aligned with the channel-side security model by moving leak scanning ahead of host credential injection through a small extracted helper, `StoreData::prepare_http_request(...)`. The permanent regression coverage now includes:

- a unit regression proving host-injected GitHub PATs no longer trip the tool-wrapper leak scan,
- a negative unit test proving WASM-supplied GitHub PATs still block,
- a higher-level behavioural test proving the real `http_request(...)` host function progresses past local leak blocking and fails later on DNS as expected.

Validation evidence:

- `cargo fmt --all`
- `cargo test tools::wasm::wrapper --lib -- --nocapture`
- `cargo test test_scan_http_request_blocks_secret_in_header --lib -- --nocapture`
- `cargo test --test tool_schema_validation -- --nocapture`

All targeted validation passed. The remaining gap is that there is still no live GitHub end-to-end test in this repo, but the wrapper-level behavioural test now covers the precise local failure mode that previously prevented any real outbound attempt.
