# Persist /model selection to .env, TOML, and DB

## Summary

- Source commit: `5847479fd851726e7e1e848b45bcf48a195f9aa9`
- Source date: `2026-03-23`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: broad agent runtime, src.

## What the upstream commit addressed

Upstream commit `5847479fd851726e7e1e848b45bcf48a195f9aa9` (`fix(agent): persist
/model selection to .env, TOML, and DB (#1581)`) addresses persist /model
selection to .env, toml, and db.

Changed upstream paths:

- src/agent/agent_loop.rs
- src/agent/commands.rs
- src/agent/dispatcher.rs
- src/main.rs
- src/settings.rs
- src/testing/mod.rs
- tests/e2e_telegram_message_routing.rs
- tests/support/gateway_workflow_harness.rs
- tests/support/test_rig.rs

Upstream stats:

```text
 src/agent/agent_loop.rs                   |   3 +++
 src/agent/commands.rs                     |  52 ++++++++++++++++++++++++++++++++++++++++---
 src/agent/dispatcher.rs                   |   3 +++
 src/main.rs                               |   1 +
 src/settings.rs                           | 108 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/testing/mod.rs                        |   1 +
 tests/e2e_telegram_message_routing.rs     |   1 +
 tests/support/gateway_workflow_harness.rs |   1 +
 tests/support/test_rig.rs                 |   1 +
 9 files changed, 168 insertions(+), 3 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was broad agent runtime,
src.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (broad agent runtime, src) means the fix could touch
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
