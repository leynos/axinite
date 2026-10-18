//! Exercise process-wide provider choices without sharing TLS globals between tests.

use std::process::Command;

use rustls::crypto::CryptoProvider;
use rustls::ClientConfig;

use super::builder::ConnectorBuilder;
use crate::config::ConfigBuilderExt;

/// Run one ignored provider scenario in a child process so rustls globals start clean.
fn run_isolated(test_name: &str) {
    let output = Command::new(std::env::current_exe().expect("test executable is available"))
        .args(["--exact", test_name, "--ignored", "--nocapture"])
        .output()
        .expect("isolated provider test starts");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "isolated provider scenario {test_name} failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("1 passed;"),
        "isolated provider scenario {test_name} did not run exactly one test\n{stdout}"
    );
}

/// Compare the active key provider with the expected provider.
fn assert_uses_provider(config: &ClientConfig, expected: &CryptoProvider) {
    assert!(std::ptr::eq(
        config.crypto_provider().key_provider,
        expected.key_provider
    ));
}

/// Build with an explicit provider and confirm no process default is installed.
fn explicit_provider_contract(provider: CryptoProvider) -> Result<(), rustls::Error> {
    assert!(CryptoProvider::get_default().is_none());
    let expected = provider.clone();
    let connector = ConnectorBuilder::new()
        .with_provider_and_webpki_roots(provider)?
        .https_only()
        .enable_http1()
        .build();
    assert!(CryptoProvider::get_default().is_none());
    assert_uses_provider(&connector.tls_config, &expected);
    Ok(())
}

/// Build the library's native-root connector with its explicit Ring policy.
fn native_connector() -> super::HttpsConnector<hyper::client::HttpConnector> {
    ConnectorBuilder::new()
        .with_native_roots()
        .expect("native roots are available")
        .https_only()
        .enable_http1()
        .build()
}

/// Verify native-root shorthand selects Ring without installing a process default.
#[test]
fn native_roots_keep_the_ring_policy_without_a_global_default() {
    run_isolated("connector::provider_tests::native_ring_policy_child");
}

/// Verify explicit Ring works with WebPKI roots without a process default.
#[test]
fn explicit_ring_webpki_provider_does_not_need_a_global_default() {
    run_isolated("connector::provider_tests::explicit_ring_provider_child");
}

/// Verify WebPKI shorthand uses the installed AWS-LC default provider.
#[cfg(feature = "aws-lc-rs")]
#[test]
fn webpki_shorthand_uses_the_installed_process_default() {
    run_isolated("connector::provider_tests::installed_default_provider_child");
}

/// Verify explicit AWS-LC works without installing a process default.
#[cfg(feature = "aws-lc-rs")]
#[test]
fn explicit_aws_lc_provider_does_not_need_a_global_default() {
    run_isolated("connector::provider_tests::explicit_aws_lc_provider_child");
}

/// Verify WebPKI shorthand honours an installed Ring process default.
#[test]
fn webpki_shorthand_uses_an_installed_ring_default() {
    run_isolated("connector::provider_tests::ring_default_provider_child");
}

/// Check native-root construction keeps Ring selected without a process default.
#[test]
#[ignore = "invoked by a parent test in its own process"]
fn native_ring_policy_child() {
    assert!(CryptoProvider::get_default().is_none());
    let connector = native_connector();
    assert!(CryptoProvider::get_default().is_none());
    assert_uses_provider(
        &connector.tls_config,
        &rustls::crypto::ring::default_provider(),
    );

    #[cfg(feature = "aws-lc-rs")]
    assert!(!std::ptr::eq(
        rustls::crypto::ring::default_provider().key_provider,
        rustls::crypto::aws_lc_rs::default_provider().key_provider
    ));
}

/// Check explicit Ring construction leaves the process default unset.
#[test]
#[ignore = "invoked by a parent test in its own process"]
fn explicit_ring_provider_child() -> Result<(), rustls::Error> {
    explicit_provider_contract(rustls::crypto::ring::default_provider())
}

/// Check WebPKI shorthand uses the installed provider while native roots keep Ring.
fn installed_provider_contract(expected: CryptoProvider) {
    assert!(CryptoProvider::get_default().is_none());
    CryptoProvider::install_default(expected.clone())
        .expect("the child process has no installed provider yet");
    let default = CryptoProvider::get_default().expect("provider was installed");
    assert_uses_provider(
        &ClientConfig::builder()
            .with_webpki_roots()
            .with_no_client_auth(),
        &expected,
    );

    let connector = ConnectorBuilder::new()
        .with_webpki_roots()
        .https_only()
        .enable_http1()
        .build();
    let native = native_connector();
    assert_uses_provider(
        &native.tls_config,
        &rustls::crypto::ring::default_provider(),
    );
    assert!(std::sync::Arc::ptr_eq(
        default,
        connector.tls_config.crypto_provider()
    ));
    assert!(std::sync::Arc::ptr_eq(
        default,
        CryptoProvider::get_default().expect("the installed process default is unchanged")
    ));
}

/// Check WebPKI shorthand uses an installed AWS-LC process default.
#[cfg(feature = "aws-lc-rs")]
#[test]
#[ignore = "invoked by a parent test in its own process"]
fn installed_default_provider_child() {
    installed_provider_contract(rustls::crypto::aws_lc_rs::default_provider());
}

/// Check explicit AWS-LC construction leaves the process default unset.
#[cfg(feature = "aws-lc-rs")]
#[test]
#[ignore = "invoked by a parent test in its own process"]
fn explicit_aws_lc_provider_child() -> Result<(), rustls::Error> {
    explicit_provider_contract(rustls::crypto::aws_lc_rs::default_provider())
}

/// Check WebPKI shorthand uses an installed Ring process default.
#[test]
#[ignore = "invoked by a parent test in its own process"]
fn ring_default_provider_child() {
    installed_provider_contract(rustls::crypto::ring::default_provider());
}
