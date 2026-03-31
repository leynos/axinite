# Discard truncated tool calls when finish_reason == Length (#1631)

## Summary

- Source commit: `ed4d92932ac5d2d9123a8448aac4627bb8bb2d7c`
- Source date: `2026-03-26`
- Severity: `medium`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: broad agent runtime, LLM stack.

## What the upstream commit addressed

Upstream commit `ed4d92932ac5d2d9123a8448aac4627bb8bb2d7c` (`fix(agent): discard
truncated tool calls when finish_reason == Length (#1631) (#1632)`) addresses
discard truncated tool calls when finish_reason == length (#1631).

Changed upstream paths:

- src/agent/agentic_loop.rs
- src/agent/dispatcher.rs
- src/llm/mod.rs
- src/llm/reasoning.rs
- src/worker/job.rs

Upstream stats:

```text
 src/agent/agentic_loop.rs | 126 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-
 src/agent/dispatcher.rs   |   2 ++
 src/llm/mod.rs            |   3 ++-
 src/llm/reasoning.rs      | 110 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--
 src/worker/job.rs         |   2 ++
 5 files changed, 239 insertions(+), 4 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was broad agent runtime,
LLM stack.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (broad agent runtime, LLM stack) means the fix could
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
