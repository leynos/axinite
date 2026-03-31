# Attach session manager for non-OAuth HTTP clients (#793)

## Summary

- Source commit: `f31cd13135ab75db1ae7011c01ae0da6431048c5`
- Source date: `2026-03-11`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow src.

## What the upstream commit addressed

Upstream commit `f31cd13135ab75db1ae7011c01ae0da6431048c5` (`fix(mcp): attach
session manager for non-OAuth HTTP clients (#793) (#986)`) addresses attach
session manager for non-oauth http clients (#793).

Changed upstream paths:

- src/tools/mcp/client.rs
- src/tools/mcp/factory.rs

Upstream stats:

```text
 src/tools/mcp/client.rs  | 22 ++++++++++++++++++++++
 src/tools/mcp/factory.rs | 33 +++++++++++++++++++++++++++++++--
 2 files changed, 53 insertions(+), 2 deletions(-)
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
diff --git a/src/tools/mcp/client.rs b/src/tools/mcp/client.rs
index d3bf17b8..7780ff80 100644
--- a/src/tools/mcp/client.rs
+++ b/src/tools/mcp/client.rs
@@ -210,4 +210,10 @@ impl McpClient {
     }
 
+    /// Attach a session manager for Streamable HTTP session tracking.
+    pub fn with_session_manager(mut self, session_manager: Arc<McpSessionManager>) -> Self {
+        self.session_manager = Some(session_manager);
+        self
+    }
+
     /// Get the server name.
     pub fn server_name(&self) -> &str {
@@ -220,4 +226,9 @@ impl McpClient {
     }
 
+    /// Whether this client has a session manager attached.
+    pub fn has_session_manager(&self) -> bool {
+        self.session_manager.is_some()
+    }
+
     /// Get the next request ID.
     fn next_request_id(&self) -> u64 {
@@ -717,4 +728,15 @@ mod tests {
     }
 
+    #[test]
+    fn test_with_session_manager() {
+        let client = McpClient::new("http://localhost:8080");
+        assert!(!client.has_session_manager());
+
+        let session_manager = Arc::new(McpSessionManager::new());
+        let client = client.with_session_manager(session_manager);
+
+        assert!(client.has_session_manager());
+    }
+
     #[test]
     fn test_next_request_id_monotonically_increasing() {
diff --git a/src/tools/mcp/factory.rs b/src/tools/mcp/factory.rs
index b5acb3f9..1cc714bc 100644
--- a/src/tools/mcp/factory.rs
+++ b/src/tools/mcp/factory.rs
@@ -89,10 +89,39 @@ pub async fn create_client_from_config(
                     ))
                 } else {
-                    Ok(McpClient::new_with_config(server))
+                    Ok(McpClient::new_with_config(server)
+                        .with_session_manager(Arc::clone(session_manager)))
                 }
             } else {
-                Ok(McpClient::new_with_config(server))
+                Ok(McpClient::new_with_config(server)
+                    .with_session_manager(Arc::clone(session_manager)))
             }
         }
     }
 }
+
+#[cfg(test)]
+mod tests {
+    use super::*;
+
+    #[tokio::test]
+    async fn test_factory_non_oauth_http_has_session_manager() {
+        let server = McpServerConfig::new("test-server", "http://localhost:9999");
+        let session_manager = Arc::new(McpSessionManager::new());
+        let process_manager = Arc::new(McpProcessManager::new());
+
+        let client = create_client_from_config(
+            server,
+            &session_manager,
+            &process_manager,
+            None,
+            "test-user",
+        )
+        .await
+        .expect("factory should succeed for HTTP config");
+
+        assert!(
+            client.has_session_manager(),
+            "non-OAuth HTTP clients must carry a session manager"
+        );
+    }
+}
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
