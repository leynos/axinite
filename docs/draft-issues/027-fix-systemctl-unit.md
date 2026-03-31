# Fix systemctl unit

## Summary

- Source commit: `acea1143cf70f7fa593c077620c979d5aa260de9`
- Source date: `2026-03-12`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: narrow src.

## What the upstream commit addressed

Upstream commit `acea1143cf70f7fa593c077620c979d5aa260de9` (`Fix systemctl unit
(#472)`) addresses fix systemctl unit.

Changed upstream paths:

- src/service.rs

Upstream stats:

```text
 src/service.rs | 1 +
 1 file changed, 1 insertion(+)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow src.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow src) means the fix could touch more behaviour
  than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/service.rs b/src/service.rs
index 9bc6088f..4249120e 100644
--- a/src/service.rs
+++ b/src/service.rs
@@ -115,4 +115,5 @@ fn install_linux() -> Result<()> {
          [Service]\n\
          Type=simple\n\
+         Environment=\"CLI_ENABLED=false\"\n\
          ExecStart=\"{exe}\" run\n\
          Restart=always\n\
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
