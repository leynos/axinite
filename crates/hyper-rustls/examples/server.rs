//! Serve a small HTTPS echo endpoint with a PEM certificate and key.

#![cfg(feature = "acceptor")]

use std::{env, io, net::SocketAddr};

use cap_std::fs::File;
use hyper::server::conn::AddrIncoming;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use hyper_rustls::TlsAcceptor;
use pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};

mod support;

use support::{pem_read_error, read_certificates};

/// Start the current-thread runtime used by the TLS server example.
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run_server())
}

/// Load the sample PEM identity and serve the echo endpoint on loopback.
async fn run_server() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = env::args()
        .nth(1)
        .map_or(Ok(1337), |value| value.parse::<u16>())?;
    let address = SocketAddr::from(([127, 0, 0, 1], port));

    let certificates =
        load_certificates(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/sample.pem"))?;
    let private_key =
        load_private_key(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/sample.rsa"))?;
    let incoming = AddrIncoming::bind(&address)?;
    let acceptor = TlsAcceptor::builder()
        .with_single_cert(certificates, private_key)?
        .with_all_versions_alpn()
        .with_incoming(incoming);
    let service = make_service_fn(|_| async { Ok::<_, io::Error>(service_fn(echo)) });

    println!("Starting to serve on https://{address}.");
    Server::builder(acceptor)
        .serve(service)
        .await?;
    Ok(())
}

/// Return a greeting for `GET /`, echo the body for `POST /echo`, or a 404.
async fn echo(request: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    let mut response = Response::new(Body::empty());
    match (request.method(), request.uri().path()) {
        (&Method::GET, "/") => *response.body_mut() = Body::from("Try POST /echo\n"),
        (&Method::POST, "/echo") => *response.body_mut() = request.into_body(),
        _ => *response.status_mut() = StatusCode::NOT_FOUND,
    }
    Ok(response)
}

/// Read certificates from a caller-relative PEM path, rejecting an empty store.
fn load_certificates(path: &str) -> io::Result<Vec<CertificateDer<'static>>> {
    // Resolve the path as selected by the caller.
    let file = File::open_ambient(path, cap_std::ambient_authority())?;
    let certificates = read_certificates(file)?;
    if certificates.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the PEM file {path} contains no certificates"),
        ));
    }
    Ok(certificates)
}

/// Read one private key while retaining I/O errors and classifying bad PEM.
fn load_private_key(path: &str) -> io::Result<PrivateKeyDer<'static>> {
    // Resolve the path as selected by the caller.
    let file = File::open_ambient(path, cap_std::ambient_authority())?;
    PrivateKeyDer::from_pem_reader(file).map_err(pem_read_error)
}

#[cfg(test)]
mod tests {
    //! Check maintained PEM parsing and retain the underlying I/O error kind.

    use std::io;

    use super::{load_certificates, load_private_key, read_certificates};

    const CERTIFICATE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/sample.pem");
    const PRIVATE_KEY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/sample.rsa");
    const EMPTY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/support/empty.pem");

    /// Load the maintained certificate and private-key sample files.
    #[test]
    fn sample_pem_material_loads() -> io::Result<()> {
        assert!(!load_certificates(CERTIFICATE)?.is_empty());
        load_private_key(PRIVATE_KEY)?;
        Ok(())
    }

    /// Classify PEM inputs without the requested material as invalid data.
    #[rstest::rstest]
    #[case::mismatched(CERTIFICATE, PRIVATE_KEY)]
    #[case::empty(EMPTY, EMPTY)]
    fn unsupported_pem_inputs_are_invalid_data(
        #[case] key_path: &str,
        #[case] certificate_path: &str,
    ) {
        assert_eq!(
            load_private_key(key_path)
                .expect_err("PEM input does not contain a private key")
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            load_certificates(certificate_path)
                .expect_err("PEM input does not contain a certificate")
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    /// Keep a reader's original I/O error kind when certificate parsing fails.
    #[test]
    fn pem_io_error_kind_is_preserved() {
        struct Unreadable;

        impl io::Read for Unreadable {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "the selected PEM input cannot be read",
                ))
            }
        }

        let error = read_certificates(Unreadable)
            .expect_err("the underlying reader's failure must propagate");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }
}

#[cfg(test)]
#[path = "server/behaviour_tests.rs"]
mod behaviour_tests;
