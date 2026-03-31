# Skip NEAR AI session check when backend is not nearai

## Summary

- Source commit: `71f9012de37f663ce967cd1068ef7f381b287a56`
- Source date: `2026-03-19`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: narrow src.

## What the upstream commit addressed

Upstream commit `71f9012de37f663ce967cd1068ef7f381b287a56` (`fix: skip NEAR AI
session check when backend is not nearai (#1413)`) addresses skip near ai
session check when backend is not nearai.

Changed upstream paths:

- src/cli/doctor.rs

Upstream stats:

```text
 src/cli/doctor.rs | 62 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++---
 1 file changed, 59 insertions(+), 3 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow src.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow src) means the fix could touch more behaviour
  than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/cli/doctor.rs b/src/cli/doctor.rs
index dfc04de7..7510635a 100644
--- a/src/cli/doctor.rs
+++ b/src/cli/doctor.rs
@@ -34,5 +34,5 @@ pub async fn run_doctor_command() -> anyhow::Result<()> {
     check(
         "NEAR AI session",
-        check_nearai_session().await,
+        check_nearai_session(&settings).await,
         &mut passed,
         &mut failed,
@@ -216,5 +216,20 @@ fn check_settings_file() -> CheckResult {
 // ── NEAR AI session ─────────────────────────────────────────
 
-async fn check_nearai_session() -> CheckResult {
+async fn check_nearai_session(settings: &Settings) -> CheckResult {
+    // Skip entirely when the configured backend is not NEAR AI.
+    let llm_config = match crate::config::LlmConfig::resolve(settings) {
+        Ok(config) => config,
+        Err(e) => {
+            // check_llm_config will report the full error; just skip here.
+            return CheckResult::Skip(format!("LLM config error: {e}"));
+        }
+    };
+    if llm_config.backend != "nearai" {
+        return CheckResult::Skip(format!(
+            "not using NEAR AI backend (backend={})",
+            llm_config.backend
+        ));
+    }
+
     // Check if session file exists
     let session_path = crate::config::llm::default_session_path();
@@ -621,5 +636,6 @@ mod tests {
     #[tokio::test]
     async fn check_nearai_session_does_not_panic() {
-        let result = check_nearai_session().await;
+        let settings = Settings::default();
+        let result = check_nearai_session(&settings).await;
         match result {
             CheckResult::Pass(_) | CheckResult::Fail(_) | CheckResult::Skip(_) => {}
@@ -627,4 +643,44 @@ mod tests {
     }
 
+    #[test]
+    fn check_nearai_session_skips_for_non_nearai_backend() {
+        struct EnvGuard(&'static str, Option<String>);
+        impl Drop for EnvGuard {
+            fn drop(&mut self) {
+                // SAFETY: Under ENV_MUTEX.
+                unsafe {
+                    match &self.1 {
+                        Some(val) => std::env::set_var(self.0, val),
+                        None => std::env::remove_var(self.0),
+                    }
+                }
+            }
+        }
+
+        let _mutex = crate::config::helpers::ENV_MUTEX.lock().expect("env mutex");
+        let prev = std::env::var("LLM_BACKEND").ok();
+        // SAFETY: Under ENV_MUTEX, no concurrent env access.
+        unsafe {
+            std::env::set_var("LLM_BACKEND", "anthropic");
+        }
+        let _env_guard = EnvGuard("LLM_BACKEND", prev);
+
+        let settings = Settings::default();
+        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
+        let result = rt.block_on(check_nearai_session(&settings));
+        match result {
+            CheckResult::Skip(msg) => {
+                assert!(
+                    msg.contains("backend=anthropic"),
+                    "expected backend name in skip message, got: {msg}"
+                );
+            }
+            other => panic!(
+                "expected Skip for non-nearai backend, got: {}",
+                format_result(&other)
+            ),
+        }
+    }
+
     #[test]
     fn check_settings_file_handles_missing() {
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
