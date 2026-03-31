# Filter XML tool-call recovery by context

## Summary

- Source commit: `b3fbef5287c84d0388fcdab1713c69d5ef62104a`
- Source date: `2026-03-26`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow LLM stack.

## What the upstream commit addressed

Upstream commit `b3fbef5287c84d0388fcdab1713c69d5ef62104a` (`fix(llm): filter
XML tool-call recovery by context (#1641)`) addresses filter xml tool-call
recovery by context.

Changed upstream paths:

- src/llm/reasoning.rs

Upstream stats:

```text
 src/llm/reasoning.rs | 98 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++------
 1 file changed, 92 insertions(+), 6 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow LLM stack.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow LLM stack) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
