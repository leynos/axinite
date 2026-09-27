CREATE TABLE IF NOT EXISTS _migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

INSERT OR IGNORE INTO _migrations (version, name)
VALUES (9, 'flexible_embedding_dimension');

CREATE TABLE IF NOT EXISTS wasm_tools (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL DEFAULT '1.0.0',
    wit_version TEXT NOT NULL DEFAULT '0.1.0',
    description TEXT NOT NULL,
    wasm_binary BLOB NOT NULL,
    binary_hash BLOB NOT NULL,
    parameters_schema TEXT NOT NULL,
    source_url TEXT,
    trust_level TEXT NOT NULL DEFAULT 'user',
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (user_id, name, version)
);

CREATE TABLE IF NOT EXISTS wasm_channels (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL DEFAULT '0.1.0',
    wit_version TEXT NOT NULL DEFAULT '0.1.0',
    description TEXT NOT NULL DEFAULT '',
    wasm_binary BLOB NOT NULL,
    binary_hash BLOB NOT NULL,
    capabilities_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (user_id, name)
);
