# Stdio/unix transports skip initialize handshake (#890)

## Summary

- Source commit: `c8cac0925dbb2ee7d3eea573cce985136a17ce31`
- Source date: `2026-03-12`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: moderate src.

## What the upstream commit addressed

Upstream commit `c8cac0925dbb2ee7d3eea573cce985136a17ce31` (`fix(mcp):
stdio/unix transports skip initialize handshake (#890) (#935)`) addresses
stdio/unix transports skip initialize handshake (#890).

Changed upstream paths:

- src/cli/mcp.rs
- src/tools/mcp/client.rs
- src/tools/mcp/stdio_transport.rs
- src/tools/mcp/unix_transport.rs

Upstream stats:

```text
 src/cli/mcp.rs                   | 18 ++++++++++++++----
 src/tools/mcp/client.rs          | 98 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++----------
 src/tools/mcp/stdio_transport.rs | 22 ++++++++++++++++++----
 src/tools/mcp/unix_transport.rs  | 22 ++++++++++++++++++----
 4 files changed, 138 insertions(+), 22 deletions(-)
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
