# Post-merge review sweep — 8 fixes across security, perf, and correctness

## Summary

- Source commit: `fa51b9f52dde0727f5dd65f134b93095832de959`
- Source date: `2026-03-23`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `comprehensive`
- Scope and blast radius: very broad agent runtime, WASM channels.

## What the upstream commit addressed

Upstream commit `fa51b9f52dde0727f5dd65f134b93095832de959` (`fix: post-merge
review sweep — 8 fixes across security, perf, and correctness (#1550)`)
addresses post-merge review sweep — 8 fixes across security, perf, and
correctness.

Changed upstream paths:

- src/agent/dispatcher.rs
- src/agent/routine_engine.rs
- src/channels/wasm/router.rs
- src/config/llm.rs
- src/llm/github_copilot.rs
- src/tools/builtin/routine.rs
- src/workspace/embedding_cache.rs

Upstream stats:

```text
 src/agent/dispatcher.rs          |  26 ++++++++++++++++---
 src/agent/routine_engine.rs      |  19 ++++++++++++--
 src/channels/wasm/router.rs      |  11 ++++++--
 src/config/llm.rs                |  16 +++++++++---
 src/llm/github_copilot.rs        |  71 ++++++++++++++--------------------------------------
 src/tools/builtin/routine.rs     |   6 +++--
 src/workspace/embedding_cache.rs | 135 +++++++++++++++++++++++----------------------------------------------------------------------------
 7 files changed, 114 insertions(+), 170 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad agent
runtime, WASM channels.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `comprehensive` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad agent runtime, WASM channels) means the fix
  could touch more behaviour than the narrow symptom suggests.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
