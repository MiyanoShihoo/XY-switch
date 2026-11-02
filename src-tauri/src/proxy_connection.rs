use crate::governance::tasks::{Cancellation, RunGuard};
use axum::body::Body;
use axum::extract::connect_info::Connected;
use axum::response::Response;
use axum::serve::{IncomingStream, Listener};
use futures_util::StreamExt;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};

pub(crate) struct CancelListener(pub TcpListener);
pub(crate) struct CancelIo {
    stream: TcpStream,
    cancellation: Cancellation,
}

impl Listener for CancelListener {
    type Io = CancelIo;
    type Addr = SocketAddr;
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let (stream, address) = <TcpListener as Listener>::accept(&mut self.0).await;
        (
            CancelIo {
                stream,
                cancellation: Cancellation::default(),
            },
            address,
        )
    }
    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.0.local_addr()
    }
}

impl Connected<IncomingStream<'_, CancelListener>> for Cancellation {
    fn connect_info(stream: IncomingStream<'_, CancelListener>) -> Self {
        stream.io().cancellation.clone()
    }
}

impl AsyncRead for CancelIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buffer.filled().len();
        let available = buffer.remaining();
        let result = Pin::new(&mut self.stream).poll_read(context, buffer);
        if matches!(&result, Poll::Ready(Err(_)))
            || matches!(&result, Poll::Ready(Ok(())))
                && available > 0
                && buffer.filled().len() == before
        {
            self.cancellation.cancel();
        }
        result
    }
}
impl AsyncWrite for CancelIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.stream).poll_write(context, buffer);
        if matches!(&result, Poll::Ready(Err(_))) {
            self.cancellation.cancel();
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let result = Pin::new(&mut self.stream).poll_flush(context);
        if matches!(&result, Poll::Ready(Err(_))) {
            self.cancellation.cancel();
        }
        result
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(context)
    }
}
impl Drop for CancelIo {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

pub(crate) fn track_response(response: Response, guard: RunGuard) -> Response {
    let (parts, body) = response.into_parts();
    let stream = futures_util::stream::unfold(
        (body.into_data_stream(), guard),
        |(mut body, mut guard)| async move {
            let next = tokio::select! {
                biased;
                _ = guard.context.connection.cancelled() => return None,
                _ = guard.context.signal.cancelled() => return None,
                next = body.next() => next,
            };
            match next {
                Some(chunk) => {
                    if chunk.is_err() {
                        guard.finish();
                    }
                    Some((chunk, (body, guard)))
                }
                None => {
                    guard.finish();
                    None
                }
            }
        },
    );
    Response::from_parts(parts, Body::from_stream(stream))
}
