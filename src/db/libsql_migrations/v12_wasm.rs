const V12_WASM_TOOLS_COLUMNS: &str = r#"    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL DEFAULT '1.0.0',
    wit_version TEXT NOT NULL DEFAULT '0.3.0',
    description TEXT NOT NULL,
    wasm_binary BLOB NOT NULL,
    binary_hash BLOB NOT NULL,
    parameters_schema TEXT NOT NULL,
    source_url TEXT,
    trust_level TEXT NOT NULL DEFAULT 'user',
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (user_id, name, version)"#;

const V12_WASM_TOOLS_COPY_COLUMNS: &str = r#"id, user_id, name, version, wit_version, description, wasm_binary, binary_hash,
    parameters_schema, source_url, trust_level, status, created_at, updated_at"#;

const V12_WASM_TOOLS_POST_REBUILD_SQL: &str = r#"CREATE INDEX IF NOT EXISTS idx_wasm_tools_user ON wasm_tools(user_id);
CREATE INDEX IF NOT EXISTS idx_wasm_tools_status ON wasm_tools(status);
CREATE INDEX IF NOT EXISTS idx_wasm_tools_trust ON wasm_tools(trust_level);"#;

const V12_WASM_CHANNELS_COLUMNS: &str = r#"    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL DEFAULT '0.1.0',
    wit_version TEXT NOT NULL DEFAULT '0.3.0',
    description TEXT NOT NULL DEFAULT '',
    wasm_binary BLOB NOT NULL,
    binary_hash BLOB NOT NULL,
    capabilities_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (user_id, name)"#;

const V12_WASM_CHANNELS_COPY_COLUMNS: &str = r#"id, user_id, name, version, wit_version, description, wasm_binary, binary_hash,
    capabilities_json, status, created_at, updated_at"#;

struct TableRebuildParams<'a> {
    table_name: &'a str,
    columns: &'a str,
    copy_columns: &'a str,
    post_rebuild_sql: Option<&'a str>,
}

fn append_table_rebuild_sql(sql: &mut String, params: TableRebuildParams<'_>) {
    let TableRebuildParams {
        table_name,
        columns,
        copy_columns,
        post_rebuild_sql,
    } = params;
    let old_table_name = format!("{table_name}_old");
    sql.push_str(&format!(
        "ALTER TABLE {table_name} RENAME TO {old_table_name};\n\n"
    ));
    sql.push_str(&format!("CREATE TABLE {table_name} (\n"));
    sql.push_str(columns);
    sql.push_str("\n);\n\n");
    sql.push_str(&format!("INSERT INTO {table_name} (\n"));
    sql.push_str(&format!("    {copy_columns}\n"));
    sql.push_str(")\nSELECT\n");
    sql.push_str(&format!("    {copy_columns}\n"));
    sql.push_str(&format!("FROM {old_table_name};\n\n"));
    sql.push_str(&format!("DROP TABLE {old_table_name};"));

    if let Some(post_rebuild_sql) = post_rebuild_sql {
        sql.push_str("\n\n");
        sql.push_str(post_rebuild_sql);
    }

    sql.push_str("\n\n");
}

pub(crate) fn v12_wasm_wit_default_migration_sql() -> String {
    let mut sql = String::from(
        "PRAGMA legacy_alter_table=ON;\nPRAGMA foreign_keys=OFF;\nBEGIN IMMEDIATE;\n\n",
    );

    append_table_rebuild_sql(
        &mut sql,
        TableRebuildParams {
            table_name: "wasm_tools",
            columns: V12_WASM_TOOLS_COLUMNS,
            copy_columns: V12_WASM_TOOLS_COPY_COLUMNS,
            post_rebuild_sql: Some(V12_WASM_TOOLS_POST_REBUILD_SQL),
        },
    );
    append_table_rebuild_sql(
        &mut sql,
        TableRebuildParams {
            table_name: "wasm_channels",
            columns: V12_WASM_CHANNELS_COLUMNS,
            copy_columns: V12_WASM_CHANNELS_COPY_COLUMNS,
            post_rebuild_sql: None,
        },
    );

    sql.push_str("COMMIT;\nPRAGMA foreign_keys=ON;\nPRAGMA legacy_alter_table=OFF;\n");
    sql
}

#[cfg(test)]
mod tests {
    use super::v12_wasm_wit_default_migration_sql;
    use crate::db::libsql_migrations::SCHEMA;

    #[test]
    fn schema_uses_current_wit_defaults_for_new_wasm_records() {
        let expected = "wit_version TEXT NOT NULL DEFAULT '0.3.0'";
        let count = SCHEMA.matches(expected).count();
        assert_eq!(
            count, 2,
            "expected fresh libSQL schema to declare {expected} for both wasm tables"
        );
        assert!(
            !SCHEMA.contains("wit_version TEXT NOT NULL DEFAULT '0.1.0'"),
            "fresh libSQL schema should not default new wasm records to the historical 0.1.0 WIT version"
        );
    }

    #[test]
    fn generated_sql_updates_existing_wasm_wit_defaults_to_0_3_0() {
        let sql = v12_wasm_wit_default_migration_sql();

        assert!(
            sql.contains("0.3.0"),
            "expected V12 libSQL migration to set wasm wit_version defaults to 0.3.0"
        );
        assert!(
            !sql.contains("wit_version TEXT NOT NULL DEFAULT '0.1.0'"),
            "expected V12 libSQL migration to remove stale 0.1.0 wit_version defaults"
        );
        assert!(
            !sql.contains("INSERT INTO _migrations"),
            "non-transactional migration SQL should not manage _migrations rows itself"
        );
    }
}
