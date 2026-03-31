# Persist refreshed Anthropic OAuth token after Keychain re-read

## Summary

- Source commit: `9e41b8acea49f38b0414d3f7955f69e8e204a0e5`
- Source date: `2026-03-16`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow LLM stack.

## What the upstream commit addressed

Upstream commit `9e41b8acea49f38b0414d3f7955f69e8e204a0e5` (`fix(llm): persist
refreshed Anthropic OAuth token after Keychain re-read (#1213)`) addresses
persist refreshed anthropic oauth token after keychain re-read.

Changed upstream paths:

- src/llm/anthropic_oauth.rs

Upstream stats:

```text
 src/llm/anthropic_oauth.rs | 52 +++++++++++++++++++++++++++++++++++++++++++++++++---
 1 file changed, 49 insertions(+), 3 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow LLM stack.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow LLM stack) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/llm/anthropic_oauth.rs b/src/llm/anthropic_oauth.rs
index 12ca223c..12c527f1 100644
--- a/src/llm/anthropic_oauth.rs
+++ b/src/llm/anthropic_oauth.rs
@@ -35,5 +35,7 @@ const DEFAULT_MAX_TOKENS: u32 = 8192;
 pub struct AnthropicOAuthProvider {
     client: Client,
-    token: SecretString,
+    /// OAuth token, wrapped in RwLock so it can be updated after a successful
+    /// Keychain refresh (fixes #1136: stale token reuse after expiry).
+    token: std::sync::RwLock<SecretString>,
     model: String,
     base_url: Option<String>,
@@ -72,5 +74,5 @@ impl AnthropicOAuthProvider {
         Ok(Self {
             client,
-            token,
+            token: std::sync::RwLock::new(token),
             model: config.model.clone(),
             base_url,
@@ -99,4 +101,20 @@ impl AnthropicOAuthProvider {
     }
 
+    /// Read the current token from the RwLock.
+    fn current_token(&self) -> String {
+        match self.token.read() {
+            Ok(guard) => guard.expose_secret().to_string(),
+            Err(poisoned) => poisoned.into_inner().expose_secret().to_string(),
+        }
+    }
+
+    /// Update the stored token after a successful Keychain refresh.
+    fn update_token(&self, new_token: SecretString) {
+        match self.token.write() {
+            Ok(mut guard) => *guard = new_token,
+            Err(poisoned) => *poisoned.into_inner() = new_token,
+        }
+    }
+
     async fn send_request<R: for<'de> Deserialize<'de>>(
         &self,
@@ -110,5 +128,5 @@ impl AnthropicOAuthProvider {
             .client
             .post(&url)
-            .bearer_auth(self.token.expose_secret())
+            .bearer_auth(self.current_token())
             .header("anthropic-version", ANTHROPIC_API_VERSION)
             .header("anthropic-beta", ANTHROPIC_OAUTH_BETA)
@@ -142,4 +160,9 @@ impl AnthropicOAuthProvider {
                 // to re-extract a fresh token from the OS credential store
                 // (macOS Keychain / Linux credentials file) before giving up.
+                //
+                // Brief delay to give Claude Code time to complete its async
+                // Keychain refresh write (fixes race in #1136).
+                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
+
                 if let Some(fresh) = crate::config::ClaudeCodeConfig::extract_oauth_token() {
                     let fresh_token = SecretString::from(fresh);
@@ -160,4 +183,9 @@ impl AnthropicOAuthProvider {
                         })?;
                     if retry.status().is_success() {
+                        // Persist the refreshed token so subsequent requests
+                        // don't hit 401 again (fixes #1136).
+                        self.update_token(fresh_token);
+                        tracing::info!("Anthropic OAuth token refreshed from credential store");
+
                         let text = retry.text().await.map_err(|e| LlmError::RequestFailed {
                             provider: "anthropic_oauth".to_string(),
@@ -660,3 +688,21 @@ mod tests {
         assert_eq!(tool_calls[0].name, "search");
     }
+
+    /// Regression test for #1136: token field must be mutable via RwLock
+    /// so that a refreshed token persists across subsequent requests.
+    #[test]
+    fn test_token_update_persists() {
+        let original = SecretString::from("old_token".to_string());
+        let token = std::sync::RwLock::new(original);
+
+        // Read the original
+        assert_eq!(token.read().unwrap().expose_secret(), "old_token");
+
+        // Simulate a successful refresh
+        let refreshed = SecretString::from("new_token".to_string());
+        *token.write().unwrap() = refreshed;
+
+        // Subsequent reads see the updated token
+        assert_eq!(token.read().unwrap().expose_secret(), "new_token");
+    }
 }
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
