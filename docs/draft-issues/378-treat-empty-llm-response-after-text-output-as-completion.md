# Treat empty LLM response after text output as completion

## Summary

- Source commit: `fd41bdf4bed3c9b43cf12788b717ac4c0fa8b5b5`
- Source date: `2026-03-29`
- Severity: `medium`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: moderate LLM stack, sandbox/worker.

## What the upstream commit addressed

Upstream commit `fd41bdf4bed3c9b43cf12788b717ac4c0fa8b5b5` (`fix(worker): treat
empty LLM response after text output as completion (#1677)`) addresses treat
empty llm response after text output as completion.

Changed upstream paths:

- src/llm/circuit_breaker.rs
- src/llm/error.rs
- src/llm/github_copilot.rs
- src/llm/nearai_chat.rs
- src/llm/retry.rs
- src/worker/job.rs

Upstream stats:

```text
 src/llm/circuit_breaker.rs |   1 +
 src/llm/error.rs           |   3 +++
 src/llm/github_copilot.rs  |   6 ++---
 src/llm/nearai_chat.rs     |   6 ++---
 src/llm/retry.rs           |   1 +
 src/worker/job.rs          | 152 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++---
 6 files changed, 157 insertions(+), 12 deletions(-)
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
