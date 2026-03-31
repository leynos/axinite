# Ensure LLM calls always end with user message (closes #763)

## Summary

- Source commit: `6daa2f155f2683cf93669cac5844b6d85400b7a5`
- Source date: `2026-03-25`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: very broad agent runtime, LLM stack.

## What the upstream commit addressed

Upstream commit `6daa2f155f2683cf93669cac5844b6d85400b7a5` (`fix: ensure LLM
calls always end with user message (closes #763) (#1259)`) addresses ensure llm
calls always end with user message (closes #763).

Changed upstream paths:

- src/agent/routine_engine.rs
- src/llm/nearai_chat.rs
- src/util.rs
- src/worker/container.rs
- src/worker/job.rs

Upstream stats:

```text
 src/agent/routine_engine.rs |  5 ++++-
 src/llm/nearai_chat.rs      | 70 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--
 src/util.rs                 | 52 +++++++++++++++++++++++++++++++++++++++++++++++++++-
 src/worker/container.rs     |  6 +++++-
 src/worker/job.rs           |  5 +++++
 5 files changed, 133 insertions(+), 5 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad agent
runtime, LLM stack.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

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
