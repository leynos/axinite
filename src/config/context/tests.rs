//! Unit tests for `EnvContext` environment and secret lookup precedence.

use super::EnvContext;
use std::collections::HashMap;
use std::path::PathBuf;

#[test]
fn get_prefers_env_over_secret() {
    let ctx = EnvContext::for_testing(
        HashMap::from([(String::from("OPENAI_API_KEY"), String::from("env"))]),
        HashMap::from([(String::from("OPENAI_API_KEY"), String::from("secret"))]),
    );

    assert_eq!(ctx.get("OPENAI_API_KEY"), Some("env"));
}

/// An empty environment value is treated as unset, so the secret wins.
#[test]
fn empty_env_value_falls_back_to_secret() {
    let ctx = EnvContext::for_testing(
        HashMap::from([(String::from("OPENAI_API_KEY"), String::new())]),
        HashMap::from([(String::from("OPENAI_API_KEY"), String::from("secret"))]),
    );

    assert_eq!(ctx.get("OPENAI_API_KEY"), Some("secret"));
}

#[test]
fn contains_env_var_counts_empty_values_and_ignores_secrets() {
    let ctx = EnvContext::default()
        .with_env("EMPTY_BACKEND", "")
        .with_secret("SECRET_BACKEND", "postgres");

    assert!(ctx.contains_env_var("EMPTY_BACKEND"));
    assert!(!ctx.contains_env_var("SECRET_BACKEND"));
}

#[cfg(windows)]
#[test]
fn lookup_is_case_insensitive_on_windows() {
    let ctx = EnvContext::default().with_env("Path", r"C:\Windows\System32");

    assert_eq!(ctx.get("PATH"), Some(r"C:\Windows\System32"));
    assert!(ctx.contains_env_var("PATH"));
    assert_eq!(ctx.env_var_raw("PATH"), Some(r"C:\Windows\System32"));
}

#[test]
fn builder_helpers_populate_context() {
    let ctx = EnvContext::default()
        .with_env("A", "1")
        .with_secret("B", "2");

    assert_eq!(ctx.get("A"), Some("1"));
    assert_eq!(ctx.get("B"), Some("2"));
}

#[test]
fn base_dir_uses_snapshot_override() {
    let ctx = EnvContext::default().with_env("AXINITE_BASE_DIR", "/tmp/axinite");

    assert_eq!(ctx.axinite_base_dir(), PathBuf::from("/tmp/axinite"));
}

/// Neither `{:?}` nor `{:#?}` may print a captured value or key.
#[test]
fn debug_output_redacts_values_and_keys() {
    const SENTINEL: &str = "sk-debug-sentinel-9f3c1a";
    let ctx = EnvContext::default()
        .with_env("SENTINEL_ENV_VAR", SENTINEL)
        .with_secret("SENTINEL_SECRET", SENTINEL);

    let rendered = [format!("{ctx:?}"), format!("{ctx:#?}")];

    for output in &rendered {
        assert!(
            output.starts_with("EnvContext"),
            "summary expected: {output}"
        );
        assert!(!output.contains(SENTINEL), "value leaked: {output}");
        assert!(
            !output.contains("SENTINEL_ENV_VAR"),
            "env key leaked: {output}"
        );
        assert!(
            !output.contains("SENTINEL_SECRET"),
            "secret key leaked: {output}"
        );
    }
    assert!(
        rendered[0].contains("secret_count"),
        "shape expected: {}",
        rendered[0]
    );
    assert!(
        rendered[0].contains("base_dir_overridden: false"),
        "no override expected: {}",
        rendered[0]
    );
}

/// An `AXINITE_BASE_DIR` override is reported as a flag, never as its value,
/// in either formatting mode.
#[test]
fn debug_output_withholds_base_dir_override_value() {
    const BASE_DIR: &str = "/tmp/axinite-debug-base";
    const SENTINEL: &str = "sk-debug-sentinel-2a6f04";
    let ctx = EnvContext::default()
        .with_env("AXINITE_BASE_DIR", BASE_DIR)
        .with_env("SENTINEL_ENV_VAR", SENTINEL)
        .with_secret("SENTINEL_SECRET", SENTINEL);

    let rendered = [format!("{ctx:?}"), format!("{ctx:#?}")];

    for output in &rendered {
        assert!(
            output.contains("base_dir_overridden: true"),
            "override flag expected: {output}"
        );
        assert!(
            output.contains("env_var_count: 2"),
            "env count expected: {output}"
        );
        assert!(!output.contains(BASE_DIR), "base dir leaked: {output}");
        assert!(!output.contains(SENTINEL), "value leaked: {output}");
        assert!(
            !output.contains("SENTINEL_SECRET"),
            "secret key leaked: {output}"
        );
    }
}

#[cfg(unix)]
#[test]
fn ambient_snapshot_skips_non_utf8_entries() {
    use super::collect_utf8_env_vars;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let env_vars = collect_utf8_env_vars([
        (OsString::from("VALID_KEY"), OsString::from("valid")),
        (
            OsString::from_vec(b"BAD\xffKEY".to_vec()),
            OsString::from("ignored"),
        ),
        (
            OsString::from("BAD_VALUE"),
            OsString::from_vec(b"bad\xffvalue".to_vec()),
        ),
    ]);

    assert_eq!(env_vars.get("VALID_KEY"), Some(&String::from("valid")));
    assert!(!env_vars.contains_key("BAD_VALUE"));
    assert_eq!(env_vars.len(), 1);
}
