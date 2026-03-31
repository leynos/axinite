# Make unsafe env::set_var calls safe with explicit invariants

## Summary

- Source commit: `a9821ac20f0509bcfdfef8c5f2ed95b40ca4ec05`
- Source date: `2026-03-11`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: broad src, config.

## What the upstream commit addressed

Upstream commit `a9821ac20f0509bcfdfef8c5f2ed95b40ca4ec05` (`fix(security): make
unsafe env::set_var calls safe with explicit invariants (#968)`) addresses make
unsafe env::set_var calls safe with explicit invariants.

Changed upstream paths:

- src/bootstrap.rs
- src/cli/doctor.rs
- src/config/helpers.rs
- src/config/mod.rs
- src/llm/session.rs
- src/setup/wizard.rs

Upstream stats:

```text
 src/bootstrap.rs      |  15 ++++++++++---
 src/cli/doctor.rs     |   2 +-
 src/config/helpers.rs | 135 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-
 src/config/mod.rs     |   4 ++++
 src/llm/session.rs    |  21 ++++++++---------
 src/setup/wizard.rs   |  11 ++++-----
 6 files changed, 166 insertions(+), 22 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was broad src, config.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (broad src, config) means the fix could touch more
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
