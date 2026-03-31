# Set CLI_ENABLED=false in macOS launchd plist

## Summary

- Source commit: `d8bcfe15cf54be18fc36b60feb1582e8d6a5a962`
- Source date: `2026-03-12`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: narrow src.

## What the upstream commit addressed

Upstream commit `d8bcfe15cf54be18fc36b60feb1582e8d6a5a962` (`fix(service): set
CLI_ENABLED=false in macOS launchd plist (#1079)`) addresses set
cli_enabled=false in macos launchd plist.

Changed upstream paths:

- src/service.rs

Upstream stats:

```text
 src/service.rs | 40 ++++++++++++++++++++++++++++++----------
 1 file changed, 30 insertions(+), 10 deletions(-)
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
index 4249120e..679e6fe2 100644
--- a/src/service.rs
+++ b/src/service.rs
@@ -66,5 +66,18 @@ fn install_macos() -> Result<()> {
     let stderr = logs_dir.join("daemon.stderr.log");
 
-    let plist = format!(
+    let plist = macos_plist_content(
+        &exe.display().to_string(),
+        &stdout.display().to_string(),
+        &stderr.display().to_string(),
+    );
+
+    std::fs::write(&file, plist)?;
+    println!("Installed launchd service: {}", file.display());
+    println!("  Start with: ironclaw service start");
+    Ok(())
+}
+
+fn macos_plist_content(exe: &str, stdout: &str, stderr: &str) -> String {
+    format!(
         r#"<?xml version="1.0" encoding="UTF-8"?>
 <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
@@ -82,4 +95,9 @@ fn install_macos() -> Result<()> {
   <key>KeepAlive</key>
   <true/>
+  <key>EnvironmentVariables</key>
+  <dict>
+    <key>CLI_ENABLED</key>
+    <string>false</string>
+  </dict>
   <key>StandardOutPath</key>
   <string>{stdout}</string>
@@ -90,13 +108,8 @@ fn install_macos() -> Result<()> {
 "#,
         label = SERVICE_LABEL,
-        exe = xml_escape(&exe.display().to_string()),
-        stdout = xml_escape(&stdout.display().to_string()),
-        stderr = xml_escape(&stderr.display().to_string()),
-    );
-
-    std::fs::write(&file, plist)?;
-    println!("Installed launchd service: {}", file.display());
-    println!("  Start with: ironclaw service start");
-    Ok(())
+        exe = xml_escape(exe),
+        stdout = xml_escape(stdout),
+        stderr = xml_escape(stderr),
+    )
 }
 
@@ -357,3 +370,10 @@ mod tests {
         assert!(s.ends_with(".ironclaw/logs"), "unexpected path: {s}");
     }
+
+    #[test]
+    fn macos_plist_sets_cli_enabled_false() {
+        let plist = macos_plist_content("/tmp/ironclaw", "/tmp/stdout.log", "/tmp/stderr.log");
+        assert!(plist.contains("<key>EnvironmentVariables</key>"));
+        assert!(plist.contains("    <key>CLI_ENABLED</key>\n    <string>false</string>"));
+    }
 }
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
