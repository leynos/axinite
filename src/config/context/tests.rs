//! Unit tests for `EnvContext` environment and secret lookup precedence.

use super::{EnvContext, ambient_value_from_layers, merge_refreshed_secrets};
use crate::config::dotenv_layers::DotenvLayers;
use std::collections::{HashMap, HashSet};
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

#[cfg(windows)]
#[test]
fn ambient_single_key_lookup_preserves_case_insensitive_layer_precedence() {
    let layers = DotenvLayers::from_contents(
        Some(b"provider_key=cwd\ncwd_key=cwd\n"),
        Some(b"provider_key=bootstrap\ncwd_key=bootstrap\nbootstrap_only=bootstrap\n"),
        Path::new("C:/axinite"),
    );
    let process = HashMap::from([(String::from("PrOvIdEr_KeY"), String::from("process"))]);
    let secrets = HashMap::from([
        (String::from("provider_key"), String::from("secret")),
        (String::from("secret_only"), String::from("secret")),
    ]);

    assert_eq!(
        ambient_value_from_layers("PROVIDER_KEY", &process, &layers, &secrets),
        Some(String::from("process"))
    );
    assert_eq!(
        ambient_value_from_layers("CWD_KEY", &process, &layers, &secrets),
        Some(String::from("cwd"))
    );
    assert_eq!(
        ambient_value_from_layers("BOOTSTRAP_ONLY", &process, &layers, &secrets),
        Some(String::from("bootstrap"))
    );
    assert_eq!(
        ambient_value_from_layers("SECRET_ONLY", &process, &layers, &secrets),
        Some(String::from("secret"))
    );
}

#[cfg(windows)]
#[test]
fn context_merge_keeps_process_precedence_for_case_variant_keys() {
    let layers = DotenvLayers::from_contents(
        Some(b"OPENAI_API_KEY=dotenv-value\n"),
        Some(b"openai_api_key=bootstrap-value\n"),
        Path::new("C:/axinite"),
    );
    let process = HashMap::from([(
        String::from("openai_api_key"),
        String::from("process-value"),
    )]);
    let context = EnvContext::from_dotenv_layers(process, HashMap::new(), &layers);

    assert_eq!(context.get("OPENAI_API_KEY"), Some("process-value"));
}

#[test]
fn secret_refresh_uses_current_inherited_values_and_preserves_explicit_secrets() {
    let layers = DotenvLayers::from_contents(None, None, Path::new("/tmp/axinite"));
    let process = super::ProcessEnvSnapshot {
        env_vars: HashMap::new(),
        non_unicode_values: HashSet::new(),
        home_dir_fallback: None,
    };
    let captured = EnvContext::from_dotenv_layers_with_non_unicode(
        &process,
        HashMap::from([(
            String::from("ANTHROPIC_OAUTH_TOKEN"),
            String::from("old-global-token"),
        )]),
        &layers,
    )
    .with_secret("EXPLICIT_SECRET", "attached");
    assert_eq!(
        captured.get("ANTHROPIC_OAUTH_TOKEN"),
        Some("old-global-token")
    );
    let current_inherited = HashMap::from([
        (
            String::from("ANTHROPIC_OAUTH_TOKEN"),
            String::from("new-global-token"),
        ),
        (
            String::from("GLOBAL_SECRET"),
            String::from("new-global-value"),
        ),
    ]);

    let refreshed = merge_refreshed_secrets(current_inherited, &captured.explicit_secrets);

    assert_eq!(
        refreshed.get("ANTHROPIC_OAUTH_TOKEN").map(String::as_str),
        Some("new-global-token")
    );
    assert_eq!(
        refreshed.get("GLOBAL_SECRET").map(String::as_str),
        Some("new-global-value")
    );
    assert_eq!(
        refreshed.get("EXPLICIT_SECRET").map(String::as_str),
        Some("attached")
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
