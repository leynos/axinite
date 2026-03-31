# Redact database error details from API responses

## Summary

- Source commit: `9bb19a98f767f2e4db0d4f3dda0e495a356a726a`
- Source date: `2026-03-28`
- Severity: `medium`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow web API surface. Redacting database errors
  belongs in any user-facing Axinite gateway.

## What the upstream commit addressed

Upstream commit `9bb19a98f767f2e4db0d4f3dda0e495a356a726a` (`fix(web): redact
database error details from API responses (#1711)`) addresses redact database
error details from api responses.

Changed upstream paths:

- src/channels/web/handlers/jobs.rs

Upstream stats:

```text
 src/channels/web/handlers/jobs.rs | 62 ++++++++++++++++++++++++++++++--------------------------------
 1 file changed, 30 insertions(+), 32 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow web API surface.
Redacting database errors belongs in any user-facing Axinite gateway.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow web API surface. Redacting database errors
  belongs in any user-facing Axinite gateway) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/channels/web/handlers/jobs.rs b/src/channels/web/handlers/jobs.rs
index 35adeec6..b171561c 100644
--- a/src/channels/web/handlers/jobs.rs
+++ b/src/channels/web/handlers/jobs.rs
@@ -16,4 +16,12 @@ use crate::channels::web::server::GatewayState;
 use crate::channels::web::types::*;
 
+fn db_error(context: &str, e: impl std::fmt::Display) -> (StatusCode, String) {
+    tracing::error!(%e, context, "Database error in jobs handler");
+    (
+        StatusCode::INTERNAL_SERVER_ERROR,
+        "Internal database error".to_string(),
+    )
+}
+
 pub async fn jobs_list_handler(
     State(state): State<Arc<GatewayState>>,
@@ -214,8 +222,5 @@ pub async fn jobs_detail_handler(
         Ok(None) => {}
         Err(e) => {
-            return Err((
-                StatusCode::INTERNAL_SERVER_ERROR,
-                format!("Database error: {}", e),
-            ));
+            return Err(db_error("jobs_handler", e));
         }
     }
@@ -258,8 +263,5 @@ pub async fn jobs_detail_handler(
         }
         Ok(None) => Err((StatusCode::NOT_FOUND, "Job not found".to_string())),
-        Err(e) => Err((
-            StatusCode::INTERNAL_SERVER_ERROR,
-            format!("Database error: {}", e),
-        )),
+        Err(e) => Err(db_error("jobs_handler", e)),
     }
 }
@@ -305,8 +307,5 @@ pub async fn jobs_cancel_handler(
             Ok(None) => {}
             Err(e) => {
-                return Err((
-                    StatusCode::INTERNAL_SERVER_ERROR,
-                    format!("Database error: {}", e),
-                ));
+                return Err(db_error("jobs_handler", e));
             }
         }
@@ -351,8 +350,5 @@ pub async fn jobs_cancel_handler(
             Ok(None) => {}
             Err(e) => {
-                return Err((
-                    StatusCode::INTERNAL_SERVER_ERROR,
-                    format!("Database error: {}", e),
-                ));
+                return Err(db_error("jobs_handler", e));
             }
         }
@@ -472,8 +468,5 @@ pub async fn jobs_restart_handler(
         Ok(None) => {}
         Err(e) => {
-            return Err((
-                StatusCode::INTERNAL_SERVER_ERROR,
-                format!("Database error: {}", e),
-            ));
+            return Err(db_error("jobs_handler", e));
         }
     }
@@ -531,8 +524,5 @@ pub async fn jobs_restart_handler(
         }
         Ok(None) => Err((StatusCode::NOT_FOUND, "Job not found".to_string())),
-        Err(e) => Err((
-            StatusCode::INTERNAL_SERVER_ERROR,
-            format!("Database error: {}", e),
-        )),
+        Err(e) => Err(db_error("jobs_handler", e)),
     }
 }
@@ -610,8 +600,5 @@ pub async fn jobs_prompt_handler(
             }
             Err(e) => {
-                return Err((
-                    StatusCode::INTERNAL_SERVER_ERROR,
-                    format!("Database error: {}", e),
-                ));
+                return Err(db_error("jobs_handler", e));
             }
         }
@@ -668,8 +655,5 @@ pub async fn jobs_events_handler(
         }
         Err(e) => {
-            return Err((
-                StatusCode::INTERNAL_SERVER_ERROR,
-                format!("Database error: {}", e),
-            ));
+            return Err(db_error("jobs_handler", e));
         }
     }
@@ -824,2 +808,16 @@ pub async fn job_files_read_handler(
     }))
 }
+
+#[cfg(test)]
+mod tests {
+    use super::*;
+
+    #[test]
+    fn test_db_error_does_not_leak_details() {
+        let (status, body) = db_error("test_context", "relation \"jobs\" does not exist");
+        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
+        assert_eq!(body, "Internal database error");
+        assert!(!body.contains("relation"));
+        assert!(!body.contains("does not exist"));
+    }
+}
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
