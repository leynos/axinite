# ADR-012 — Monotonic clock seam for build duration measurement

**Status:** Accepted
**Date:** 2026-05-22
**Deciders:** `@leynos`

## Context

`BuildSoftwareTool` records elapsed build duration in `ToolOutput`. Issue `#194`
proposed adopting `mockable::Clock` from the project dependency injection (DI)
guide. However, `mockable::Clock` wraps `SystemTime`, which is not monotonic:
host-clock adjustments, such as Network Time Protocol (NTP) or virtual machine
(VM) time sync, can make elapsed durations negative or zero. PR `#195`
therefore diverges from the DI guide by using a bespoke abstraction.

## Decision

Introduce a narrow `MonotonicClock` trait backed by `std::time::Instant` in
`src/tools/builder/core/clock.rs`. Provide:

- `StdMonotonicClock` — production adapter calling `Instant::now()`.
- `FixedMonotonicClock` — test-only adapter holding a
  `Mutex<VecDeque<Instant>>` seeded via `with_elapsed(duration)`.

`BuildSoftwareTool` stores `Arc<dyn MonotonicClock>` and exposes a
`#[cfg(test)] pub(crate) fn new_with_clock(...)` constructor for injection.

## Rationale

`mockable::Clock` is the right tool for wall-clock comparisons, such as cache
expiry and rate limiting. It is the wrong tool for elapsed-time measurement
because `SystemTime::elapsed()` is fallible and non-monotonic. Using
`Instant`-backed injection keeps the domain free from infrastructure coupling
while remaining fully deterministic in tests.

## Consequences

- Tests assert exact duration values, not merely `> Duration::ZERO`.
- The fixed clock panics on queue exhaustion, making test misconfiguration
  visible at the point of failure.
- `mockable::Clock` remains the standard for wall-clock DI per the guide;
  `MonotonicClock` is the standard for elapsed-time DI.
- The developer's guide must document both patterns to avoid future confusion.
  See PR `#195` and issue `#207`.

## References

- Issue `#194` — Replace custom Clock trait in BuildSoftwareTool with
  mockable::Clock
- Issue `#207` — Re-evaluate BuildSoftwareTool timing: reconcile
  Instant::now() with DI guide
- `docs/reliable-testing-in-rust-via-dependency-injection.md`
- `docs/developers-guide.md`

# ADR-012 — Monotonic clock seam for build duration measurement

**Status:** Accepted
**Date:** 2026-05-22
**Deciders:** `@leynos`
