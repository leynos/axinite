# Fix lifecycle bugs + comprehensive E2E tests

## Summary

- Source commit: `9fbdd4298855e60d1e661a6f59e07f231c14693b`
- Source date: `2026-03-12`
- Severity: `medium-high`
- Main relevance: `maybe`
- Effectiveness: `comprehensive`
- Scope and blast radius: very broad CI/release, web gateway.

## What the upstream commit addressed

Upstream commit `9fbdd4298855e60d1e661a6f59e07f231c14693b` (`fix(extensions):
fix lifecycle bugs + comprehensive E2E tests (#1070)`) addresses fix lifecycle
bugs + comprehensive e2e tests.

Changed upstream paths:

- .github/workflows/e2e.yml
- src/channels/web/server.rs
- src/channels/web/static/app.js
- src/channels/web/static/style.css
- src/extensions/manager.rs
- tests/e2e/conftest.py
- tests/e2e/helpers.py
- tests/e2e/mock_llm.py
- tests/e2e/scenarios/test_extension_oauth.py
- tests/e2e/scenarios/test_extensions.py
- tests/e2e/scenarios/test_pairing.py
- tests/e2e/scenarios/test_tool_execution.py
- tests/e2e/scenarios/test_wasm_lifecycle.py

Upstream stats:

```text
 .github/workflows/e2e.yml                   |   2 +-
 src/channels/web/server.rs                  | 192 ++++++++++++++++++++-------------
 src/channels/web/static/app.js              | 140 +++++++++++++++++-------
 src/channels/web/static/style.css           |  26 ++++-
 src/extensions/manager.rs                   | 195 +++++++++++++++++++++++++++++++--
 tests/e2e/conftest.py                       |  78 +++++++++++++-
 tests/e2e/helpers.py                        |  29 +++++
 tests/e2e/mock_llm.py                       | 243 +++++++++++++++++++++++++++++++----------
 tests/e2e/scenarios/test_extension_oauth.py | 264 +++++++++++++++++++++++++++++++++++++++++++++
 tests/e2e/scenarios/test_extensions.py      | 185 +++++++++++++++++++++++++++++---
 tests/e2e/scenarios/test_pairing.py         |  79 ++++++++++++++
 tests/e2e/scenarios/test_tool_execution.py  |  94 ++++++++++++++++
 tests/e2e/scenarios/test_wasm_lifecycle.py  | 517 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 13 files changed, 1851 insertions(+), 193 deletions(-)
 create mode 100644 tests/e2e/scenarios/test_extension_oauth.py
 create mode 100644 tests/e2e/scenarios/test_pairing.py
 create mode 100644 tests/e2e/scenarios/test_tool_execution.py
 create mode 100644 tests/e2e/scenarios/test_wasm_lifecycle.py
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium-high` severity. That keeps
the underlying failure mode close enough to Axinite's current runtime to justify
a follow-up review. The recorded blast radius upstream was very broad
CI/release, web gateway.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `comprehensive` effectiveness
  in the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad CI/release, web gateway) means the fix could
  touch more behaviour than the narrow symptom suggests.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
