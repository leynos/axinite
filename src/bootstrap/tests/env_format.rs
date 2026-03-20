//! Tests for bootstrap `.env` formatting and round-trip behaviour.

use tempfile::tempdir;

use super::super::*;

fn assert_env_roundtrip(key: &str, value: &str) {
    let dir = tempdir().expect("create temp dir for env round-trip test");
    let env_path = dir.path().join(".env");
    let write_error = format!("write round-trip env at {}", env_path.display());
    let parse_error = format!("parse round-trip env at {}", env_path.display());
    let vars = [(key, value)];

    upsert_bootstrap_vars_to(&env_path, &vars).expect(write_error.as_str());

    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .expect(parse_error.as_str())
        .filter_map(|result| result.ok())
        .collect();

    assert_eq!(
        parsed.len(),
        1,
        "{key} round-trip should produce one env var"
    );
    let found = parsed.iter().find(|(parsed_key, _)| parsed_key == key);
    assert!(found.is_some(), "{key} must be present");
    assert_eq!(
        found.expect("round-trip env entry present").1,
        value,
        "{key} must survive .env round-trip"
    );
}

macro_rules! env_roundtrip_test {
    ($name:ident, $key:expr, $value:expr) => {
        #[test]
        fn $name() {
            assert_env_roundtrip($key, $value);
        }
    };
}

#[test]
fn test_save_and_load_database_url() {
    let dir = tempdir().expect("create temp dir for test_save_and_load_database_url");
    let env_path = dir.path().join(".env");
    let write_error = format!("write .env at {}", env_path.display());
    let read_error = format!("read .env at {}", env_path.display());
    let parse_error = format!("parse dotenv from {}", env_path.display());

    let url = "postgres://localhost:5432/ironclaw_test";
    std::fs::write(&env_path, format!("DATABASE_URL=\"{}\"\n", url)).expect(write_error.as_str());

    let content = std::fs::read_to_string(&env_path).expect(read_error.as_str());
    assert_eq!(
        content,
        "DATABASE_URL=\"postgres://localhost:5432/ironclaw_test\"\n"
    );

    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .expect(parse_error.as_str())
        .filter_map(|result| result.ok())
        .collect();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].0, "DATABASE_URL");
    assert_eq!(parsed[0].1, url);
}

#[test]
fn test_save_database_url_with_hash_in_password() {
    let dir = tempdir().expect("create temp dir for hash-in-password test");
    let env_path = dir.path().join(".env");
    let url = "postgres://user:p%23ss@localhost:5432/ironclaw";

    std::fs::write(&env_path, format!("DATABASE_URL=\"{}\"\n", url))
        .expect("write .env for hash-in-password test");

    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .expect("parse dotenv for hash-in-password test")
        .filter_map(|result| result.ok())
        .collect();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].0, "DATABASE_URL");
    assert_eq!(parsed[0].1, url);
}

#[test]
fn test_save_database_url_creates_parent_dirs() {
    let dir = tempdir().expect("create temp dir for parent-dir test");
    let nested = dir.path().join("deep").join("nested");
    let env_path = nested.join(".env");

    assert!(!nested.exists());
    std::fs::create_dir_all(&nested).expect("create nested directory for .env");
    std::fs::write(&env_path, "DATABASE_URL=postgres://test\n").expect("write nested .env");

    assert!(env_path.exists());
    let content = std::fs::read_to_string(&env_path).expect("read nested .env");
    assert!(content.contains("DATABASE_URL=postgres://test"));
}

#[test]
fn test_save_bootstrap_env_escapes_quotes() {
    let dir = tempdir().expect("create temp dir for quote escaping test");
    let env_path = dir.path().join(".env");
    let malicious = r#"http://evil.com"
INJECTED="pwned"#;
    let escaped = malicious.replace('\\', "\\\\").replace('"', "\\\"");
    let content = format!("LLM_BASE_URL=\"{}\"\n", escaped);
    std::fs::write(&env_path, &content).expect("write escaped bootstrap env");

    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .expect("parse escaped bootstrap env")
        .filter_map(|result| result.ok())
        .collect();

    assert_eq!(parsed.len(), 1, "injection must not create extra vars");
    assert_eq!(parsed[0].0, "LLM_BASE_URL");
    assert!(
        parsed[0].1.contains("INJECTED"),
        "value should contain the literal injection attempt, not execute it"
    );
}

#[test]
fn test_save_bootstrap_env_multiple_vars() {
    let dir = tempdir().expect("create temp dir for multi-var bootstrap env");
    let env_path = dir.path().join("nested").join(".env");
    let vars = [
        ("DATABASE_BACKEND", "libsql"),
        ("LIBSQL_PATH", "/home/user/.ironclaw/ironclaw.db"),
    ];

    std::fs::create_dir_all(env_path.parent().expect("env_path has parent"))
        .expect("create nested env parent");

    let mut content = String::new();
    for (key, value) in &vars {
        content.push_str(&format!("{}=\"{}\"\n", key, value));
    }
    std::fs::write(&env_path, &content).expect("write multi-var bootstrap env");

    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .expect("parse multi-var bootstrap env")
        .filter_map(|result| result.ok())
        .collect();
    assert_eq!(parsed.len(), 2);
    assert_eq!(
        parsed[0],
        ("DATABASE_BACKEND".to_string(), "libsql".to_string())
    );
    assert_eq!(
        parsed[1],
        (
            "LIBSQL_PATH".to_string(),
            "/home/user/.ironclaw/ironclaw.db".to_string()
        )
    );
}

#[test]
fn test_save_bootstrap_env_overwrites_previous() {
    let dir = tempdir().expect("create temp dir for overwrite bootstrap env");
    let env_path = dir.path().join(".env");

    std::fs::write(&env_path, "DATABASE_URL=\"postgres://old\"\n")
        .expect("write initial bootstrap env");
    let content = "DATABASE_BACKEND=\"libsql\"\nLIBSQL_PATH=\"/new/path.db\"\n";
    std::fs::write(&env_path, content).expect("overwrite bootstrap env");

    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .expect("parse overwritten bootstrap env")
        .filter_map(|result| result.ok())
        .collect();
    assert_eq!(parsed.len(), 2);
    assert!(parsed.iter().all(|(key, _)| key != "DATABASE_URL"));
}

env_roundtrip_test!(
    test_onboard_completed_round_trips_through_env,
    "ONBOARD_COMPLETED",
    "true"
);
env_roundtrip_test!(
    bootstrap_env_round_trips_llm_backend,
    "LLM_BACKEND",
    "openai"
);

#[test]
fn bootstrap_env_special_chars_in_url() {
    let dir = tempdir().expect("create temp dir for special-char URL round-trip");
    let env_path = dir.path().join(".env");
    let url = "postgres://user:p%23ss@host:5432/db?sslmode=require";
    let escaped = url.replace('\\', "\\\\").replace('"', "\\\"");
    let content = format!("DATABASE_URL=\"{}\"\n", escaped);
    std::fs::write(&env_path, &content).expect("write special-char URL env");

    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .expect("parse special-char URL env")
        .filter_map(|result| result.ok())
        .collect();

    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].1, url, "URL with special chars must survive");
}

#[test]
fn upsert_bootstrap_var_preserves_existing() {
    let dir = tempdir().expect("create temp dir for single upsert test");
    let env_path = dir.path().join(".env");
    let initial = "DATABASE_BACKEND=\"libsql\"\nONBOARD_COMPLETED=\"true\"\n";

    std::fs::write(&env_path, initial).expect("write initial env for single upsert test");

    let content = std::fs::read_to_string(&env_path).expect("read initial env for single upsert");
    let new_line = "LLM_BACKEND=\"anthropic\"";
    let mut result = content.clone();
    result.push_str(new_line);
    result.push('\n');
    std::fs::write(&env_path, &result).expect("write single upsert env");

    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .expect("parse single upsert env")
        .filter_map(|result| result.ok())
        .collect();

    assert_eq!(parsed.len(), 3, "should have 3 vars after upsert");
    assert!(
        parsed
            .iter()
            .any(|(key, value)| key == "DATABASE_BACKEND" && value == "libsql"),
        "original DATABASE_BACKEND must be preserved"
    );
    assert!(
        parsed
            .iter()
            .any(|(key, value)| key == "ONBOARD_COMPLETED" && value == "true"),
        "original ONBOARD_COMPLETED must be preserved"
    );
    assert!(
        parsed
            .iter()
            .any(|(key, value)| key == "LLM_BACKEND" && value == "anthropic"),
        "new LLM_BACKEND must be present"
    );
}

#[test]
fn bootstrap_env_all_wizard_vars_round_trip() {
    let dir = tempdir().expect("create temp dir for full wizard round-trip");
    let env_path = dir.path().join(".env");
    let vars = [
        ("DATABASE_BACKEND", "postgres"),
        ("DATABASE_URL", "postgres://u:p@h:5432/db"),
        ("LLM_BACKEND", "nearai"),
        ("ONBOARD_COMPLETED", "true"),
        ("EMBEDDING_ENABLED", "false"),
    ];

    let mut content = String::new();
    for (key, value) in &vars {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        content.push_str(&format!("{}=\"{}\"\n", key, escaped));
    }
    std::fs::write(&env_path, &content).expect("write full wizard round-trip env");

    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .expect("parse full wizard round-trip env")
        .filter_map(|result| result.ok())
        .collect();

    assert_eq!(parsed.len(), vars.len(), "all vars must survive round-trip");
    for (key, value) in &vars {
        let found = parsed.iter().find(|(parsed_key, _)| parsed_key == key);
        assert!(found.is_some(), "{key} must be present");
        assert_eq!(
            &found.expect("wizard key present").1,
            value,
            "{key} value mismatch"
        );
    }
}

#[test]
fn upsert_bootstrap_vars_preserves_unknown_keys() {
    let dir = tempdir().expect("create temp dir for multi-upsert preserve test");
    let env_path = dir.path().join(".env");
    let initial = "HTTP_HOST=\"0.0.0.0\"\nDATABASE_BACKEND=\"postgres\"\nCUSTOM_VAR=\"keep_me\"\n";

    std::fs::write(&env_path, initial).expect("write initial env for preserve test");

    let vars = [("DATABASE_BACKEND", "libsql"), ("LLM_BACKEND", "openai")];
    upsert_bootstrap_vars_to(&env_path, &vars).expect("upsert wizard vars");

    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .expect("parse env after first upsert")
        .filter_map(|result| result.ok())
        .collect();

    assert_eq!(
        parsed.len(),
        4,
        "should have 4 vars (2 preserved + 2 upserted)"
    );
    assert!(
        parsed
            .iter()
            .any(|(key, value)| key == "HTTP_HOST" && value == "0.0.0.0"),
        "HTTP_HOST must be preserved"
    );
    assert!(
        parsed
            .iter()
            .any(|(key, value)| key == "CUSTOM_VAR" && value == "keep_me"),
        "CUSTOM_VAR must be preserved"
    );
    assert!(
        parsed
            .iter()
            .any(|(key, value)| key == "DATABASE_BACKEND" && value == "libsql"),
        "DATABASE_BACKEND must be updated to libsql"
    );
    assert!(
        parsed
            .iter()
            .any(|(key, value)| key == "LLM_BACKEND" && value == "openai"),
        "LLM_BACKEND must be added"
    );

    let vars2 = [("LLM_BACKEND", "anthropic")];
    upsert_bootstrap_vars_to(&env_path, &vars2).expect("upsert LLM backend a second time");

    let parsed2: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .expect("parse env after second upsert")
        .filter_map(|result| result.ok())
        .collect();

    assert_eq!(
        parsed2.len(),
        4,
        "should still have 4 vars after second upsert"
    );
    assert!(
        parsed2
            .iter()
            .any(|(key, value)| key == "HTTP_HOST" && value == "0.0.0.0"),
        "HTTP_HOST must still be preserved after second upsert"
    );
    assert!(
        parsed2
            .iter()
            .any(|(key, value)| key == "LLM_BACKEND" && value == "anthropic"),
        "LLM_BACKEND must be updated to anthropic"
    );
}
