# Load WASM tool description and schema from capabilities.json

## Summary

- Source commit: `94b448ffab521d82ff8226e8eb26887b6ed00155`
- Source date: `2026-03-11`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: moderate WASM tools.

## What the upstream commit addressed

Upstream commit `94b448ffab521d82ff8226e8eb26887b6ed00155` (`fix(security): load
WASM tool description and schema from capabilities.json (#520)`) addresses load
wasm tool description and schema from capabilities.json.

Changed upstream paths:

- src/tools/wasm/capabilities_schema.rs
- src/tools/wasm/loader.rs
- src/tools/wasm/runtime.rs

Upstream stats:

```text
 src/tools/wasm/capabilities_schema.rs | 124 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/tools/wasm/loader.rs              |  93 +++++++++++++++++++++++++++++++++++++++++++++++++---------------------
 src/tools/wasm/runtime.rs             |  23 +++++++++++-------
 3 files changed, 204 insertions(+), 36 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate WASM tools.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate WASM tools) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
