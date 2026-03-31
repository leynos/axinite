# Treat empty url param as absent when installing skills

## Summary

- Source commit: `3f6d2ab6c2c7e47fe5b3c6761a491fd4cd54a5cc`
- Source date: `2026-03-15`
- Severity: `medium`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow tool runtime.

## What the upstream commit addressed

Upstream commit `3f6d2ab6c2c7e47fe5b3c6761a491fd4cd54a5cc` (`fix(skill): treat
empty url param as absent when installing skills (#1128)`) addresses treat empty
url param as absent when installing skills.

Changed upstream paths:

- src/tools/builtin/skill_tools.rs

Upstream stats:

```text
 src/tools/builtin/skill_tools.rs | 25 ++++++++++++++++++++++++-
 1 file changed, 24 insertions(+), 1 deletion(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow tool runtime.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow tool runtime) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/tools/builtin/skill_tools.rs b/src/tools/builtin/skill_tools.rs
index a7581ac4..457f1613 100644
--- a/src/tools/builtin/skill_tools.rs
+++ b/src/tools/builtin/skill_tools.rs
@@ -302,5 +302,9 @@ impl Tool for SkillInstallTool {
             // Direct content provided
             raw.to_string()
-        } else if let Some(url) = params.get("url").and_then(|v| v.as_str()) {
+        } else if let Some(url) = params
+            .get("url")
+            .and_then(|v| v.as_str())
+            .filter(|s| !s.is_empty())
+        {
             // Fetch from explicit URL
             fetch_skill_content(url).await?
@@ -1298,3 +1302,22 @@ mod tests {
         }
     }
+
+    #[test]
+    fn test_empty_url_param_is_treated_as_absent() {
+        // LLMs sometimes pass "" for optional parameters instead of omitting them.
+        // Before the fix, url: "" would match Some("") and attempt to fetch from an
+        // empty URL (failing with an invalid URL error) instead of falling through to
+        // the catalog lookup. The full execute path cannot be tested here without a
+        // real catalog and database, so this test verifies the parameter filtering
+        // behaviour directly.
+        let params = serde_json::json!({"name": "my-skill", "url": ""});
+        let url = params
+            .get("url")
+            .and_then(|v| v.as_str())
+            .filter(|s| !s.is_empty());
+        assert!(
+            url.is_none(),
+            "empty url string should be treated as absent"
+        );
+    }
 }
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
