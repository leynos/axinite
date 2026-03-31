# Fix hosted OAuth refresh via proxy

## Summary

- Source commit: `dcb2d89e3a5ed19b30878557adfe505b66484483`
- Source date: `2026-03-24`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: very broad src, extensions/registry.

## What the upstream commit addressed

Upstream commit `dcb2d89e3a5ed19b30878557adfe505b66484483` (`Fix hosted OAuth
refresh via proxy (#1602)`) addresses fix hosted oauth refresh via proxy.

Changed upstream paths:

- src/cli/oauth_defaults.rs
- src/extensions/manager.rs
- src/tools/wasm/loader.rs
- src/tools/wasm/wrapper.rs
- tests/e2e/CLAUDE.md
- tests/e2e/conftest.py
- tests/e2e/mock_llm.py
- tests/e2e/scenarios/test_oauth_refresh.py

Upstream stats:

```text
 src/cli/oauth_defaults.rs                 | 424 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-----
 src/extensions/manager.rs                 |  35 +++----
 src/tools/wasm/loader.rs                  | 141 +++++++++++++++++++++++++++
 src/tools/wasm/wrapper.rs                 | 481 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--------
 tests/e2e/CLAUDE.md                       |   9 ++
 tests/e2e/conftest.py                     | 128 +++++++++++++++++++++---
 tests/e2e/mock_llm.py                     |  61 ++++++++++++
 tests/e2e/scenarios/test_oauth_refresh.py | 227 +++++++++++++++++++++++++++++++++++++++++++
 8 files changed, 1407 insertions(+), 99 deletions(-)
 create mode 100644 tests/e2e/scenarios/test_oauth_refresh.py
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad src,
extensions/registry.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad src, extensions/registry) means the fix
  could touch more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
