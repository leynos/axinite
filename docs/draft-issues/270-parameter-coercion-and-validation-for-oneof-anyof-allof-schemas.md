# Parameter coercion and validation for oneOf/anyOf/allOf schemas

## Summary

- Source commit: `8ad7d78a707bc12bf5fc3c3a8a07647962da6927`
- Source date: `2026-03-21`
- Severity: `medium-high`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: very broad tool runtime, src.

## What the upstream commit addressed

Upstream commit `8ad7d78a707bc12bf5fc3c3a8a07647962da6927` (`fix: parameter
coercion and validation for oneOf/anyOf/allOf schemas (#1397)`) addresses
parameter coercion and validation for oneof/anyof/allof schemas.

Changed upstream paths:

- src/tools/builtin/tool_info.rs
- src/tools/coercion.rs
- src/tools/schema_validator.rs
- src/tools/tool.rs
- src/tools/wasm/wrapper.rs
- tests/e2e_tool_param_coercion.rs
- tests/e2e_wasm_github_coercion.rs
- tests/support/test_rig.rs

Upstream stats:

```text
 src/tools/builtin/tool_info.rs    |  22 ++-
 src/tools/coercion.rs             | 713 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--
 src/tools/schema_validator.rs     |  88 +++++++++++-
 src/tools/tool.rs                 |  92 ++++++++++++-
 src/tools/wasm/wrapper.rs         | 203 +++++++++++++++++++++++++---
 tests/e2e_tool_param_coercion.rs  | 408 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 tests/e2e_wasm_github_coercion.rs | 277 ++++++++++++++++++++++++++++++++++++++
 tests/support/test_rig.rs         | 127 +++++++++++++++---
 8 files changed, 1869 insertions(+), 61 deletions(-)
 create mode 100644 tests/e2e_wasm_github_coercion.rs
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium-high` severity. That keeps
the underlying failure mode close enough to Axinite's current runtime to justify
a follow-up review. The recorded blast radius upstream was very broad tool
runtime, src.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad tool runtime, src) means the fix could touch
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
