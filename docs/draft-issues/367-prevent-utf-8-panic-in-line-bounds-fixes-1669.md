# Prevent UTF-8 panic in line_bounds() (fixes #1669)

## Summary

- Source commit: `7234700c78d985ddc872721bc2a7130eeaa0b8c3`
- Source date: `2026-03-27`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: moderate web gateway, LLM stack.

## What the upstream commit addressed

Upstream commit `7234700c78d985ddc872721bc2a7130eeaa0b8c3` (`fix(llm): prevent
UTF-8 panic in line_bounds() (fixes #1669) (#1679)`) addresses prevent utf-8
panic in line_bounds() (fixes #1669).

Changed upstream paths:

- src/channels/web/handlers/chat.rs
- src/channels/web/server.rs
- src/llm/reasoning.rs

Upstream stats:

```text
 src/channels/web/handlers/chat.rs |  2 +-
 src/channels/web/server.rs        |  2 +-
 src/llm/reasoning.rs              | 58 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++--
 3 files changed, 58 insertions(+), 4 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate web gateway,
LLM stack.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate web gateway, LLM stack) means the fix could
  touch more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/channels/web/handlers/chat.rs b/src/channels/web/handlers/chat.rs
index bc4e3dbc..e2458571 100644
--- a/src/channels/web/handlers/chat.rs
+++ b/src/channels/web/handlers/chat.rs
@@ -534,5 +534,5 @@ pub async fn chat_threads_handler(
     let sess = session.lock().await;
     let mut sorted_threads: Vec<_> = sess.threads.values().collect();
-    sorted_threads.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
+    sorted_threads.sort_by_key(|t| std::cmp::Reverse(t.updated_at));
     let threads: Vec<ThreadInfo> = sorted_threads
         .into_iter()
diff --git a/src/channels/web/server.rs b/src/channels/web/server.rs
index ab50c94a..73e183d2 100644
--- a/src/channels/web/server.rs
+++ b/src/channels/web/server.rs
@@ -1891,5 +1891,5 @@ async fn chat_threads_handler(
     // Fallback: in-memory only (no assistant thread without DB)
     let mut sorted_threads: Vec<_> = sess.threads.values().collect();
-    sorted_threads.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
+    sorted_threads.sort_by_key(|t| std::cmp::Reverse(t.updated_at));
     let threads: Vec<ThreadInfo> = sorted_threads
         .into_iter()
diff --git a/src/llm/reasoning.rs b/src/llm/reasoning.rs
index 6e078ac7..a0852cef 100644
--- a/src/llm/reasoning.rs
+++ b/src/llm/reasoning.rs
@@ -1377,7 +1377,16 @@ fn overlaps_code_region(start: usize, end: usize, regions: &[CodeRegion]) -> boo
 
 /// Return the byte bounds of the line containing `pos`, excluding the trailing newline.
+///
+/// `pos` is clamped to `text.len()` and adjusted to the nearest char boundary,
+/// so callers need not guarantee that `pos` falls on a boundary.
 fn line_bounds(text: &str, pos: usize) -> (usize, usize) {
-    let start = text[..pos].rfind('\n').map_or(0, |idx| idx + 1);
-    let end = text[pos..].find('\n').map_or(text.len(), |idx| pos + idx);
+    let pos = pos.min(text.len());
+    // Walk backward to find a valid char boundary (at most 3 bytes for UTF-8).
+    let mut safe = pos;
+    while safe > 0 && !text.is_char_boundary(safe) {
+        safe -= 1;
+    }
+    let start = text[..safe].rfind('\n').map_or(0, |idx| idx + 1);
+    let end = text[safe..].find('\n').map_or(text.len(), |idx| safe + idx);
     (start, end)
 }
@@ -2303,4 +2312,49 @@ That's my plan."#;
     }
 
+    // ---- line_bounds UTF-8 safety (issue #1669) ----
+
+    #[test]
+    fn test_line_bounds_ascii() {
+        let text = "hello\nworld\n";
+        assert_eq!(line_bounds(text, 0), (0, 5));
+        assert_eq!(line_bounds(text, 6), (6, 11));
+    }
+
+    #[test]
+    fn test_line_bounds_at_text_len() {
+        let text = "abc";
+        assert_eq!(line_bounds(text, 3), (0, 3));
+    }
+
+    #[test]
+    fn test_line_bounds_mid_multibyte_char() {
+        // '🔥' is 4 bytes (F0 9F 94 A5). Passing pos=1 lands inside the char.
+        // line_bounds must not panic — it should snap to a valid boundary.
+        let text = "🔥\n<tool_call>";
+        // All mid-char positions should snap back to byte 0 (start of '🔥'),
+        // so line bounds cover the first line: "🔥" = bytes 0..4.
+        assert_eq!(line_bounds(text, 1), (0, 4)); // would panic before fix
+        assert_eq!(line_bounds(text, 2), (0, 4));
+        assert_eq!(line_bounds(text, 3), (0, 4));
+    }
+
+    #[test]
+    fn test_line_bounds_emoji_before_newline() {
+        // 'Result: 🔥\n<tool_call>' — end.saturating_sub(1) from the \n position
+        // should not panic even with multi-byte chars on the same line.
+        let text = "Result: 🔥\n<tool_call>";
+        let newline_pos = text.find('\n').unwrap();
+        // saturating_sub(1) lands inside '🔥' (byte 11 → 10, but char ends at 12).
+        // Snaps back to byte 8 (start of '🔥'), line covers "Result: 🔥" = bytes 0..12.
+        assert_eq!(line_bounds(text, newline_pos.saturating_sub(1)), (0, 12));
+    }
+
+    #[test]
+    fn test_line_bounds_pos_beyond_len() {
+        let text = "abc";
+        // pos > text.len() should be clamped, not panic
+        assert_eq!(line_bounds(text, 100), (0, 3));
+    }
+
     // ---- recover_tool_calls_from_content tests ----
 
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
