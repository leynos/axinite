# Open MCP OAuth in same browser as gateway

## Summary

- Source commit: `8a26cfae736526dc13aed47dd17f53ca135c496e`
- Source date: `2026-03-12`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: very broad .githooks, agent runtime.

## What the upstream commit addressed

Upstream commit `8a26cfae736526dc13aed47dd17f53ca135c496e` (`fix(mcp): open MCP
OAuth in same browser as gateway (#951)`) addresses open mcp oauth in same
browser as gateway.

Changed upstream paths:

- .githooks/pre-push
- src/agent/agent_loop.rs
- src/agent/thread_ops.rs
- src/channels/web/server.rs
- src/cli/oauth_defaults.rs
- src/extensions/manager.rs
- src/extensions/mod.rs
- src/main.rs
- src/tools/builtin/extension_tools.rs
- tests/e2e_advanced_traces.rs
- tests/fixtures/llm_traces/advanced/mcp_extension_lifecycle.json
- tests/support/mock_mcp_server.rs
- tests/support/mod.rs
- tests/support/test_rig.rs

Upstream stats:

```text
 .githooks/pre-push                                              |  23 ++++
 src/agent/agent_loop.rs                                         |  29 +---
 src/agent/thread_ops.rs                                         |  21 ++-
 src/channels/web/server.rs                                      |  55 +++++++-
 src/cli/oauth_defaults.rs                                       |  79 +++++++++++
 src/extensions/manager.rs                                       | 506 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--------
 src/extensions/mod.rs                                           |   3 +
 src/main.rs                                                     |   8 ++
 src/tools/builtin/extension_tools.rs                            |  24 +++-
 tests/e2e_advanced_traces.rs                                    | 132 ++++++++++++++++++
 tests/fixtures/llm_traces/advanced/mcp_extension_lifecycle.json |  98 +++++++++++++
 tests/support/mock_mcp_server.rs                                | 340 ++++++++++++++++++++++++++++++++++++++++++++++
 tests/support/mod.rs                                            |   1 +
 tests/support/test_rig.rs                                       |  10 ++
 14 files changed, 1239 insertions(+), 90 deletions(-)
 create mode 100755 .githooks/pre-push
 create mode 100644 tests/fixtures/llm_traces/advanced/mcp_extension_lifecycle.json
 create mode 100644 tests/support/mock_mcp_server.rs
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad .githooks,
agent runtime.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad .githooks, agent runtime) means the fix
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
