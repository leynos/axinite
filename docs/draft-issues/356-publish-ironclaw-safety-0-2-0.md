# Publish ironclaw_safety 0.2.0

## Summary

- Source commit: `ab67f028860094dc8086f4e9866ed0e8ff44b3cd`
- Source date: `2026-03-25`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: very broad Cargo.lock, Cargo.toml.

## What the upstream commit addressed

Upstream commit `ab67f028860094dc8086f4e9866ed0e8ff44b3cd` (`fix: publish
ironclaw_safety 0.2.0 (#1659)`) addresses publish ironclaw_safety 0.2.0.

Changed upstream paths:

- Cargo.lock
- Cargo.toml
- crates/ironclaw_safety/Cargo.toml
- release-plz.toml

Upstream stats:

```text
 Cargo.lock                        | 2 +-
 Cargo.toml                        | 2 +-
 crates/ironclaw_safety/Cargo.toml | 3 +--
 release-plz.toml                  | 5 -----
 4 files changed, 3 insertions(+), 9 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad Cargo.lock,
Cargo.toml.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad Cargo.lock, Cargo.toml) means the fix could
  touch more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/Cargo.lock b/Cargo.lock
index 581c75bb..c3747590 100644
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -3497,5 +3497,5 @@ dependencies = [
 [[package]]
 name = "ironclaw_safety"
-version = "0.1.0"
+version = "0.2.0"
 dependencies = [
  "aho-corasick",
diff --git a/Cargo.toml b/Cargo.toml
index 8a77f5b4..41895b16 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -105,5 +105,5 @@ ironclaw_common = { path = "crates/ironclaw_common", version = "0.1.0" }
 
 # Safety/sanitization
-ironclaw_safety = { path = "crates/ironclaw_safety", version = "0.1.0" }
+ironclaw_safety = { path = "crates/ironclaw_safety", version = "0.2.0" }
 regex = "1"
 aho-corasick = "1"
diff --git a/crates/ironclaw_safety/Cargo.toml b/crates/ironclaw_safety/Cargo.toml
index d12aa909..38b8718a 100644
--- a/crates/ironclaw_safety/Cargo.toml
+++ b/crates/ironclaw_safety/Cargo.toml
@@ -1,5 +1,5 @@
 [package]
 name = "ironclaw_safety"
-version = "0.1.0"
+version = "0.2.0"
 edition = "2024"
 rust-version = "1.92"
@@ -9,5 +9,4 @@ license = "MIT OR Apache-2.0"
 homepage = "https://github.com/nearai/ironclaw"
 repository = "https://github.com/nearai/ironclaw"
-publish = false
 
 [package.metadata.dist]
diff --git a/release-plz.toml b/release-plz.toml
index b003952d..e8e0670f 100644
--- a/release-plz.toml
+++ b/release-plz.toml
@@ -1,7 +1,2 @@
 [workspace]
 git_release_enable = false
-
-[[package]]
-name = "ironclaw_safety"
-publish = false
-release = false
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
