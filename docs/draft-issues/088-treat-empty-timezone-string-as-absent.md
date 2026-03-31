# Treat empty timezone string as absent

## Summary

- Source commit: `275bcfb65866a25334909393ebeaa2ed32827055`
- Source date: `2026-03-14`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: narrow tool runtime.

## What the upstream commit addressed

Upstream commit `275bcfb65866a25334909393ebeaa2ed32827055` (`fix(time): treat
empty timezone string as absent (#1127)`) addresses treat empty timezone string
as absent.

Changed upstream paths:

- src/tools/builtin/time.rs

Upstream stats:

```text
 src/tools/builtin/time.rs | 56 ++++++++++++++++++++++++++++++++++++++++++++++++++++++--
 1 file changed, 54 insertions(+), 2 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow tool runtime.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow tool runtime) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/tools/builtin/time.rs b/src/tools/builtin/time.rs
index bafbd4d7..5f037964 100644
--- a/src/tools/builtin/time.rs
+++ b/src/tools/builtin/time.rs
@@ -248,5 +248,9 @@ fn resolve_timezone_for_output(
     ctx: &JobContext,
 ) -> Result<Option<(Tz, String)>, ToolError> {
-    if let Some(name) = params.get("timezone").and_then(|v| v.as_str()) {
+    if let Some(name) = params
+        .get("timezone")
+        .and_then(|v| v.as_str())
+        .filter(|s| !s.is_empty())
+    {
         let tz = parse_timezone(name)?;
         return Ok(Some((tz, tz.to_string())));
@@ -287,5 +291,9 @@ fn context_timezone(ctx: &JobContext) -> Result<Option<(Tz, String)>, ToolError>
 fn optional_timezone(params: &serde_json::Value, keys: &[&str]) -> Result<Option<Tz>, ToolError> {
     for key in keys {
-        if let Some(value) = params.get(*key).and_then(|v| v.as_str()) {
+        if let Some(value) = params
+            .get(*key)
+            .and_then(|v| v.as_str())
+            .filter(|s| !s.is_empty())
+        {
             return parse_timezone(value).map(Some);
         }
@@ -535,3 +543,47 @@ mod tests {
         assert_eq!(dt.to_rfc3339(), "2026-03-08T07:30:00+00:00");
     }
+
+    #[tokio::test]
+    async fn test_now_with_empty_timezone_string_does_not_error() {
+        // LLMs sometimes pass "" for optional fields instead of omitting them.
+        // Empty timezone should be treated as absent and fall back to UTC.
+        let tool = TimeTool;
+        let ctx = JobContext::with_user("test", "chat", "test");
+
+        let output = tool
+            .execute(
+                serde_json::json!({
+                    "operation": "now",
+                    "timezone": ""
+                }),
+                &ctx,
+            )
+            .await
+            .expect("empty timezone string should not error");
+
+        assert!(output.result.get("iso").is_some(), "should have iso");
+    }
+
+    #[tokio::test]
+    async fn test_convert_with_empty_from_timezone_string_does_not_error() {
+        // LLMs sometimes pass "" for optional fields instead of omitting them.
+        // Empty from_timezone should be treated as absent.
+        let tool = TimeTool;
+        let ctx = JobContext::with_user("test", "chat", "test");
+
+        let output = tool
+            .execute(
+                serde_json::json!({
+                    "operation": "convert",
+                    "timestamp": "2026-03-08T12:00:00Z",
+                    "to_timezone": "America/New_York",
+                    "from_timezone": ""
+                }),
+                &ctx,
+            )
+            .await
+            .expect("empty from_timezone string should not error");
+
+        assert!(output.result.get("output").is_some(), "should have output");
+    }
 }
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
