# Run cron checks immediately on ticker startup

## Summary

- Source commit: `7a9cbb3b504c82eb6456b20b1c339734fc2f93f2`
- Source date: `2026-03-12`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate agent runtime, tool runtime.

## What the upstream commit addressed

Upstream commit `7a9cbb3b504c82eb6456b20b1c339734fc2f93f2` (`fix(routines): run
cron checks immediately on ticker startup (#1066)`) addresses run cron checks
immediately on ticker startup.

Changed upstream paths:

- src/agent/routine_engine.rs
- src/tools/builtin/memory.rs

Upstream stats:

```text
 src/agent/routine_engine.rs | 13 +++++++++++--
 src/tools/builtin/memory.rs | 20 ++++++++++++++++++++
 2 files changed, 31 insertions(+), 2 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate agent runtime,
tool runtime.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate agent runtime, tool runtime) means the fix
  could touch more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/agent/routine_engine.rs b/src/agent/routine_engine.rs
index a973437a..b4aa5e0c 100644
--- a/src/agent/routine_engine.rs
+++ b/src/agent/routine_engine.rs
@@ -1151,7 +1151,9 @@ pub fn spawn_cron_ticker(
 ) -> tokio::task::JoinHandle<()> {
     tokio::spawn(async move {
+        // Run one check immediately so routines due at startup don't wait
+        // an extra full polling interval.
+        engine.check_cron_triggers().await;
+
         let mut ticker = tokio::time::interval(interval);
-        // Skip immediate first tick
-        ticker.tick().await;
 
         loop {
@@ -1359,3 +1361,10 @@ mod tests {
         assert_eq!(finish_reason_stop, crate::llm::FinishReason::Stop);
     }
+
+    #[test]
+    fn test_truncate_adds_ellipsis_when_over_limit() {
+        let input = "abcdefghijk";
+        let out = super::truncate(input, 5);
+        assert_eq!(out, "abcde...");
+    }
 }
diff --git a/src/tools/builtin/memory.rs b/src/tools/builtin/memory.rs
index de04575b..c8f5178f 100644
--- a/src/tools/builtin/memory.rs
+++ b/src/tools/builtin/memory.rs
@@ -637,2 +637,22 @@ mod tests {
     }
 }
+
+#[cfg(test)]
+mod path_routing_tests {
+    use super::looks_like_filesystem_path;
+
+    #[test]
+    fn detects_filesystem_paths() {
+        assert!(looks_like_filesystem_path("/Users/nige/file.md"));
+        assert!(looks_like_filesystem_path("C:\\Users\\nige\\file.md"));
+        assert!(looks_like_filesystem_path("D:/work/file.md"));
+        assert!(looks_like_filesystem_path("~/notes.md"));
+    }
+
+    #[test]
+    fn allows_workspace_memory_paths() {
+        assert!(!looks_like_filesystem_path("MEMORY.md"));
+        assert!(!looks_like_filesystem_path("daily/2026-03-11.md"));
+        assert!(!looks_like_filesystem_path("projects/alpha/notes.md"));
+    }
+}
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
