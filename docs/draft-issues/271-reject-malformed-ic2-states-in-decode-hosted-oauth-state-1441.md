# Reject malformed ic2.* states in decode_hosted_oauth_state (#1441)

## Summary

- Source commit: `9d538136b5d86a1eb0a11ef469729b7304db24fb`
- Source date: `2026-03-21`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow src.

## What the upstream commit addressed

Upstream commit `9d538136b5d86a1eb0a11ef469729b7304db24fb` (`fix(oauth): reject
malformed ic2.* states in decode_hosted_oauth_state (#1441) (#1454)`) addresses
reject malformed ic2.* states in decode_hosted_oauth_state (#1441).

Changed upstream paths:

- src/cli/oauth_defaults.rs

Upstream stats:

```text
 src/cli/oauth_defaults.rs | 101 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++------------------
 1 file changed, 83 insertions(+), 18 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow src.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow src) means the fix could touch more behaviour
  than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
