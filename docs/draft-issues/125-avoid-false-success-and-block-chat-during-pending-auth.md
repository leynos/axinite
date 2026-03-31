# Avoid false success and block chat during pending auth

## Summary

- Source commit: `e0f393bf04ffc29d9de4108c6725b3380b83536b`
- Source date: `2026-03-15`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: moderate agent runtime, web gateway.

## What the upstream commit addressed

Upstream commit `e0f393bf04ffc29d9de4108c6725b3380b83536b` (`fix(auth): avoid
false success and block chat during pending auth (#1111)`) addresses avoid false
success and block chat during pending auth.

Changed upstream paths:

- src/agent/thread_ops.rs
- src/channels/web/server.rs
- src/channels/web/static/app.js

Upstream stats:

```text
 src/agent/thread_ops.rs        | 25 ++++++++++++++++++++++++-
 src/channels/web/server.rs     | 89 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-----
 src/channels/web/static/app.js | 40 +++++++++++++++++++++++++++++++++++++---
 3 files changed, 145 insertions(+), 9 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate agent runtime,
web gateway.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate agent runtime, web gateway) means the fix
  could touch more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
