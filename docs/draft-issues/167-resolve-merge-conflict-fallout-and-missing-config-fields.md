# Resolve merge conflict fallout and missing config fields

## Summary

- Source commit: `fc18064be9e3d9c3ad474f9deccb91a70c06d3e9`
- Source date: `2026-03-16`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate LLM stack, src.

## What the upstream commit addressed

Upstream commit `fc18064be9e3d9c3ad474f9deccb91a70c06d3e9` (`fix: resolve merge
conflict fallout and missing config fields`) addresses resolve merge conflict
fallout and missing config fields.

Changed upstream paths:

- src/llm/mod.rs
- src/llm/models.rs
- src/setup/wizard.rs

Upstream stats:

```text
 src/llm/mod.rs      |  2 +-
 src/llm/models.rs   |  2 ++
 src/setup/wizard.rs | 54 ------------------------------------------------------
 3 files changed, 3 insertions(+), 55 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate LLM stack,
src.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate LLM stack, src) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/llm/mod.rs b/src/llm/mod.rs
index 77102e32..3b6b01c4 100644
--- a/src/llm/mod.rs
+++ b/src/llm/mod.rs
@@ -430,5 +430,5 @@ fn create_cheap_provider_for_backend(
     let mut cheap_reg_config = reg_config.clone();
     cheap_reg_config.model = cheap_model.to_string();
-    let provider = create_registry_provider(&cheap_reg_config)?;
+    let provider = create_registry_provider(&cheap_reg_config, config.request_timeout_secs)?;
     Ok(Some(provider))
 }
diff --git a/src/llm/models.rs b/src/llm/models.rs
index 7022d3cf..daec9df3 100644
--- a/src/llm/models.rs
+++ b/src/llm/models.rs
@@ -346,4 +346,6 @@ pub(crate) fn build_nearai_model_fetch_config() -> crate::config::LlmConfig {
         bedrock: None,
         request_timeout_secs: 120,
+        cheap_model: None,
+        smart_routing_cascade: false,
     }
 }
diff --git a/src/setup/wizard.rs b/src/setup/wizard.rs
index d2f773d2..23494d12 100644
--- a/src/setup/wizard.rs
+++ b/src/setup/wizard.rs
@@ -3100,58 +3100,4 @@ async fn discover_wasm_channels(dir: &std::path::Path) -> Vec<(String, ChannelCa
 ///
 /// Uses char-based indexing to avoid panicking on multi-byte UTF-8.
-/// Build the `LlmConfig` used by `fetch_nearai_models` to list available models.
-///
-/// Reads `NEARAI_API_KEY` from the environment so that users who authenticated
-/// via Cloud API key (option 4) don't get re-prompted during model selection.
-fn build_nearai_model_fetch_config() -> crate::config::LlmConfig {
-    // If the user authenticated via API key (option 4), the key is stored
-    // as an env var. Pass it through so `resolve_bearer_token()` doesn't
-    // re-trigger the interactive auth prompt.
-    let api_key = std::env::var("NEARAI_API_KEY")
-        .ok()
-        .filter(|k| !k.is_empty())
-        .map(secrecy::SecretString::from);
-
-    // Match the same base_url logic as LlmConfig::resolve(): use cloud-api
-    // when an API key is present, private.near.ai for session-token auth.
-    let default_base = if api_key.is_some() {
-        "https://cloud-api.near.ai"
-    } else {
-        "https://private.near.ai"
-    };
-    let base_url = std::env::var("NEARAI_BASE_URL").unwrap_or_else(|_| default_base.to_string());
-    let auth_base_url =
-        std::env::var("NEARAI_AUTH_URL").unwrap_or_else(|_| "https://private.near.ai".to_string());
-
-    crate::config::LlmConfig {
-        backend: "nearai".to_string(),
-        session: crate::llm::session::SessionConfig {
-            auth_base_url,
-            session_path: crate::config::llm::default_session_path(),
-        },
-        nearai: crate::config::NearAiConfig {
-            model: "dummy".to_string(),
-            cheap_model: None,
-            base_url,
-            api_key,
-            fallback_model: None,
-            max_retries: 3,
-            circuit_breaker_threshold: None,
-            circuit_breaker_recovery_secs: 30,
-            response_cache_enabled: false,
-            response_cache_ttl_secs: 3600,
-            response_cache_max_entries: 1000,
-            failover_cooldown_secs: 300,
-            failover_cooldown_threshold: 3,
-            smart_routing_cascade: true,
-        },
-        provider: None,
-        bedrock: None,
-        request_timeout_secs: 120,
-        cheap_model: None,
-        smart_routing_cascade: true,
-    }
-}
-
 fn mask_api_key(key: &str) -> String {
     let chars: Vec<char> = key.chars().collect();
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
