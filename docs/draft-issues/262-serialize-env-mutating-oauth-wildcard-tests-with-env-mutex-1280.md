# Serialize env-mutating OAuth wildcard tests with ENV_MUTEX (#1280)

## Summary

- Source commit: `c6d4abdb31b4f2e19b2149836d3ef1cb4a11ce35`
- Source date: `2026-03-20`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow LLM stack.

## What the upstream commit addressed

Upstream commit `c6d4abdb31b4f2e19b2149836d3ef1cb4a11ce35` (`fix(ci): serialize
env-mutating OAuth wildcard tests with ENV_MUTEX (#1280) (#1468)`) addresses
serialize env-mutating oauth wildcard tests with env_mutex (#1280).

Changed upstream paths:

- src/llm/oauth_helpers.rs

Upstream stats:

```text
 src/llm/oauth_helpers.rs | 4 ++--
 1 file changed, 2 insertions(+), 2 deletions(-)
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
diff --git a/src/llm/oauth_helpers.rs b/src/llm/oauth_helpers.rs
index 2fd97c55..2881e60e 100644
--- a/src/llm/oauth_helpers.rs
+++ b/src/llm/oauth_helpers.rs
@@ -391,5 +391,5 @@ mod tests {
     #[tokio::test]
     async fn bind_rejects_wildcard_ipv4() {
-        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
+        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
         let original = std::env::var("OAUTH_CALLBACK_HOST").ok();
         // SAFETY: Under ENV_MUTEX, no concurrent env access.
@@ -415,5 +415,5 @@ mod tests {
     #[tokio::test]
     async fn bind_rejects_wildcard_ipv6() {
-        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
+        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
         let original = std::env::var("OAUTH_CALLBACK_HOST").ok();
         // SAFETY: Under ENV_MUTEX, no concurrent env access.
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
