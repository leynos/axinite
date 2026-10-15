//! Share maintained PEM decoding and preserve filesystem I/O error kinds.

use std::io;

use pki_types::{pem, pem::PemObject, CertificateDer};

/// Decode certificates while preserving the error contract used by both drivers.
pub(super) fn read_certificates(reader: impl io::Read) -> io::Result<Vec<CertificateDer<'static>>> {
    CertificateDer::pem_reader_iter(reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(pem_read_error)
}

/// Keep I/O failures intact and classify malformed or missing PEM as invalid data.
pub(super) fn pem_read_error(error: pem::Error) -> io::Error {
    match error {
        pem::Error::Io(error) => error,
        error => io::Error::new(io::ErrorKind::InvalidData, error),
    }
}
