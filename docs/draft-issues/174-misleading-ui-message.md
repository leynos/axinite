# Misleading UI message

## Summary

- Source commit: `c6128f4e41b5bd43d69a4432a6050df4d675590a`
- Source date: `2026-03-16`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate agent runtime, tests.

## What the upstream commit addressed

Upstream commit `c6128f4e41b5bd43d69a4432a6050df4d675590a` (`fix: misleading UI
message (#1265)`) addresses misleading ui message.

Changed upstream paths:

- src/agent/submission.rs
- src/agent/thread_ops.rs
- tests/e2e/scenarios/test_tool_approval.py

Upstream stats:

```text
 src/agent/submission.rs                   |   8 +++++++
 src/agent/thread_ops.rs                   | 118 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++----
 tests/e2e/scenarios/test_tool_approval.py |  59 +++++++++++++++++++++++++++++++++++++++++++++
 3 files changed, 180 insertions(+), 5 deletions(-)
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
