# Remove debug_assert guards that panic on valid error paths

## Summary

- Source commit: `b7a1edf346e352590fa1c07d1807ac7c98c53a8c`
- Source date: `2026-03-18`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow src.

## What the upstream commit addressed

Upstream commit `b7a1edf346e352590fa1c07d1807ac7c98c53a8c` (`fix: remove
debug_assert guards that panic on valid error paths (#1385)`) addresses remove
debug_assert guards that panic on valid error paths.

Changed upstream paths:

- src/context/state.rs
- src/tools/execute.rs

Upstream stats:

```text
 src/context/state.rs |  7 -------
 src/tools/execute.rs | 18 +++++++++++-------
 2 files changed, 11 insertions(+), 14 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow src.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow src) means the fix could touch more behaviour
  than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/context/state.rs b/src/context/state.rs
index bae5bdf1..f5307947 100644
--- a/src/context/state.rs
+++ b/src/context/state.rs
@@ -259,11 +259,4 @@ impl JobContext {
         reason: Option<String>,
     ) -> Result<(), String> {
-        debug_assert!(
-            self.state.can_transition_to(new_state),
-            "BUG: invalid job state transition {} -> {} for job {}",
-            self.state,
-            new_state,
-            self.job_id
-        );
         if !self.state.can_transition_to(new_state) {
             return Err(format!(
diff --git a/src/tools/execute.rs b/src/tools/execute.rs
index fa52c59c..bb8a7b9d 100644
--- a/src/tools/execute.rs
+++ b/src/tools/execute.rs
@@ -23,8 +23,4 @@ pub async fn execute_tool_with_safety(
     job_ctx: &JobContext,
 ) -> Result<String, Error> {
-    debug_assert!(
-        !tool_name.is_empty(),
-        "BUG: execute_tool_with_safety called with empty tool_name"
-    );
     let tool = tools
         .get(tool_name)
@@ -298,6 +294,6 @@ mod tests {
     #[tokio::test]
     async fn test_execute_empty_tool_name_returns_not_found() {
-        // Regression: execute_tool_with_safety must reject empty tool names before
-        // even attempting a registry lookup (the debug_assert guards this invariant).
+        // Regression: execute_tool_with_safety must reject empty tool names
+        // gracefully via ToolError::NotFound (not a panic).
         let registry = registry_with(vec![]).await;
         let safety = test_safety();
@@ -312,5 +308,13 @@ mod tests {
         .await;
 
-        assert!(result.is_err(), "Empty tool name should return an error"); // safety: test-only assertion
+        assert!(
+            matches!(
+                result,
+                Err(crate::error::Error::Tool(
+                    crate::error::ToolError::NotFound { .. }
+                ))
+            ),
+            "Empty tool name should return ToolError::NotFound, got: {result:?}"
+        );
     }
 
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
