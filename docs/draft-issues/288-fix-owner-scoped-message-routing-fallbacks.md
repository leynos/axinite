# Fix owner-scoped message routing fallbacks

## Summary

- Source commit: `4d7501a9684469998f2b518f6bd3da8bc95b266a`
- Source date: `2026-03-22`
- Severity: `medium-high`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: broad src, tool runtime.

## What the upstream commit addressed

Upstream commit `4d7501a9684469998f2b518f6bd3da8bc95b266a` (`Fix owner-scoped
message routing fallbacks (#1574)`) addresses fix owner-scoped message routing
fallbacks.

Changed upstream paths:

- src/testing/mod.rs
- src/tools/builtin/message.rs
- src/worker/job.rs

Upstream stats:

```text
 src/testing/mod.rs           |  71 +++++++++++++++++++++++++++++++++++++++++++-
 src/tools/builtin/message.rs | 167 +++++++++++++++++++++++++++++++++++++++++++++++--------------------------------------------------------
 src/worker/job.rs            |  65 ++++++++++++++++++++++++++++++++++++++++
 3 files changed, 211 insertions(+), 92 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium-high` severity. That keeps
the underlying failure mode close enough to Axinite's current runtime to justify
a follow-up review. The recorded blast radius upstream was broad src, tool
runtime.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (broad src, tool runtime) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
