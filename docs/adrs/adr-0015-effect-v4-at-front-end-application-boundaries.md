# Architectural decision record (ADR) 0015: Effect v4 at front-end application boundaries

## Status

Proposed.

## Date

2026-10-28.

## Context and problem statement

The SolidJS application currently combines API Promise clients, TanStack Query
for remote caching, Solid signals for local state, and browser `EventSource`
callbacks for long-lived streams. Some multi-step workflows need clearer
dependency ownership, typed failure handling, cancellation, and resource
cleanup. The proposal to adopt Effect v4 must preserve the current state and
protocol owners while improving those workflows.

The detailed proposal is [RFC 0019]
(../rfcs/rfc-0019-effect-v4-in-the-axinite-solidjs-front-end.md). Its
implementation sequence is tracked in the [Effect v4 adoption ExecPlan]
(../execplans/adopt-effect-v4.md).

## Decision Drivers

- Preserve the established Solid, TanStack Query, and gateway boundaries.
- Make application workflow dependencies and expected failures explicit.
- Tie asynchronous work and browser resources to a clear owner.
- Keep browser ports replaceable in tests without importing browser APIs into
  policy code.
- Avoid adopting APIs that do not work with the current browser build and both
  configured TypeScript compilers.

## Requirements

### Functional requirements

- Selected multi-step UI actions compose typed asynchronous operations and
  publish existing user-visible states.
- Cancellation, timeout, request failure, and successful completion remain
  distinguishable to the owning workflow.
- Chat and log streams close when their owner is disposed and retain the
  existing event contract.
- Restart recovery is published only for the current attempt after
  acknowledgement, outage observation, and recovery in that same attempt.
- The proof is conditional on an adapter-supplied current-attempt value; adapter
  identity checks and publication timing remain tested integration obligations.

### Technical requirements

- Domain policy remains pure and framework-independent.
- Effect services define application-owned dependencies; adapters implement
  them at the browser composition root.
- TanStack Query remains the owner of query cache and read retry policy.
- The Effect runtime is created and disposed with the application root; route
  and action scopes own non-Query work and subscriptions, with no detached
  fibres. Query retains shared request lifetime and supplies its
  `AbortSignal`.
- The Query bridge inspects Effect `Exit` and `Cause`: typed failures retain
  their category, interruption becomes Query cancellation, and defects use a
  separate safe reporting path. Mixed defect/interruption causes report the
  defect without publishing state to a closed owner; an already-aborted Query
  signal cancels before starting transport.
- Promise boundaries required by TanStack Query stay explicit. No shim layer
  is added for Axinite's private APIs.
- New unit and workflow behaviour tests use `bun:test`; existing rendered
  component and accessibility tests remain under Vitest.
- The restart transition receives an event and `current: boolean`, derived
  from opaque object identity and closure state immediately before transition
  and publication. With `current: false`, it stutters without changing state
  or emitting commands. Completed and disposed states are absorbing; failure
  remains retryable; a new attempt clears prior evidence; restart requires
  acknowledgement, outage, and recovery in order. Generated tests and a
  substantive in-place LemmaScript proof cover this kernel. If the actual
  kernel cannot be proved, implementation pauses for design revision.
- Aborting a POST does not imply that the server rolled back its work; cancelled
  mutations are never replayed automatically.

## Options considered

### Option A: Keep promises and callbacks

This avoids dependency and migration cost. It leaves multi-step workflow
dependencies, error channels, and resource ownership distributed among
callers.

### Option B: Replace current state owners with Effect

This would move reactive state and server caching into a second architecture
and require broad changes to established routes and query behaviour.

### Option C: Adopt Effect for application workflows

This allows workflows to use typed dependencies, errors, cancellation, and
scoped resource management while Solid and TanStack retain their existing
responsibilities.

## Decision outcome / proposed direction

Adopt Option C, subject to the feasibility and review gates in RFC 0019. Effect
will orchestrate application workflows and resource lifetimes. Domain and
policy functions remain pure. Ports belong to the application boundary and
browser adapters implement them. TanStack Query owns cache identity,
invalidation, and read retries; Solid owns rendering and local interaction
state. The Promise conversion required by Query is a small explicit boundary.

## Goals and non-goals

- Goals:
  - Reduce implicit asynchronous dependency and lifetime ownership in
    multi-step front-end workflows.
  - Preserve testable boundaries around browser I/O and external protocols.
  - Validate browser compatibility, bundle impact, and proof feasibility before
    broad adoption.
- Non-goals:
  - Replace SolidJS, TanStack Router, or TanStack Query.
  - Wrap every one-step request or move all application state into Effect.
  - Change gateway contracts or persisted data.

## Migration plan

1. Verify the v4 package, browser build, TypeScript 6 and 7, Bun test
   interoperability, bundle impact, cancellation bridge, and temporary
   LemmaScript proof-feasibility harness. This harness does not discharge the
   production proof.
2. Migrate the shared HTTP client and its callers atomically while preserving
   transport behaviour.
3. Migrate restart orchestration, which waits three seconds after each status
   check settles and polls sequentially until recovery or disposal without
   adding an attempt limit or deadline. Replace the temporary proof harness
   with the actual production transition kernel and its proof.
4. Give chat and log stream adapters scoped ownership while preserving native
   EventSource reconnection and recovery behaviour.
5. Update front-end architecture and developer guidance and run all required
   repository and browser gates.

## Known risks and limitations

- `ManagedRuntime` lazily builds and caches the initial layer result, including
  failure; recoverable bootstrap must run as an Effect, not during layer
  construction.
- Native EventSource is not controlled by Effect's TestClock and the gateway
  does not supply event IDs for replay. Tests must distinguish runtime timer
  guarantees from browser delivery behaviour.
- Disposing one Solid observer of a shared Query read detaches that observer;
  it must not cancel the request while another observer still uses it. Actual
  Query cancellation aborts transport; root/session shutdown cancels all Query
  work. A contract test covers two observers and explicit Query cancellation.
- The Query bridge must distinguish typed failure, interruption, and defect
  from Effect `Exit`/`Cause`. A generic `FiberFailure` must not escape into
  Query retry or user-facing failure handling.
- Cancelling an in-flight POST cannot undo a server-side commit and must not
  cause automatic replay. Restart status checks run sequentially, waiting
  3,000 ms after each settles until recovery or disposal. This replaces the
  current interval start cadence with a post-settlement delay to prevent
  overlap.
- Native EventSource reconnect and `setStreamConnectionErrorHandler` remain
  authoritative. Named server `error` MessageEvents and malformed frames are
  not connection outages. Existing stream handler behaviour remains covered.
- LemmaScript is a technology preview and models JavaScript numbers as
  mathematical integers in the verified subset. Any future numeric transition
  property needs a separate JavaScript bounds argument; the proposed restart
  policy retains opaque object identity rather than numeric attempt IDs.
- The existing front-end test command uses Vitest. New unit and workflow
  behaviour tests need a separate Bun discovery path; rendered component tests
  retain Vitest's JSX transform.
- LemmaScript must prove the actual restart transition policy. Tool or subset
  incompatibility requires a design revision rather than replacing proof with
  sampled tests.
- The proof toolchain is pinned and run as a required CI gate. The M1/M2
  temporary feasibility harness is replaced with the production kernel proof
  in M3.

## Outstanding decisions

- Confirm the Bun test directory and aggregate command include new unit and
  workflow behaviour tests exactly once, while Vitest remains the component
  runner.
- Confirm the proposed bundle review threshold in RFC 0019.
- Confirm the restart transition is stable and pure enough to warrant a
  LemmaScript proof after the tool compatibility check.
