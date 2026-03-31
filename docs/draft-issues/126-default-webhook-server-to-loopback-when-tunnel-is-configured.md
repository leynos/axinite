# Default webhook server to loopback when tunnel is configured

## Summary

- Source commit: `6aaa89010a5bf766e90095024638cde1e39eaecf`
- Source date: `2026-03-15`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: moderate src, config.

## What the upstream commit addressed

Upstream commit `6aaa89010a5bf766e90095024638cde1e39eaecf` (`fix(security):
default webhook server to loopback when tunnel is configured (#1194)`) addresses
default webhook server to loopback when tunnel is configured.

Changed upstream paths:

- src/cli/doctor.rs
- src/config/channels.rs
- src/config/mod.rs

Upstream stats:

```text
 src/cli/doctor.rs      |  5 ++++-
 src/config/channels.rs | 91 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++----------
 src/config/mod.rs      |  8 ++++++--
 3 files changed, 91 insertions(+), 13 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate src, config.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate src, config) means the fix could touch more
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
