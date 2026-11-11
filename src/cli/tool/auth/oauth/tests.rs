//! Unit tests for OAuth client credential resolution precedence.

use rstest::rstest;

use super::resolve_oauth_client_credentials;
use crate::config::EnvContext;
use crate::tools::wasm::{AuthCapabilitySchema, OAuthConfigSchema};

const CLIENT_ID_ENV: &str = "AXINITE_TEST_OAUTH_CLIENT_ID";
const CLIENT_SECRET_ENV: &str = "AXINITE_TEST_OAUTH_CLIENT_SECRET";

fn oauth_config(client_id: Option<&str>, client_secret: Option<&str>) -> OAuthConfigSchema {
    OAuthConfigSchema {
        authorization_url: "https://example.test/authorize".to_string(),
        token_url: "https://example.test/token".to_string(),
        client_id: client_id.map(str::to_string),
        client_id_env: Some(CLIENT_ID_ENV.to_string()),
        client_secret: client_secret.map(str::to_string),
        client_secret_env: Some(CLIENT_SECRET_ENV.to_string()),
        ..OAuthConfigSchema::default()
    }
}

fn auth(secret_name: &str) -> AuthCapabilitySchema {
    AuthCapabilitySchema {
        secret_name: secret_name.to_string(),
        ..AuthCapabilitySchema::default()
    }
}

/// Credentials supplied by each precedence layer.
#[derive(Clone, Copy, Default)]
struct Layer {
    env_id: Option<&'static str>,
    env_secret: Option<&'static str>,
    overlay_id: Option<&'static str>,
    overlay_secret: Option<&'static str>,
}

impl Layer {
    /// Build the snapshot that carries this layer's values.
    fn context(self) -> EnvContext {
        let mut ctx = EnvContext::default();
        let layers = [
            (self.env_id, self.env_secret, false),
            (self.overlay_id, self.overlay_secret, true),
        ];
        for (id, secret, overlay) in layers {
            for (name, value) in [(CLIENT_ID_ENV, id), (CLIENT_SECRET_ENV, secret)] {
                let Some(value) = value else { continue };
                ctx = if overlay {
                    ctx.with_secret(name, value)
                } else {
                    ctx.with_env(name, value)
                };
            }
        }
        ctx
    }
}

/// One credential-resolution scenario.
struct Scenario {
    secret_name: &'static str,
    layer: Layer,
    inline_id: Option<&'static str>,
    inline_secret: Option<&'static str>,
    /// `None` means the bundled app must supply non-empty credentials.
    expected: Option<(&'static str, &'static str)>,
}

/// Resolve credentials that must fail, returning the error message.
fn resolve_error(secret_name: &str, config: OAuthConfigSchema, ctx: &EnvContext) -> String {
    resolve_oauth_client_credentials(&auth(secret_name), &config, ctx)
        .expect_err("resolution must fail")
        .to_string()
}

/// Credential sources resolve in precedence order, highest layer first.
#[rstest]
#[case::inline_outranks_snapshot_and_builtin(Scenario {
    secret_name: "google_oauth_token",
    layer: Layer {
        env_id: Some("snapshot-id"),
        env_secret: Some("snapshot-secret"),
        overlay_id: Some("overlay-id"),
        overlay_secret: Some("overlay-secret"),
    },
    inline_id: Some("inline-id"),
    inline_secret: Some("inline-secret"),
    expected: Some(("inline-id", "inline-secret")),
})]
#[case::snapshot_supplies_missing_credentials(Scenario {
    secret_name: "custom_oauth_token",
    layer: Layer {
        env_id: Some("snapshot-id"),
        env_secret: Some("snapshot-secret"),
        ..Layer::default()
    },
    inline_id: None,
    inline_secret: None,
    expected: Some(("snapshot-id", "snapshot-secret")),
})]
#[case::overlay_supplies_credentials_when_environment_is_absent(Scenario {
    secret_name: "custom_oauth_token",
    layer: Layer {
        overlay_id: Some("overlay-id"),
        overlay_secret: Some("overlay-secret"),
        ..Layer::default()
    },
    inline_id: None,
    inline_secret: None,
    expected: Some(("overlay-id", "overlay-secret")),
})]
#[case::builtin_used_when_nothing_else_is_configured(Scenario {
    secret_name: "google_oauth_token",
    layer: Layer::default(),
    inline_id: None,
    inline_secret: None,
    expected: None,
})]
fn credentials_resolve_by_precedence(#[case] scenario: Scenario) {
    let ctx = scenario.layer.context();
    let config = oauth_config(scenario.inline_id, scenario.inline_secret);
    let (client_id, client_secret) =
        resolve_oauth_client_credentials(&auth(scenario.secret_name), &config, &ctx)
            .expect("credentials must resolve");

    match scenario.expected {
        Some((expected_id, expected_secret)) => {
            assert_eq!(client_id, expected_id);
            assert_eq!(client_secret.as_deref(), Some(expected_secret));
        }
        None => {
            assert!(!client_id.is_empty());
            assert!(client_secret.is_some_and(|secret| !secret.is_empty()));
        }
    }
}

/// A missing client ID names the environment variable to set.
#[test]
fn missing_credentials_report_the_configured_environment_variable() {
    let ctx = EnvContext::default();

    let message = resolve_error("custom_oauth_token", oauth_config(None, None), &ctx);

    assert!(
        message.contains(CLIENT_ID_ENV),
        "the error must name the environment variable: {message}"
    );
}

/// Resolution must not fall back to the ambient process environment.
///
/// The snapshot supplies a client ID but no client secret. The assertion runs
/// in a child process whose ambient environment defines both credential
/// variables: an ambient fallback resolves the child's client ID and secret,
/// while the snapshot-driven path resolves only the snapshot client ID and
/// leaves the secret unset. The variables are set on the child process only,
/// so the parent never mutates its own environment.
#[test]
fn snapshot_credentials_ignore_the_ambient_environment() -> anyhow::Result<()> {
    const CHILD_MARKER: &str = "AXINITE_OAUTH_CREDENTIALS_AMBIENT_CHILD";

    if EnvContext::capture_ambient()
        .get_owned(CHILD_MARKER)
        .is_none()
    {
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "cli::tool::auth::oauth::tests::snapshot_credentials_ignore_the_ambient_environment",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_MARKER, "1")
            .env(CLIENT_ID_ENV, "ambient-id")
            .env(CLIENT_SECRET_ENV, "ambient-secret")
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "child failed: {}\n{stdout}",
            output.status
        );
        assert!(
            stdout.contains("1 passed"),
            "child did not run the test: {stdout}"
        );
        return Ok(());
    }

    // The snapshot supplies only the client ID. If resolution consulted
    // the ambient environment, it would resolve the child's ambient client
    // ID and secret instead.
    let ctx = EnvContext::default().with_env(CLIENT_ID_ENV, "snapshot-id");
    let (client_id, client_secret) = resolve_oauth_client_credentials(
        &auth("custom_oauth_token"),
        &oauth_config(None, None),
        &ctx,
    )
    .expect("the snapshot client ID must resolve");

    assert_eq!(
        client_id, "snapshot-id",
        "the ambient client ID must not be used"
    );
    assert_eq!(
        client_secret, None,
        "the ambient client secret must not be used"
    );
    Ok(())
}
