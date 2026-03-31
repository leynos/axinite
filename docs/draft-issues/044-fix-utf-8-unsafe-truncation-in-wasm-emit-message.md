# Fix UTF-8 unsafe truncation in WASM emit_message

## Summary

- Source commit: `0b81342b5cd1e0948d73a8f6582d7ea0098be0d7`
- Source date: `2026-03-12`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow WASM channels.

## What the upstream commit addressed

Upstream commit `0b81342b5cd1e0948d73a8f6582d7ea0098be0d7` (`Fix UTF-8 unsafe
truncation in WASM emit_message (#1015)`) addresses fix utf-8 unsafe truncation
in wasm emit_message.

Changed upstream paths:

- src/channels/wasm/host.rs

Upstream stats:

```text
 src/channels/wasm/host.rs | 28 ++++++++++++++++++++++++++--
 1 file changed, 26 insertions(+), 2 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow WASM channels.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow WASM channels) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/channels/wasm/host.rs b/src/channels/wasm/host.rs
index 9f09455f..eeaccb20 100644
--- a/src/channels/wasm/host.rs
+++ b/src/channels/wasm/host.rs
@@ -64,5 +64,9 @@ const ALLOWED_MIME_PREFIXES: &[&str] = &[
     "application/octet-stream",
 ];
-
+/// Truncate a string to at most `max_bytes` without splitting UTF-8 code points.
+fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
+    let end = crate::util::floor_char_boundary(s, max_bytes);
+    &s[..end]
+}
 /// A message emitted by a WASM channel to be sent to the agent.
 #[derive(Debug, Clone)]
@@ -265,5 +269,5 @@ impl ChannelHostState {
                 "Message content too large, truncating"
             );
-            let mut truncated = msg.content[..MAX_MESSAGE_CONTENT_SIZE].to_string();
+            let mut truncated = truncate_utf8(&msg.content, MAX_MESSAGE_CONTENT_SIZE).to_string();
             truncated.push_str("... (truncated)");
             let msg = EmittedMessage {
@@ -632,4 +636,5 @@ mod tests {
         Attachment, ChannelEmitRateLimiter, ChannelHostState, EmittedMessage,
         MAX_ATTACHMENT_TOTAL_SIZE, MAX_ATTACHMENTS_PER_MESSAGE, MAX_EMITS_PER_EXECUTION,
+        MAX_MESSAGE_CONTENT_SIZE,
     };
 
@@ -690,4 +695,23 @@ mod tests {
     }
 
+    #[test]
+    fn test_emit_message_truncates_utf8_safely() {
+        let caps = ChannelCapabilities::for_channel("test");
+        let mut state = ChannelHostState::new("test", caps);
+
+        let prefix = "a".repeat(MAX_MESSAGE_CONTENT_SIZE - 1);
+        let content = format!("{}🙂suffix", prefix);
+        let msg = EmittedMessage::new("user123", content);
+
+        state.emit_message(msg).unwrap();
+        let messages = state.take_emitted_messages();
+        assert_eq!(messages.len(), 1);
+
+        let emitted = &messages[0].content;
+        assert!(emitted.starts_with(&prefix));
+        assert!(emitted.ends_with("... (truncated)"));
+        assert!(!emitted.contains("🙂"));
+    }
+
     #[test]
     fn test_workspace_write_prefixing() {
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
