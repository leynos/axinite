# Persist startup-loaded MCP clients in ExtensionManager

## Summary

- Source commit: `1d6f7d50850e30bc41ea85bb055dbd0af0655a29`
- Source date: `2026-03-20`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: moderate src, extensions/registry.

## What the upstream commit addressed

Upstream commit `1d6f7d50850e30bc41ea85bb055dbd0af0655a29` (`fix: persist
startup-loaded MCP clients in ExtensionManager (#1509)`) addresses persist
startup-loaded mcp clients in extensionmanager.

Changed upstream paths:

- src/app.rs
- src/extensions/manager.rs

Upstream stats:

```text
 src/app.rs                | 38 ++++++++++++++++++++++++++++++++++----
 src/extensions/manager.rs | 25 +++++++++++++++++++++++++
 2 files changed, 59 insertions(+), 4 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate src,
extensions/registry.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate src, extensions/registry) means the fix could
  touch more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/app.rs b/src/app.rs
index bca0f110..28e7ada5 100644
--- a/src/app.rs
+++ b/src/app.rs
@@ -537,5 +537,5 @@ impl AppBuilder {
                                             e
                                         );
-                                        return;
+                                        return None;
                                     }
                                 };
@@ -554,4 +554,8 @@ impl AppBuilder {
                                                     server_name
                                                 );
+                                                return Some((
+                                                    server_name,
+                                                    Arc::new(client),
+                                                ));
                                             }
                                             Err(e) => {
@@ -584,12 +588,25 @@ impl AppBuilder {
                                     }
                                 }
+                                None
                             });
                         }
 
+                        let mut startup_clients = Vec::new();
                         while let Some(result) = join_set.join_next().await {
-                            if let Err(e) = result {
-                                tracing::warn!("MCP server loading task panicked: {}", e);
+                            match result {
+                                Ok(Some(client_pair)) => {
+                                    startup_clients.push(client_pair);
+                                }
+                                Ok(None) => {}
+                                Err(e) => {
+                                    if e.is_panic() {
+                                        tracing::error!("MCP server loading task panicked: {}", e);
+                                    } else {
+                                        tracing::warn!("MCP server loading task failed: {}", e);
+                                    }
+                                }
                             }
                         }
+                        return startup_clients;
                     }
                     Err(e) => {
@@ -609,8 +626,10 @@ impl AppBuilder {
                     }
                 }
+                Vec::new()
             }
         };
 
-        let (dev_loaded_tool_names, _) = tokio::join!(wasm_tools_future, mcp_servers_future);
+        let (dev_loaded_tool_names, startup_mcp_clients) =
+            tokio::join!(wasm_tools_future, mcp_servers_future);
 
         // Load registry catalog entries for extension discovery
@@ -674,4 +693,15 @@ impl AppBuilder {
             tools.register_extension_tools(Arc::clone(&manager));
             tracing::debug!("Extension manager initialized with in-chat discovery tools");
+
+            if !startup_mcp_clients.is_empty() {
+                tracing::info!(
+                    count = startup_mcp_clients.len(),
+                    "Injecting startup MCP clients into extension manager"
+                );
+                for (name, client) in startup_mcp_clients {
+                    manager.inject_mcp_client(name, client).await;
+                }
+            }
+
             Some(manager)
         };
diff --git a/src/extensions/manager.rs b/src/extensions/manager.rs
index f06def20..b8af4c68 100644
--- a/src/extensions/manager.rs
+++ b/src/extensions/manager.rs
@@ -938,4 +938,29 @@ impl ExtensionManager {
     }
 
+    /// Inject a pre-created MCP client (from startup loading) into the manager.
+    ///
+    /// Startup-loaded MCP clients register their tools in `ToolRegistry` but are
+    /// otherwise dropped. This method stores the client so that `list()` reports
+    /// accurate "connected" status and reconnection/session management works.
+    pub(crate) async fn inject_mcp_client(
+        &self,
+        name: String,
+        client: Arc<crate::tools::mcp::McpClient>,
+    ) {
+        if name.is_empty() {
+            tracing::warn!("inject_mcp_client called with empty name; ignoring");
+            return;
+        }
+        if let Err(e) = Self::validate_extension_name(&name) {
+            tracing::warn!(
+                error = %e,
+                name = %name,
+                "inject_mcp_client called with invalid name; ignoring"
+            );
+            return;
+        }
+        self.mcp_clients.write().await.insert(name, client);
+    }
+
     /// Register channel names that were loaded at startup.
     /// Called after WASM channels are loaded so `list()` reports accurate active status.
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
