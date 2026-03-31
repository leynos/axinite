# Sanitize tool error results before llm injection

## Summary

- Source commit: `2f4eb08613cefff1af8b7b1a475fda00c84dd855`
- Source date: `2026-03-27`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: broad agent runtime, web gateway.

## What the upstream commit addressed

Upstream commit `2f4eb08613cefff1af8b7b1a475fda00c84dd855` (`fix: sanitize tool
error results before llm injection (#1639)`) addresses sanitize tool error
results before llm injection.

Changed upstream paths:

- src/agent/dispatcher.rs
- src/agent/thread_ops.rs
- src/channels/web/handlers/chat.rs
- src/channels/web/util.rs
- src/tools/builder/core.rs
- src/tools/execute.rs

Upstream stats:

```text
 src/agent/dispatcher.rs           | 88 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++----------------------------
 src/agent/thread_ops.rs           | 32 ++++++++++++++++++++++++++++++--
 src/channels/web/handlers/chat.rs |  6 ++++--
 src/channels/web/util.rs          | 30 +++++++++++++++++++++++++++++-
 src/tools/builder/core.rs         | 60 ++++++++++++++++++++++++++++++++++++++++++++++++++----------
 src/tools/execute.rs              | 47 ++++++++++++++++++++++++++++++++++++++---------
 6 files changed, 211 insertions(+), 52 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was broad agent runtime,
web gateway.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (broad agent runtime, web gateway) means the fix could
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
