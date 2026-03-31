# Require explicit SANDBOX_ALLOW_FULL_ACCESS to enable FullAccess policy

## Summary

- Source commit: `8bbb43da52c3503833ceb30fc5c633175f672010`
- Source date: `2026-03-11`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow sandbox policy surface. Making full-access
  execution opt-in matches Axinite's security-first posture.

## What the upstream commit addressed

Upstream commit `8bbb43da52c3503833ceb30fc5c633175f672010` (`fix(security):
require explicit SANDBOX_ALLOW_FULL_ACCESS to enable FullAccess policy (#967)`)
addresses require explicit sandbox_allow_full_access to enable fullaccess
policy.

Changed upstream paths:

- .env.example
- src/config/sandbox.rs
- src/sandbox/config.rs
- src/sandbox/manager.rs

Upstream stats:

```text
 .env.example           | 12 ++++++++++++
 src/config/sandbox.rs  | 81 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-
 src/sandbox/config.rs  | 19 ++++++++++++++++++-
 src/sandbox/manager.rs | 78 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-
 4 files changed, 187 insertions(+), 3 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow sandbox policy
surface. Making full-access execution opt-in matches Axinite's security-first
posture.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow sandbox policy surface. Making full-access
  execution opt-in matches Axinite's security-first posture) means the fix could
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
