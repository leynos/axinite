# Preserve AuthError type in oauth_http_client cache

## Summary

- Source commit: `f059d5033155a84551d3bcad25268c956c50f0a4`
- Source date: `2026-03-15`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow src.

## What the upstream commit addressed

Upstream commit `f059d5033155a84551d3bcad25268c956c50f0a4` (`fix: preserve
AuthError type in oauth_http_client cache (#1152)`) addresses preserve autherror
type in oauth_http_client cache.

Changed upstream paths:

- src/tools/mcp/auth.rs

Upstream stats:

```text
 src/tools/mcp/auth.rs | 19 +++++++++++++++----
 1 file changed, 15 insertions(+), 4 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow src.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow src) means the fix could touch more behaviour
  than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/tools/mcp/auth.rs b/src/tools/mcp/auth.rs
index 7a8e384f..1926e78d 100644
--- a/src/tools/mcp/auth.rs
+++ b/src/tools/mcp/auth.rs
@@ -25,5 +25,5 @@ use crate::tools::mcp::config::McpServerConfig;
 /// the request builder.
 fn oauth_http_client() -> Result<&'static reqwest::Client, AuthError> {
-    static CLIENT: std::sync::OnceLock<Result<reqwest::Client, String>> =
+    static CLIENT: std::sync::OnceLock<Result<reqwest::Client, AuthError>> =
         std::sync::OnceLock::new();
     CLIENT
@@ -33,8 +33,8 @@ fn oauth_http_client() -> Result<&'static reqwest::Client, AuthError> {
                 .redirect(reqwest::redirect::Policy::none())
                 .build()
-                .map_err(|e| e.to_string())
+                .map_err(|e| AuthError::Http(e.to_string()))
         })
         .as_ref()
-        .map_err(|e| AuthError::Http(e.clone()))
+        .map_err(Clone::clone)
 }
 
@@ -58,5 +58,5 @@ fn log_redirect_if_applicable(url: &str, response: &reqwest::Response) {
 
 /// OAuth authorization error.
-#[derive(Debug, thiserror::Error)]
+#[derive(Debug, Clone, thiserror::Error)]
 pub enum AuthError {
     #[error("Server does not support OAuth authorization")]
@@ -1521,4 +1521,15 @@ mod tests {
     }
 
+    #[test]
+    fn test_auth_error_clone_preserves_http_variant_and_payload() {
+        let original = AuthError::Http("builder failed".to_string());
+        let cloned = original.clone();
+
+        match cloned {
+            AuthError::Http(message) => assert_eq!(message, "builder failed"),
+            other => panic!("expected AuthError::Http variant, got {other:?}"),
+        }
+    }
+
     // --- New tests for well-known URI construction ---
 
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
