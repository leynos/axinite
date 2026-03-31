# Fix duplicate LLM responses for matched event routines

## Summary

- Source commit: `20202700dbef968297e24976ed45edaae10ce135`
- Source date: `2026-03-18`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate agent runtime, tests.

## What the upstream commit addressed

Upstream commit `20202700dbef968297e24976ed45edaae10ce135` (`Fix duplicate LLM
responses for matched event routines (#1275)`) addresses fix duplicate llm
responses for matched event routines.

Changed upstream paths:

- src/agent/agent_loop.rs
- src/agent/routine_engine.rs
- tests/e2e_routine_heartbeat.rs

Upstream stats:

```text
 src/agent/agent_loop.rs        | 64 ++++++++++++++++++++++++++++++----------------------------------
 src/agent/routine_engine.rs    | 16 ++++++++--------
 tests/e2e_routine_heartbeat.rs | 49 +++++++++++++++++++++++++++++++++++++++----------
 3 files changed, 77 insertions(+), 52 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate agent runtime,
tests.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate agent runtime, tests) means the fix could
  touch more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
