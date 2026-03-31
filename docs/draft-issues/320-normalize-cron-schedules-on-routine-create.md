# Normalize cron schedules on routine create

## Summary

- Source commit: `ab0ad948f36c7cc88b1aecf2e92dd0ff94569a94`
- Source date: `2026-03-25`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate tool runtime, tests.

## What the upstream commit addressed

Upstream commit `ab0ad948f36c7cc88b1aecf2e92dd0ff94569a94` (`Normalize cron
schedules on routine create (#1648)`) addresses normalize cron schedules on
routine create.

Changed upstream paths:

- src/tools/builtin/routine.rs
- tests/e2e_builtin_tool_coverage.rs

Upstream stats:

```text
 src/tools/builtin/routine.rs       | 16 +++++++++++++++-
 tests/e2e_builtin_tool_coverage.rs |  2 +-
 2 files changed, 16 insertions(+), 2 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate tool runtime,
tests.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate tool runtime, tests) means the fix could touch
  more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/tools/builtin/routine.rs b/src/tools/builtin/routine.rs
index f4313483..bbc24139 100644
--- a/src/tools/builtin/routine.rs
+++ b/src/tools/builtin/routine.rs
@@ -916,5 +916,5 @@ fn build_routine_trigger(trigger: &NormalizedTriggerRequest) -> Trigger {
     match trigger {
         NormalizedTriggerRequest::Cron { schedule, timezone } => Trigger::Cron {
-            schedule: schedule.clone(),
+            schedule: normalize_cron_expression(schedule),
             timezone: timezone.clone(),
         },
@@ -1837,4 +1837,18 @@ mod tests {
     }
 
+    #[test]
+    fn build_routine_trigger_normalizes_cron_schedule() {
+        let trigger = build_routine_trigger(&NormalizedTriggerRequest::Cron {
+            schedule: "0 0 9 * * MON-FRI".to_string(),
+            timezone: Some("UTC".to_string()),
+        });
+
+        assert!(matches!(
+            trigger,
+            Trigger::Cron { schedule, timezone }
+                if schedule == "0 0 9 * * MON-FRI *" && timezone.as_deref() == Some("UTC")
+        ));
+    }
+
     #[test]
     fn parses_grouped_message_event_with_tools() {
diff --git a/tests/e2e_builtin_tool_coverage.rs b/tests/e2e_builtin_tool_coverage.rs
index 42d7fb75..1c3cc6a2 100644
--- a/tests/e2e_builtin_tool_coverage.rs
+++ b/tests/e2e_builtin_tool_coverage.rs
@@ -440,5 +440,5 @@ mod tests {
         match &routine.trigger {
             Trigger::Cron { schedule, timezone } => {
-                assert_eq!(schedule, "0 0 9 * * MON-FRI");
+                assert_eq!(schedule, "0 0 9 * * MON-FRI *");
                 assert_eq!(timezone.as_deref(), Some("UTC"));
             }
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
