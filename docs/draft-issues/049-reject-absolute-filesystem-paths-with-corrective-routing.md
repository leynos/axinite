# Reject absolute filesystem paths with corrective routing

## Summary

- Source commit: `d420abfa6ac1a80f7ebd426913d503447b66786e`
- Source date: `2026-03-12`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: moderate CI/release, tool runtime.

## What the upstream commit addressed

Upstream commit `d420abfa6ac1a80f7ebd426913d503447b66786e` (`fix(memory): reject
absolute filesystem paths with corrective routing (#934)`) addresses reject
absolute filesystem paths with corrective routing.

Changed upstream paths:

- .github/workflows/staging-ci.yml
- src/tools/builtin/memory.rs

Upstream stats:

```text
 .github/workflows/staging-ci.yml | 23 +++++++++++++----------
 src/tools/builtin/memory.rs      | 65 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--
 2 files changed, 76 insertions(+), 12 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate CI/release,
tool runtime.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate CI/release, tool runtime) means the fix could
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
