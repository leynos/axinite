# Surface errors when sandbox unavailable for full_job routines

## Summary

- Source commit: `455f543ba50d610eb9e181fd41bf4c77615d3af6`
- Source date: `2026-03-19`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: very broad agent runtime, database.

## What the upstream commit addressed

Upstream commit `455f543ba50d610eb9e181fd41bf4c77615d3af6` (`fix(routines):
surface errors when sandbox unavailable for full_job routines (#769)`) addresses
surface errors when sandbox unavailable for full_job routines.

Changed upstream paths:

- src/agent/agent_loop.rs
- src/agent/dispatcher.rs
- src/agent/mod.rs
- src/agent/routine_engine.rs
- src/db/mod.rs
- src/main.rs
- src/testing/mod.rs
- tests/e2e_routine_heartbeat.rs
- tests/e2e_telegram_message_routing.rs
- tests/support/gateway_workflow_harness.rs
- tests/support/test_rig.rs

Upstream stats:

```text
 src/agent/agent_loop.rs                   |   3 ++
 src/agent/dispatcher.rs                   |   3 ++
 src/agent/mod.rs                          |   2 +-
 src/agent/routine_engine.rs               | 189 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/db/mod.rs                             |   1 +
 src/main.rs                               |  44 +++++++++++++++++++++
 src/testing/mod.rs                        |   1 +
 tests/e2e_routine_heartbeat.rs            |  11 +++++-
 tests/e2e_telegram_message_routing.rs     |   1 +
 tests/support/gateway_workflow_harness.rs |   1 +
 tests/support/test_rig.rs                 |   2 +
 11 files changed, 256 insertions(+), 2 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad agent
runtime, database.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad agent runtime, database) means the fix could
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
