# Add Content-Security-Policy header to web gateway

## Summary

- Source commit: `f48fe95ac41e916a67bcc1482a9ce6450425452d`
- Source date: `2026-03-11`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow web gateway. CSP is an obvious defence-in-depth
  carry-forward for Axinite's browser UI.

## What the upstream commit addressed

Upstream commit `f48fe95ac41e916a67bcc1482a9ce6450425452d` (`fix(security): add
Content-Security-Policy header to web gateway (#966)`) addresses add
content-security-policy header to web gateway.

Changed upstream paths:

- src/channels/web/server.rs

Upstream stats:

```text
 src/channels/web/server.rs | 65 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 1 file changed, 65 insertions(+)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow web gateway. CSP
is an obvious defence-in-depth carry-forward for Axinite's browser UI.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow web gateway. CSP is an obvious defence-in-depth
  carry-forward for Axinite's browser UI) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/channels/web/server.rs b/src/channels/web/server.rs
index 3beafc20..4dc58390 100644
--- a/src/channels/web/server.rs
+++ b/src/channels/web/server.rs
@@ -373,4 +373,19 @@ pub async fn start_server(
             header::HeaderValue::from_static("DENY"),
         ))
+        .layer(SetResponseHeaderLayer::if_not_present(
+            header::HeaderName::from_static("content-security-policy"),
+            header::HeaderValue::from_static(
+                "default-src 'self'; \
+                 script-src 'self' https://cdn.jsdelivr.net https://cdnjs.cloudflare.com; \
+                 style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; \
+                 font-src https://fonts.gstatic.com; \
+                 connect-src 'self'; \
+                 img-src 'self' data:; \
+                 object-src 'none'; \
+                 frame-ancestors 'none'; \
+                 base-uri 'self'; \
+                 form-action 'self'",
+            ),
+        ))
         .with_state(state.clone());
 
@@ -2742,4 +2757,54 @@ mod tests {
     }
 
+    #[tokio::test]
+    async fn test_csp_header_present_on_responses() {
+        use std::net::SocketAddr;
+
+        let state = test_gateway_state(None);
+
+        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
+        let bound = start_server(addr, state.clone(), "test-token".to_string())
+            .await
+            .expect("server should start");
+
+        let client = reqwest::Client::new();
+        let resp = client
+            .get(format!("http://{}/api/health", bound))
+            .send()
+            .await
+            .expect("health request should succeed");
+
+        assert_eq!(resp.status(), 200);
+
+        let csp = resp
+            .headers()
+            .get("content-security-policy")
+            .expect("CSP header must be present");
+
+        let csp_str = csp.to_str().expect("CSP header should be valid UTF-8");
+        assert!(
+            csp_str.contains("default-src 'self'"),
+            "CSP must contain default-src"
+        );
+        assert!(
+            csp_str.contains(
+                "script-src 'self' https://cdn.jsdelivr.net https://cdnjs.cloudflare.com"
+            ),
+            "CSP must allow both marked and DOMPurify script CDNs"
+        );
+        assert!(
+            csp_str.contains("object-src 'none'"),
+            "CSP must contain object-src 'none'"
+        );
+        assert!(
+            csp_str.contains("frame-ancestors 'none'"),
+            "CSP must contain frame-ancestors 'none'"
+        );
+
+        if let Some(tx) = state.shutdown_tx.write().await.take() {
+            let _ = tx.send(());
+        }
+    }
+
     #[tokio::test]
     async fn test_oauth_callback_missing_params() {
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
