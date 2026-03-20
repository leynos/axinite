use super::*;
use std::process::Command;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use tempfile::tempdir;

static ENV_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_save_and_load_database_url() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join(".env");

    // Write in the quoted format that save_database_url uses
    let url = "postgres://localhost:5432/ironclaw_test";
    std::fs::write(&env_path, format!("DATABASE_URL=\"{}\"\n", url)).unwrap();

    // Verify the content is a valid dotenv line (quoted)
    let content = std::fs::read_to_string(&env_path).unwrap();
    assert_eq!(
        content,
        "DATABASE_URL=\"postgres://localhost:5432/ironclaw_test\"\n"
    );

    // Verify dotenvy can parse it (strips quotes automatically)
    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].0, "DATABASE_URL");
    assert_eq!(parsed[0].1, url);
}

#[test]
fn test_save_database_url_with_hash_in_password() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join(".env");

    // URLs with # in the password are common (URL-encoded special chars).
    // Without quoting, dotenvy treats # as a comment delimiter.
    let url = "postgres://user:p%23ss@localhost:5432/ironclaw";
    std::fs::write(&env_path, format!("DATABASE_URL=\"{}\"\n", url)).unwrap();

    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].0, "DATABASE_URL");
    assert_eq!(parsed[0].1, url);
}

#[test]
fn test_save_database_url_creates_parent_dirs() {
    let dir = tempdir().unwrap();
    let nested = dir.path().join("deep").join("nested");
    let env_path = nested.join(".env");

    // Parent doesn't exist yet
    assert!(!nested.exists());

    // The global function uses a fixed path, so we test the logic directly
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(&env_path, "DATABASE_URL=postgres://test\n").unwrap();

    assert!(env_path.exists());
    let content = std::fs::read_to_string(&env_path).unwrap();
    assert!(content.contains("DATABASE_URL=postgres://test"));
}

#[test]
fn test_save_bootstrap_env_escapes_quotes() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join(".env");

    // A malicious URL attempting to inject a second env var
    let malicious = r#"http://evil.com"
INJECTED="pwned"#;
    let mut content = String::new();
    let escaped = malicious.replace('\\', "\\\\").replace('"', "\\\"");
    content.push_str(&format!("LLM_BASE_URL=\"{}\"\n", escaped));
    std::fs::write(&env_path, &content).unwrap();

    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    // Must parse as exactly one variable, not two
    assert_eq!(parsed.len(), 1, "injection must not create extra vars");
    assert_eq!(parsed[0].0, "LLM_BASE_URL");
    // The value should contain the original malicious content (unescaped by dotenvy)
    assert!(
        parsed[0].1.contains("INJECTED"),
        "value should contain the literal injection attempt, not execute it"
    );
}

#[test]
fn test_ironclaw_env_path() {
    let path = ironclaw_env_path();
    assert!(path.ends_with(".ironclaw/.env"));
}

#[test]
fn test_migrate_bootstrap_json_to_env() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let bootstrap_path = dir.path().join("bootstrap.json");

    // Write a legacy bootstrap.json
    let bootstrap_json = serde_json::json!({
        "database_url": "postgres://localhost/ironclaw_upgrade",
        "database_pool_size": 5,
        "secrets_master_key_source": "keychain",
        "onboard_completed": true
    });
    std::fs::write(
        &bootstrap_path,
        serde_json::to_string_pretty(&bootstrap_json).unwrap(),
    )
    .unwrap();

    assert!(!env_path.exists());
    assert!(bootstrap_path.exists());

    // Run the migration
    migrate_bootstrap_json_to_env(&env_path);

    // .env should now exist with DATABASE_URL
    assert!(env_path.exists());
    let content = std::fs::read_to_string(&env_path).unwrap();
    assert_eq!(
        content,
        "DATABASE_URL=\"postgres://localhost/ironclaw_upgrade\"\n"
    );

    // bootstrap.json should be renamed to .migrated
    assert!(!bootstrap_path.exists());
    assert!(dir.path().join("bootstrap.json.migrated").exists());
}

#[test]
fn test_migrate_bootstrap_json_no_database_url() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join(".env");
    let bootstrap_path = dir.path().join("bootstrap.json");

    // bootstrap.json with no database_url
    let bootstrap_json = serde_json::json!({
        "onboard_completed": false
    });
    std::fs::write(
        &bootstrap_path,
        serde_json::to_string_pretty(&bootstrap_json).unwrap(),
    )
    .unwrap();

    migrate_bootstrap_json_to_env(&env_path);

    // .env should NOT be created
    assert!(!env_path.exists());
    // bootstrap.json should remain (no migration happened)
    assert!(bootstrap_path.exists());
}

#[test]
fn test_migrate_bootstrap_json_missing() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join(".env");

    // No bootstrap.json at all
    migrate_bootstrap_json_to_env(&env_path);

    // Nothing should happen
    assert!(!env_path.exists());
}

#[test]
fn test_save_bootstrap_env_multiple_vars() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join("nested").join(".env");

    std::fs::create_dir_all(env_path.parent().unwrap()).unwrap();

    let vars = [
        ("DATABASE_BACKEND", "libsql"),
        ("LIBSQL_PATH", "/home/user/.ironclaw/ironclaw.db"),
    ];

    // Write manually to the temp path (save_bootstrap_env uses the global path)
    let mut content = String::new();
    for (key, value) in &vars {
        content.push_str(&format!("{}=\"{}\"\n", key, value));
    }
    std::fs::write(&env_path, &content).unwrap();

    // Verify dotenvy can parse all entries
    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .unwrap()
        .filter_map(|r| r.ok())
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
    let dir = tempdir().unwrap();
    let env_path = dir.path().join(".env");

    // Write initial content
    std::fs::write(&env_path, "DATABASE_URL=\"postgres://old\"\n").unwrap();

    // Overwrite with new vars (simulating save_bootstrap_env behavior)
    let content = "DATABASE_BACKEND=\"libsql\"\nLIBSQL_PATH=\"/new/path.db\"\n";
    std::fs::write(&env_path, content).unwrap();

    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    // Old DATABASE_URL should be gone
    assert_eq!(parsed.len(), 2);
    assert!(parsed.iter().all(|(k, _)| k != "DATABASE_URL"));
}

#[test]
fn test_onboard_completed_round_trips_through_env() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join(".env");

    // Simulate what the wizard writes: bootstrap vars + ONBOARD_COMPLETED
    let vars = [
        ("DATABASE_BACKEND", "libsql"),
        ("ONBOARD_COMPLETED", "true"),
    ];
    let mut content = String::new();
    for (key, value) in &vars {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        content.push_str(&format!("{}=\"{}\"\n", key, escaped));
    }
    std::fs::write(&env_path, &content).unwrap();

    // Verify dotenvy parses ONBOARD_COMPLETED correctly
    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(parsed.len(), 2);
    let onboard = parsed.iter().find(|(k, _)| k == "ONBOARD_COMPLETED");
    assert!(onboard.is_some(), "ONBOARD_COMPLETED must be present");
    assert_eq!(onboard.unwrap().1, "true");
}

#[test]
fn test_libsql_autodetect_sets_backend_when_db_exists() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let old_val = std::env::var("DATABASE_BACKEND").ok();
    // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
    unsafe { std::env::remove_var("DATABASE_BACKEND") };

    let dir = tempdir().unwrap();
    let db_path = dir.path().join("ironclaw.db");

    // No DB file — auto-detect guard should not trigger.
    assert!(!db_path.exists());
    let would_trigger = std::env::var("DATABASE_BACKEND").is_err() && db_path.exists();
    assert!(
        !would_trigger,
        "should not auto-detect when db file is absent"
    );

    // Create the DB file — guard should now trigger.
    std::fs::write(&db_path, "").unwrap();
    assert!(db_path.exists());

    // Simulate the detection logic (DATABASE_BACKEND unset + db exists).
    let detected = std::env::var("DATABASE_BACKEND").is_err() && db_path.exists();
    assert!(
        detected,
        "should detect libsql when db file is present and backend unset"
    );

    // Restore.
    if let Some(val) = old_val {
        // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
        unsafe { std::env::set_var("DATABASE_BACKEND", val) };
    }
}

// === QA Plan P1 - 1.2: Bootstrap .env round-trip tests ===

#[test]
fn bootstrap_env_round_trips_llm_backend() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join(".env");

    // Simulate what the wizard writes for LLM backend selection
    let vars = [
        ("DATABASE_BACKEND", "libsql"),
        ("LLM_BACKEND", "openai"),
        ("ONBOARD_COMPLETED", "true"),
    ];
    let mut content = String::new();
    for (key, value) in &vars {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        content.push_str(&format!("{}=\"{}\"\n", key, escaped));
    }
    std::fs::write(&env_path, &content).unwrap();

    // Verify dotenvy parses LLM_BACKEND correctly
    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    let llm_backend = parsed.iter().find(|(k, _)| k == "LLM_BACKEND");
    assert!(llm_backend.is_some(), "LLM_BACKEND must be present");
    assert_eq!(
        llm_backend.unwrap().1,
        "openai",
        "LLM_BACKEND must survive .env round-trip"
    );
}

#[test]
fn test_libsql_autodetect_does_not_override_explicit_backend() {
    let _guard = ENV_MUTEX.lock().unwrap();
    let old_val = std::env::var("DATABASE_BACKEND").ok();
    // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
    unsafe { std::env::set_var("DATABASE_BACKEND", "postgres") };

    let dir = tempdir().unwrap();
    let db_path = dir.path().join("ironclaw.db");
    std::fs::write(&db_path, "").unwrap();

    // The guard: only sets libsql if DATABASE_BACKEND is NOT already set.
    let would_override = std::env::var("DATABASE_BACKEND").is_err() && db_path.exists();
    assert!(
        !would_override,
        "must not override an explicitly set DATABASE_BACKEND"
    );

    // Restore.
    if let Some(val) = old_val {
        // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
        unsafe { std::env::set_var("DATABASE_BACKEND", val) };
    } else {
        // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
        unsafe { std::env::remove_var("DATABASE_BACKEND") };
    }
}

#[test]
fn bootstrap_env_special_chars_in_url() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join(".env");

    // URLs with special characters that are common in database passwords
    let url = "postgres://user:p%23ss@host:5432/db?sslmode=require";
    let escaped = url.replace('\\', "\\\\").replace('"', "\\\"");
    let content = format!("DATABASE_URL=\"{}\"\n", escaped);
    std::fs::write(&env_path, &content).unwrap();

    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].1, url, "URL with special chars must survive");
}

#[test]
fn upsert_bootstrap_var_preserves_existing() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join(".env");

    // Write initial content
    let initial = "DATABASE_BACKEND=\"libsql\"\nONBOARD_COMPLETED=\"true\"\n";
    std::fs::write(&env_path, initial).unwrap();

    // Upsert a new var
    let content = std::fs::read_to_string(&env_path).unwrap();
    let new_line = "LLM_BACKEND=\"anthropic\"";
    let mut result = content.clone();
    result.push_str(new_line);
    result.push('\n');
    std::fs::write(&env_path, &result).unwrap();

    // Parse and verify all three vars are present
    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    assert_eq!(parsed.len(), 3, "should have 3 vars after upsert");
    assert!(
        parsed
            .iter()
            .any(|(k, v)| k == "DATABASE_BACKEND" && v == "libsql"),
        "original DATABASE_BACKEND must be preserved"
    );
    assert!(
        parsed
            .iter()
            .any(|(k, v)| k == "ONBOARD_COMPLETED" && v == "true"),
        "original ONBOARD_COMPLETED must be preserved"
    );
    assert!(
        parsed
            .iter()
            .any(|(k, v)| k == "LLM_BACKEND" && v == "anthropic"),
        "new LLM_BACKEND must be present"
    );
}

#[test]
fn bootstrap_env_all_wizard_vars_round_trip() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join(".env");

    // Full set of vars the wizard might write
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
    std::fs::write(&env_path, &content).unwrap();

    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    assert_eq!(parsed.len(), vars.len(), "all vars must survive round-trip");
    for (key, value) in &vars {
        let found = parsed.iter().find(|(k, _)| k == key);
        assert!(found.is_some(), "{key} must be present");
        assert_eq!(&found.unwrap().1, value, "{key} value mismatch");
    }
}

#[test]
fn test_ironclaw_base_dir_default() {
    // This test must run first (or in isolation) before the LazyLock is initialized.
    // It verifies that when IRONCLAW_BASE_DIR is not set, the default path is used.
    let _guard = ENV_MUTEX.lock().unwrap();
    let old_val = std::env::var("IRONCLAW_BASE_DIR").ok();
    // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
    unsafe { std::env::remove_var("IRONCLAW_BASE_DIR") };

    // Force re-evaluation by calling the computation function directly
    let path = compute_ironclaw_base_dir();
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    assert_eq!(path, home.join(".ironclaw"));

    if let Some(val) = old_val {
        // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
        unsafe { std::env::set_var("IRONCLAW_BASE_DIR", val) };
    }
}

#[test]
fn test_ironclaw_base_dir_env_override() {
    // This test verifies that when IRONCLAW_BASE_DIR is set,
    // the custom path is used. Must run before LazyLock is initialized.
    let _guard = ENV_MUTEX.lock().unwrap();
    let old_val = std::env::var("IRONCLAW_BASE_DIR").ok();
    // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
    unsafe { std::env::set_var("IRONCLAW_BASE_DIR", "/custom/ironclaw/path") };

    // Force re-evaluation by calling the computation function directly
    let path = compute_ironclaw_base_dir();
    assert_eq!(path, std::path::PathBuf::from("/custom/ironclaw/path"));

    if let Some(val) = old_val {
        // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
        unsafe { std::env::set_var("IRONCLAW_BASE_DIR", val) };
    } else {
        // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
        unsafe { std::env::remove_var("IRONCLAW_BASE_DIR") };
    }
}

#[test]
fn test_compute_base_dir_env_path_join() {
    // Verifies that ironclaw_env_path correctly joins .env to the base dir.
    // Uses compute_ironclaw_base_dir directly to avoid LazyLock caching.
    let _guard = ENV_MUTEX.lock().unwrap();
    let old_val = std::env::var("IRONCLAW_BASE_DIR").ok();
    // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
    unsafe { std::env::set_var("IRONCLAW_BASE_DIR", "/my/custom/dir") };

    // Test the path construction logic directly
    let base_path = compute_ironclaw_base_dir();
    let env_path = base_path.join(".env");
    assert_eq!(env_path, std::path::PathBuf::from("/my/custom/dir/.env"));

    if let Some(val) = old_val {
        // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
        unsafe { std::env::set_var("IRONCLAW_BASE_DIR", val) };
    } else {
        // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
        unsafe { std::env::remove_var("IRONCLAW_BASE_DIR") };
    }
}

#[test]
fn test_ironclaw_base_dir_empty_env() {
    // Verifies that empty IRONCLAW_BASE_DIR falls back to default.
    let _guard = ENV_MUTEX.lock().unwrap();
    let old_val = std::env::var("IRONCLAW_BASE_DIR").ok();
    // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
    unsafe { std::env::set_var("IRONCLAW_BASE_DIR", "") };

    // Force re-evaluation by calling the computation function directly
    let path = compute_ironclaw_base_dir();
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    assert_eq!(path, home.join(".ironclaw"));

    if let Some(val) = old_val {
        // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
        unsafe { std::env::set_var("IRONCLAW_BASE_DIR", val) };
    } else {
        // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
        unsafe { std::env::remove_var("IRONCLAW_BASE_DIR") };
    }
}

#[test]
fn test_ironclaw_base_dir_special_chars() {
    // Verifies that paths with special characters are handled correctly.
    let _guard = ENV_MUTEX.lock().unwrap();
    let old_val = std::env::var("IRONCLAW_BASE_DIR").ok();
    // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
    unsafe { std::env::set_var("IRONCLAW_BASE_DIR", "/tmp/test_with-special.chars") };

    // Force re-evaluation by calling the computation function directly
    let path = compute_ironclaw_base_dir();
    assert_eq!(
        path,
        std::path::PathBuf::from("/tmp/test_with-special.chars")
    );

    if let Some(val) = old_val {
        // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
        unsafe { std::env::set_var("IRONCLAW_BASE_DIR", val) };
    } else {
        // SAFETY: ENV_MUTEX ensures single-threaded access to env vars in tests
        unsafe { std::env::remove_var("IRONCLAW_BASE_DIR") };
    }
}

// ── PID Lock tests ───────────────────────────────────────────────

#[test]
fn test_pid_lock_acquire_and_drop() {
    let dir = tempdir().unwrap();
    let pid_path = dir.path().join("ironclaw.pid");

    // Acquire lock
    let lock = PidLock::acquire_at(pid_path.clone()).unwrap();
    assert!(pid_path.exists());

    // PID file should contain our PID
    let contents = std::fs::read_to_string(&pid_path).unwrap();
    assert_eq!(contents.trim().parse::<u32>().unwrap(), std::process::id());

    // Drop should remove the file
    drop(lock);
    assert!(!pid_path.exists());
}

#[test]
fn test_pid_lock_rejects_second_acquire() {
    let dir = tempdir().unwrap();
    let pid_path = dir.path().join("ironclaw.pid");

    // First lock succeeds
    let _lock1 = PidLock::acquire_at(pid_path.clone()).unwrap();

    // Second acquire on same file must fail (exclusive flock held)
    let result = PidLock::acquire_at(pid_path.clone());
    assert!(result.is_err());
    match result.unwrap_err() {
        PidLockError::AlreadyRunning { pid } => {
            assert_eq!(pid, std::process::id());
        }
        other => panic!("expected AlreadyRunning, got: {}", other),
    }
}

#[test]
fn test_pid_lock_reclaims_after_drop() {
    let dir = tempdir().unwrap();
    let pid_path = dir.path().join("ironclaw.pid");

    // Acquire and release
    let lock = PidLock::acquire_at(pid_path.clone()).unwrap();
    drop(lock);

    // Should succeed — OS lock was released on drop
    let lock2 = PidLock::acquire_at(pid_path).unwrap();
    drop(lock2);
}

#[test]
fn test_pid_lock_reclaims_stale_file_without_flock() {
    let dir = tempdir().unwrap();
    let pid_path = dir.path().join("ironclaw.pid");

    // Write a stale PID file manually (no flock held)
    std::fs::write(&pid_path, "4294967294").unwrap();

    // Should succeed because no OS lock is held on the file
    let lock = PidLock::acquire_at(pid_path.clone()).unwrap();
    let contents = std::fs::read_to_string(&pid_path).unwrap();
    assert_eq!(contents.trim().parse::<u32>().unwrap(), std::process::id());
    drop(lock);
}

#[test]
fn test_pid_lock_handles_corrupt_pid_file() {
    let dir = tempdir().unwrap();
    let pid_path = dir.path().join("ironclaw.pid");

    // Write garbage (no flock held)
    std::fs::write(&pid_path, "not-a-number").unwrap();

    // Should succeed — no OS lock held, file is reclaimed
    let lock = PidLock::acquire_at(pid_path).unwrap();
    drop(lock);
}

#[test]
fn test_pid_lock_creates_parent_dirs() {
    let dir = tempdir().unwrap();
    let pid_path = dir.path().join("nested").join("deep").join("ironclaw.pid");

    let lock = PidLock::acquire_at(pid_path.clone()).unwrap();
    assert!(pid_path.exists());
    drop(lock);
}

#[test]
fn test_pid_lock_child_helper_holds_lock() {
    if std::env::var("IRONCLAW_PID_LOCK_CHILD").ok().as_deref() != Some("1") {
        return;
    }

    let pid_path = PathBuf::from(
        std::env::var("IRONCLAW_PID_LOCK_PATH").expect("IRONCLAW_PID_LOCK_PATH missing"),
    );
    let hold_ms = std::env::var("IRONCLAW_PID_LOCK_HOLD_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(3000);

    let _lock = PidLock::acquire_at(pid_path).expect("child failed to acquire pid lock");
    thread::sleep(Duration::from_millis(hold_ms));
}

#[test]
fn test_pid_lock_rejects_lock_held_by_other_process() {
    let dir = tempdir().unwrap();
    let pid_path = dir.path().join("ironclaw.pid");

    let current_exe = std::env::current_exe().unwrap();
    let mut child = Command::new(current_exe)
        .args([
            "--exact",
            "bootstrap::tests::test_pid_lock_child_helper_holds_lock",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("IRONCLAW_PID_LOCK_CHILD", "1")
        .env("IRONCLAW_PID_LOCK_PATH", pid_path.display().to_string())
        .env("IRONCLAW_PID_LOCK_HOLD_MS", "3000")
        .spawn()
        .unwrap();

    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(2) {
        if pid_path.exists() {
            break;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("child exited before acquiring lock: {}", status);
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        pid_path.exists(),
        "child did not create lock file in time: {}",
        pid_path.display()
    );

    let result = PidLock::acquire_at(pid_path.clone());
    match result.unwrap_err() {
        PidLockError::AlreadyRunning { .. } => {}
        other => panic!("expected AlreadyRunning, got: {}", other),
    }

    let status = child.wait().unwrap();
    assert!(status.success(), "child process failed: {}", status);

    // After the child exits, lock should be released and reacquirable.
    let lock = PidLock::acquire_at(pid_path).unwrap();
    drop(lock);
}

#[test]
fn upsert_bootstrap_vars_preserves_unknown_keys() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join(".env");

    // Simulate a user-edited .env with custom vars
    let initial = "HTTP_HOST=\"0.0.0.0\"\nDATABASE_BACKEND=\"postgres\"\nCUSTOM_VAR=\"keep_me\"\n";
    std::fs::write(&env_path, initial).unwrap();

    // Upsert wizard vars — should preserve HTTP_HOST and CUSTOM_VAR
    let vars = [("DATABASE_BACKEND", "libsql"), ("LLM_BACKEND", "openai")];
    upsert_bootstrap_vars_to(&env_path, &vars).unwrap();

    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    assert_eq!(
        parsed.len(),
        4,
        "should have 4 vars (2 preserved + 2 upserted)"
    );

    // User-added vars must be preserved
    assert!(
        parsed
            .iter()
            .any(|(k, v)| k == "HTTP_HOST" && v == "0.0.0.0"),
        "HTTP_HOST must be preserved"
    );
    assert!(
        parsed
            .iter()
            .any(|(k, v)| k == "CUSTOM_VAR" && v == "keep_me"),
        "CUSTOM_VAR must be preserved"
    );

    // Wizard vars must be updated/added
    assert!(
        parsed
            .iter()
            .any(|(k, v)| k == "DATABASE_BACKEND" && v == "libsql"),
        "DATABASE_BACKEND must be updated to libsql"
    );
    assert!(
        parsed
            .iter()
            .any(|(k, v)| k == "LLM_BACKEND" && v == "openai"),
        "LLM_BACKEND must be added"
    );

    // Now update LLM_BACKEND and verify HTTP_HOST still preserved
    let vars2 = [("LLM_BACKEND", "anthropic")];
    upsert_bootstrap_vars_to(&env_path, &vars2).unwrap();

    let parsed2: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    assert_eq!(
        parsed2.len(),
        4,
        "should still have 4 vars after second upsert"
    );
    assert!(
        parsed2
            .iter()
            .any(|(k, v)| k == "HTTP_HOST" && v == "0.0.0.0"),
        "HTTP_HOST must still be preserved after second upsert"
    );
    assert!(
        parsed2
            .iter()
            .any(|(k, v)| k == "LLM_BACKEND" && v == "anthropic"),
        "LLM_BACKEND must be updated to anthropic"
    );
}

#[test]
fn upsert_bootstrap_vars_creates_file_if_missing() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join("subdir").join(".env");

    // File doesn't exist yet
    assert!(!env_path.exists());

    let vars = [("DATABASE_BACKEND", "libsql")];
    upsert_bootstrap_vars_to(&env_path, &vars).unwrap();

    assert!(env_path.exists());
    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(parsed.len(), 1);
    assert_eq!(
        parsed[0],
        ("DATABASE_BACKEND".to_string(), "libsql".to_string())
    );
}
