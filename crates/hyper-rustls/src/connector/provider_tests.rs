//! Exercise process-wide provider choices without sharing TLS globals between tests.

use std::process::Command;

use rustls::crypto::CryptoProvider;
use rustls::ClientConfig;

use super::builder::ConnectorBuilder;
use crate::config::ConfigBuilderExt;

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

fn assert_uses_provider(config: &ClientConfig, expected: &CryptoProvider) {
    assert!(std::ptr::eq(
        config.crypto_provider().key_provider,
        expected.key_provider
    ));
}

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

fn native_connector() -> super::HttpsConnector<hyper::client::HttpConnector> {
    ConnectorBuilder::new()
        .with_native_roots()
        .expect("native roots are available")
        .https_only()
        .enable_http1()
        .build()
}

#[test]
fn native_roots_keep_the_ring_policy_without_a_global_default() {
    run_isolated("connector::provider_tests::native_ring_policy_child");
}

#[test]
fn explicit_ring_webpki_provider_does_not_need_a_global_default() {
    run_isolated("connector::provider_tests::explicit_ring_provider_child");
}

#[cfg(feature = "aws-lc-rs")]
#[test]
fn webpki_shorthand_uses_the_installed_process_default() {
    run_isolated("connector::provider_tests::installed_default_provider_child");
}

#[cfg(feature = "aws-lc-rs")]
#[test]
fn explicit_aws_lc_provider_does_not_need_a_global_default() {
    run_isolated("connector::provider_tests::explicit_aws_lc_provider_child");
}

#[test]
fn webpki_shorthand_uses_an_installed_ring_default() {
    run_isolated("connector::provider_tests::ring_default_provider_child");
}

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

#[test]
#[ignore = "invoked by a parent test in its own process"]
fn explicit_ring_provider_child() -> Result<(), rustls::Error> {
    explicit_provider_contract(rustls::crypto::ring::default_provider())
}

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

#[cfg(feature = "aws-lc-rs")]
#[test]
#[ignore = "invoked by a parent test in its own process"]
fn installed_default_provider_child() {
    installed_provider_contract(rustls::crypto::aws_lc_rs::default_provider());
}

#[cfg(feature = "aws-lc-rs")]
#[test]
#[ignore = "invoked by a parent test in its own process"]
fn explicit_aws_lc_provider_child() -> Result<(), rustls::Error> {
    explicit_provider_contract(rustls::crypto::aws_lc_rs::default_provider())
}

#[test]
#[ignore = "invoked by a parent test in its own process"]
fn ring_default_provider_child() {
    installed_provider_contract(rustls::crypto::ring::default_provider());
}
