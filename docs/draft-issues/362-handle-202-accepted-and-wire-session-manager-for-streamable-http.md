# Handle 202 Accepted and wire session manager for Streamable HTTP

## Summary

- Source commit: `1d5777824c617450ac2ce685d15b52c99ef69db3`
- Source date: `2026-03-26`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: moderate src.

## What the upstream commit addressed

Upstream commit `1d5777824c617450ac2ce685d15b52c99ef69db3` (`fix(mcp): handle
202 Accepted and wire session manager for Streamable HTTP (#1437)`) addresses
handle 202 accepted and wire session manager for streamable http.

Changed upstream paths:

- src/tools/mcp/client.rs
- src/tools/mcp/factory.rs
- src/tools/mcp/http_transport.rs

Upstream stats:

```text
 src/tools/mcp/client.rs         |  20 ++++++++++++++++-
 src/tools/mcp/factory.rs        | 117 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--------------
 src/tools/mcp/http_transport.rs |  28 ++++++++++++++++++++++++
 3 files changed, 148 insertions(+), 17 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate src.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate src) means the fix could touch more behaviour
  than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
