# Fail closed when webhook secret is missing at runtime

## Summary

- Source commit: `1ba6a83ca4c939514700dc099c882524a9108de9`
- Source date: `2026-03-12`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow webhooks.

## What the upstream commit addressed

Upstream commit `1ba6a83ca4c939514700dc099c882524a9108de9` (`fix(http): fail
closed when webhook secret is missing at runtime (#1075)`) addresses fail closed
when webhook secret is missing at runtime.

Changed upstream paths:

- src/channels/http.rs

Upstream stats:

```text
 src/channels/http.rs | 192 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++---------------------------------------------
 1 file changed, 114 insertions(+), 78 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow webhooks.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow webhooks) means the fix could touch more
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
