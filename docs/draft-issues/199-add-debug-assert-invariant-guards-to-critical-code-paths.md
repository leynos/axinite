# Add debug_assert invariant guards to critical code paths

## Summary

- Source commit: `07e6e30ee3e6dd1ecbdbf46a65e08e50d16e82fe`
- Source date: `2026-03-19`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `comprehensive`
- Scope and blast radius: moderate src, LLM stack.

## What the upstream commit addressed

Upstream commit `07e6e30ee3e6dd1ecbdbf46a65e08e50d16e82fe` (`fix: add
debug_assert invariant guards to critical code paths (#1312)`) addresses add
debug_assert invariant guards to critical code paths.

Changed upstream paths:

- src/context/state.rs
- src/llm/circuit_breaker.rs
- src/tools/execute.rs

Upstream stats:

```text
 src/context/state.rs       |  7 +++++++
 src/llm/circuit_breaker.rs |  6 ++++++
 src/tools/execute.rs       | 23 +++++++++++++++++++++++
 3 files changed, 36 insertions(+)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate src, LLM
stack.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `comprehensive` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate src, LLM stack) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
