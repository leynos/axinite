# Prevent UTF-8 panics in byte-index string truncation

## Summary

- Source commit: `64fe9ba6077e3478e2684daf97d43556aff6996d`
- Source date: `2026-03-29`
- Severity: `medium`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow string-handling helpers with potentially broad
  runtime reach. UTF-8 panic fixes are low-cost defensive carry items.

## What the upstream commit addressed

Upstream commit `64fe9ba6077e3478e2684daf97d43556aff6996d` (`fix: prevent UTF-8
panics in byte-index string truncation (#1688)`) addresses prevent utf-8 panics
in byte-index string truncation.

Changed upstream paths:

- src/cli/config.rs
- src/cli/memory.rs
- src/llm/nearai_chat.rs

Upstream stats:

```text
 src/cli/config.rs      |  3 ++-
 src/cli/memory.rs      | 16 +++++++++++++++-
 src/llm/nearai_chat.rs |  2 +-
 3 files changed, 18 insertions(+), 3 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow string-handling
helpers with potentially broad runtime reach. UTF-8 panic fixes are low-cost
defensive carry items.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow string-handling helpers with potentially broad
  runtime reach. UTF-8 panic fixes are low-cost defensive carry items) means the
  fix could touch more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/cli/config.rs b/src/cli/config.rs
index fc1312f6..f1e107ec 100644
--- a/src/cli/config.rs
+++ b/src/cli/config.rs
@@ -128,5 +128,6 @@ async fn list_settings(
 
         let display_value = if value.len() > 60 {
-            format!("{}...", &value[..57])
+            let end = crate::util::floor_char_boundary(&value, 57);
+            format!("{}...", &value[..end])
         } else {
             value
diff --git a/src/cli/memory.rs b/src/cli/memory.rs
index 2d0606a8..fca6d03b 100644
--- a/src/cli/memory.rs
+++ b/src/cli/memory.rs
@@ -257,5 +257,6 @@ fn truncate_content(s: &str, max_len: usize) -> String {
         s.to_string()
     } else {
-        format!("{}...", &s[..max_len])
+        let end = crate::util::floor_char_boundary(s, max_len);
+        format!("{}...", &s[..end])
     }
 }
@@ -293,3 +294,16 @@ mod tests {
         assert_eq!(truncate_content("hello world", 5), "hello...");
     }
+
+    #[test]
+    fn test_truncate_content_multibyte_does_not_panic() {
+        // \u{00e9} is precomposed 'é' (2 bytes in UTF-8)
+        let s = "caf\u{00e9} au lait"; // "café au lait", é starts at byte 3
+        let result = truncate_content(s, 4); // byte 4 is inside 2-byte é
+        assert_eq!(result, "caf...");
+
+        // 4-byte emoji: slicing mid-emoji must not panic
+        let emoji = "Hi \u{1F600} there"; // 😀 is 4 bytes, starts at byte 3
+        let result = truncate_content(emoji, 4); // byte 4 is inside 😀
+        assert_eq!(result, "Hi ...");
+    }
 }
diff --git a/src/llm/nearai_chat.rs b/src/llm/nearai_chat.rs
index 80335d86..26807c99 100644
--- a/src/llm/nearai_chat.rs
+++ b/src/llm/nearai_chat.rs
@@ -452,5 +452,5 @@ impl NearAiChatProvider {
             reason: format!(
                 "No model names found in response: {}",
-                &response_text[..response_text.len().min(300)]
+                &response_text[..crate::util::floor_char_boundary(&response_text, 300)]
             ),
         })
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
