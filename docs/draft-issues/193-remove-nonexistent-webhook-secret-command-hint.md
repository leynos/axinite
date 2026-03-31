# Remove nonexistent webhook secret command hint

## Summary

- Source commit: `e9b0823db90f3229ca4a064ef0f1ae799e9bf6db`
- Source date: `2026-03-18`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow src.

## What the upstream commit addressed

Upstream commit `e9b0823db90f3229ca4a064ef0f1ae799e9bf6db` (`fix(setup): remove
nonexistent webhook secret command hint (#1349)`) addresses remove nonexistent
webhook secret command hint.

Changed upstream paths:

- src/setup/channels.rs

Upstream stats:

```text
 src/setup/channels.rs | 19 ++++++++++++++++---
 1 file changed, 16 insertions(+), 3 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow src.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow src) means the fix could touch more behaviour
  than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/setup/channels.rs b/src/setup/channels.rs
index 1c184b0b..2612076d 100644
--- a/src/setup/channels.rs
+++ b/src/setup/channels.rs
@@ -519,5 +519,5 @@ pub async fn setup_http(secrets: &SecretsContext) -> Result<HttpSetupResult, Cha
             .await?;
         print_success("Webhook secret generated and saved to database");
-        print_info("Retrieve it later with: ironclaw secret get http_webhook_secret");
+        print_info(http_webhook_secret_hint());
     }
 
@@ -536,4 +536,8 @@ pub fn generate_webhook_secret() -> String {
 }
 
+fn http_webhook_secret_hint() -> &'static str {
+    "The secret is stored in the encrypted secrets database and will be loaded automatically on startup."
+}
+
 fn validate_e164(account: &str) -> Result<(), String> {
     if !account.starts_with('+') {
@@ -1137,6 +1141,7 @@ mod tests {
     use crate::secrets::{InMemorySecretsStore, SecretsCrypto, SecretsStore};
     use crate::setup::channels::{
-        SecretsContext, generate_webhook_secret, substitute_validation_placeholders,
-        validate_cloudflare_token_format, validate_public_https_url,
+        SecretsContext, generate_webhook_secret, http_webhook_secret_hint,
+        substitute_validation_placeholders, validate_cloudflare_token_format,
+        validate_public_https_url,
     };
 
@@ -1338,3 +1343,11 @@ mod tests {
         assert!(err.contains("DNS resolution failed"));
     }
+
+    #[test]
+    fn test_http_webhook_secret_hint_reflects_current_behavior() {
+        let hint = http_webhook_secret_hint();
+        assert!(hint.contains("encrypted secrets database"));
+        assert!(hint.contains("loaded automatically on startup"));
+        assert!(!hint.contains("ironclaw secret get"));
+    }
 }
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
