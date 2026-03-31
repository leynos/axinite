# Mark ironclaw_safety unpublished in release-plz

## Summary

- Source commit: `ef5715cb9675a01654faa498efe78857cfaaded4`
- Source date: `2026-03-16`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: narrow release-plz.toml.

## What the upstream commit addressed

Upstream commit `ef5715cb9675a01654faa498efe78857cfaaded4` (`fix: mark
ironclaw_safety unpublished in release-plz (#1286)`) addresses mark
ironclaw_safety unpublished in release-plz.

Changed upstream paths:

- release-plz.toml

Upstream stats:

```text
 release-plz.toml | 1 +
 1 file changed, 1 insertion(+)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow
release-plz.toml.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow release-plz.toml) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/release-plz.toml b/release-plz.toml
index ee7037df..b003952d 100644
--- a/release-plz.toml
+++ b/release-plz.toml
@@ -4,3 +4,4 @@ git_release_enable = false
 [[package]]
 name = "ironclaw_safety"
+publish = false
 release = false
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
