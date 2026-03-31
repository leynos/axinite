# Remove redundant LLM config and API keys from bootstrap .env

## Summary

- Source commit: `9603fefd01645e4b0645512661581dd11402ef43`
- Source date: `2026-03-20`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate LLM stack, src.

## What the upstream commit addressed

Upstream commit `9603fefd01645e4b0645512661581dd11402ef43` (`fix(setup): remove
redundant LLM config and API keys from bootstrap .env (#1448)`) addresses remove
redundant llm config and api keys from bootstrap .env.

Changed upstream paths:

- src/llm/config.rs
- src/llm/models.rs
- src/setup/README.md
- src/setup/wizard.rs

Upstream stats:

```text
 src/llm/config.rs   |   4 ++--
 src/llm/models.rs   |   4 ++--
 src/setup/README.md |  42 ++++++++++++++++++++++--------------------
 src/setup/wizard.rs | 110 ++++++++++++++++++++++++++++++++++++++++++--------------------------------------------------------------------
 4 files changed, 68 insertions(+), 92 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate LLM stack,
src.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate LLM stack, src) means the fix could touch more
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
