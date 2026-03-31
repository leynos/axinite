# Non-transactional multi-step context updates between metadata/to…

## Summary

- Source commit: `3f2796b7453137a1e0de5b450a0574a521a68614`
- Source date: `2026-03-14`
- Severity: `medium-high`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate agent runtime, src.

## What the upstream commit addressed

Upstream commit `3f2796b7453137a1e0de5b450a0574a521a68614` (`fix:
Non-transactional multi-step context updates between metadata/to… (#1161)`)
addresses non-transactional multi-step context updates between metadata/to….

Changed upstream paths:

- src/agent/scheduler.rs
- src/context/manager.rs

Upstream stats:

```text
 src/agent/scheduler.rs | 42 +++++++++++++++++++++++++++++++++---------
 src/context/manager.rs | 88 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 2 files changed, 121 insertions(+), 9 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium-high` severity. That keeps
the underlying failure mode close enough to Axinite's current runtime to justify
a follow-up review. The recorded blast radius upstream was moderate agent
runtime, src.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate agent runtime, src) means the fix could touch
  more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
