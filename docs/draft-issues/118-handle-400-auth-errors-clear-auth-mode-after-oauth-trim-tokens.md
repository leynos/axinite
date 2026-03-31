# Handle 400 auth errors, clear auth mode after OAuth, trim tokens

## Summary

- Source commit: `62d16e69ac89762c7a53429406ee90340de02055`
- Source date: `2026-03-15`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: very broad scripts, agent runtime.

## What the upstream commit addressed

Upstream commit `62d16e69ac89762c7a53429406ee90340de02055` (`fix(mcp): handle
400 auth errors, clear auth mode after OAuth, trim tokens (#1158)`) addresses
handle 400 auth errors, clear auth mode after oauth, trim tokens.

Changed upstream paths:

- scripts/pre-commit-safety.sh
- src/agent/agent_loop.rs
- src/agent/session.rs
- src/channels/web/server.rs
- src/extensions/manager.rs
- src/tools/mcp/auth.rs
- src/tools/mcp/client.rs
- tests/e2e/mock_llm.py
- tests/e2e/scenarios/test_mcp_auth_flow.py

Upstream stats:

```text
 scripts/pre-commit-safety.sh              |   8 +++
 src/agent/agent_loop.rs                   |  41 ++++++++---
 src/agent/session.rs                      |  54 +++++++++++---
 src/channels/web/server.rs                |  26 ++++++-
 src/extensions/manager.rs                 |  14 +++-
 src/tools/mcp/auth.rs                     |  15 +++-
 src/tools/mcp/client.rs                   | 144 ++++++++++++++++++++++++++++++++++++-
 tests/e2e/mock_llm.py                     | 132 ++++++++++++++++++++++++++++++++++
 tests/e2e/scenarios/test_mcp_auth_flow.py | 355 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 9 files changed, 760 insertions(+), 29 deletions(-)
 create mode 100644 tests/e2e/scenarios/test_mcp_auth_flow.py
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad scripts,
agent runtime.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad scripts, agent runtime) means the fix could
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
