# Add stop_sequences parity for tool completions

## Summary

- Source commit: `ffe384b66ea326d58056cd6315b50fefa7c6beee`
- Source date: `2026-03-14`
- Severity: `medium`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: broad web gateway, LLM stack.

## What the upstream commit addressed

Upstream commit `ffe384b66ea326d58056cd6315b50fefa7c6beee` (`fix(llm): add
stop_sequences parity for tool completions (#1170)`) addresses add
stop_sequences parity for tool completions.

Changed upstream paths:

- src/channels/web/openai_compat.rs
- src/llm/bedrock.rs
- src/llm/nearai_chat.rs
- src/llm/provider.rs
- src/llm/response_cache.rs
- src/orchestrator/api.rs
- src/worker/api.rs

Upstream stats:

```text
 src/channels/web/openai_compat.rs | 88 ++++++++++++++++++++++++++++++++++++++++++----------------------------------------------
 src/llm/bedrock.rs                |  7 +++++--
 src/llm/nearai_chat.rs            |  6 ++++++
 src/llm/provider.rs               | 27 ++++++++++++++++++++++++---
 src/llm/response_cache.rs         |  1 +
 src/orchestrator/api.rs           |  1 +
 src/worker/api.rs                 |  2 ++
 7 files changed, 81 insertions(+), 51 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was broad web gateway, LLM
stack.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (broad web gateway, LLM stack) means the fix could touch
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
