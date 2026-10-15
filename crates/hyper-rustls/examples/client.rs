//! Fetch one URL over HTTP or HTTPS with an optional PEM trust store.

use std::{env, error::Error, io, sync::Arc};

use cap_std::fs::File;
use hyper::{body::to_bytes, client, Body, Uri};
use hyper_rustls::ConfigBuilderExt;
use rustls::{crypto::ring, ClientConfig, RootCertStore};

mod support;

use support::read_certificates;

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run_client())
}

async fn run_client() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut arguments = env::args().skip(1);
    let url = arguments.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: client <url> [ca_store]",
        )
    })?;
    let url = url.parse::<Uri>()?;
    let tls = client_config(arguments.next().as_deref())?;

    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls)
        .https_or_http()
        .enable_http1()
        .build();
    let client: client::Client<_, Body> = client::Client::builder().build(connector);
    let response = client.get(url).await?;

    println!("Status:\n{}", response.status());
    println!("Headers:\n{:#?}", response.headers());

    let body = to_bytes(response.into_body()).await?;
    println!("Body:\n{}", String::from_utf8_lossy(&body));
    Ok(())
}

fn client_config(
    custom_ca_store: Option<&str>,
) -> Result<ClientConfig, Box<dyn Error + Send + Sync>> {
    let provider = Arc::new(ring::default_provider());
    let builder =
        ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions()?;

    let config = if let Some(path) = custom_ca_store {
        // The user selected this PEM path; ambient access preserves its usual
        // absolute or working-directory-relative command-line meaning.
        let file = File::open_ambient(path, cap_std::ambient_authority())?;
        let certificates = read_certificates(file)?;
        let mut roots = RootCertStore::empty();
        let (valid, _) = roots.add_parsable_certificates(certificates);
        if valid == 0 {
            return Err(io::Error::other(format!(
                "the PEM trust store {path} contains no usable certificates"
            ))
            .into());
        }
        builder
            .with_root_certificates(roots)
            .with_no_client_auth()
    } else {
        builder
            .with_native_roots()?
            .with_no_client_auth()
    };

    Ok(config)
}

#[cfg(test)]
mod tests {
    //! Verify PEM trust-store parsing without making a network request.

    use std::io;

    use super::client_config;

    #[test]
    fn sample_pem_trust_store_loads() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        client_config(Some(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/sample.pem"
        )))?;
        Ok(())
    }

    #[rstest::rstest]
    #[case::private_key("examples/sample.rsa")]
    #[case::empty("examples/support/empty.pem")]
    fn unusable_pem_trust_store_is_rejected(#[case] relative_path: &str) {
        let path = format!("{}/{relative_path}", env!("CARGO_MANIFEST_DIR"));
        let error = client_config(Some(&path))
            .expect_err("PEM input without certificates cannot supply trust anchors");

        assert_eq!(
            error
                .downcast_ref::<io::Error>()
                .map(io::Error::kind),
            Some(io::ErrorKind::Other)
        );
    }

    #[test]
    fn malformed_pem_retains_the_io_error_type() {
        let error = client_config(Some(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/support/malformed-certificate.pem"
        )))
        .expect_err("malformed PEM must fail before a request is made");
        assert_eq!(
            error
                .downcast_ref::<io::Error>()
                .map(io::Error::kind),
            Some(io::ErrorKind::InvalidData)
        );
    }
}
