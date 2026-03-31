# Preserve thought signatures on all tool calls

## Summary

- Source commit: `86389dab23ef4c49c56caf8eb0e9da451d916798`
- Source date: `2026-03-29`
- Severity: `medium`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow LLM stack.

## What the upstream commit addressed

Upstream commit `86389dab23ef4c49c56caf8eb0e9da451d916798` (`fix(gemini):
preserve thought signatures on all tool calls (#1565)`) addresses preserve
thought signatures on all tool calls.

Changed upstream paths:

- src/llm/gemini_oauth.rs

Upstream stats:

```text
 src/llm/gemini_oauth.rs | 35 +++++++++++++++++++++++++++++++++--
 1 file changed, 33 insertions(+), 2 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow LLM stack.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow LLM stack) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/llm/gemini_oauth.rs b/src/llm/gemini_oauth.rs
index a19eec12..a12e5f82 100644
--- a/src/llm/gemini_oauth.rs
+++ b/src/llm/gemini_oauth.rs
@@ -973,5 +973,5 @@ impl GeminiOauthProvider {
         };
 
-        // For each model turn in the active loop, ensure the first functionCall has a thoughtSignature.
+        // For each model turn in the active loop, ensure functionCall parts have a thoughtSignature.
         for item in contents.iter_mut().skip(start) {
             let is_model = item.get("role").and_then(|r| r.as_str()) == Some("model");
@@ -993,5 +993,4 @@ impl GeminiOauthProvider {
                         }
                         modified = true;
-                        break; // Only the first functionCall
                     }
                 }
@@ -2584,3 +2583,35 @@ mod tests {
         assert_eq!(curated[1]["parts"][0]["text"], "again");
     }
+
+    #[test]
+    fn test_ensure_thought_signatures_adds_signatures_to_all_function_calls() {
+        let mut contents = vec![
+            serde_json::json!({
+                "role": "user",
+                "parts": [{ "text": "call tools" }]
+            }),
+            serde_json::json!({
+                "role": "model",
+                "parts": [
+                    { "functionCall": { "name": "memory_write", "args": { "key": "a" } } },
+                    { "functionCall": { "name": "memory_write", "args": { "key": "b" } } }
+                ]
+            }),
+        ];
+
+        GeminiOauthProvider::ensure_thought_signatures(&mut contents);
+
+        let parts = contents[1]
+            .get("parts")
+            .and_then(|p| p.as_array())
+            .expect("model turn should have parts");
+
+        let signed_calls = parts
+            .iter()
+            .filter(|part| part.get("functionCall").is_some())
+            .filter(|part| part.get("thoughtSignature").is_some())
+            .count();
+
+        assert_eq!(signed_calls, 2); // safety: test-only assertion
+    }
 }
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
