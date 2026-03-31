# Fix schema-guided tool parameter coercion

## Summary

- Source commit: `c79754df2888ac7e2704d6cf4686b111eceee959`
- Source date: `2026-03-14`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: very broad agent runtime, src.

## What the upstream commit addressed

Upstream commit `c79754df2888ac7e2704d6cf4686b111eceee959` (`Fix schema-guided
tool parameter coercion (#1143)`) addresses fix schema-guided tool parameter
coercion.

Changed upstream paths:

- src/agent/routine_engine.rs
- src/agent/scheduler.rs
- src/tools/builder/core.rs
- src/tools/coercion.rs
- src/tools/execute.rs
- src/tools/mod.rs
- src/tools/wasm/wrapper.rs
- src/worker/job.rs
- tests/e2e_tool_param_coercion.rs

Upstream stats:

```text
 src/agent/routine_engine.rs      |  14 ++--
 src/agent/scheduler.rs           |  87 +++++++++++++++++++++++-
 src/tools/builder/core.rs        |   5 +-
 src/tools/coercion.rs            | 367 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/tools/execute.rs             |  63 +++++++++++++++--
 src/tools/mod.rs                 |   2 +
 src/tools/wasm/wrapper.rs        | 289 ++++++++++++++++++++++++++++++++++--------------------------------------------
 src/worker/job.rs                |  43 +++++++-----
 tests/e2e_tool_param_coercion.rs | 346 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 9 files changed, 1024 insertions(+), 192 deletions(-)
 create mode 100644 src/tools/coercion.rs
 create mode 100644 tests/e2e_tool_param_coercion.rs
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad agent
runtime, src.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
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
