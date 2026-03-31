# Preserve polling after secret-blocked updates

## Summary

- Source commit: `33a2dd2c78b25b3f333b9924ae7186bf637ac83f`
- Source date: `2026-03-18`
- Severity: `high`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: narrow WASM channels.

## What the upstream commit addressed

Upstream commit `33a2dd2c78b25b3f333b9924ae7186bf637ac83f` (`fix(telegram):
preserve polling after secret-blocked updates (#1353)`) addresses preserve
polling after secret-blocked updates.

Changed upstream paths:

- src/channels/wasm/wrapper.rs

Upstream stats:

```text
 src/channels/wasm/wrapper.rs | 41 +++++++++++++++++++++++++++++++++++++++--
 1 file changed, 39 insertions(+), 2 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow WASM channels.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow WASM channels) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/channels/wasm/wrapper.rs b/src/channels/wasm/wrapper.rs
index 6ca79831..65f978ac 100644
--- a/src/channels/wasm/wrapper.rs
+++ b/src/channels/wasm/wrapper.rs
@@ -493,6 +493,14 @@ impl near::agent::channel_host::Host for ChannelStoreData {
             }
 
-            // Leak detection on response body (best-effort)
-            if let Ok(body_str) = std::str::from_utf8(&body) {
+            // Leak detection on response body (best-effort).
+            //
+            // Telegram `getUpdates` is special: it is inbound polling data, so
+            // user-pasted secrets can legitimately appear in the response body.
+            // Those messages are still checked later by the inbound message
+            // safety layer before they reach the LLM, so we allow the polling
+            // response to continue here to avoid poisoning the offset state.
+            if let Ok(body_str) = std::str::from_utf8(&body)
+                && !should_skip_response_leak_scan(&url)
+            {
                 leak_detector
                     .scan_and_clean(body_str)
@@ -3123,4 +3131,17 @@ fn extract_host_from_url(url: &str) -> Option<String> {
 }
 
+fn should_skip_response_leak_scan(url: &str) -> bool {
+    url::Url::parse(url).is_ok_and(|parsed| {
+        matches!(parsed.scheme(), "http" | "https")
+            && parsed
+                .host_str()
+                .is_some_and(|host| host.eq_ignore_ascii_case("api.telegram.org"))
+            && parsed
+                .path_segments()
+                .and_then(|segments| segments.rev().find(|segment| !segment.is_empty()))
+                .is_some_and(|segment| segment == "getUpdates")
+    })
+}
+
 /// Pre-resolve host credentials for all HTTP capability mappings.
 ///
@@ -4387,4 +4408,20 @@ mod tests {
     }
 
+    #[test]
+    fn test_should_skip_response_leak_scan_only_for_telegram_getupdates() {
+        use super::should_skip_response_leak_scan;
+
+        assert!(should_skip_response_leak_scan(
+            "https://api.telegram.org/bot123/getUpdates?offset=1"
+        ));
+        assert!(!should_skip_response_leak_scan(
+            "https://api.telegram.org/bot123/sendMessage"
+        ));
+        assert!(!should_skip_response_leak_scan(
+            "https://api.example.com/getUpdates"
+        ));
+        assert!(!should_skip_response_leak_scan("not a url"));
+    }
+
     /// Verify that WASM HTTP host functions work using a dedicated
     /// current-thread runtime inside spawn_blocking.
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
