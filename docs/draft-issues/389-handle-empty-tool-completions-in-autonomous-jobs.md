# Handle empty tool completions in autonomous jobs

## Summary

- Source commit: `4f277c91be53366caaad00cfbf4245693dcd2ac7`
- Source date: `2026-03-29`
- Severity: `medium`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: very broad agent runtime, LLM stack.

## What the upstream commit addressed

Upstream commit `4f277c91be53366caaad00cfbf4245693dcd2ac7` (`Handle empty tool
completions in autonomous jobs (#1720)`) addresses handle empty tool completions
in autonomous jobs.

Changed upstream paths:

- src/agent/agentic_loop.rs
- src/agent/dispatcher.rs
- src/llm/mod.rs
- src/llm/reasoning.rs
- src/worker/autonomous_recovery.rs
- src/worker/container.rs
- src/worker/job.rs
- src/worker/mod.rs
- tests/e2e_builtin_tool_coverage.rs

Upstream stats:

```text
 src/agent/agentic_loop.rs          |  93 ++++++++++++++++++++++++++++++++++--
 src/agent/dispatcher.rs            |   6 +++
 src/llm/mod.rs                     |   6 +--
 src/llm/reasoning.rs               | 139 +++++++++++++++++++++++++++++++++++++++++++++++++++--
 src/worker/autonomous_recovery.rs  | 150 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/worker/container.rs            |  89 ++++++++++++++++++++++++++++++++--
 src/worker/job.rs                  |  91 +++++++++++++++++++++++++++++++++--
 src/worker/mod.rs                  |   1 +
 tests/e2e_builtin_tool_coverage.rs | 254 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-
 9 files changed, 809 insertions(+), 20 deletions(-)
 create mode 100644 src/worker/autonomous_recovery.rs
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad agent
runtime, LLM stack.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad agent runtime, LLM stack) means the fix
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
