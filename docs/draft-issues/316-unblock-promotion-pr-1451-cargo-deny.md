# Unblock promotion PR #1451 cargo-deny

## Summary

- Source commit: `67a025e2faf73c9f970129523c7bc18b5d3c3c9e`
- Source date: `2026-03-25`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate Cargo.lock, deny.toml.

## What the upstream commit addressed

Upstream commit `67a025e2faf73c9f970129523c7bc18b5d3c3c9e` (`fix(deps): unblock
promotion PR #1451 cargo-deny`) addresses unblock promotion pr #1451 cargo-deny.

Changed upstream paths:

- Cargo.lock
- deny.toml

Upstream stats:

```text
 Cargo.lock | 18 +++++++++---------
 deny.toml  |  2 ++
 2 files changed, 11 insertions(+), 9 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate Cargo.lock,
deny.toml.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate Cargo.lock, deny.toml) means the fix could
  touch more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/Cargo.lock b/Cargo.lock
index 2c5547e0..0a6b5797 100644
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -2340,5 +2340,5 @@ checksum = "39cab71617ae0d63f51a36d69f866391735b51691dbda63cf6f96d042b63efeb"
 dependencies = [
  "libc",
- "windows-sys 0.52.0",
+ "windows-sys 0.59.0",
 ]
 
@@ -5576,5 +5576,5 @@ dependencies = [
  "libc",
  "linux-raw-sys 0.12.1",
- "windows-sys 0.52.0",
+ "windows-sys 0.59.0",
 ]
 
@@ -5625,5 +5625,5 @@ dependencies = [
  "ring",
  "rustls-pki-types",
- "rustls-webpki 0.103.9",
+ "rustls-webpki 0.103.10",
  "subtle",
  "zeroize",
@@ -5697,7 +5697,7 @@ dependencies = [
 [[package]]
 name = "rustls-webpki"
-version = "0.103.9"
+version = "0.103.10"
 source = "registry+https://github.com/rust-lang/crates.io-index"
-checksum = "d7df23109aa6c1567d1c575b9952556388da57401e4ace1d15f79eedad0d8f53"
+checksum = "df33b2b81ac578cabaf06b89b0631153a3f416b0a886e8a7a1707fb51abbd1ef"
 dependencies = [
  "aws-lc-rs",
@@ -6458,7 +6458,7 @@ checksum = "55937e1799185b12863d447f42597ed69d9928686b8d88a1df17376a097d8369"
 [[package]]
 name = "tar"
-version = "0.4.44"
+version = "0.4.45"
 source = "registry+https://github.com/rust-lang/crates.io-index"
-checksum = "1d863878d212c87a19c1a610eb53bb01fe12951c0501cf5a0d65f724914a667a"
+checksum = "22692a6476a21fa75fdfc11d452fda482af402c008cdbaf3476414e122040973"
 dependencies = [
  "filetime",
@@ -6480,8 +6480,8 @@ checksum = "32497e9a4c7b38532efcdebeef879707aa9f794296a4f0244f6f69e9bc8574bd"
 dependencies = [
  "fastrand",
- "getrandom 0.3.4",
+ "getrandom 0.4.2",
  "once_cell",
  "rustix 1.1.4",
- "windows-sys 0.52.0",
+ "windows-sys 0.59.0",
 ]
 
diff --git a/deny.toml b/deny.toml
index 80aa2215..fddb3d43 100644
--- a/deny.toml
+++ b/deny.toml
@@ -16,4 +16,6 @@ ignore = [
     # wasmtime wasi:http/types.fields panic — mitigated by fuel limits
     "RUSTSEC-2026-0021",
+    # rustls-webpki CRL distributionPoint matching — 0.102.8 pinned by libsql transitive dep
+    "RUSTSEC-2026-0049",
 ]
 
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
