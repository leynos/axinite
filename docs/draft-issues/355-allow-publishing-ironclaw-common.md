# Allow publishing ironclaw_common

## Summary

- Source commit: `f02345fd1f9140573d341cf9b4028d55eb021a1d`
- Source date: `2026-03-25`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: narrow crates.

## What the upstream commit addressed

Upstream commit `f02345fd1f9140573d341cf9b4028d55eb021a1d` (`fix: allow
publishing ironclaw_common (#1657)`) addresses allow publishing ironclaw_common.

Changed upstream paths:

- crates/ironclaw_common/Cargo.toml

Upstream stats:

```text
 crates/ironclaw_common/Cargo.toml | 1 -
 1 file changed, 1 deletion(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow crates.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow crates) means the fix could touch more behaviour
  than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/crates/ironclaw_common/Cargo.toml b/crates/ironclaw_common/Cargo.toml
index 353ab747..6e7db5a4 100644
--- a/crates/ironclaw_common/Cargo.toml
+++ b/crates/ironclaw_common/Cargo.toml
@@ -9,5 +9,4 @@ license = "MIT OR Apache-2.0"
 homepage = "https://github.com/nearai/ironclaw"
 repository = "https://github.com/nearai/ironclaw"
-publish = false
 
 [package.metadata.dist]
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
