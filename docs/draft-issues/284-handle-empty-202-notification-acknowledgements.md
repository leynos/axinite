# Handle empty 202 notification acknowledgements

## Summary

- Source commit: `969b559e2abca655731da98e85ca4b62313f77a7`
- Source date: `2026-03-22`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow src.

## What the upstream commit addressed

Upstream commit `969b559e2abca655731da98e85ca4b62313f77a7` (`fix(mcp): handle
empty 202 notification acknowledgements (#1539)`) addresses handle empty 202
notification acknowledgements.

Changed upstream paths:

- src/tools/mcp/http_transport.rs

Upstream stats:

```text
 src/tools/mcp/http_transport.rs | 61 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 1 file changed, 61 insertions(+)
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
diff --git a/src/tools/mcp/http_transport.rs b/src/tools/mcp/http_transport.rs
index ec7139c9..59873ce4 100644
--- a/src/tools/mcp/http_transport.rs
+++ b/src/tools/mcp/http_transport.rs
@@ -131,4 +131,14 @@ impl McpTransport for HttpMcpTransport {
         }
 
+        // MCP notifications commonly acknowledge with 202 Accepted and no body.
+        if response.status() == reqwest::StatusCode::ACCEPTED {
+            return Ok(McpResponse {
+                jsonrpc: "2.0".to_string(),
+                id: request.id,
+                result: None,
+                error: None,
+            });
+        }
+
         // Determine response format from Content-Type.
         let content_type = response
@@ -507,3 +517,54 @@ mod tests {
         assert_eq!(echoed["authorization"], "Bearer custom-token");
     }
+
+    async fn spawn_accepted_server() -> (String, tokio::task::JoinHandle<()>) {
+        use axum::{Router, routing::post};
+        use tokio::net::TcpListener;
+
+        async fn accepted() -> axum::http::StatusCode {
+            axum::http::StatusCode::ACCEPTED
+        }
+
+        let app = Router::new().route("/", post(accepted));
+        let listener = TcpListener::bind("127.0.0.1:0")
+            .await
+            .expect("Failed to bind to an ephemeral port");
+        let addr = listener
+            .local_addr()
+            .expect("Failed to get listener's local address");
+        let url = format!("http://127.0.0.1:{}", addr.port());
+
+        let handle = tokio::spawn(async move {
+            axum::serve(listener, app)
+                .await
+                .expect("Test server failed to run");
+        });
+
+        (url, handle)
+    }
+
+    fn notification_request(method: &str) -> McpRequest {
+        McpRequest {
+            jsonrpc: "2.0".to_string(),
+            id: None,
+            method: method.to_string(),
+            params: None,
+        }
+    }
+
+    #[tokio::test]
+    async fn test_accepted_notification_returns_empty_response() {
+        let (url, _handle) = spawn_accepted_server().await;
+        let transport = HttpMcpTransport::new(&url, "accepted-test");
+        let request = notification_request("notifications/initialized");
+
+        let response = transport
+            .send(&request, &HashMap::new())
+            .await
+            .expect("202 notification response");
+        assert_eq!(response.jsonrpc, "2.0");
+        assert_eq!(response.id, request.id);
+        assert!(response.result.is_none());
+        assert!(response.error.is_none());
+    }
 }
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
