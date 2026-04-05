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

fn main() {}
