# Address 14 audit findings across MCP module

## Summary

- Source commit: `f53c1bb10beba3f6bb1f127c34371a6c0bf6f510`
- Source date: `2026-03-13`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `comprehensive`
- Scope and blast radius: medium MCP/runtime core. A 14-finding MCP sweep is
  directly relevant because MCP over HTTPS is central to Axinite's mission.

## What the upstream commit addressed

Upstream commit `f53c1bb10beba3f6bb1f127c34371a6c0bf6f510` (`fix(mcp): address
14 audit findings across MCP module (#1094)`) addresses address 14 audit
findings across mcp module.

Changed upstream paths:

- src/tools/mcp/auth.rs
- src/tools/mcp/client.rs
- src/tools/mcp/config.rs
- src/tools/mcp/factory.rs
- src/tools/mcp/http_transport.rs
- src/tools/mcp/stdio_transport.rs
- src/tools/mcp/transport.rs
- src/tools/mcp/unix_transport.rs

Upstream stats:

```text
 src/tools/mcp/auth.rs            | 100 ++++++++++++++++++++++---------------
 src/tools/mcp/client.rs          | 266 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-----------------------
 src/tools/mcp/config.rs          |  44 ++++++++++++++---
 src/tools/mcp/factory.rs         |  10 ++++
 src/tools/mcp/http_transport.rs  |  23 +++++----
 src/tools/mcp/stdio_transport.rs |  67 ++++---------------------
 src/tools/mcp/transport.rs       | 106 +++++++++++++++++++++++++++++++++++++++-
 src/tools/mcp/unix_transport.rs  |  67 ++++---------------------
 8 files changed, 449 insertions(+), 234 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was medium MCP/runtime
core. A 14-finding MCP sweep is directly relevant because MCP over HTTPS is
central to Axinite's mission.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `comprehensive` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (medium MCP/runtime core. A 14-finding MCP sweep is
  directly relevant because MCP over HTTPS is central to Axinite's mission)
  means the fix could touch more behaviour than the narrow symptom suggests.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
