# Escape tool output XML content and remove misleading sanitized attr

## Summary

- Source commit: `07c338f55da7f1496a338810fddcdb1f8eccfe2c`
- Source date: `2026-03-21`
- Severity: `medium`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: very broad benches, crates.

## What the upstream commit addressed

Upstream commit `07c338f55da7f1496a338810fddcdb1f8eccfe2c` (`fix(safety): escape
tool output XML content and remove misleading sanitized attr (#1067)`) addresses
escape tool output xml content and remove misleading sanitized attr.

Changed upstream paths:

- benches/safety_pipeline.rs
- crates/ironclaw_safety/src/lib.rs
- src/agent/dispatcher.rs
- src/agent/routine_engine.rs
- src/channels/web/util.rs
- src/llm/codex_test_helpers.rs
- src/tools/execute.rs
- tests/support/trace_llm.rs

Upstream stats:

```text
 benches/safety_pipeline.rs        |   2 +-
 crates/ironclaw_safety/src/lib.rs | 231 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++----
 src/agent/dispatcher.rs           |   8 ++--
 src/agent/routine_engine.rs       |  12 +-----
 src/channels/web/util.rs          |   4 +-
 src/llm/codex_test_helpers.rs     |   2 -
 src/tools/execute.rs              |   2 +-
 tests/support/trace_llm.rs        |  15 ++-----
 8 files changed, 235 insertions(+), 41 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad benches,
crates.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad benches, crates) means the fix could touch
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
