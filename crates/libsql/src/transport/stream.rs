//! Present modern Tokio TLS streams through libSQL's legacy Hyper I/O traits.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use hyper::client::connect::{Connected, Connection};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

/// Socket returned to the SDK, preserving the underlying connection metadata.
pub(crate) enum Stream {
    Http(TcpStream),
    Https(Box<TlsStream<TcpStream>>),
}

trait SocketIo: AsyncRead + AsyncWrite + Unpin {}
impl<T: AsyncRead + AsyncWrite + Unpin> SocketIo for T {}

impl Stream {
    pub(super) fn tls(stream: TlsStream<TcpStream>) -> Self {
        Self::Https(Box::new(stream))
    }

    fn io(&mut self) -> Pin<&mut dyn SocketIo> {
        match self {
            Self::Http(stream) => Pin::new(stream),
            Self::Https(stream) => Pin::new(stream.as_mut()),
        }
    }
}

impl Connection for Stream {
    fn connected(&self) -> Connected {
        match self {
            Self::Http(stream) => stream.connected(),
            Self::Https(stream) => stream.get_ref().0.connected(),
        }
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.get_mut().io().poll_read(cx, buf)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.get_mut().io().poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().io().poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().io().poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.get_mut().io().poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        match self {
            Self::Http(stream) => stream.is_write_vectored(),
            Self::Https(stream) => stream.is_write_vectored(),
        }
    }
}
