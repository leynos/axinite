# Make approval requests appear without page reload (#996)

## Summary

- Source commit: `e522d33a53866ab62327bb70002930e9509182a2`
- Source date: `2026-03-12`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: narrow web gateway.

## What the upstream commit addressed

Upstream commit `e522d33a53866ab62327bb70002930e9509182a2` (`fix(web): make
approval requests appear without page reload (#996) (#1073)`) addresses make
approval requests appear without page reload (#996).

Changed upstream paths:

- src/channels/web/static/app.js

Upstream stats:

```text
 src/channels/web/static/app.js | 19 +++++++++++++++++--
 1 file changed, 17 insertions(+), 2 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow web gateway.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow web gateway) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/channels/web/static/app.js b/src/channels/web/static/app.js
index 6128f05f..5a55051d 100644
--- a/src/channels/web/static/app.js
+++ b/src/channels/web/static/app.js
@@ -343,6 +343,17 @@ function connectSSE() {
   eventSource.addEventListener('approval_needed', (e) => {
     const data = JSON.parse(e.data);
-    if (!isCurrentThread(data.thread_id)) return;
-    showApproval(data);
+    const hasThread = !!data.thread_id;
+    const forCurrentThread = !hasThread || isCurrentThread(data.thread_id);
+
+    if (forCurrentThread) {
+      showApproval(data);
+    } else {
+      // Keep thread list fresh when approval is requested in a background thread.
+      unreadThreads.set(data.thread_id, (unreadThreads.get(data.thread_id) || 0) + 1);
+      debouncedLoadThreads();
+    }
+
+    // Extension setup flows can surface approvals while user is on Extensions tab.
+    if (currentTab === 'extensions') loadExtensions();
   });
 
@@ -992,4 +1003,8 @@ function finalizeActivityGroup() {
 
 function showApproval(data) {
+  // Avoid duplicate cards on reconnect/history refresh.
+  const existing = document.querySelector('.approval-card[data-request-id="' + CSS.escape(data.request_id) + '"]');
+  if (existing) return;
+
   const container = document.getElementById('chat-messages');
   const card = document.createElement('div');
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
