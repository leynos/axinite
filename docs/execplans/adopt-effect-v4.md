# Adopt Effect v4 in the SolidJS front end

Status: DRAFT. Implementation requires explicit approval of this plan. This
branch contains planning documents only; no Effect dependency or runtime change
has landed. Created 2026-10-28 against repository baseline `228bd47e`.

This ExecPlan is a living document. Keep Constraints, Tolerances, Risks,
Progress, Surprises & discoveries, Decision log, Outcomes & retrospective,
Conformance basis, and Verification plan current at every milestone.

## Purpose / big picture

Make asynchronous front-end work readable, cancellable, and explicit about its
failures and dependencies. A user should still navigate the same SolidJS UI,
send chat messages, inspect jobs, and restart the gateway. Navigating away or
retrying must retire old work; failures must retain their useful, localized
presentation. Developers should be able to test these behaviours with supplied
services and controlled time rather than browser globals and arbitrary sleeps.

Effect is a TypeScript library describing computations with success, failure,
and required-service types. A service is an application-owned dependency
interface; a layer constructs its implementation. A fibre is an executing
computation, and a scope owns resources and their cleanup. Solid owns rendering
and local state; TanStack Query continues to own server-state caching.

## Constraints

Preserve the Rust gateway's HTTP and Server-Sent Events (SSE) contracts,
authentication, ten-locale UI, accessibility, embedded single-binary deployment,
and legacy rollback switch. Do not change Rust persistence, backend semantics,
feature-flag precedence, endpoint paths, or introduce a second cache. Existing
runtime DTOs remain authoritative; this is not a wholesale schema migration.

Keep pure domain/policy functions independent of Effect, Solid, Query, browser
globals, and I/O. Application workflows may use Effect; application-owned ports
must not expose concrete browser adapters. Preserve feature-oriented files; do
not transplant a canonical domain/application/adapters directory tree.

New unit and workflow behavioural tests must use `bun:test`. Retain existing
Vitest component/a11y suites and Playwright system tests. Use `fast-check` for
range/order invariants, and substantive in-place proof for the restart policy
contract described below. Proof does not replace real-boundary tests.

Run formatting, format checks, typechecks, lint, and tests sequentially for this
candidate. Never bypass compiler admission or isolate the shared Cargo cache. Do
not build in `/tmp`; use it for logs. Commit each coherent change only after its
gates pass. New executable examples must be exercised from their source fence or
replaced with explicitly labelled non-executable pseudocode.

## Tolerances (exception triggers)

Stop and seek a design revision if implementation requires a backend/wire or
persistent-format change, a replacement for Query/Solid, automatic mutation
retries, an additional production package beyond `effect`, or an unproved
replacement for the required pure contract. LemmaScript and its pinned Dafny
backend are allowed development tools; `fast-check` is already installed.

Review a milestone exceeding 35 non-ExecPlan source/configuration/documentation
files or 2,000 net handwritten lines before continuing. Exclude ExecPlans,
lockfiles, generated proof output, and embedded build artefacts from line
counts; include non-ExecPlan documentation. No time-based limit applies.

Stop if three attempts to fix the same gate failure do not resolve it, if the
proof tool cannot model the actual kernel, or if initial compressed JavaScript
size grows by more than the larger of 10% or 25 KiB against M1's same-toolchain
baseline. These are review triggers, not permission to suppress findings or
weaken a theorem. Record impacts on RFC requirements before resuming.

## Risks

- High impact, medium likelihood: framework cancellation and Effect
  interruption disagree. Test the actual Query signal, Fetch abort, Solid
  disposal, and late callback boundaries; do not rely on typechecking alone.
- High impact, medium likelihood: a stream overflow or reconnect loses events.
  Preserve native EventSource reconnect, surface overflow, and resynchronize
  history. The gateway has no replay cursor; do not promise lossless delivery.
- Medium impact, medium likelihood: Effect or proof tooling differs from the
  documentation. M1 checks the resolved release against both TypeScript 6 and
  7, Bun, Vite, and the real proof toolchain before the rollout.
- High impact, medium likelihood: a proof models arithmetic or external calls
  differently from JavaScript. Prove the actual pure source, bound numerical
  identifiers, inspect generated assumptions, and test the unproved boundary.
- Medium impact, medium likelihood: a runtime's initial layer failure poisons
  all subsequent work. Keep layer construction free of network bootstrap;
  run retryable initialization as an ordinary effect.

## Progress

- [x] (2026-10-28) Renamed local branch to `adopt-effect-v4`; publication
  targets `origin/audit-to-adopt-effect-v4`.
- [x] (2026-10-28) Read repository guidance; two Wyvern investigations mapped
  the application and test boundaries; primary-source research checked v4.
- [x] (2026-10-28) Drafted the RFC, proposed ADR, and implementation plan.
- [x] (2026-10-28) Six-perspective expert review identified shared Query
  ownership and proof-plateau defects; revisions incorporated and final
  correctness review found no remaining blockers.
- [x] (2026-10-28) Planning-branch `make check-fmt` and all five
  `make typecheck` configurations passed.
- [x] (2026-10-28) Planning-branch `make lint` passed all Clippy and Whitaker
  configurations.
- [x] (2026-10-28) Workspace tests passed (4,442 passed, 12 skipped);
  standalone GitHub-tool tests passed (5 passed).
- [x] (2026-10-28) Transport and probe tests passed on the resumed aggregate
  after build-directory removal interrupted the original test invocation.
  The planning PR records final documentation checks and publication receipts.
- [ ] Obtain explicit implementation approval.
- [ ] M1: prove compatibility and establish the verification harness.
- [ ] M2: migrate HTTP application boundaries and all affected callers.
- [ ] M3: migrate and verify restart orchestration.
- [ ] M4: migrate scoped chat and log subscriptions.
- [ ] M5: complete system acceptance and documentation reconciliation.

## Surprises & discoveries

The front end already ships and is embedded in the Rust gateway. RFC 0018
retains historical proposal language; `docs/solidjs-frontend.md` and current
source describe the implementation. The package script `test` invokes Vitest,
not Bun's test runner. New Bun suites therefore need explicit discovery and must
not run a second time under Vitest. `fast-check` is already a dependency.

`make all` currently includes `spelling` as well as check-fmt, lint, and test.
Root Rust targets do not cover the separate browser workspace. CodeGraph's
initial results included unrelated repositories, so Wyvern verified locations
against this checkout rather than trusting those results as local evidence.

## Decision log

2026-10-28: Propose Effect at application boundaries, preserving Query as cache
owner (RFC D1 / ADR 0015). This gains explicit dependencies and cancellation
without changing the reactive model. The Promise bridge serves Query's actual
library interface, not an obsolete internal API.

2026-10-28: Add Bun-native workflow tests alongside existing rendered suites
(RFC D5). This honours the requested runner without an unrelated test-framework
migration. Separate discovery and CI inclusion are part of M1 acceptance.

2026-10-28: Expert review chose to preserve opaque attempt identity. The
verified pure policy consumes a current/stale classification; actual identity
classification and publication timing remain independently tested obligations.
This avoids adding numeric exhaustion semantics solely for proof tooling.

2026-10-28: Verify the repository-owned restart transition policy in place
rather than claiming to prove Effect, Fetch, or browser scheduling. The current
`RestartLifecycle` already fences attempt identity before cleanup; preserve that
contract while exposing a pure kernel for proof (RFC D3).

## Outcomes & retrospective

Planning documents are drafted and reviewed. Format, typecheck, lint, and all
test constituents passed. The original test aggregate failed after build
outputs disappeared; `make -o test-workspace -o test-github-tool test` passed
the remaining stages. Final documentation receipts belong in the planning PR.
Implementation, new tests, and proofs have not run.
Expert review corrected shared-query ownership, retained the proof feasibility
fixture until production integration, and strengthened actual-boundary tests.
Performance review requires a reachable bundle pilot and explicit poll cadence;
maintainability review preserves the narrow declaration probe and adds proof CI
setup. These changes trade a small additional verification harness for evidence
that matches the proposed guarantees. The important discovery is that adoption
must respect two existing owners: Query for server state and Solid for UI
lifetimes. Gate receipts and expert review dispositions will be recorded before
publishing this planning branch.

## Context and orientation

Run repository commands from the root. The browser package is `web-src/`, with
source under `web-src/axinite/src/` and tests under `web-src/axinite/tests/`.
Read `web-src/AGENTS.md`, `docs/solidjs-frontend.md`, and
`web-src/docs/axinite-v2a-frontend-architecture.md` before implementation.

`app/providers.tsx` constructs Query/i18n/feature-flag providers. The current
Query defaults are one retry and a 15-second stale interval. `lib/api/client.ts`
provides `requestJson`, `postJson`, `deleteJson`, `putJson`, and
`withRequestTimeout`, using a five-second request deadline and bearer headers.
Endpoint modules under `lib/api/` bind gateway DTOs and existing validators.
`lib/api/chat.ts` and `lib/api/logs.ts` own native EventSource bindings.

`lib/restart.ts` contains `RestartLifecycle` and `createRestartController`;
`components/restart-control.tsx` supplies browser wiring. The contract requires
an acknowledged restart, then an observed outage and recovery; failed sends
allow explicit retry, while disposal and completion close the controller.
Chat/log components currently close subscriptions through their Solid owner.

`web-src/package.json` and both tsconfigs enforce strict types under TypeScript
7 and 6. Vite performs Solid JSX compilation. `web-src/vitest.config.ts` owns
existing component tests; a direct Bun invocation does not replace that JSX
transform. `tests/e2e/CLAUDE.md` explains Python Playwright against the actual
Rust gateway. `web-src/axinite/tests/e2e/app-shell.pw.ts` covers the stub stack.

## Conformance basis

The baseline is commit `228bd47e`; there is no separate Terms of Reference or
approved Effect design. The proposed design is [RFC
0019](../rfcs/rfc-0019-effect-v4-in-the-axinite-solidjs-front-end.md), with [ADR
0015](../adrs/adr-0015-effect-v4-at-front-end-application-boundaries.md). Both
remain Proposed. RFC 0018, ADR 0014's lint policy, `docs/repository-layout.md`,
`docs/documentation-style-guide.md`, and the two front-end architecture
references constrain implementation.

Trace the RFC's requirements as follows: R1 dependency/readability -> M1/M2/M3
-> service substitution and workflow tests; R2 errors -> M2 -> HTTP matrix and
localized failure E2E; R3 lifetime/concurrency -> M3/M4 -> transition proof,
generated schedules, scope tests, reconnect E2E; R4 boundaries -> M1 through M5
-> import checks and code review; R5 verification -> every milestone -> named
Bun, property, proof, and browser receipts; R6 delivery -> M5 -> embedded
staleness and full gates. Update these links when a design revision changes
scope; do not silently amend an accepted requirement.

## Interfaces and dependencies

New names below are proposed, not existing APIs. Under `lib/api/`, define
application-owned gateway service shapes by cohesive endpoint family, with
`Context.Service` keys and explicit `Layer` implementations. A method such as
`JobsGateway.list` returns `Effect.Effect<JobListResponse, GatewayFailure>`; it
does not return a Fetch `Response`. `GatewayFailure` is a tagged union of
transport, HTTP status, timeout, and decode failures. It contains bounded safe
metadata; raw URLs, response bodies, and tokens never become diagnostics.

Keep HTTP implementation and token access in adapters, with injected Fetch and a
current-token reader for tests. Read the token per operation, not at layer
construction. Do not add services for every pure helper. Preserve explicit
header precedence and no-content response handling. Retain existing endpoint
validators; do not infer runtime safety from a generic `T` assertion.

Add `lib/effects/runtime.ts` for the application runtime factory and
`lib/effects/query.ts` for the permanent Query execution boundary. The boundary
accepts an Effect and the Query `AbortSignal`, returns the Promise Query
requires, and inspects `Exit` and its cause to preserve typed failures rather
than leaking a generic fibre rejection. Translate pure interruption into Query
cancellation without retry or error UI; report defects once through the safe
diagnostic boundary. If a cause contains both defect and interruption, report
the defect without publishing state into a closed owner. Test mixed causes and
already-aborted signals against the real Query contract. A `ManagedRuntime`
belongs to each mounted application root, not a module singleton. Register Solid
cleanup synchronously before awaiting work; supervise asynchronous disposal and
reject new work once closure starts. Non-Query route/action work has shorter
scopes; root disposal alone is insufficient. Query-managed reads belong to Query
and its supplied AbortSignal, including shared reads with multiple observers.
Unmounting one observer does not independently abort a shared request. Query
cancellation or session/root retirement interrupts it.

For pure restart policy, propose `lib/restart-policy.ts` beside
`lib/restart.ts`. Represent acknowledgement, outage observation, active attempt,
and closure as explicit state. `transitionRestart(state, event)` returns the
next state and publication decision without I/O. Retire attempt identity before
releasing adapters; check it immediately before every state publication. Keep
all proof relevant helper bodies in the verified unit rather than trusting
cross-file uninterpreted calls. The workflow owns scheduling and resources, not
the pure policy. Preserve existing restart semantics; any new terminal deadline
needs separate design acceptance.

Use `Effect.gen` and `yield*` for sequential dependencies and branches, and
`Effect.fn` for named workflows. Short `.pipe(...)` chains suit transformations,
error mapping, provisioning, and policy combinators. Prefer ordinary pure
TypeScript for calculations. No `runPromise` inside services, no
`Effect.succeed(alreadyStartedPromise)`, and no discarded fibres. Check exact v4
exports in M1 rather than copying v3 `Context.Tag`, `Effect.catchAll`, or
`Layer.scoped` examples.

## Verification plan

V1, HTTP/error contract: parameterized Bun tests in
`web-src/axinite/tests/effect/http.test.ts` cover JSON success, 204, custom
header precedence, token changes, 401/403/429/500, malformed JSON, transport
rejection, body-read failure, five-second deadline, caller abort, and completion
racing abort. Assert that interruption is not a timeout or a user-facing error
and that finalizers run once. Include already-aborted signals and mixed
failure/interruption causes. An aborted or timed-out mutation may already have
committed on the server; assert no automatic replay and no false rollback claim.
Reconciliation reads, where present, remain explicitly owned. Exercise the
actual browser adapter with controlled Fetch promises; confirm real abort in
browser tests. Mutation: disconnect the signal or classify every abort as
timeout; tests must fail.

V2, restart publication invariant: for every admissible event sequence, a
retired attempt cannot publish or schedule work; completed/disposed states
cannot become active; a recovery publication requires acknowledgement and
observed outage from that same attempt. Lemma A proves one transition preserves
these predicates. Lemma B proves by induction on event sequences that repeated
transitions preserve them. Prove `transitionRestart` in place with
LemmaScript/Dafny and inspect its generated body and assumptions. A proof of
only a helper equality does not discharge the transition contract.

Retain opaque attempt object identity in the workflow. The pure kernel accepts
an event classified with `current: boolean`; the adapter computes this from
actual identity and closure immediately before transition/publication. Prove
that stale events stutter (unchanged state, no commands or publications),
completed/disposed states are absorbing, failed states remain retryable, and a
new attempt clears old acknowledgement/outage evidence. This is a verified
transition policy conditional on correct event classification, not an exhaustive
proof of asynchronous stale-event isolation. Do not assume the transition
conclusions in preconditions. Identity comparison and atomic publication
discipline remain unproved adapter obligations exercised by V3/V4. No numeric
generation counter or exhaustion behaviour is introduced merely for the prover;
if proof arithmetic is later needed, justify JavaScript bounds.

Store proof companions next to `lib/restart-policy.ts` and track it in
`web-src/LemmaScript-files.txt`. Add `make -C web-src verify-effect-policy` that
runs pinned `lsc check --backend=dafny` over the list and fails on missing or
unverified entries. M1 records and installs exact compatible LemmaScript and
Dafny versions, their prerequisites, and a reproducible contributor setup
command. Wire the target into `frontend-full`/workspace `ff` and the existing
front-end CI job: it checks the labelled feasibility fixture in M1/M2 and the
production theorem from M3. CI must not silently skip an unavailable prover.
Check generated files for drift. No `assume`, unchecked external summary, axiom
equivalent to the goal, or contradictory precondition may discharge V2.
Witnesses must include start/acknowledge/down/up, failed send/retry/late-old-up,
disposal during cleanup, and retry reset. Allowing a stale classified event to
change state or allowing recovery before outage must make verification fail.
Separately remove the actual object comparison in the workflow and require the
runtime tests to reject it. Record the tool versions, source digest,
assumptions, and counterexample or failed verification condition.

V3, ordering/lifetime integration: use existing `fast-check` from Bun in
`restart.property.test.ts` and `scope.property.test.ts` under the new effect
test directory. Generate sequences of start, acknowledgement, stream error,
status completion, retry, and disposal with deliberately delayed callbacks.
Compare production policy against an independently written small reference
transition model. Generate at least 1,000 cases per normal run and retain seeds
and shrunk counterexamples. Classify current/stale, success/failure, repeated
cleanup, before/after disposal, and both race winners; fail if a material class
is unexercised. Do not filter away invalid events: assert their rejection. These
sampled tests complement, but do not constitute, the exhaustive proof.

V4, scope/resource contract: Bun workflow behaviour tests use real Effect
runtime/scopes and injected transports. Verify active requests and registered
listeners return to zero after disposal, no publication happens after owner
closure, restart polls never overlap, and mutation requests are never replayed
automatically. Use `TestClock` only for Effect-managed timers. A fake
EventSource must permit synchronous callbacks during close and callbacks after
close to challenge the generation fence. Negative controls remove cleanup or
permit two polls and must fail. Add rendered Vitest tests for the actual Solid
owner bridge; pure workflow tests alone cannot verify Solid disposal. Add an
actual Query two-observer test: unmounting one consumer leaves the shared read
alive for the other, while explicit Query cancellation aborts Fetch. Do not
assume last-observer removal always cancels: verify the configured Query policy.

V5, network/system contract: extend existing `api-streams.test.ts`,
`api-contract-alignment.test.ts`, and the mock backend contract tests. Preserve
named event handling, malformed-frame recovery, token encoding, inert missing
EventSource behaviour, and the existing bounded diagnostic counters. Named
server `error` MessageEvents and malformed frames are not transport outages:
preserve `setStreamConnectionErrorHandler` filtering and assert neither can
falsely complete a restart. Add Playwright scenarios for route change during a
pending request, retry after failure, chat and logs reconnect, restart
failure/retry, and safe diagnostics. Run the actual-daemon connection, chat, and
SSE reconnect scenarios as well as the stub suite. Assert UI outcomes and
request counts, not only screenshots.

Trusted external interfaces are Effect scope/interruption semantics, Fetch's
AbortSignal, native EventSource reconnect, Query cancellation, Solid cleanup,
and gateway responses. These are external assumptions, not newly proved axioms.
Real-runtime and browser tests exercise the repository's integration with them.
LemmaScript's translator, Dafny solver, and integer model are also trusted; true
async, browser scheduling, and third-party internals remain outside the proof.
Any new repository-owned contractual rule requires its own proof obligation
before implementation. No formal verification is claimed for this planning-only
commit.

## Milestones and plateaus

### M1: compatibility and verification feasibility

After explicit approval, add a stable Effect v4 requirement to
`web-src/package.json`, resolve and record its exact lockfile version, and
verify core exports under both TS compilers and the browser build. Prefer core
modules and browser Fetch/EventSource adapters, without Node/Bun platform code
in the browser. Record pre/post gzip bundle bytes with the same toolchain and
build mode. Use a reachable browser pilot entry that constructs the runtime and
invokes a representative workflow; an unused, tree-shaken dependency is not a
meaningful baseline. Measure the initial entry plus its required chunks and
repeat after M2 and M4 with the actual application. Remove the isolated pilot
entry before shipping it as a production route.

Create the `axinite/tests/effect/` Bun-only suite and a `test:effect` package
script using `bun test ./axinite/tests/effect`. Explicitly exclude that folder
from Vitest and a11y discovery; check it with both compiler versions through the
main tsconfig. Preserve the narrow, isolated declaration-probe tsconfig; do not
add Bun test globals to it. Add `make -C web-src test-effect` and integrate it
into root frontend-test, frontend-full, workspace ff, and
`.github/workflows/frontend.yml` via their existing aggregate scripts. Prove
tests execute once by enumerating discovered files and making a sentinel test
fail each aggregate during the experiment. Remove the sentinel before commit. Do
not remove existing rendered tests.

Prototype service substitution, typed errors, signal cancellation, scope
cleanup, and TestClock using `bun:test`. Prototype proof extraction on the
actual planned pure transition kernel with the pinned LemmaScript/Dafny pair;
review numerical semantics and generated assumptions. Keep a clearly labelled
proof-feasibility fixture in M1/M2, with a nonempty manifest and a negative
control. It establishes tool compatibility, not a production guarantee. In M3
replace that fixture and manifest entry atomically with the actual production
kernel and full V2 proof, retaining the feasibility evidence here. Tool failure
is a stop condition, not a reason to omit proof. M1 discharges R1/R4/R5
feasibility; M2-M4 remain unimplemented.

Acceptance E1: a tiny controlled workflow succeeds, fails with its tagged error,
and cancels with no leaked resource; the intended proof mutation is rejected.
Run the common gates below. Recovery is reverting M1's additive
harness/dependency commit; no production route depends on it yet.

### M2: HTTP boundaries and Query integration

Inventory all imports and consumers first, compare the resulting file list with
the 35-file tolerance, and resolve any excess before production edits. Write V1
failing tests before editing production code. Replace Promise-based endpoint
internals under `lib/api/` with Effect-returning application services, and
update all affected call sites in one coherent migration. Include auth
bootstrap, feature flags, pairing, chat, jobs, memory, routines, extensions,
skills, and log-level operations; inventory imports before committing. Pure
validators and data transformations remain ordinary functions. Every Query call
passes its cancellation signal through the execution boundary. Mutation and
non-Query callbacks have an explicit action owner.

Keep Query's existing one-retry read policy as the sole retry owner for these
queries; Effect transport makes one attempt. Mutations have zero automatic
retry. Decode errors remain distinct and safe; do not log raw transport
exceptions. Preserve existing user-visible error mapping, localization, query
keys, invalidation, auth storage, and feature-flag fallback. Session changes
retire outstanding work and clear session-owned cache before publishing data for
another token; test the ordering without redesigning authentication.

Acceptance E2: V1, existing API/component contracts, and a browser failure and
route-cancellation scenario pass. R1/R2/R4 are advanced; asynchronous restart
and stream ownership remain for M3/M4. Update the architecture documents and
regenerate embedded assets in the same commit. Recovery reverts source and
assets together. Do not retain old internal Promise APIs as compatibility shims;
the permanent Query bridge is the only framework conversion boundary.

### M3: restart workflow and proof

Record red tests for failed send/retry, duplicate start, stale callbacks,
cleanup re-entrancy, non-overlapping polls, and recovery ordering. Extract and
prove the V2 pure kernel, then replace callback/interval orchestration in
`lib/restart.ts` with an Effect workflow using owned scopes and sequential
polling. Update `components/restart-control.tsx` and every controller test in
the same change. Preserve the existing initiation heuristic and UI phases. A
poll begins only after acknowledgement, and the next poll waits for the previous
request to settle, then sleeps 3,000 ms through Effect Clock. This deliberately
replaces the existing 3,000 ms interval start cadence with a three-second
post-settlement delay to prevent overlap. TestClock tests must reject both a
busy loop and an overlapping poll. Subscribe before sending `/restart`. The
proof does not add a retry cap or a new terminal timeout.

Acceptance E3: V2 proof, V3 generated sequences, V4 behaviour, existing
`restart-control.behaviour.test.tsx`, and stub restart E2E all pass; failure
remains explicitly retryable and no mutation is repeated. R3/R5 are advanced.
Retire old controller internals rather than wrapping them. Roll back this
milestone's source and assets together if needed; HTTP services remain usable.

### M4: scoped chat and log subscriptions

Record red tests for unmount, remount, rapid route changes, malformed frames,
reconnect, and callbacks after close. Convert `connectChatEvents` and
`connectLogEvents` internals to scoped subscriptions; migrate every subscriber,
including restart and the documented `window.__axinite` E2E hooks. Keep
EventSource's native reconnect as the sole reconnect mechanism. Do not add an
Effect retry loop around it or invent Last-Event-ID support.

Avoid a new asynchronous queue when synchronous dispatch suffices. If a queue is
needed, use an explicit 256-frame capacity, preserve accepted-frame order, and
treat overflow as a visible resynchronization failure: close the local
subscription, invalidate/refetch authoritative history, and reconnect once under
the owner. Never silently drop chat approvals or completion events. Logs must
visibly indicate a gap because history cannot guarantee recovery. Test overflow
at 255/256/257 frames and failed resynchronization. This queue policy is a new
contract: prove its admission arithmetic and ordering kernel in place and extend
V3 before introducing it. A queue-free adapter avoids that additional
obligation.

Acceptance E4: V4/V5 and all existing stream/connection hooks pass; resource
counts return to baseline after repeated navigation. R3/R4/R5 are satisfied for
stream ownership. No server delivery guarantee is inferred from local ordering.
Recovery reverts subscriptions and callers together, preserving the wire
contracts and already migrated restart behaviour.

### M5: system acceptance and documentation

Run the complete browser and gateway acceptance below. Audit for discarded
promises, detached fibres, runtime execution inside services, duplicate retry
owners, and browser/framework imports in pure policy. Reconcile the service
inventory: every in-scope gateway operation uses the new application boundary;
localization loading and development mock-server internals are outside this
adoption. No dormant second transport implementation remains.

Update `docs/solidjs-frontend.md`,
`web-src/docs/axinite-v2a-frontend-architecture.md`, and
`docs/developers-guide.md` with implemented interfaces, owner lifetimes, error
and retry conventions, examples, Bun commands, proof trust boundaries, and
upgrade procedure. Check `FEATURE_PARITY.md`; change statuses only if actual
behaviour changed. Move RFC/ADR status only with recorded acceptance. R6 and all
remaining evidence are complete when E1-E5 are recorded and no deviation is
unaccepted. Revert the final integration commit if a new regression appears.

At each plateau verify assigned requirements, actual interfaces, dependencies,
wire/persistence boundaries, proof assumptions, and trace links against the
approved RFC. Record evidence and remaining work here. An architecture exception
sets Status to BLOCKED until explicitly accepted. No milestone requires
compatibility machinery for a private source API.

## Concrete steps and validation

For every implementation change, add the smallest failing test first, run it,
and record the expected assertion failure. Implement the minimum behaviour,
rerun that test, then refactor and run broader gates. Missing imports alone are
not adequate regression evidence when an existing behaviour is testable. New Bun
tests import from `bun:test`; existing DOM tests retain Vitest's Solid
transform. The proposed `test-effect` and `verify-effect-policy` Make targets
must be created in M1; they do not exist on this planning branch.

From the root, after each major milestone, run the following sequentially with
`set -o pipefail` and a distinct `/tmp/effect-v4-MN-<target>.log` via `tee`:

```sh
make check-fmt
make typecheck
make lint
make test
make spelling
make -C web-src check-fmt
make frontend-full
make frontend-build
make frontend-verify
make markdownlint
make nixie
git diff --check
```

After M1, `frontend-full` includes the new Bun and proof targets; run them
individually for red/green evidence, not as redundant full-suite repeats. These
include all constituents of the current `make all`; an aggregate run may replace
those constituents, but the explicit standalone `make typecheck` is still
required. Expect exit zero from every command, no skipped new verification
obligations, and no staleness diff. Do not overlap gates or call a running
compiler queue a pass. Inspect logs rather than repeating failed gates without a
fix. Run `make audit` if Rust dependency/audit files change; such changes are
otherwise outside the approved scope.

For M2 onward, run the affected stub Playwright flows through `make -C web-src
test-e2e`. For M4/M5, also follow `tests/e2e/CLAUDE.md` setup and run from
`tests/e2e/`:

```sh
pytest scenarios/test_connection.py scenarios/test_chat.py scenarios/test_sse_reconnect.py
```

The fixture builds and starts the real gateway. Expect the named scenarios to
pass with actual Chromium and the mock LLM. Stub-only results do not establish
real gateway integration. For interactive rendered validation, load the
agent-browser skill, run `agent-browser skills get core`, then inspect the
requested flows; use css-view only if computed styles need investigation.

Behaviour specifications belong in
`web-src/axinite/tests/effect/restart.behaviour.test.ts` as named Bun scenarios:
"Given an acknowledged attempt, when outage then recovery arrive, then publish
restarted once"; "Given a failed attempt and a retry, when the old callback
arrives, then the new phase is unchanged"; "Given a mounted owner with pending
work, when it is disposed, then resources close and no further state is
published". No separate Gherkin runner is required.

For the current documentation-only milestone, run the four explicitly requested
root gates plus spelling/Markdown/Mermaid/diff checks. There is no
implementation or new executable test to claim as passing. Record exact gate
outcomes in the PR, including environmental failures rather than calling
incomplete work green.

## Idempotence and recovery

Frozen dependency installs, test runs, and proof checks must be repeatable. Use
LemmaScript `regen` after source changes to preserve handwritten proof
additions; never edit generated-only files or delete proofs to obtain a pass. Do
not overwrite another agent's branch or processes. Revert only owned commits,
retaining source/build artefact consistency. The existing
`AXINITE_WEB_UI=legacy` switch is an operator rollback path, not an additional
implementation architecture. Gate failure blocks the next milestone and commit.

## Artefacts and notes

Primary-source research on 2026-10-28 checked Effect's
[onboarding](https://effect.website/docs/v4/onboarding),
[generators](https://effect.website/docs/v4/getting-started/using-generators),
[pipelines](https://effect.website/docs/v4/getting-started/building-pipelines),
[runtime](https://effect.website/docs/v4/runtime), and
[TestClock](https://effect.website/docs/v4/api/effect/testing/TestClock)
documentation. The installed version must still be checked in M1. LemmaScript's
[specification](https://github.com/midspiral/LemmaScript/blob/main/SPEC.md)
limits proof scope; it does not support proving browser async code directly.

Revision note (2026-10-28): Initial plan grounded in the shipped SolidJS
workspace and primary-source v4 documentation. Implementation remains awaiting
approval; review and planning-branch validation are the next steps.

Revision note (2026-10-28, expert review): Clarified Query-shared request
ownership, proof feasibility through M2, pinned proof CI integration, meaningful
bundle measurement, declaration-probe isolation, front-end formatting, and
restart polling cadence. Added cancellation cause, mutation outcome, and SSE
outage distinctions to acceptance evidence. Implementation remains unstarted.

Revision note (2026-10-28, validation): Recorded successful format and complete
typecheck gates; corrected one RFC spelling finding. Remaining validation and
publication are still pending.

Revision note (2026-10-28, lint): All Clippy and Whitaker checks passed. An
explicit seven-file Markdown check caught four incorrectly wrapped RFC links;
they were corrected. Tests and final documentation checks remain pending.

Revision note (2026-10-28, test recovery): Workspace and GitHub-tool suites
passed. The transport build then failed with missing output directories;
readback found the entire candidate `target/` absent and disk capacity
recovered. The remover is unknown. Resume the failed transport and pending
probe stages without repeating completed suites; record the resumed command
and results explicitly in the PR.

Revision note (2026-10-28, validation completion): The resumed transport and
probe stages passed. All root gate constituents now have successful receipts;
the original test failure remains disclosed. Final documentation validation
and branch publication are recorded in the accompanying planning PR.
