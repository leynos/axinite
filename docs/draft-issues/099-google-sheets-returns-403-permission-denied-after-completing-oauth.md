# Google Sheets returns 403 PERMISSION_DENIED after completing OAuth

## Summary

- Source commit: `17706632794fe90674bad01cef9dad89a15fd10a`
- Source date: `2026-03-14`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: very broad CI/release, WASM tools.

## What the upstream commit addressed

Upstream commit `17706632794fe90674bad01cef9dad89a15fd10a` (`fix: Google Sheets
returns 403 PERMISSION_DENIED after completing OAuth (#1164)`) addresses google
sheets returns 403 permission_denied after completing oauth.

Changed upstream paths:

- .github/workflows/e2e.yml
- src/tools/wasm/wrapper.rs
- tests/e2e/__pycache__/conftest.cpython-313-pytest-8.4.0.pyc
- tests/e2e/__pycache__/helpers.cpython-313.pyc
- tests/e2e/conftest.py
- tests/e2e/ironclaw_e2e.egg-info/PKG-INFO
- tests/e2e/ironclaw_e2e.egg-info/SOURCES.txt
- tests/e2e/ironclaw_e2e.egg-info/dependency_links.txt
- tests/e2e/ironclaw_e2e.egg-info/requires.txt
- tests/e2e/ironclaw_e2e.egg-info/top_level.txt
- tests/e2e/scenarios/__pycache__/__init__.cpython-313.pyc
- tests/e2e/scenarios/__pycache__/test_chat.cpython-313-pytest-8.4.0.pyc
- tests/e2e/scenarios/__pycache__/test_connection.cpython-313-pytest-8.4.0.pyc
- tests/e2e/scenarios/__pycache__/test_csp.cpython-313-pytest-8.4.0.pyc
- tests/e2e/scenarios/__pycache__/test_extension_oauth.cpython-313-pytest-8.4.0.pyc
- tests/e2e/scenarios/__pycache__/test_extensions.cpython-313-pytest-8.4.0.pyc
- tests/e2e/scenarios/__pycache__/test_html_injection.cpython-313-pytest-8.4.0.pyc
- tests/e2e/scenarios/__pycache__/test_oauth_credential_fallback.cpython-313-pytest-8.4.0.pyc
- tests/e2e/scenarios/__pycache__/test_pairing.cpython-313-pytest-8.4.0.pyc
- tests/e2e/scenarios/__pycache__/test_routine_oauth_credential_injection.cpython-313-pytest-8.4.0.pyc
- tests/e2e/scenarios/__pycache__/test_skills.cpython-313-pytest-8.4.0.pyc
- tests/e2e/scenarios/__pycache__/test_sse_reconnect.cpython-313-pytest-8.4.0.pyc
- tests/e2e/scenarios/__pycache__/test_tool_approval.cpython-313-pytest-8.4.0.pyc
- tests/e2e/scenarios/__pycache__/test_tool_execution.cpython-313-pytest-8.4.0.pyc
- tests/e2e/scenarios/__pycache__/test_wasm_lifecycle.cpython-313-pytest-8.4.0.pyc
- tests/e2e/scenarios/test_oauth_credential_fallback.py
- tests/e2e/scenarios/test_routine_oauth_credential_injection.py

Upstream stats:

```text
 .github/workflows/e2e.yml                                                                |   2 +-
 src/tools/wasm/wrapper.rs                                                                | 202 +++++++++++++++++++++++++++++++++++++++++--
 tests/e2e/__pycache__/conftest.cpython-313-pytest-8.4.0.pyc                              | Bin 0 -> 14380 bytes
 tests/e2e/__pycache__/helpers.cpython-313.pyc                                            | Bin 0 -> 9139 bytes
 tests/e2e/conftest.py                                                                    |   2 +-
 tests/e2e/ironclaw_e2e.egg-info/PKG-INFO                                                 |  13 +++
 tests/e2e/ironclaw_e2e.egg-info/SOURCES.txt                                              |  22 +++++
 tests/e2e/ironclaw_e2e.egg-info/dependency_links.txt                                     |   1 +
 tests/e2e/ironclaw_e2e.egg-info/requires.txt                                             |  10 +++
 tests/e2e/ironclaw_e2e.egg-info/top_level.txt                                            |   1 +
 tests/e2e/scenarios/__pycache__/__init__.cpython-313.pyc                                 | Bin 0 -> 201 bytes
 tests/e2e/scenarios/__pycache__/test_chat.cpython-313-pytest-8.4.0.pyc                   | Bin 0 -> 9147 bytes
 tests/e2e/scenarios/__pycache__/test_connection.cpython-313-pytest-8.4.0.pyc             | Bin 0 -> 5072 bytes
 tests/e2e/scenarios/__pycache__/test_csp.cpython-313-pytest-8.4.0.pyc                    | Bin 0 -> 7373 bytes
 tests/e2e/scenarios/__pycache__/test_extension_oauth.cpython-313-pytest-8.4.0.pyc        | Bin 0 -> 35326 bytes
 tests/e2e/scenarios/__pycache__/test_extensions.cpython-313-pytest-8.4.0.pyc             | Bin 0 -> 128293 bytes
 tests/e2e/scenarios/__pycache__/test_html_injection.cpython-313-pytest-8.4.0.pyc         | Bin 0 -> 9543 bytes
 .../scenarios/__pycache__/test_oauth_credential_fallback.cpython-313-pytest-8.4.0.pyc    | Bin 0 -> 9259 bytes
 tests/e2e/scenarios/__pycache__/test_pairing.cpython-313-pytest-8.4.0.pyc                | Bin 0 -> 15874 bytes
 .../__pycache__/test_routine_oauth_credential_injection.cpython-313-pytest-8.4.0.pyc     | Bin 0 -> 12487 bytes
 tests/e2e/scenarios/__pycache__/test_skills.cpython-313-pytest-8.4.0.pyc                 | Bin 0 -> 7997 bytes
 tests/e2e/scenarios/__pycache__/test_sse_reconnect.cpython-313-pytest-8.4.0.pyc          | Bin 0 -> 7541 bytes
 tests/e2e/scenarios/__pycache__/test_tool_approval.cpython-313-pytest-8.4.0.pyc          | Bin 0 -> 11995 bytes
 tests/e2e/scenarios/__pycache__/test_tool_execution.cpython-313-pytest-8.4.0.pyc         | Bin 0 -> 7772 bytes
 tests/e2e/scenarios/__pycache__/test_wasm_lifecycle.cpython-313-pytest-8.4.0.pyc         | Bin 0 -> 89759 bytes
 tests/e2e/scenarios/test_oauth_credential_fallback.py                                    | 110 +++++++++++++++++++++++
 tests/e2e/scenarios/test_routine_oauth_credential_injection.py                           | 182 ++++++++++++++++++++++++++++++++++++++
 27 files changed, 538 insertions(+), 7 deletions(-)
 create mode 100644 tests/e2e/__pycache__/conftest.cpython-313-pytest-8.4.0.pyc
 create mode 100644 tests/e2e/__pycache__/helpers.cpython-313.pyc
 create mode 100644 tests/e2e/ironclaw_e2e.egg-info/PKG-INFO
 create mode 100644 tests/e2e/ironclaw_e2e.egg-info/SOURCES.txt
 create mode 100644 tests/e2e/ironclaw_e2e.egg-info/dependency_links.txt
 create mode 100644 tests/e2e/ironclaw_e2e.egg-info/requires.txt
 create mode 100644 tests/e2e/ironclaw_e2e.egg-info/top_level.txt
 create mode 100644 tests/e2e/scenarios/__pycache__/__init__.cpython-313.pyc
 create mode 100644 tests/e2e/scenarios/__pycache__/test_chat.cpython-313-pytest-8.4.0.pyc
 create mode 100644 tests/e2e/scenarios/__pycache__/test_connection.cpython-313-pytest-8.4.0.pyc
 create mode 100644 tests/e2e/scenarios/__pycache__/test_csp.cpython-313-pytest-8.4.0.pyc
 create mode 100644 tests/e2e/scenarios/__pycache__/test_extension_oauth.cpython-313-pytest-8.4.0.pyc
 create mode 100644 tests/e2e/scenarios/__pycache__/test_extensions.cpython-313-pytest-8.4.0.pyc
 create mode 100644 tests/e2e/scenarios/__pycache__/test_html_injection.cpython-313-pytest-8.4.0.pyc
 create mode 100644 tests/e2e/scenarios/__pycache__/test_oauth_credential_fallback.cpython-313-pytest-8.4.0.pyc
 create mode 100644 tests/e2e/scenarios/__pycache__/test_pairing.cpython-313-pytest-8.4.0.pyc
 create mode 100644 tests/e2e/scenarios/__pycache__/test_routine_oauth_credential_injection.cpython-313-pytest-8.4.0.pyc
 create mode 100644 tests/e2e/scenarios/__pycache__/test_skills.cpython-313-pytest-8.4.0.pyc
 create mode 100644 tests/e2e/scenarios/__pycache__/test_sse_reconnect.cpython-313-pytest-8.4.0.pyc
 create mode 100644 tests/e2e/scenarios/__pycache__/test_tool_approval.cpython-313-pytest-8.4.0.pyc
 create mode 100644 tests/e2e/scenarios/__pycache__/test_tool_execution.cpython-313-pytest-8.4.0.pyc
 create mode 100644 tests/e2e/scenarios/__pycache__/test_wasm_lifecycle.cpython-313-pytest-8.4.0.pyc
 create mode 100644 tests/e2e/scenarios/test_oauth_credential_fallback.py
 create mode 100644 tests/e2e/scenarios/test_routine_oauth_credential_injection.py
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad CI/release,
WASM tools.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad CI/release, WASM tools) means the fix could
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
