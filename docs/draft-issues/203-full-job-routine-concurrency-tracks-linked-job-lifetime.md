# Full_job routine concurrency tracks linked job lifetime

## Summary

- Source commit: `6831bb4d7b2bf7bf841c07de098ec023ddb26a5c`
- Source date: `2026-03-18`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate agent runtime, tests.

## What the upstream commit addressed

Upstream commit `6831bb4d7b2bf7bf841c07de098ec023ddb26a5c` (`fix: full_job
routine concurrency tracks linked job lifetime (#1372)`) addresses full_job
routine concurrency tracks linked job lifetime.

Changed upstream paths:

- src/agent/routine_engine.rs
- tests/e2e_routine_heartbeat.rs

Upstream stats:

```text
 src/agent/routine_engine.rs    | 106 ++++++++++++++++++++++++++++++++++++++++++++++++----
 tests/e2e_routine_heartbeat.rs | 206 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 2 files changed, 305 insertions(+), 7 deletions(-)
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
