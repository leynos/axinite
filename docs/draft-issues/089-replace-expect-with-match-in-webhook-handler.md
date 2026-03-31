# Replace .expect() with match in webhook handler

## Summary

- Source commit: `bc6725205ada24f26ed30fd042dc4aa6b546cb93`
- Source date: `2026-03-13`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow webhooks.

## What the upstream commit addressed

Upstream commit `bc6725205ada24f26ed30fd042dc4aa6b546cb93` (`fix(http): replace
.expect() with match in webhook handler (#1133)`) addresses replace .expect()
with match in webhook handler.

Changed upstream paths:

- src/channels/http.rs

Upstream stats:

```text
 src/channels/http.rs | 39 +++++++++++++++++++--------------------
 1 file changed, 19 insertions(+), 20 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow webhooks.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow webhooks) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/channels/http.rs b/src/channels/http.rs
index 00a48048..7c1b9789 100644
--- a/src/channels/http.rs
+++ b/src/channels/http.rs
@@ -270,23 +270,22 @@ async fn webhook_handler(
     {
         let webhook_secret = state.webhook_secret.read().await;
-        if webhook_secret.is_none() {
-            // No secret configured — reject all requests. This guards against
-            // the secret being cleared at runtime via update_secret(None).
-            // The start() method also prevents startup without a secret, but
-            // this is defense-in-depth for the SIGHUP hot-swap path.
-            return (
-                StatusCode::SERVICE_UNAVAILABLE,
-                Json(WebhookResponse {
-                    message_id: Uuid::nil(),
-                    status: "error".to_string(),
-                    response: Some("Webhook authentication not configured".to_string()),
-                }),
-            )
-                .into_response();
-        }
-        let expected_secret = webhook_secret
-            .as_ref()
-            .expect("checked is_none above")
-            .expose_secret();
+        let expected_secret = match webhook_secret.as_ref() {
+            Some(secret) => secret.expose_secret(),
+            None => {
+                // No secret configured — reject all requests. This guards against
+                // the secret being cleared at runtime via update_secret(None).
+                // The start() method also prevents startup without a secret, but
+                // this is defense-in-depth for the SIGHUP hot-swap path.
+                return (
+                    StatusCode::SERVICE_UNAVAILABLE,
+                    Json(WebhookResponse {
+                        message_id: Uuid::nil(),
+                        status: "error".to_string(),
+                        response: Some("Webhook authentication not configured".to_string()),
+                    }),
+                )
+                    .into_response();
+            }
+        };
 
         match headers.get("x-ironclaw-signature") {
@@ -1090,5 +1089,5 @@ mod tests {
 
         let resp = app.oneshot(req).await.unwrap();
-        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
+        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE); // safety: test assertion
     }
 
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
