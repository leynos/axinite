# Header safety validation and Authorization conflict bug from #704

## Summary

- Source commit: `a1b3911b27ac20daae1eb2e3e1fd7d5f6b35f548`
- Source date: `2026-03-12`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: moderate src.

## What the upstream commit addressed

Upstream commit `a1b3911b27ac20daae1eb2e3e1fd7d5f6b35f548` (`fix(mcp): header
safety validation and Authorization conflict bug from #704 (#752)`) addresses
header safety validation and authorization conflict bug from #704.

Changed upstream paths:

- src/app.rs
- src/tools/mcp/client.rs
- src/tools/mcp/config.rs
- src/tools/mcp/http_transport.rs

Upstream stats:

```text
 src/app.rs                      |  14 ++++++++-
 src/tools/mcp/client.rs         |  12 +++++++-
 src/tools/mcp/config.rs         | 162 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/tools/mcp/http_transport.rs | 117 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 4 files changed, 303 insertions(+), 2 deletions(-)
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
