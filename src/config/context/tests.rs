//! Unit tests for `EnvContext` environment and secret lookup precedence.

use super::{EnvContext, ambient_value_from_layers};
use crate::config::dotenv_layers::DotenvLayers;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

#[test]
fn get_prefers_env_over_secret() {
    let ctx = EnvContext::for_testing(
        HashMap::from([(String::from("OPENAI_API_KEY"), String::from("env"))]),
        HashMap::from([(String::from("OPENAI_API_KEY"), String::from("secret"))]),
    );

    assert_eq!(ctx.get("OPENAI_API_KEY"), Some("env"));
}

#[test]
fn empty_env_value_falls_back_to_secret() {
    let ctx = EnvContext::for_testing(
        HashMap::from([(String::from("OPENAI_API_KEY"), String::new())]),
        HashMap::from([(String::from("OPENAI_API_KEY"), String::from("secret"))]),
    );

    assert_eq!(ctx.get("OPENAI_API_KEY"), Some("secret"));
    assert!(ctx.contains_key("OPENAI_API_KEY"));
}

#[rstest::rstest]
#[case(None, false)]
#[case(Some(""), false)]
#[case(Some("false"), false)]
#[case(Some("TRUE"), true)]
#[case(Some("true"), true)]
fn docker_restart_flag_requires_true(#[case] value: Option<&str>, #[case] expected: bool) {
    let ctx = value.map_or_else(EnvContext::default, |value| {
        EnvContext::default().with_env("AXINITE_IN_DOCKER", value)
    });

    assert_eq!(ctx.is_docker_restart_environment(), expected);
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

#[test]
fn ambient_single_key_lookup_preserves_layer_precedence_and_empty_fallback() {
    let layers = DotenvLayers::from_contents(
        Some(b"PROCESS_LOOKUP=cwd\nCWD_LOOKUP=cwd\nEMPTY_LOOKUP=cwd\n"),
        Some(b"PROCESS_LOOKUP=bootstrap\nCWD_LOOKUP=bootstrap\nBOOTSTRAP_LOOKUP=bootstrap\n"),
        Path::new("/tmp/axinite"),
    );
    let process = HashMap::from([
        (String::from("PROCESS_LOOKUP"), String::from("process")),
        (String::from("EMPTY_LOOKUP"), String::new()),
    ]);
    let secrets = HashMap::from([
        (String::from("PROCESS_LOOKUP"), String::from("secret")),
        (String::from("CWD_LOOKUP"), String::from("secret")),
        (String::from("BOOTSTRAP_LOOKUP"), String::from("secret")),
        (String::from("EMPTY_LOOKUP"), String::from("secret")),
        (String::from("SECRET_LOOKUP"), String::from("secret")),
    ]);

    assert_eq!(
        ambient_value_from_layers("PROCESS_LOOKUP", &process, &layers, &secrets),
        Some(String::from("process"))
    );
    assert_eq!(
        ambient_value_from_layers("CWD_LOOKUP", &process, &layers, &secrets),
        Some(String::from("cwd"))
    );
    assert_eq!(
        ambient_value_from_layers("BOOTSTRAP_LOOKUP", &process, &layers, &secrets),
        Some(String::from("bootstrap"))
    );
    assert_eq!(
        ambient_value_from_layers("EMPTY_LOOKUP", &process, &layers, &secrets),
        Some(String::from("secret"))
    );
    assert_eq!(
        ambient_value_from_layers("SECRET_LOOKUP", &process, &layers, &secrets),
        Some(String::from("secret"))
    );
}

#[test]
fn non_empty_env_presence_excludes_secret_fallback() {
    let layers = DotenvLayers::from_contents(
        Some(b"DOTENV_VALUE=from-dotenv\nEMPTY_VALUE=\n"),
        None,
        Path::new("/tmp/axinite"),
    );
    let ctx = EnvContext::from_dotenv_layers(
        HashMap::from([
            (String::from("PROCESS_VALUE"), String::from("from-process")),
            (String::from("EMPTY_VALUE"), String::new()),
        ]),
        HashMap::from([
            (String::from("SECRET_ONLY"), String::from("injected")),
            (String::from("EMPTY_VALUE"), String::from("injected")),
        ]),
        &layers,
    );

    assert!(ctx.has_non_empty_env_value("PROCESS_VALUE"));
    assert!(ctx.has_non_empty_env_value("DOTENV_VALUE"));
    assert!(!ctx.has_non_empty_env_value("SECRET_ONLY"));
    assert!(!ctx.has_non_empty_env_value("EMPTY_VALUE"));
}

#[cfg(unix)]
#[test]
fn ambient_snapshot_skips_non_utf8_entries() {
    use super::collect_utf8_env_vars;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let (env_vars, non_unicode_values) = collect_utf8_env_vars([
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
    assert!(non_unicode_values.contains("BAD_VALUE"));
    assert_eq!(env_vars.len(), 1);
}
