# RFC 0019: Effect v4 in the Axinite SolidJS front end

## Preamble

- **RFC number:** 0019
- **Status:** Proposed
- **Created:** 2026-10-28
- **Related:** [RFC 0018](rfc-0018-solidjs-front-end-adoption.md),
  [front-end architecture](../solidjs-frontend.md), and
  [ADR 0015](../adrs/adr-0015-effect-v4-at-front-end-application-boundaries.md).
- **Execution plan:** [Adopt Effect v4](../execplans/adopt-effect-v4.md).

## Summary

This RFC proposes adopting Effect v4 selectively in the Axinite SolidJS
front end to make multi-step asynchronous workflows, dependency selection,
typed failures, and resource lifetimes easier to follow and test. Effect will
compose application workflows; Solid will continue to own reactive rendering
and local interaction state, TanStack Query will continue to own remote query
caching and invalidation, and browser adapters will continue to own HTTP,
Server-Sent Events (SSE), and storage protocol details.

The proposal introduces no new gateway protocol or persisted format. Existing
HTTP DTOs, authentication behaviour, SSE event shapes, and user-visible
workflows remain the compatibility contract.

## Problem

The SPA currently uses native promises and callbacks across API clients,
TanStack Query mutations, and components. Request errors are categorized at the
HTTP client, but asynchronous workflows that combine several operations must
manually coordinate failures, retries, cancellation, and updates to Solid
state. SSE resources are separately opened and closed by their consumers.
These choices work, but they leave dependency ownership and asynchronous
lifetime rules implicit in each caller.

Replacing these mechanisms indiscriminately would duplicate existing state
owners and expand the change beyond the intended problem. In particular,
TanStack Query already coordinates cache keys, freshness, and invalidation, and
Solid already owns component scopes and reactive values.

## Current state

The browser application lives in `web-src/axinite/src`. The shared HTTP
adapter in `lib/api/client.ts` handles bearer authentication, a five-second
request timeout, JSON responses, empty 204 responses, and timeout, transport,
and HTTP error categories. API modules provide typed endpoint functions.
Components and providers combine TanStack Query with Solid signals. The chat
and logs clients expose browser `EventSource` connections; their owning UI
surfaces close them as part of lifecycle cleanup.

The front-end workspace is typechecked with TypeScript 7 and TypeScript 6,
uses Vitest for its unit and behaviour suites, and uses Playwright for browser
tests. Bun runs the development scripts and mock backend. `fast-check` is
already installed as a development dependency. The existing tests are not
`bun:test` tests.

## Goals and non-goals

Goals:

- Make dependencies required by application workflows explicit and replaceable
  in tests.
- Model expected request failures as typed values while keeping defects and
  interruption distinguishable.
- Give asynchronous operations and long-lived browser resources an explicit
  owner, cancellation path, and cleanup rule.
- Use `Effect.gen` for workflows with dependent branching and use small
  `pipe` compositions for linear transformations and observation.
- Preserve current UI, HTTP, SSE, security, localization, and deployment
  behaviour.
- Define validation proportional to each guarantee, including the requested
  use of Bun's `bun:test` where it can be made compatible with the workspace.

Non-goals:

- Replacing Solid signals, TanStack Router, TanStack Query, or the Rust gateway.
- Adding Effect wrappers around pure functions or operations without an
  application boundary benefit.
- Moving all browser state into Effect services or introducing a second query
  cache.
- Adding automatic retries to mutations or competing with EventSource's native
  reconnect behaviour.
- Proving third-party Effect, browser, or network implementation correctness.
- Migrating the entire existing Vitest suite to Bun's test runner as part of
  Effect adoption.

## Proposed design

### Ownership and dependency direction

Keep the browser application divided by responsibility, without imposing a
new canonical folder tree:

```text
Solid components and Query observers
        │ Query owns shared read lifetime and supplies AbortSignal
        ▼
Query execution bridge (Effect Exit/Cause to Query Promise outcome)
        │ invokes workflow; route/action scopes own non-Query work
        ▼
Application workflows (Effect.gen / Effect.fn)
        │ require application-owned ports
        ▼
Ports declared with Effect Context.Service
        ▲ implemented by browser adapters
        │
fetch / EventSource / storage / Query Promise boundary
```

Domain and policy functions remain pure TypeScript. They do not import Effect,
Solid, TanStack, or browser APIs. Application workflows may import Effect and
depend on domain functions and application-owned ports. Adapters implement
those ports using browser facilities. The composition root in
`web-src/axinite/src/app/providers.tsx` constructs and provides a single
application runtime per mounted root. Query observers share work according to
Query's lifecycle: disposing one Solid observer detaches that observer and
must not cancel a request another observer still uses. Query supplies the
request `AbortSignal`; actual Query cancellation aborts the Effect and its
Fetch adapter. Root or session shutdown cancels all outstanding Query work.
Route and action scopes own non-Query work and subscriptions only. No
application workflow may launch an unowned detached fibre.

The application runtime is composed without performing network bootstrap in a
layer constructor. The `ManagedRuntime` API builds lazily, caches its first
build result (including a failed build), and requires disposal. Therefore
layers should only construct services and adapters; bootstrap effects run when
the application explicitly invokes them. Application disposal releases the
runtime; route or component disposal interrupts only non-Query work started in
its scope.

### Effect and framework boundaries

Use `Effect.gen` for workflows whose later steps depend on earlier results,
branch on typed failures, or coordinate cleanup. Name reusable operations with
`Effect.fn`. Use `pipe` and operators such as `map`, `tap`, and `andThen` for
short linear composition. Make concurrency limits explicit at every fan-out.
The generator syntax improves control flow; it does not make browser callbacks
synchronous or remove the need to state cancellation ownership.

Effect workflows return Effect values. The Query execution bridge converts an
Effect to the Promise that `queryFn` and mutation callbacks require. It inspects
the Effect `Exit` and `Cause` so tagged failures remain typed, interruption is
reported as Query cancellation without retry or user-facing failure, and
defects reach a separate safe reporting path rather than leaking a generic
`FiberFailure`. The bridge forwards Query's `AbortSignal` and does not bind the
request to one observer's Solid scope. This is an intentional integration
boundary, not a compatibility facade for Axinite's private APIs. Query retains
its cache, cancellation, and retry configuration; Effect does not add an
independent retry loop for those same reads.

### Readable workflow shape

The restart workflow is a concrete use of generators: acquire its services,
subscribe before sending the command, then branch on acknowledgement and
recovery events. Put sequential dependencies and local names in `Effect.gen`;
attach scope management and bounded diagnostics in a short outer pipeline.
Keep the pure transition calculation as an ordinary function. This avoids a
nested chain of callbacks while leaving the order of operations visible.

The following is non-executable pseudocode, not a claim about v4 signatures:

```text
named restart workflow:
  generator:
    obtain restart gateway service
    acquire owned event subscription
    send the restart command once
    consume events through the pure transition policy
    poll sequentially only after acknowledgement
    return the verified terminal outcome
  pipeline:
    attach operation name and safe diagnostics
    apply the action resource scope
framework boundary:
  execute with the owner's cancellation signal
  publish only while the owner remains current
```

In M1, replace the pilot's pseudocode with compiling examples against the
resolved package. Use `map` for pure output transformations and `tap` for
observation; do not hide branching workflows in long point-free pipelines.
No component should need to understand service-layer construction to render a
loading, success, or failure state.

### Failure and cancellation model

The application failure channel distinguishes the failures the UI can handle:

- transport failure;
- HTTP response failure, preserving status and safe user-facing message;
- request timeout;
- response decoding or contract failure.

Expected failures use the typed error channel and are translated to existing
localized loading, retry, or error states at the application/UI boundary. The
bridge inspects `Exit` and `Cause`, including composite causes. Pure
interruption becomes Query cancellation without retry or error UI. If a cause
contains both defect and interruption, report the defect safely without
publishing state into a closed owner. Defects otherwise remain separate from
expected failures. An interrupted HTTP operation passes its Effect-provided
`AbortSignal` to `fetch`. The five-second request deadline remains an explicit
request policy and is distinct from owner cancellation.

Authentication is resolved from the current browser session for each request.
A session change cancels outstanding Query work, clears dependent query state,
and disposes session-owned resources. A disposed or superseded non-Query
workflow cannot publish a late result into Solid state: owners check the
current attempt before publishing where the underlying library cannot enforce
cancellation. HTTP method, headers,
bearer token precedence, 204 handling, status and body handling, endpoint
decoders, and current error categories remain covered by contract tests.

Aborting a POST only stops the client from waiting; it cannot undo a server
commit that already happened. Cancellation must not be described as rollback
or cause an automatic replay. Mutations do not retry automatically because
repeating one may repeat a user action. TanStack Query remains the retry-policy
owner for reads already managed by Query. An explicit Effect workflow may
retry only where the operation's contract permits it and where it is the sole
retry owner. The restart confirmation workflow polls sequentially, waiting
three seconds after each status check settles, until recovery or disposal.
This keeps a three-second wait while preventing overlapping checks, and
deliberately changes from interval start cadence to a post-settlement delay.
It introduces no attempt limit or new deadline. Its
phase and attempt-publication policy must be represented by a small pure
transition kernel so it can be tested and formally checked.

### Long-lived streams

SSE adapters register EventSource callbacks and expose validated events through
an Effect-compatible scoped resource or stream. Finalization removes listeners
and closes the source exactly once when its application subscription owner
ends. Query observer cleanup does not own shared request cancellation. The
browser's native reconnect remains the sole reconnect mechanism.
Effect must not retry by opening competing EventSource instances.

The server does not currently provide event IDs or replay cursors. The design
therefore cannot promise lossless delivery across disconnects. If an
application-owned bounded queue fills, the stream must surface an overflow
failure and trigger the existing recovery/refetch path; it must not silently
drop events or consume unbounded memory. Existing frame validation,
event-specific parsing, diagnostics, and observable SSE wire contracts remain
owned by their adapters. Only EventSource connection errors delivered through
`setStreamConnectionErrorHandler` may signal an outage to restart
orchestration. Named server `error` MessageEvents and malformed frames remain
application events or diagnostics; they do not count as an outage.

## Requirements

### Functional requirements

- **R1 — Dependency ownership:** Workflows receive dependencies through
  explicit application-owned services, with browser implementations wired at
  the composition root.
- **R2 — Failure semantics:** Expected HTTP, timeout, transport, and decoding
  failures map to visible existing UI states; interruption and defects remain
  distinguishable.
- **R3 — Lifetime semantics:** Request cancellation reaches `fetch`; stream and
  runtime resources close with their owners; stale results cannot update a
  superseded workflow.
- **R4 — Boundary preservation:** Query owns server-state cache and read retry
  policy; Solid owns reactive and local UI state; adapters own browser I/O and
  protocol translation.
- **R5 — Evidence:** Unit tests cover success/failure mappings and cleanup;
  behaviour tests cover visible failure and recovery; Playwright covers
  externally visible cancellation or stream lifecycle changes. `bun:test`
  covers new unit and workflow behaviour tests, while existing rendered
  component suites remain under Vitest. Property tests cover range and ordering
  invariants. Formal proof covers the restart policy, and any queue policy if a
  queue is introduced.
- **R6 — Delivery:** The Effect dependency is pinned by the lockfile to a
  compatible v4 release. TypeScript 6 and 7 checks, the frontend build and
  embedded asset verification continue to pass.

### Technical requirements

- Effect version and import paths are verified against the installed v4
  package before implementation. No unstable v4 API is used without pinning and
  documenting the version constraint.
- Test APIs and runtime APIs used by production code compile under both
  configured TypeScript versions and Vite's browser build.
- Query Promise integration is centralized and preserves Query's cancellation
  and retry contract.
- `Effect.all` and other parallel fan-out operations specify concurrency.
- No unbounded queue, detached workflow fibre, silent mutation retry, or
  duplicate cache is introduced.
- The production bundle's gzip size is measured before and after the dependency
  pilot using the same build. An increase greater than the larger of 25 KiB or
  10% of the baseline initial JavaScript payload is a review trigger, not an
  automatic rejection; the effect tree-shaking and import shape must then be
  assessed before continuing.

## Alternatives considered

### Keep native promises and callbacks

This requires no dependency or migration cost and remains suitable for simple
single-request API calls. It does not improve explicit dependency ownership or
make composed error and cancellation policy easier to test across workflows.

### Replace TanStack Query with Effect-managed state

This would unify asynchronous execution but would also replace established
cache identity, invalidation, and component integration. The RFC has no
requirement that justifies this migration cost, and it would make the initial
adoption larger and harder to isolate.

### Adopt Effect at application workflow boundaries

This keeps current UI and cache owners while making selected workflows' ports,
failure channels, concurrency, and lifetimes explicit. It is the recommended
option, subject to the compatibility and verification gates below.

## Compatibility and migration

1. **Feasibility gate:** Add the Effect v4 dependency and a minimal browser
   runtime experiment. Verify version, browser build, TS6/TS7, Bun test runner
   interoperability, cancellation bridge, API stability, and measured bundle
   delta. Prototype extraction and proof feasibility against the planned
   restart transition shape using a temporary harness. This validates tooling
   only; it does not discharge the production proof. If the actual kernel
   cannot be proved, stop and return for design revision.
2. **HTTP client:** Migrate the shared HTTP operation signatures and all
   affected gateway callers atomically. Pure validators and data transformations
   remain ordinary functions. Keep the Promise conversion TanStack Query
   requires at its boundary. Cover cancellation, timeout,
   transport/HTTP/decode errors, auth header precedence, JSON, 204, and
   generation-safe publication.
3. **Restart workflow:** Express the acknowledged restart, outage observation,
   recovery, failed request, retry action, and success transition as a named
   Effect workflow. Poll sequentially until recovery or disposal. Keep
   mutation attempts user-controlled. Preserve the 3,000 ms poll interval by
   waiting three seconds after each status check settles, so checks never
   overlap. Replace the temporary proof harness with the actual pure attempt
   transition kernel, generated tests, and required in-place proof.
4. **SSE lifecycle:** Wrap chat and log EventSource lifetime behind scoped
   adapters. Preserve native reconnection, parsing, recovery/refetch, and
   diagnostics. Add visible overflow handling if the chosen bounded stream
   adapter can overflow under production event rates.
5. **Adoption closeout:** Update the SolidJS architecture document and
   `docs/developers-guide.md` with the service boundary, Promise integration,
   error/cancellation and testing conventions. Run the full front-end suite,
   Playwright and embedded-asset check, plus required root repository gates.

Each migration milestone remains a coherent state. Changes are atomic where
they update a private interface and its callers; no compatibility aliases or
dual implementations are introduced for internal code.

## Verification strategy

New unit and workflow behaviour tests in the Bun-only directory use
`bun:test`; rendered Solid component and accessibility tests remain in Vitest.
Both TypeScript versions check the Bun files, and Vitest discovery excludes
them so each test runs once. Tests exercise actual adapter contracts with
injected fetch/EventSource implementations and the real Effect runtime
boundary. Use TestClock for Effect timers and retry schedules; it does not
virtualize native browser EventSource, so stream callback and browser ownership
behaviour require contract tests and Playwright. Required HTTP cases include
success, transport rejection, 4xx/5xx, timeout, malformed response, 204,
already-aborted signal, caller cancellation, cancellation racing a typed
failure, and a mixed defect/interruption cause. A Query cancellation must abort
its transport; disposing one of two actual Query observers of a shared read
must leave that request alive for the other observer. Explicit Query
cancellation must abort the same request. Also verify root/session shutdown
cancels all Query work. A cancelled POST must not be automatically replayed,
and tests must not infer that abort rolls back a server commit. Cover no stale
non-Query publication, finalizer close-once, callbacks during close and after
close,
malformed SSE, the distinction between named server `error` MessageEvents and
connection failures, browser reconnect, and visible recovery. Preserve the
client's `setStreamConnectionErrorHandler` contract.

The pure restart transition receives an event and `current: boolean`. The
adapter derives that value from opaque object identity and closure state
immediately before transition and publication. The kernel contract, conditional
on that classification, is:

> A stale event stutters without changing state or emitting commands; completed
> and disposed states are absorbing; failed states remain retryable; a new
> attempt clears earlier evidence; and restarted requires acknowledgement,
> outage, then recovery for the same current attempt.

Use `fast-check` to generate event sequences and superseding-attempt schedules.
The generator reports counts for active, stale, closed, acknowledged, outage,
and recovered states. Mutations that let stale events change state or emit a
command, reactivate completed/disposed states, remove retryability after
failure, carry evidence between attempts, or publish recovery out of order
must fail. In M3 prove the actual production transition function in place with
LemmaScript and the pinned Dafny toolchain. Prove one-step preservation and
trace induction over the kernel. M1/M2's temporary harness checks feasibility
only; it does not discharge this required production proof. The guarantee is
conditional on correct `current` classification. The proof does not establish
asynchronous event isolation or object-identity correctness; test those adapter
duties separately. V3 generated delayed-callback schedules and V4 fake
EventSource callbacks during close and after close exercise those unproved
adapter obligations. Do not model Effect execution, browser callbacks, or
network behaviour as proved.

If an asynchronous stream queue is introduced, its admission, capacity, and
ordering policy also creates an invariant requiring generated tests and
substantive proof before implementation. The queue-free adapter avoids that
additional obligation. Effect runtime scheduling and cancellation semantics,
browser Fetch and EventSource behaviour, TanStack Query's Promise contract, and
LemmaScript's verified source subset are external axioms; test repository-owned
integration against the actual libraries and document their versions and
remaining trust boundary. If LemmaScript cannot prove the actual kernel, stop
and request a design revision; do not replace the required proof with sampled
tests. Pin the LemmaScript and Dafny toolchain in the repository and make the
proof command a required CI gate; local proof alone is not acceptance evidence.

## Risks and open questions

- **Effect package size:** the core runtime may exceed the bundle review
  threshold. Measure the real optimized browser build before selecting the
  migration scope.
- **Runner integration:** the existing workspace uses Vitest for rendered
  component tests. Add the requested `bun:test` suites with explicit discovery
  so each test runs under exactly one runner.
- **ManagedRuntime failure caching:** a failed initial layer build is cached.
  Layers must remain construction-only, and recoverable bootstrap work must be
  an invoked Effect.
- **EventSource buffering:** server-sent events have no event IDs. The selected
  stream integration must document overflow and recovery behaviour instead of
  claiming replay guarantees.
- **Boundary expansion:** wrapping every API call can add little value and make
  the Promise seam obscure. Migrate callers in coherent slices and require a
  named workflow or failure/lifetime benefit for each Effect wrapper.
- **Formal tool maturity:** LemmaScript is a technology preview. Confirm its
  executable, supported subset, and proof output before treating it as a gate.

Open questions for RFC review:

1. What discovery configuration ensures each `bun:test` case runs exactly once,
   while the existing Vitest rendered suites remain unchanged?
2. Is the proposed bundle review threshold suitable for the browser payload?

## Decision record

The following proposals are recorded in
[ADR 0015](../adrs/adr-0015-effect-v4-at-front-end-application-boundaries.md):

- **D1:** Effect owns selected application workflows, not Solid rendering or
  TanStack Query caching.
- **D2:** Pure policy stays framework-independent; application ports point
  inward and browser adapters implement them at the composition root.
- **D3:** Expected failures, defects, and interruption remain separate; retries
  and cancellation each have one owner.
- **D4:** Long-lived stream resources are scoped, bounded, and honest about
  delivery gaps.
- **D5:** Adoption proceeds only after browser, dual-compiler, Bun test, bundle,
  and proof feasibility are demonstrated; CI runs the pinned production proof.

## Design review dispositions

The six-perspective community review on 2026-10-28 changed the proposal before
publication. Pandalump's architecture review and Telefono's contract review
required Query-owned shared request lifetimes and a two-observer regression.
Wafflecat's alternatives review retained opaque attempt identity instead of
adding numerical exhaustion behaviour for the prover. Doggylump's reliability
review separated mutation cancellation from server rollback and distinguished
SSE messages from transport outages.

Buzzy Bee's performance review required a reachable browser pilot for bundle
measurement and an explicit polling cadence. Dinolump's maintainability review
kept the declaration probe isolated, retained the M1/M2 proof fixture until M3,
and required proof-tool installation and CI coverage. The revised ExecPlan
received a final correctness review with no remaining blocking findings.

The trade-off is deliberate: Effect supplies workflow structure while Query
and Solid keep their existing ownership. The proof and additional test runner
increase contributor setup, so their scope and CI integration are explicit.
These dispositions do not constitute maintainer acceptance of the RFC.

## References

- [Effect v4 onboarding](https://effect.website/docs/v4/onboarding)
- [Effect v4: using generators][effect-generators]
- [Effect v4: building pipelines][effect-pipelines]
- [Effect v4 ManagedRuntime](https://effect.website/docs/v4/runtime)
- [Effect v4 TestClock][effect-test-clock]
- [LemmaScript repository](https://github.com/midspiral/LemmaScript) and
  [specification][lemmascript-spec]
- [Documentation style guide](../documentation-style-guide.md)
- [RFC 0018: SolidJS front-end adoption](rfc-0018-solidjs-front-end-adoption.md)
- [Front-end architecture](../solidjs-frontend.md)

[effect-generators]: https://effect.website/docs/v4/getting-started/using-generators
[effect-pipelines]: https://effect.website/docs/v4/getting-started/building-pipelines
[effect-test-clock]: https://effect.website/docs/v4/api/effect/testing/TestClock
[lemmascript-spec]: https://github.com/midspiral/LemmaScript/blob/main/SPEC.md
