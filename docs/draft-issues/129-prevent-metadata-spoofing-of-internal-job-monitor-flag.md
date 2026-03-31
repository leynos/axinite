# Prevent metadata spoofing of internal job monitor flag

## Summary

- Source commit: `bde0b77a86f6118a9a15afa576f0d995f77cda8b`
- Source date: `2026-03-15`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: very broad agent runtime, src.

## What the upstream commit addressed

Upstream commit `bde0b77a86f6118a9a15afa576f0d995f77cda8b` (`fix(security):
prevent metadata spoofing of internal job monitor flag (#1195)`) addresses
prevent metadata spoofing of internal job monitor flag.

Changed upstream paths:

- src/agent/agent_loop.rs
- src/agent/dispatcher.rs
- src/agent/job_monitor.rs
- src/channels/channel.rs
- src/tools/builtin/job.rs
- tests/e2e_routine_heartbeat.rs

Upstream stats:

```text
 src/agent/agent_loop.rs        | 14 ++++++++++++++
 src/agent/dispatcher.rs        |  5 +++++
 src/agent/job_monitor.rs       | 79 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--------------
 src/channels/channel.rs        | 12 ++++++++++++
 src/tools/builtin/job.rs       | 44 +++++++++++++++++++++++++++++++++++++++++++-
 tests/e2e_routine_heartbeat.rs | 40 ++++------------------------------------
 6 files changed, 143 insertions(+), 51 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad agent
runtime, src.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad agent runtime, src) means the fix could
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
