# Restore owner-scoped gateway startup

## Summary

- Source commit: `82822d7b2556a1cf29c6525d211cadd9b0a5917f`
- Source date: `2026-03-24`
- Severity: `medium`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: very broad src, web gateway.

## What the upstream commit addressed

Upstream commit `82822d7b2556a1cf29c6525d211cadd9b0a5917f` (`fix: restore
owner-scoped gateway startup (#1625)`) addresses restore owner-scoped gateway
startup.

Changed upstream paths:

- src/app.rs
- src/channels/web/mod.rs
- src/channels/web/server.rs
- src/channels/web/test_helpers.rs
- src/channels/web/tests/multi_tenant.rs
- src/channels/web/ws.rs
- src/config/mod.rs
- src/main.rs
- tests/e2e/conftest.py
- tests/multi_tenant_integration.rs
- tests/openai_compat_integration.rs
- tests/support/gateway_workflow_harness.rs
- tests/ws_gateway_integration.rs

Upstream stats:

```text
 src/app.rs                                |   8 +-----
 src/channels/web/mod.rs                   |  25 ++++++++++++++++---
 src/channels/web/server.rs                |  32 +++++++++++++-----------
 src/channels/web/test_helpers.rs          |   3 ++-
 src/channels/web/tests/multi_tenant.rs    |  39 ++++++++++++++++++++++++++++-
 src/channels/web/ws.rs                    |   3 ++-
 src/config/mod.rs                         |  12 ++++-----
 src/main.rs                               |   1 +
 tests/e2e/conftest.py                     |  91 +++++++++++++++++++++++++++++++++++++++++++++---------------------
 tests/multi_tenant_integration.rs         | 123 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++---
 tests/openai_compat_integration.rs        |   6 +++--
 tests/support/gateway_workflow_harness.rs |   3 ++-
 tests/ws_gateway_integration.rs           |   3 ++-
 13 files changed, 278 insertions(+), 71 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad src, web
gateway.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad src, web gateway) means the fix could touch
  more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
