//! libSQL settings-store regression tests for the shared test harness.

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use super::*;
#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use rstest::rstest;

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
#[tokio::test]
async fn test_settings_crud() {
    let harness = TestHarnessBuilder::new().build().await;
    let db = &harness.db;

    let val = db
        .get_setting(UserId::from("user1"), SettingKey::from("theme"))
        .await
        .expect("get");
    assert!(val.is_none());

    db.set_setting(
        UserId::from("user1"),
        SettingKey::from("theme"),
        &serde_json::json!("dark"),
    )
    .await
    .expect("set");

    let val = db
        .get_setting(UserId::from("user1"), SettingKey::from("theme"))
        .await
        .expect("get")
        .expect("should exist");
    assert_eq!(val, serde_json::json!("dark"));

    db.set_setting(
        UserId::from("user1"),
        SettingKey::from("theme"),
        &serde_json::json!("light"),
    )
    .await
    .expect("set update");
    let val = db
        .get_setting(UserId::from("user1"), SettingKey::from("theme"))
        .await
        .expect("get")
        .expect("should exist");
    assert_eq!(val, serde_json::json!("light"));

    let all = db.list_settings(UserId::from("user1")).await.expect("list");
    assert_eq!(all.len(), 1);

    let deleted = db
        .delete_setting(UserId::from("user1"), SettingKey::from("theme"))
        .await
        .expect("delete");
    assert!(deleted);

    let val = db
        .get_setting(UserId::from("user1"), SettingKey::from("theme"))
        .await
        .expect("get");
    assert!(val.is_none());

    let deleted = db
        .delete_setting(UserId::from("user1"), SettingKey::from("theme"))
        .await
        .expect("delete");
    assert!(!deleted);
}

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
async fn run_settings_crud_flow(db: &Arc<dyn Database>, user_id: UserId, key: SettingKey) {
    let initial_value = serde_json::json!("dark");
    let updated_value = serde_json::json!("light");

    db.set_setting(user_id.clone(), key.clone(), &initial_value)
        .await
        .expect("set setting");

    let stored = db
        .get_setting(user_id.clone(), key.clone())
        .await
        .expect("get setting")
        .expect("setting should exist");
    assert_eq!(stored, initial_value);

    let listed = db
        .list_settings(user_id.clone())
        .await
        .expect("list settings");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].key, key);
    assert_eq!(listed[0].value, initial_value);

    db.set_setting(user_id.clone(), key.clone(), &updated_value)
        .await
        .expect("update setting");

    let stored = db
        .get_setting(user_id.clone(), key.clone())
        .await
        .expect("get updated setting")
        .expect("updated setting should exist");
    assert_eq!(stored, updated_value);

    let deleted = db
        .delete_setting(user_id.clone(), key.clone())
        .await
        .expect("delete setting");
    assert!(deleted);
    assert!(
        db.get_setting(user_id, key)
            .await
            .expect("get deleted setting")
            .is_none()
    );
}

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn test_settings_crud_variants(#[case] use_owned_strings: bool) {
    let harness = TestHarnessBuilder::new().build().await;
    let db = &harness.db;

    let (user_id, key) = if use_owned_strings {
        (
            UserId::from("owned-user".to_string()),
            SettingKey::from("theme".to_string()),
        )
    } else {
        (UserId::from("literal-user"), SettingKey::from("theme"))
    };

    run_settings_crud_flow(db, user_id, key).await;
}

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
#[tokio::test]
async fn test_settings_bulk_operations() {
    let harness = TestHarnessBuilder::new().build().await;
    let db = &harness.db;

    let has = db
        .has_settings(UserId::from("bulk_user"))
        .await
        .expect("has_settings");
    assert!(!has);

    let mut settings = std::collections::HashMap::new();
    settings.insert("key1".to_string(), serde_json::json!("value1"));
    settings.insert("key2".to_string(), serde_json::json!(42));
    db.set_all_settings(UserId::from("bulk_user"), &settings)
        .await
        .expect("set_all");

    let has = db
        .has_settings(UserId::from("bulk_user"))
        .await
        .expect("has_settings");
    assert!(has);

    let all = db
        .get_all_settings(UserId::from("bulk_user"))
        .await
        .expect("get_all");
    assert_eq!(all.len(), 2);
    assert_eq!(all["key1"], serde_json::json!("value1"));
    assert_eq!(all["key2"], serde_json::json!(42));

    db.set_setting(
        UserId::from("bulk_user"),
        SettingKey::from("key3"),
        &serde_json::json!("stale"),
    )
    .await
    .expect("set stale key");

    db.set_all_settings(UserId::from("bulk_user"), &settings)
        .await
        .expect("replace settings should prune omitted rows");
    let replaced = db
        .get_all_settings(UserId::from("bulk_user"))
        .await
        .expect("get_all after replace");
    assert_eq!(replaced.len(), 2);
    assert!(!replaced.contains_key("key3"));

    let full = db
        .get_setting_full(UserId::from("bulk_user"), SettingKey::from("key1"))
        .await
        .expect("get_full")
        .expect("should exist");
    assert_eq!(full.key, SettingKey::from("key1"));

    db.set_all_settings(UserId::from("bulk_user"), &std::collections::HashMap::new())
        .await
        .expect("empty bulk write should clear settings");
    let cleared = db
        .get_all_settings(UserId::from("bulk_user"))
        .await
        .expect("get_all after clear");
    assert!(cleared.is_empty());
}
