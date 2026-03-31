# Recover delete name after failed update fallback

## Summary

- Source commit: `dd0a0e10abebcd7c161c6e86fb89b8bd06e38592`
- Source date: `2026-03-26`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: broad src, tool runtime.

## What the upstream commit addressed

Upstream commit `dd0a0e10abebcd7c161c6e86fb89b8bd06e38592` (`fix(routines):
recover delete name after failed update fallback (#1108)`) addresses recover
delete name after failed update fallback.

Changed upstream paths:

- src/context/state.rs
- src/tools/builtin/routine.rs
- tests/e2e_builtin_tool_coverage.rs
- tests/fixtures/llm_traces/tools/routine_update_fail_delete_fallback.json

Upstream stats:

```text
 src/context/state.rs                                                     |  3 +++
 src/tools/builtin/routine.rs                                             | 38 ++++++++++++++++++++++++++++++---
 tests/e2e_builtin_tool_coverage.rs                                       | 43 ++++++++++++++++++++++++++++++++++---
 tests/fixtures/llm_traces/tools/routine_update_fail_delete_fallback.json | 70 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 4 files changed, 148 insertions(+), 6 deletions(-)
 create mode 100644 tests/fixtures/llm_traces/tools/routine_update_fail_delete_fallback.json
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was broad src, tool
runtime.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (broad src, tool runtime) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
