# Validate embedding base URLs to prevent SSRF

## Summary

- Source commit: `ef3d76974239f3113e390a3af9d0809c70af6492`
- Source date: `2026-03-19`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: moderate config.

## What the upstream commit addressed

Upstream commit `ef3d76974239f3113e390a3af9d0809c70af6492` (`fix(security):
validate embedding base URLs to prevent SSRF (#1221)`) addresses validate
embedding base urls to prevent ssrf.

Changed upstream paths:

- src/config/embeddings.rs
- src/config/helpers.rs
- src/config/llm.rs
- src/config/transcription.rs

Upstream stats:

```text
 src/config/embeddings.rs    |   8 +++-
 src/config/helpers.rs       | 263 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/config/llm.rs           |  32 +++++++++----
 src/config/transcription.rs |   7 ++-
 4 files changed, 298 insertions(+), 12 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate config.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate config) means the fix could touch more
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
