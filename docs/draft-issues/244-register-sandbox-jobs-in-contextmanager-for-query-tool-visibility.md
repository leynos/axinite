# Register sandbox jobs in ContextManager for query tool visibility

## Summary

- Source commit: `e82f4bd2e56f547079838f88b33ca731d1e921e6`
- Source date: `2026-03-20`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: broad agent runtime, src.

## What the upstream commit addressed

Upstream commit `e82f4bd2e56f547079838f88b33ca731d1e921e6` (`fix: register
sandbox jobs in ContextManager for query tool visibility (#1426)`) addresses
register sandbox jobs in contextmanager for query tool visibility.

Changed upstream paths:

- src/agent/job_monitor.rs
- src/context/manager.rs
- src/tools/builtin/job.rs
- src/tools/registry.rs

Upstream stats:

```text
 src/agent/job_monitor.rs | 224 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/context/manager.rs   | 133 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-----
 src/tools/builtin/job.rs | 138 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++---
 src/tools/registry.rs    |   9 ++++-
 4 files changed, 488 insertions(+), 16 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was broad agent runtime,
src.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (broad agent runtime, src) means the fix could touch
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
