# Rate limiter returns retry after None instead of a duration

## Summary

- Source commit: `5c56032b888b436825e150853c88ca3ea4172dbc`
- Source date: `2026-03-16`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate LLM stack, workspace/memory.

## What the upstream commit addressed

Upstream commit `5c56032b888b436825e150853c88ca3ea4172dbc` (`fix: Rate limiter
returns retry after None instead of a duration (#1269)`) addresses rate limiter
returns retry after none instead of a duration.

Changed upstream paths:

- src/llm/anthropic_oauth.rs
- src/llm/nearai_chat.rs
- src/llm/retry.rs
- src/workspace/embeddings.rs

Upstream stats:

```text
 src/llm/anthropic_oauth.rs  |  78 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-
 src/llm/nearai_chat.rs      | 115 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-
 src/llm/retry.rs            |  27 +++++++++++++++++++++++++
 src/workspace/embeddings.rs |  50 +++++++++++++++++++++++++++++++++++++++++++--
 4 files changed, 266 insertions(+), 4 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate LLM stack,
workspace/memory.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate LLM stack, workspace/memory) means the fix
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
