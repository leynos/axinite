# Use typed WASM schema as advertised schema when available

## Summary

- Source commit: `27e8d6f8dd106c462c7d616ae8c13f78a6d8b423`
- Source date: `2026-03-28`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow WASM schema publication path. Canonical typed
  schemas are core to Axinite's tool-definition roadmap.

## What the upstream commit addressed

Upstream commit `27e8d6f8dd106c462c7d616ae8c13f78a6d8b423` (`fix(wasm): use
typed WASM schema as advertised schema when available (#1699)`) addresses use
typed wasm schema as advertised schema when available.

Changed upstream paths:

- src/tools/wasm/loader.rs
- src/tools/wasm/wrapper.rs

Upstream stats:

```text
 src/tools/wasm/loader.rs  |  6 ++++--
 src/tools/wasm/wrapper.rs | 97 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-----
 2 files changed, 96 insertions(+), 7 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow WASM schema
publication path. Canonical typed schemas are core to Axinite's tool-definition
roadmap.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow WASM schema publication path. Canonical typed
  schemas are core to Axinite's tool-definition roadmap) means the fix could
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
