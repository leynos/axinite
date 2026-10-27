//! Focused tests for dotenv parsing, relocation, refresh, and layer precedence.

use std::collections::HashMap;

use tempfile::tempdir;
use tracing_test::traced_test;

use super::DotenvLayers;
use crate::bootstrap::upsert_bootstrap_vars_to;
use crate::config::EnvContext;
use crate::test_support::ExpectValid;

#[test]
fn dotenv_layers_resolve_precedence_and_libsql_autodetection() {
    let dir = tempdir().expect_valid("create isolated dotenv base directory");
    let base_dir = dir.path();
    ambient_fs::write(base_dir.join("axinite.db"), b"").expect_valid("write libSQL marker");
    let layers = DotenvLayers::from_contents(
        Some(
            b"CWD_WINS=cwd\nCWD_OVERRIDES_BOOTSTRAP=cwd\nPROCESS_WINS=shadowed\nEMPTY_MASK=lower-cwd\n",
        ),
        Some(
            b"CWD_WINS=shadowed\nCWD_OVERRIDES_BOOTSTRAP=bootstrap\nPROCESS_WINS=shadowed\nEMPTY_MASK=lower-bootstrap\n",
        ),
        base_dir,
    );
    let process = HashMap::from([
        (String::from("PROCESS_WINS"), String::from("process")),
        (String::from("EMPTY_MASK"), String::new()),
    ]);
    let secrets = HashMap::from([
        (String::from("CWD_WINS"), String::from("secret")),
        (
            String::from("CWD_OVERRIDES_BOOTSTRAP"),
            String::from("secret"),
        ),
        (String::from("PROCESS_WINS"), String::from("secret")),
        (String::from("DATABASE_BACKEND"), String::from("secret")),
        (String::from("EMPTY_MASK"), String::from("secret")),
        (String::from("SECRET_ONLY"), String::from("secret")),
    ]);

    let context = EnvContext::from_dotenv_layers(process, secrets, &layers);

    assert_eq!(context.get("PROCESS_WINS"), Some("process"));
    assert_eq!(context.get("CWD_WINS"), Some("cwd"));
    assert_eq!(context.get("CWD_OVERRIDES_BOOTSTRAP"), Some("cwd"));
    assert_eq!(context.get("DATABASE_BACKEND"), Some("libsql"));
    assert_eq!(context.get("EMPTY_MASK"), Some("secret"));
    assert_eq!(context.get("SECRET_ONLY"), Some("secret"));
}

#[test]
fn empty_backend_blocks_autodetection_and_bootstrap_root_does_not_relocate_itself() {
    let dir = tempdir().expect_valid("create fixed-root dotenv directory");
    let base_dir = dir.path().join("fixed-root");
    ambient_fs::create_dir_all(&base_dir).expect_valid("create fixed bootstrap root");
    ambient_fs::write(base_dir.join("axinite.db"), b"")
        .expect_valid("write libSQL database marker");
    let layers =
        DotenvLayers::from_contents(None, Some(b"AXINITE_BASE_DIR=/another/root\n"), &base_dir);
    let process = HashMap::from([(String::from("DATABASE_BACKEND"), String::new())]);

    let context = EnvContext::from_dotenv_layers(process, HashMap::new(), &layers);

    assert_eq!(context.axinite_base_dir(), base_dir);
    assert_eq!(context.get("DATABASE_BACKEND"), None);
    assert!(context.contains_key("DATABASE_BACKEND"));
}

#[test]
fn cwd_base_directory_value_relocates_bootstrap_file_lookup() {
    let dir = tempdir().expect_valid("create dotenv relocation tree");
    let cwd = dir.path().join("project");
    let relocated = dir.path().join("custom-axinite");
    ambient_fs::create_dir_all(&cwd).expect_valid("create project directory");
    ambient_fs::create_dir_all(&relocated).expect_valid("create relocated base directory");
    ambient_fs::write(
        cwd.join(".env"),
        format!("AXINITE_BASE_DIR={}\n", relocated.display()),
    )
    .expect_valid("write current-directory dotenv file");
    ambient_fs::write(relocated.join(".env"), "AXINITE_TEST_RELOCATED=loaded\n")
        .expect_valid("write relocated bootstrap dotenv file");

    let layers = DotenvLayers::load(&cwd, &HashMap::new(), None);

    assert_eq!(layers.base_dir(), Some(relocated.as_path()));
    assert_eq!(
        layers.bootstrap_path(),
        Some(relocated.join(".env").as_path())
    );
    assert_eq!(
        layers
            .bootstrap_values()
            .get("AXINITE_TEST_RELOCATED")
            .map(String::as_str),
        Some("loaded")
    );
}

#[test]
fn cwd_home_value_selects_the_default_bootstrap_root_when_process_home_is_absent() {
    let dir = tempdir().expect_valid("create dotenv home-resolution directory");
    let cwd = dir.path().join("project");
    let home = dir.path().join("home");
    ambient_fs::create_dir_all(&cwd).expect_valid("create project directory");
    ambient_fs::create_dir_all(&home).expect_valid("create home directory");
    ambient_fs::write(cwd.join(".env"), format!("HOME={}\n", home.display()))
        .expect_valid("write current-directory HOME override");

    let layers = DotenvLayers::load(&cwd, &HashMap::new(), None);

    assert_eq!(layers.base_dir(), Some(home.join(".axinite").as_path()));
}

#[test]
fn passwd_database_home_is_used_when_home_variables_are_absent() {
    let dir = tempdir().expect_valid("create fallback home directory");
    let cwd = dir.path().join("project");
    let home = dir.path().join("passwd-home");
    ambient_fs::create_dir_all(&cwd).expect_valid("create project directory");
    ambient_fs::create_dir_all(&home).expect_valid("create fallback home directory");

    let layers = DotenvLayers::load(&cwd, &HashMap::new(), Some(&home));

    assert_eq!(layers.base_dir(), Some(home.join(".axinite").as_path()));
}

#[test]
fn loading_again_after_bootstrap_write_refreshes_the_file_layer() {
    let dir = tempdir().expect_valid("create refresh test directory");
    let base_dir = dir.path().join("axinite");
    ambient_fs::create_dir_all(&base_dir).expect_valid("create bootstrap directory");
    let process = HashMap::from([(
        String::from("AXINITE_BASE_DIR"),
        base_dir.to_string_lossy().into_owned(),
    )]);
    let start_dir = dir.path();
    let before_write = DotenvLayers::load(start_dir, &process, None);
    let before = EnvContext::from_dotenv_layers(process.clone(), HashMap::new(), &before_write);
    assert_eq!(before.get("AXINITE_TEST_ONBOARDING_REFRESH"), None);

    upsert_bootstrap_vars_to(
        &base_dir.join(".env"),
        &[("AXINITE_TEST_ONBOARDING_REFRESH", "written")],
    )
    .expect_valid("write onboarding value to bootstrap dotenv file");
    let after_write = DotenvLayers::load(start_dir, &process, None);
    let after = EnvContext::from_dotenv_layers(process, HashMap::new(), &after_write);

    assert_eq!(
        after.get("AXINITE_TEST_ONBOARDING_REFRESH"),
        Some("written")
    );
}

#[test]
fn cross_file_substitution_no_longer_reads_the_current_directory_layer() {
    let dir = tempdir().expect_valid("create cross-file substitution directory");
    let layers = DotenvLayers::from_contents(
        Some(b"AXINITE_TEST_CROSS_FILE_INPUT=from-cwd\n"),
        Some(b"AXINITE_TEST_CROSS_FILE_OUTPUT=${AXINITE_TEST_CROSS_FILE_INPUT}\n"),
        dir.path(),
    );

    assert_eq!(
        layers
            .cwd_values()
            .get("AXINITE_TEST_CROSS_FILE_INPUT")
            .map(String::as_str),
        Some("from-cwd")
    );
    assert_eq!(
        layers
            .bootstrap_values()
            .get("AXINITE_TEST_CROSS_FILE_OUTPUT")
            .map(String::as_str),
        Some("")
    );
}

#[test]
fn repeated_keys_keep_the_first_value_but_later_substitution_uses_last_value() {
    let dir = tempdir().expect_valid("create duplicate substitution directory");
    let layers = DotenvLayers::from_contents(
        None,
        Some(
            b"AXINITE_TEST_DUPLICATE_KEY=first\nAXINITE_TEST_DUPLICATE_KEY=last\nAXINITE_TEST_DUPLICATE_OUTPUT=${AXINITE_TEST_DUPLICATE_KEY}\n",
        ),
        dir.path(),
    );

    assert_eq!(
        layers
            .bootstrap_values()
            .get("AXINITE_TEST_DUPLICATE_KEY")
            .map(String::as_str),
        Some("first")
    );
    assert_eq!(
        layers
            .bootstrap_values()
            .get("AXINITE_TEST_DUPLICATE_OUTPUT")
            .map(String::as_str),
        Some("last")
    );
}

#[test]
#[traced_test]
fn parser_keeps_entries_before_the_first_error_and_strips_a_bom() {
    let dir = tempdir().expect_valid("create parser error directory");
    let layers = DotenvLayers::from_contents(
        None,
        Some(
            b"\xef\xbb\xbfAXINITE_TEST_BEFORE_ERROR=kept\nBROKEN=\"dotenv-test-secret\nAXINITE_TEST_AFTER_ERROR=ignored\n",
        ),
        dir.path(),
    );

    assert_eq!(
        layers
            .bootstrap_values()
            .get("AXINITE_TEST_BEFORE_ERROR")
            .map(String::as_str),
        Some("kept")
    );
    assert!(
        !layers
            .bootstrap_values()
            .contains_key("AXINITE_TEST_AFTER_ERROR")
    );
    assert!(!logs_contain("dotenv-test-secret"));
}
