//! Serve a small HTTPS echo endpoint with a PEM certificate and key.

#![cfg(feature = "acceptor")]

use std::{
    env,
    io::{self, BufReader},
    net::SocketAddr,
};

use cap_std::fs::File;
use hyper::server::conn::AddrIncoming;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use hyper_rustls::TlsAcceptor;
use pki_types::{CertificateDer, PrivateKeyDer};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run_server())
}

async fn run_server() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = env::args()
        .nth(1)
        .map_or(Ok(1337), |value| value.parse::<u16>())?;
    let address = SocketAddr::from(([127, 0, 0, 1], port));

    let certificates = load_certificates("examples/sample.pem")?;
    let private_key = load_private_key("examples/sample.rsa")?;
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

async fn echo(request: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    let mut response = Response::new(Body::empty());
    match (request.method(), request.uri().path()) {
        (&Method::GET, "/") => *response.body_mut() = Body::from("Try POST /echo\n"),
        (&Method::POST, "/echo") => *response.body_mut() = request.into_body(),
        _ => *response.status_mut() = StatusCode::NOT_FOUND,
    }
    Ok(response)
}

fn load_certificates(path: &str) -> io::Result<Vec<CertificateDer<'static>>> {
    // Resolve the sample PEM path from the caller's working directory.
    let file = File::open_ambient(path, cap_std::ambient_authority())?;
    let mut reader = BufReader::new(file);
    let certificates = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    if certificates.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the PEM file {path} contains no certificates"),
        ));
    }
    Ok(certificates)
}

fn load_private_key(path: &str) -> io::Result<PrivateKeyDer<'static>> {
    // Resolve the sample PEM path from the caller's working directory.
    let file = File::open_ambient(path, cap_std::ambient_authority())?;
    let mut reader = BufReader::new(file);
    rustls_pemfile::private_key(&mut reader)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the PEM file {path} contains no private key"),
        )
    })
}
