//! Compile-contract fixture for PostgreSQL database backend.
//!
//! This module validates that the `ironclaw::db` trait forwarders compile
//! correctly when used with the PostgreSQL backend (`PgBackend`).
//! It serves as a trybuild test target to ensure the public DB trait
//! surface remains compatible with PostgreSQL implementations.

use ironclaw::db::{
    ConversationStore, Database, SettingKey, SettingsStore, UserId,
};

fn assert_dyn_database<T>(db: &T)
where
    T: ConversationStore + SettingsStore + Database,
{
    let user_id = String::from("compile-user");
    let channel = String::from("web");
    let value = serde_json::json!({"theme": "dark"});

    let _ = ConversationStore::list_conversations_with_preview(
        db,
        user_id.as_str(),
        channel.as_str(),
        10,
    );
    let _ = SettingsStore::set_setting(
        db,
        UserId::from(user_id.as_str()),
        SettingKey::from("theme"),
        &value,
    );
    let _ = Database::run_migrations(db);
}

fn assert_postgres_backend(db: &ironclaw::db::postgres::PgBackend) {
    assert_dyn_database(db);
}

fn main() {
    // Force monomorphisation of the generic assert function for PgBackend
    let _: fn(&ironclaw::db::postgres::PgBackend) = assert_postgres_backend;
}
