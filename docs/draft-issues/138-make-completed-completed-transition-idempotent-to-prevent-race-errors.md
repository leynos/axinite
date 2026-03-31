# Make completed->completed transition idempotent to prevent race errors

## Summary

- Source commit: `596d17f04b2780cea26824f92a25904a5d97339f`
- Source date: `2026-03-16`
- Severity: `medium-high`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate src, sandbox/worker.

## What the upstream commit addressed

Upstream commit `596d17f04b2780cea26824f92a25904a5d97339f` (`fix(jobs): make
completed->completed transition idempotent to prevent race errors (#1068)`)
addresses make completed->completed transition idempotent to prevent race
errors.

Changed upstream paths:

- src/context/state.rs
- src/worker/job.rs

Upstream stats:

```text
 src/context/state.rs | 59 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/worker/job.rs    | 17 ++++++++++++++---
 2 files changed, 73 insertions(+), 3 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium-high` severity. That keeps
the underlying failure mode close enough to Axinite's current runtime to justify
a follow-up review. The recorded blast radius upstream was moderate src,
sandbox/worker.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate src, sandbox/worker) means the fix could touch
  more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/context/state.rs b/src/context/state.rs
index 22aca311..768e4da6 100644
--- a/src/context/state.rs
+++ b/src/context/state.rs
@@ -49,4 +49,12 @@ impl JobState {
         use JobState::*;
 
+        // Allow idempotent Completed -> Completed transition.
+        // Both the execution loop and the worker wrapper may race to mark a
+        // job complete; the second call should be a harmless no-op rather
+        // than an error that masks the successful completion.
+        if matches!((self, target), (Completed, Completed)) {
+            return true;
+        }
+
         matches!(
             (self, target),
@@ -239,4 +247,16 @@ impl JobContext {
         }
 
+        // Idempotent: already in the target state, skip recording a duplicate
+        // transition. This handles the Completed -> Completed race between
+        // execution_loop and the worker wrapper.
+        if self.state == new_state {
+            tracing::debug!(
+                job_id = %self.job_id,
+                state = %self.state,
+                "idempotent state transition (already in target state), skipping"
+            );
+            return Ok(());
+        }
+
         let transition = StateTransition {
             from: self.state,
@@ -341,4 +361,43 @@ mod tests {
     }
 
+    #[test]
+    fn test_completed_to_completed_is_idempotent() {
+        // Regression test for the race condition where both execution_loop
+        // and the worker wrapper call mark_completed(). The second call
+        // must succeed without error and must not record a duplicate
+        // transition.
+        let mut ctx = JobContext::new("Test", "Idempotent completion test");
+        ctx.transition_to(JobState::InProgress, None).unwrap();
+        ctx.transition_to(JobState::Completed, Some("first".into()))
+            .unwrap();
+        assert_eq!(ctx.state, JobState::Completed);
+        let transitions_before = ctx.transitions.len();
+
+        // Second Completed -> Completed must be a no-op
+        let result = ctx.transition_to(JobState::Completed, Some("duplicate".into()));
+        assert!(
+            result.is_ok(),
+            "Completed -> Completed should be idempotent"
+        );
+        assert_eq!(ctx.state, JobState::Completed);
+        assert_eq!(
+            ctx.transitions.len(),
+            transitions_before,
+            "idempotent transition should not record a new history entry"
+        );
+    }
+
+    #[test]
+    fn test_other_self_transitions_still_rejected() {
+        // Ensure we only allow Completed -> Completed, not arbitrary X -> X.
+        assert!(!JobState::Pending.can_transition_to(JobState::Pending));
+        assert!(!JobState::InProgress.can_transition_to(JobState::InProgress));
+        assert!(!JobState::Failed.can_transition_to(JobState::Failed));
+        assert!(!JobState::Stuck.can_transition_to(JobState::Stuck));
+        assert!(!JobState::Submitted.can_transition_to(JobState::Submitted));
+        assert!(!JobState::Accepted.can_transition_to(JobState::Accepted));
+        assert!(!JobState::Cancelled.can_transition_to(JobState::Cancelled));
+    }
+
     #[test]
     fn test_terminal_states() {
diff --git a/src/worker/job.rs b/src/worker/job.rs
index c6c555db..0f0e969e 100644
--- a/src/worker/job.rs
+++ b/src/worker/job.rs
@@ -1592,5 +1592,5 @@ mod tests {
 
     #[tokio::test]
-    async fn test_mark_completed_twice_returns_error() {
+    async fn test_mark_completed_twice_is_idempotent() {
         let worker = make_worker(vec![]).await;
 
@@ -1613,9 +1613,20 @@ mod tests {
         assert_eq!(ctx.state, JobState::Completed);
 
+        // Second mark_completed should succeed (idempotent) rather than
+        // erroring, matching the fix for the execution_loop / worker wrapper
+        // race condition.
         let result = worker.mark_completed().await;
         assert!(
-            result.is_err(),
-            "Completed → Completed transition should be rejected by state machine"
+            result.is_ok(),
+            "Completed -> Completed transition should be idempotent"
         );
+
+        // State should still be Completed
+        let ctx = worker
+            .context_manager()
+            .get_context(worker.job_id)
+            .await
+            .unwrap();
+        assert_eq!(ctx.state, JobState::Completed);
     }
 
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
