# Relax approval requirements for low-risk tools

## Summary

- Source commit: `6f004909007035f0ec368e92908f87c45f495e16`
- Source date: `2026-03-12`
- Severity: `medium`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: moderate tool runtime.

## What the upstream commit addressed

Upstream commit `6f004909007035f0ec368e92908f87c45f495e16` (`fix: relax approval
requirements for low-risk tools (#922)`) addresses relax approval requirements
for low-risk tools.

Changed upstream paths:

- src/tools/builtin/file.rs
- src/tools/builtin/http.rs
- src/tools/builtin/image_analyze.rs
- src/tools/builtin/image_edit.rs
- src/tools/builtin/image_gen.rs

Upstream stats:

```text
 src/tools/builtin/file.rs          |  4 ----
 src/tools/builtin/http.rs          | 66 ++++++++++++++++++++++++++++++++++--------------------------------
 src/tools/builtin/image_analyze.rs | 11 ++++-------
 src/tools/builtin/image_edit.rs    |  9 +++------
 src/tools/builtin/image_gen.rs     |  8 ++------
 5 files changed, 43 insertions(+), 55 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate tool runtime.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate tool runtime) means the fix could touch more
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
