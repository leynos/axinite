# Prefer execution-local message routing metadata

## Summary

- Source commit: `b952d229f941298af5748d421edca6513382f7f5`
- Source date: `2026-03-19`
- Severity: `medium-high`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate agent runtime, tool runtime.

## What the upstream commit addressed

Upstream commit `b952d229f941298af5748d421edca6513382f7f5` (`fix: prefer
execution-local message routing metadata (#1449)`) addresses prefer
execution-local message routing metadata.

Changed upstream paths:

- src/agent/agent_loop.rs
- src/agent/dispatcher.rs
- src/agent/thread_ops.rs
- src/tools/builtin/message.rs

Upstream stats:

```text
 src/agent/agent_loop.rs      |  60 ++++++++++++++++-
 src/agent/dispatcher.rs      |   7 +-
 src/agent/thread_ops.rs      |   1 +
 src/tools/builtin/message.rs | 366 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--------------------
 4 files changed, 357 insertions(+), 77 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium-high` severity. That keeps
the underlying failure mode close enough to Axinite's current runtime to justify
a follow-up review. The recorded blast radius upstream was moderate agent
runtime, tool runtime.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate agent runtime, tool runtime) means the fix
  could touch more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
