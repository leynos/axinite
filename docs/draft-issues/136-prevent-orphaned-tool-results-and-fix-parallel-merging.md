# Prevent orphaned tool_results and fix parallel merging

## Summary

- Source commit: `58a3eb136689b1aa573415a05e78620633a6ced0`
- Source date: `2026-03-16`
- Severity: `medium`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: moderate LLM stack, sandbox/worker.

## What the upstream commit addressed

Upstream commit `58a3eb136689b1aa573415a05e78620633a6ced0` (`fix(worker):
prevent orphaned tool_results and fix parallel merging (#1069)`) addresses
prevent orphaned tool_results and fix parallel merging.

Changed upstream paths:

- src/llm/rig_adapter.rs
- src/worker/job.rs

Upstream stats:

```text
 src/llm/rig_adapter.rs |  94 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++------
 src/worker/job.rs      | 131 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-
 2 files changed, 217 insertions(+), 8 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate LLM stack,
sandbox/worker.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate LLM stack, sandbox/worker) means the fix could
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
