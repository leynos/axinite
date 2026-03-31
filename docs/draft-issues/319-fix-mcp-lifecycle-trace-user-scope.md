# Fix MCP lifecycle trace user scope

## Summary

- Source commit: `c949521d8d153ecb3af30877779f8c160278ca09`
- Source date: `2026-03-25`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow tests.

## What the upstream commit addressed

Upstream commit `c949521d8d153ecb3af30877779f8c160278ca09` (`Fix MCP lifecycle
trace user scope (#1646)`) addresses fix mcp lifecycle trace user scope.

Changed upstream paths:

- tests/e2e_advanced_traces.rs

Upstream stats:

```text
 tests/e2e_advanced_traces.rs | 5 +++--
 1 file changed, 3 insertions(+), 2 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow tests.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow tests) means the fix could touch more behaviour
  than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/tests/e2e_advanced_traces.rs b/tests/e2e_advanced_traces.rs
index b3efc8d9..ce18ad3d 100644
--- a/tests/e2e_advanced_traces.rs
+++ b/tests/e2e_advanced_traces.rs
@@ -588,4 +588,5 @@ mod advanced {
         use crate::support::mock_mcp_server::{MockToolResponse, start_mock_mcp_server};
         use ironclaw::extensions::{AuthHint, ExtensionKind, ExtensionSource, RegistryEntry};
+        const TEST_USER_ID: &str = "test-user";
 
         // 1. Start mock MCP server with pre-configured tool responses.
@@ -655,5 +656,5 @@ mod advanced {
             .secrets()
             .create(
-                "default",
+                TEST_USER_ID,
                 ironclaw::secrets::CreateSecretParams::new(secret_name, "mock-access-token")
                     .with_provider("mcp:mock-notion".to_string()),
@@ -662,5 +663,5 @@ mod advanced {
             .expect("failed to inject test token");
 
-        let activate_result = ext_mgr.activate("mock-notion", "default").await;
+        let activate_result = ext_mgr.activate("mock-notion", TEST_USER_ID).await;
         assert!(
             activate_result.is_ok(),
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
