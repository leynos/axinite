# Add missing extension_manager field in trigger_manual EngineContext

## Summary

- Source commit: `a4f6cda5c9e0cd1d0f2d8809941e2927ffca2982`
- Source date: `2026-03-20`
- Severity: `medium-high`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: narrow agent runtime.

## What the upstream commit addressed

Upstream commit `a4f6cda5c9e0cd1d0f2d8809941e2927ffca2982` (`fix(routines): add
missing extension_manager field in trigger_manual EngineContext`) addresses add
missing extension_manager field in trigger_manual enginecontext.

Changed upstream paths:

- src/agent/routine_engine.rs

Upstream stats:

```text
 src/agent/routine_engine.rs | 1 +
 1 file changed, 1 insertion(+)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium-high` severity. That keeps
the underlying failure mode close enough to Axinite's current runtime to justify
a follow-up review. The recorded blast radius upstream was narrow agent runtime.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow agent runtime) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/agent/routine_engine.rs b/src/agent/routine_engine.rs
index 16671239..2a5f4474 100644
--- a/src/agent/routine_engine.rs
+++ b/src/agent/routine_engine.rs
@@ -793,4 +793,5 @@ impl RoutineEngine {
             running_count: self.running_count.clone(),
             scheduler: self.scheduler.clone(),
+            extension_manager: self.extension_manager.clone(),
             tools: self.tools.clone(),
             safety: self.safety.clone(),
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
