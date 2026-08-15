//! Recording settings-store test double shared by wrapper dispatch tests.
//!
//! Captures setting writes so tests can assert on persistence calls without a
//! real database.

pub(super) struct RecordingSettingsStore {
    writes: std::sync::Mutex<Vec<String>>,
}

impl RecordingSettingsStore {
    pub(super) fn new() -> Self {
        Self {
            writes: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub(super) fn writes(&self) -> Vec<String> {
        self.writes
            .lock()
            .expect("settings writes lock poisoned")
            .clone()
    }
}

fn ready_db_ok<'a, T: Send + 'a>(
    value: T,
) -> crate::db::DbFuture<'a, Result<T, crate::error::DatabaseError>> {
    Box::pin(async move { Ok(value) })
}

impl crate::db::SettingsStore for RecordingSettingsStore {
    fn list_deployment_flags<'a>(
        &'a self,
        _deployment_id: &'a str,
    ) -> crate::db::DbFuture<'a, Result<Vec<(String, bool)>, crate::error::DatabaseError>> {
        ready_db_ok(Vec::new())
    }

    fn set_deployment_flag<'a>(
        &'a self,
        _deployment_id: &'a str,
        _flag_name: &'a str,
        _enabled: bool,
    ) -> crate::db::DbFuture<'a, Result<(), crate::error::DatabaseError>> {
        ready_db_ok(())
    }

    fn get_setting<'a>(
        &'a self,
        _user_id: crate::db::UserId,
        _key: crate::db::SettingKey,
    ) -> crate::db::DbFuture<'a, Result<Option<serde_json::Value>, crate::error::DatabaseError>>
    {
        ready_db_ok(None)
    }

    fn get_setting_full<'a>(
        &'a self,
        _user_id: crate::db::UserId,
        _key: crate::db::SettingKey,
    ) -> crate::db::DbFuture<
        'a,
        Result<Option<crate::history::SettingRow>, crate::error::DatabaseError>,
    > {
        ready_db_ok(None)
    }

    fn set_setting<'a>(
        &'a self,
        _user_id: crate::db::UserId,
        key: crate::db::SettingKey,
        _value: &'a serde_json::Value,
    ) -> crate::db::DbFuture<'a, Result<(), crate::error::DatabaseError>> {
        Box::pin(async move {
            self.writes
                .lock()
                .expect("settings writes lock poisoned")
                .push(key.to_string());
            Ok(())
        })
    }

    fn delete_setting<'a>(
        &'a self,
        _user_id: crate::db::UserId,
        _key: crate::db::SettingKey,
    ) -> crate::db::DbFuture<'a, Result<bool, crate::error::DatabaseError>> {
        ready_db_ok(false)
    }

    fn list_settings<'a>(
        &'a self,
        _user_id: crate::db::UserId,
    ) -> crate::db::DbFuture<'a, Result<Vec<crate::history::SettingRow>, crate::error::DatabaseError>>
    {
        ready_db_ok(Vec::new())
    }

    fn get_all_settings<'a>(
        &'a self,
        _user_id: crate::db::UserId,
    ) -> crate::db::DbFuture<
        'a,
        Result<std::collections::HashMap<String, serde_json::Value>, crate::error::DatabaseError>,
    > {
        ready_db_ok(std::collections::HashMap::new())
    }

    fn set_all_settings<'a>(
        &'a self,
        _user_id: crate::db::UserId,
        _settings: &'a std::collections::HashMap<String, serde_json::Value>,
    ) -> crate::db::DbFuture<'a, Result<(), crate::error::DatabaseError>> {
        ready_db_ok(())
    }

    fn has_settings<'a>(
        &'a self,
        _user_id: crate::db::UserId,
    ) -> crate::db::DbFuture<'a, Result<bool, crate::error::DatabaseError>> {
        ready_db_ok(false)
    }
}
