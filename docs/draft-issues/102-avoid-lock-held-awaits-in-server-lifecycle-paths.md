# Avoid lock-held awaits in server lifecycle paths

## Summary

- Source commit: `8dfad332d96137bdcf3bafa265cb56f1111f052a`
- Source date: `2026-03-14`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: moderate web gateway, src.

## What the upstream commit addressed

Upstream commit `8dfad332d96137bdcf3bafa265cb56f1111f052a` (`fix(webhook): avoid
lock-held awaits in server lifecycle paths (#1168)`) addresses avoid lock-held
awaits in server lifecycle paths.

Changed upstream paths:

- src/channels/webhook_server.rs
- src/main.rs

Upstream stats:

```text
 src/channels/webhook_server.rs | 40 ++++++++++++++++++++++++++++++++++++++--
 src/main.rs                    | 11 ++++++++++-
 2 files changed, 48 insertions(+), 3 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate web gateway,
src.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate web gateway, src) means the fix could touch
  more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/channels/webhook_server.rs b/src/channels/webhook_server.rs
index 2425ab32..228abf0a 100644
--- a/src/channels/webhook_server.rs
+++ b/src/channels/webhook_server.rs
@@ -140,10 +140,17 @@ impl WebhookServer {
     }
 
+    /// Take ownership of shutdown primitives so callers can perform async
+    /// shutdown work without holding external locks around this server.
+    pub fn begin_shutdown(&mut self) -> (Option<oneshot::Sender<()>>, Option<JoinHandle<()>>) {
+        (self.shutdown_tx.take(), self.handle.take())
+    }
+
     /// Signal graceful shutdown and wait for the server task to finish.
     pub async fn shutdown(&mut self) {
-        if let Some(tx) = self.shutdown_tx.take() {
+        let (shutdown_tx, handle) = self.begin_shutdown();
+        if let Some(tx) = shutdown_tx {
             let _ = tx.send(());
         }
-        if let Some(handle) = self.handle.take() {
+        if let Some(handle) = handle {
             let _ = handle.await;
         }
@@ -270,4 +277,33 @@ mod tests {
     }
 
+    #[tokio::test]
+    async fn test_begin_shutdown_takes_handles_for_lock_free_shutdown() {
+        let addr = SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0));
+        let mut server = WebhookServer::new(WebhookServerConfig { addr });
+
+        let test_router = axum::Router::new().route(
+            "/health",
+            axum::routing::get(|| async { Json(json!({"status": "ok"})) }),
+        );
+        server.add_routes(test_router);
+        server.start().await.expect("Failed to start server"); // safety: test assertion for setup precondition
+
+        let (shutdown_tx, handle) = server.begin_shutdown();
+        assert!(shutdown_tx.is_some(), "shutdown sender should be available");
+        assert!(handle.is_some(), "server handle should be available");
+
+        // begin_shutdown() should leave no handles behind on the server.
+        let (shutdown_tx2, handle2) = server.begin_shutdown();
+        assert!(shutdown_tx2.is_none(), "shutdown sender should be consumed");
+        assert!(handle2.is_none(), "server handle should be consumed");
+
+        if let Some(tx) = shutdown_tx {
+            let _ = tx.send(());
+        }
+        if let Some(handle) = handle {
+            let _ = handle.await;
+        }
+    }
+
     #[tokio::test]
     async fn test_restart_with_addr_rollback_on_bind_failure() {
diff --git a/src/main.rs b/src/main.rs
index 12a8caf6..a7d95bec 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -921,5 +921,14 @@ async fn async_main() -> anyhow::Result<()> {
 
     if let Some(ref ws_arc) = webhook_server {
-        ws_arc.lock().await.shutdown().await;
+        let (shutdown_tx, handle) = {
+            let mut ws = ws_arc.lock().await;
+            ws.begin_shutdown()
+        };
+        if let Some(tx) = shutdown_tx {
+            let _ = tx.send(());
+        }
+        if let Some(handle) = handle {
+            let _ = handle.await;
+        }
     }
 
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
