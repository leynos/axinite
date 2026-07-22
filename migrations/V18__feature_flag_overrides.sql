-- Deployment-scoped feature-flag overrides (RFC 0009).
--
-- Feature flags are deployment-scoped, not user-scoped, so they live in a
-- dedicated table rather than the (user_id, key)-keyed `settings` table. The
-- settings API exposes them through the `feature_flag:` key prefix plus an
-- `X-Deployment-Id` header, but persistence is keyed by (deployment_id,
-- flag_name).

CREATE TABLE IF NOT EXISTS feature_flag_overrides (
    deployment_id TEXT        NOT NULL,
    flag_name     TEXT        NOT NULL,
    enabled       BOOLEAN     NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (deployment_id, flag_name)
);
