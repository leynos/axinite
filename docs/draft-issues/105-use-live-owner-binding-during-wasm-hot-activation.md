# Use live owner binding during wasm hot activation

## Summary

- Source commit: `cc52a046c1db34d388049e7f80c06b12e465675c`
- Source date: `2026-03-14`
- Severity: `medium`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow extensions/registry.

## What the upstream commit addressed

Upstream commit `cc52a046c1db34d388049e7f80c06b12e465675c` (`fix(channels): use
live owner binding during wasm hot activation (#1171)`) addresses use live owner
binding during wasm hot activation.

Changed upstream paths:

- src/extensions/manager.rs

Upstream stats:

```text
 src/extensions/manager.rs | 159 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++------
 1 file changed, 150 insertions(+), 9 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow
extensions/registry.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow extensions/registry) means the fix could touch
  more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
