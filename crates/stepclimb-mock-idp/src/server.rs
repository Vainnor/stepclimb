// SPDX-License-Identifier: AGPL-3.0-or-later
//! The listener that lets dropping the mock cut every connection at once, kept-alive ones
//! included. axum serves each connection on a task of its own that aborting the accept loop does
//! not reach, so each connection's socket fails its next read or write once the mock is dropped,
//! and the connection task ends.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

/// Resolves once the mock is dropped: the drop takes the [`watch::Sender`] with it, and this
/// receiver's `changed` fails. Nothing is ever sent.
#[derive(Clone)]
pub(crate) struct Stop(watch::Receiver<()>);

impl Stop {
    /// A stop signal and the sender whose drop fires it.
    pub(crate) fn new() -> (watch::Sender<()>, Self) {
        let (sender, receiver) = watch::channel(());
        (sender, Self(receiver))
    }

    pub(crate) fn stopped(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut receiver = self.0.clone();
        async move { while receiver.changed().await.is_ok() {} }
    }
}

/// A TCP listener whose connections fail once the mock is dropped.
pub(crate) struct CutListener {
    pub(crate) inner: TcpListener,
    pub(crate) stop: Stop,
}

impl axum::serve::Listener for CutListener {
    type Io = CutStream;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let (stream, addr) = axum::serve::Listener::accept(&mut self.inner).await;
        let stream = CutStream {
            inner: stream,
            stopped: Box::pin(self.stop.stopped()),
            cut: false,
        };
        (stream, addr)
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

/// A connection that answers every read and write with `ConnectionAborted` once stopped.
pub(crate) struct CutStream {
    inner: TcpStream,
    stopped: Pin<Box<dyn Future<Output = ()> + Send>>,
    cut: bool,
}

impl CutStream {
    /// Whether the mock is gone. Polling registers the task's waker, so an idle connection
    /// waiting to read wakes when the mock is dropped.
    fn is_cut(&mut self, cx: &mut Context<'_>) -> bool {
        if !self.cut && self.stopped.as_mut().poll(cx).is_ready() {
            self.cut = true;
        }
        self.cut
    }
}

fn aborted<T>() -> Poll<io::Result<T>> {
    Poll::Ready(Err(io::Error::from(io::ErrorKind::ConnectionAborted)))
}

impl AsyncRead for CutStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.is_cut(cx) {
            return aborted();
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for CutStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.is_cut(cx) {
            return aborted();
        }
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.is_cut(cx) {
            return aborted();
        }
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
