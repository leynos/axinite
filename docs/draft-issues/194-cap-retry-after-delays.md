# Cap retry-after delays

## Summary

- Source commit: `bedc71ebdcdc93a605f3bce8e724c78893d090ff`
- Source date: `2026-03-18`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate LLM stack, workspace/memory.

## What the upstream commit addressed

Upstream commit `bedc71ebdcdc93a605f3bce8e724c78893d090ff` (`fix(llm): cap
retry-after delays (#1351)`) addresses cap retry-after delays.

Changed upstream paths:

- src/llm/anthropic_oauth.rs
- src/llm/nearai_chat.rs
- src/llm/retry.rs
- src/workspace/embeddings.rs

Upstream stats:

```text
 src/llm/anthropic_oauth.rs  | 12 ++++++++++--
 src/llm/nearai_chat.rs      | 28 ++++++++++++++++++----------
 src/llm/retry.rs            | 23 +++++++++++++++++++++++
 src/workspace/embeddings.rs |  5 +++++
 4 files changed, 56 insertions(+), 12 deletions(-)
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
