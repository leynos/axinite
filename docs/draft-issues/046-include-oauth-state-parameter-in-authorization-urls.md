# Include OAuth state parameter in authorization URLs

## Summary

- Source commit: `4faf81ab612eeecb2f955416ea205b7c91b95867`
- Source date: `2026-03-12`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow src.

## What the upstream commit addressed

Upstream commit `4faf81ab612eeecb2f955416ea205b7c91b95867` (`fix(mcp): include
OAuth state parameter in authorization URLs (#1049)`) addresses include oauth
state parameter in authorization urls.

Changed upstream paths:

- src/tools/mcp/auth.rs

Upstream stats:

```text
 src/tools/mcp/auth.rs | 75 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--
 1 file changed, 73 insertions(+), 2 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow src.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow src) means the fix could touch more behaviour
  than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/tools/mcp/auth.rs b/src/tools/mcp/auth.rs
index 2e483b60..a91cb8fc 100644
--- a/src/tools/mcp/auth.rs
+++ b/src/tools/mcp/auth.rs
@@ -670,5 +670,5 @@ pub async fn authorize_mcp_server(
 
     // Determine client_id and endpoints
-    let (client_id, authorization_url, token_url, use_pkce, scopes, extra_params) =
+    let (client_id, authorization_url, token_url, use_pkce, scopes, mut extra_params) =
         if let Some(oauth) = &server_config.oauth {
             // Pre-configured OAuth
@@ -712,4 +712,11 @@ pub async fn authorize_mcp_server(
     };
 
+    // Generate OAuth state parameter. While optional in OAuth 2.1 with PKCE,
+    // some MCP servers (e.g. Attio) require it.
+    let mut state_bytes = [0u8; 16];
+    rand::rngs::OsRng.fill_bytes(&mut state_bytes);
+    let state = URL_SAFE_NO_PAD.encode(state_bytes);
+    extra_params.insert("state".to_string(), state);
+
     // Compute canonical resource URI for RFC 8707
     let resource = canonical_resource_uri(&server_config.url);
@@ -742,5 +749,8 @@ pub async fn authorize_mcp_server(
     println!("  Waiting for authorization...");
 
-    // Wait for callback
+    // Wait for callback. State is sent in the URL for servers that require it
+    // (e.g. Attio), but we don't enforce validation on the callback because MCP
+    // servers use PKCE which already binds the request to the token exchange,
+    // and some servers may not echo state back.
     let code = wait_for_authorization_callback(listener, &server_config.name).await?;
 
@@ -1712,3 +1722,64 @@ mod tests {
         assert!(!url.contains("resource="));
     }
+
+    /// Regression test: MCP OAuth authorization URLs must include a `state`
+    /// parameter. While OAuth 2.1 makes `state` optional when PKCE is used,
+    /// some MCP servers (e.g. Attio) require it and reject requests without it:
+    /// {"error":"invalid_request","error_description":"Invalid value provided
+    /// for: state"}
+    ///
+    /// Including `state` is harmless for servers that don't require it, since
+    /// it is a standard OAuth parameter that compliant servers will echo back
+    /// or ignore.
+    ///
+    /// The state is generated in `authorize_mcp_server` and injected into
+    /// `extra_params` before `build_authorization_url` is called. This test
+    /// verifies that `build_authorization_url` correctly propagates state from
+    /// extra_params into the URL, and that each generated state is unique.
+    #[test]
+    fn test_authorization_url_includes_state_parameter() {
+        // Simulate what authorize_mcp_server does: generate state and
+        // insert it into extra_params.
+        let mut extra_params = HashMap::new();
+        let mut state_bytes = [0u8; 16];
+        rand::rngs::OsRng.fill_bytes(&mut state_bytes);
+        let state = URL_SAFE_NO_PAD.encode(state_bytes);
+        extra_params.insert("state".to_string(), state.clone());
+
+        let pkce = PkceChallenge::generate();
+        let url = build_authorization_url(
+            "https://app.attio.com/oidc/authorize",
+            "test-client",
+            "http://127.0.0.1:9876/callback",
+            &["mcp".to_string(), "offline_access".to_string(), "openid".to_string()],
+            Some(&pkce),
+            &extra_params,
+            Some("https://mcp.attio.com/mcp"),
+        );
+
+        // State must be present in the URL
+        assert!(
+            url.contains(&format!("state={}", state)),
+            "Authorization URL must include the state parameter, got: {}",
+            url,
+        );
+
+        // State must be base64url-encoded (no padding, no +/)
+        assert!(!state.contains('+'), "State must be base64url-safe");
+        assert!(!state.contains('/'), "State must be base64url-safe");
+        assert!(!state.contains('='), "State must not have padding");
+
+        // State must have sufficient entropy (16 bytes -> 22 base64url chars)
+        assert!(
+            state.len() >= 22,
+            "State must have at least 128 bits of entropy, got {} chars",
+            state.len(),
+        );
+
+        // Two generated states must differ
+        let mut state_bytes_2 = [0u8; 16];
+        rand::rngs::OsRng.fill_bytes(&mut state_bytes_2);
+        let state_2 = URL_SAFE_NO_PAD.encode(state_bytes_2);
+        assert_ne!(state, state_2, "State must be unique per request");
+    }
 }
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
