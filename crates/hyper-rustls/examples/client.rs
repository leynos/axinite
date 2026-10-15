//! Fetch one URL over HTTP or HTTPS with an optional PEM trust store.

use std::{
    env,
    error::Error,
    io::{self, BufReader},
    sync::Arc,
};

use cap_std::fs::File;
use hyper::{body::to_bytes, client, Body, Uri};
use hyper_rustls::ConfigBuilderExt;
use rustls::{crypto::ring, ClientConfig, RootCertStore};

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
        let mut reader = BufReader::new(file);
        let certificates = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
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
