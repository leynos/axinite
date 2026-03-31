# Resolve wasm broadcast merge conflicts with staging (#395)

## Summary

- Source commit: `1b97ef4feb07dfd24a878be9c3dd2fd32e1106d4`
- Source date: `2026-03-20`
- Severity: `medium`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: moderate agent runtime, WASM channels.

## What the upstream commit addressed

Upstream commit `1b97ef4feb07dfd24a878be9c3dd2fd32e1106d4` (`fix: resolve wasm
broadcast merge conflicts with staging (#395) (#1460)`) addresses resolve wasm
broadcast merge conflicts with staging (#395).

Changed upstream paths:

- src/agent/job_monitor.rs
- src/channels/wasm/wrapper.rs

Upstream stats:

```text
 src/agent/job_monitor.rs     |  3 +++
 src/channels/wasm/wrapper.rs | 11 +++++++++++
 2 files changed, 14 insertions(+)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate agent runtime,
WASM channels.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate agent runtime, WASM channels) means the fix
  could touch more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/agent/job_monitor.rs b/src/agent/job_monitor.rs
index 3f038764..675d0426 100644
--- a/src/agent/job_monitor.rs
+++ b/src/agent/job_monitor.rs
@@ -422,4 +422,5 @@ mod tests {
                     status: "completed".to_string(),
                     session_id: None,
+                    fallback_deliverable: None,
                 },
             ))
@@ -469,4 +470,5 @@ mod tests {
                     status: "failed".to_string(),
                     session_id: None,
+                    fallback_deliverable: None,
                 },
             ))
@@ -507,4 +509,5 @@ mod tests {
                     status: "completed".to_string(),
                     session_id: None,
+                    fallback_deliverable: None,
                 },
             ))
diff --git a/src/channels/wasm/wrapper.rs b/src/channels/wasm/wrapper.rs
index 8f0c9db4..be7768d0 100644
--- a/src/channels/wasm/wrapper.rs
+++ b/src/channels/wasm/wrapper.rs
@@ -3315,4 +3315,5 @@ mod tests {
 
     use crate::channels::Channel;
+    use crate::channels::OutgoingResponse;
     use crate::channels::wasm::capabilities::ChannelCapabilities;
     use crate::channels::wasm::runtime::{
@@ -3402,4 +3403,14 @@ mod tests {
     }
 
+    #[tokio::test]
+    async fn test_broadcast_delegates_to_call_on_broadcast() {
+        let channel = create_test_channel();
+        // With `component: None`, call_on_broadcast short-circuits to Ok(()).
+        let result = channel
+            .broadcast("146032821", OutgoingResponse::text("hello"))
+            .await;
+        assert!(result.is_ok());
+    }
+
     #[tokio::test]
     async fn test_execute_poll_no_wasm_returns_empty() {
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
