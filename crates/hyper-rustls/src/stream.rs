//! Forward socket I/O and Hyper connection metadata across HTTP and TLS streams.

// Copied from hyperium/hyper-tls#62e3376/src/stream.rs
use std::fmt;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use hyper::client::connect::{Connected, Connection};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_rustls::client::TlsStream;
use tokio_util::either::Either;

/// A stream that might be protected with TLS.
#[allow(clippy::large_enum_variant)]
pub enum MaybeHttpsStream<T> {
    /// A stream over plain text.
    Http(T),
    /// A stream protected with TLS.
    Https(TlsStream<T>),
}

impl<T> MaybeHttpsStream<T> {
    /// Reuse Tokio's I/O adapter while retaining the public stream variants.
    fn io_mut(&mut self) -> Either<&mut T, &mut TlsStream<T>> {
        match self {
            Self::Http(stream) => Either::Left(stream),
            Self::Https(stream) => Either::Right(stream),
        }
    }
}

impl<T: AsyncRead + AsyncWrite + Connection + Unpin> Connection for MaybeHttpsStream<T> {
    fn connected(&self) -> Connected {
        match self {
            Self::Http(s) => s.connected(),
            Self::Https(s) => {
                let (tcp, tls) = s.get_ref();
                if tls.alpn_protocol() == Some(b"h2") {
                    tcp.connected().negotiated_h2()
                } else {
                    tcp.connected()
                }
            }
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for MaybeHttpsStream<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Http(..) => f.pad("Http(..)"),
            Self::Https(..) => f.pad("Https(..)"),
        }
    }
}

impl<T> From<T> for MaybeHttpsStream<T> {
    fn from(inner: T) -> Self {
        Self::Http(inner)
    }
}

impl<T> From<TlsStream<T>> for MaybeHttpsStream<T> {
    fn from(inner: TlsStream<T>) -> Self {
        Self::Https(inner)
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncRead for MaybeHttpsStream<T> {
    #[inline]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.get_mut().io_mut()).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + AsyncRead + Unpin> AsyncWrite for MaybeHttpsStream<T> {
    #[inline]
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.get_mut().io_mut()).poll_write(cx, buf)
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.get_mut().io_mut()).poll_flush(cx)
    }

    #[inline]
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.get_mut().io_mut()).poll_shutdown(cx)
    }
}
