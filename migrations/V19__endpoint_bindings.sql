-- Confidential endpoint bindings (ADR 0016).
--
-- Endpoint values are encrypted before they reach this table. The table binds
-- each record to (user_id, extension_name, endpoint_name) and records the
-- endpoint kind, envelope format version, and validation-policy version, all
-- of which are authenticated as associated data by the application. There is
-- deliberately no plaintext origin, path, query, or error column.

CREATE TABLE endpoint_bindings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id TEXT NOT NULL,
    extension_name TEXT NOT NULL,
    endpoint_name TEXT NOT NULL,
    endpoint_kind TEXT NOT NULL,
    envelope_version INTEGER NOT NULL,
    validation_policy_version INTEGER NOT NULL,

    -- AES-256-GCM envelope: nonce || ciphertext || tag
    encrypted_value BYTEA NOT NULL,
    -- Per-record HKDF salt
    key_salt BYTEA NOT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT unique_endpoint_binding_per_scope
        UNIQUE (user_id, extension_name, endpoint_name)
);

CREATE INDEX idx_endpoint_bindings_scope
    ON endpoint_bindings(user_id, extension_name);

CREATE TRIGGER update_endpoint_bindings_updated_at
    BEFORE UPDATE ON endpoint_bindings
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();