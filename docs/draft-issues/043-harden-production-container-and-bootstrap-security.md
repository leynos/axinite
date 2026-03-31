# Harden production container and bootstrap security

## Summary

- Source commit: `c26f116a987961885a8193022539bef9b2d3ec13`
- Source date: `2026-03-12`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: moderate deploy.

## What the upstream commit addressed

Upstream commit `c26f116a987961885a8193022539bef9b2d3ec13` (`fix(deploy): harden
production container and bootstrap security (#1014)`) addresses harden
production container and bootstrap security.

Changed upstream paths:

- deploy/env.example
- deploy/ironclaw.service
- deploy/setup.sh

Upstream stats:

```text
 deploy/env.example      |  5 +++++
 deploy/ironclaw.service | 14 +++++++++-----
 deploy/setup.sh         |  9 ++++++++-
 3 files changed, 22 insertions(+), 6 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate deploy.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate deploy) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/deploy/env.example b/deploy/env.example
index c982d9aa..1561f49f 100644
--- a/deploy/env.example
+++ b/deploy/env.example
@@ -1,4 +1,9 @@
 # WARNING: Replace all CHANGE_ME values before deploying.
 # Do not use placeholder passwords in production.
+
+# Pin the Docker image version for deterministic deployments.
+# Update this value when deploying a new release.
+# IRONCLAW_VERSION=v1.0.0
+
 DATABASE_URL=postgres://ironclaw:CHANGE_ME@localhost:5432/ironclaw
 
diff --git a/deploy/ironclaw.service b/deploy/ironclaw.service
index b5aa0a4e..c9f9f0b0 100644
--- a/deploy/ironclaw.service
+++ b/deploy/ironclaw.service
@@ -6,11 +6,15 @@ Requires=cloud-sql-proxy.service
 [Service]
 Type=simple
-ExecStartPre=/usr/bin/docker pull us-central1-docker.pkg.dev/ironclaw-prod/ironclaw/agent:latest
-ExecStart=/usr/bin/docker run --rm \
+EnvironmentFile=/opt/ironclaw/.env
+# Pin to a specific version tag or digest instead of :latest to prevent
+# uncontrolled deployments. Update IRONCLAW_VERSION in /opt/ironclaw/.env
+# or replace the tag below when deploying a new release.
+ExecStartPre=/bin/bash -c 'docker pull us-central1-docker.pkg.dev/ironclaw-prod/ironclaw/agent:${IRONCLAW_VERSION:-latest}'
+ExecStart=/bin/bash -c 'docker run --rm \
   --name ironclaw \
   --env-file /opt/ironclaw/.env \
-  --network=host \
-  us-central1-docker.pkg.dev/ironclaw-prod/ironclaw/agent:latest \
-  --no-onboard
+  -p 3000:3000 \
+  us-central1-docker.pkg.dev/ironclaw-prod/ironclaw/agent:${IRONCLAW_VERSION:-latest} \
+  --no-onboard'
 ExecStop=/usr/bin/docker stop ironclaw
 Restart=always
diff --git a/deploy/setup.sh b/deploy/setup.sh
index 0bec03a0..10aa2b22 100755
--- a/deploy/setup.sh
+++ b/deploy/setup.sh
@@ -25,6 +25,13 @@ systemctl start docker
 
 echo "==> Installing Cloud SQL Auth Proxy"
+CLOUD_SQL_PROXY_VERSION="v2.14.3"
+CLOUD_SQL_PROXY_SHA256="75e7cc1f158ab6f97b7810e9d8419c55735cff40bc56d4f19673adfdf2406a59"
 curl -fsSL -o /usr/local/bin/cloud-sql-proxy \
-  https://storage.googleapis.com/cloud-sql-connectors/cloud-sql-proxy/v2.14.3/cloud-sql-proxy.linux.amd64
+  "https://storage.googleapis.com/cloud-sql-connectors/cloud-sql-proxy/${CLOUD_SQL_PROXY_VERSION}/cloud-sql-proxy.linux.amd64"
+echo "${CLOUD_SQL_PROXY_SHA256}  /usr/local/bin/cloud-sql-proxy" | sha256sum -c - || {
+  echo "ERROR: Cloud SQL Auth Proxy checksum verification failed -- aborting"
+  rm -f /usr/local/bin/cloud-sql-proxy
+  exit 1
+}
 chmod +x /usr/local/bin/cloud-sql-proxy
 
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
