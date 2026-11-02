//! Bound API connections for their entire lifetime, including slow request
//! bodies and slow response readers. Permits are not released at header time.
use std::{
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{self, AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{Semaphore, SemaphorePermit},
};
static CONNECTIONS: Semaphore = Semaphore::const_new(16);
pub(super) struct BoundedListener(pub TcpListener);
pub(super) struct Connection {
    stream: TcpStream,
    _permit: SemaphorePermit<'static>,
    idle: Pin<Box<tokio::time::Sleep>>,
}
impl Connection {
    fn check_idle(&mut self, cx: &mut Context<'_>) -> io::Result<()> {
        use std::future::Future;
        if self.idle.as_mut().poll(cx).is_ready() {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "API connection idle deadline",
            ))
        } else {
            Ok(())
        }
    }
    fn active(&mut self) {
        self.idle
            .as_mut()
            .reset(tokio::time::Instant::now() + Duration::from_secs(60));
    }
}
impl AsyncRead for Connection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.check_idle(cx)?;
        let before = buf.filled().len();
        let result = Pin::new(&mut self.stream).poll_read(cx, buf);
        if matches!(result, Poll::Ready(Ok(()))) && buf.filled().len() > before {
            self.active();
        }
        result
    }
}
impl AsyncWrite for Connection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.check_idle(cx)?;
        let result = Pin::new(&mut self.stream).poll_write(cx, buf);
        if matches!(result, Poll::Ready(Ok(n)) if n > 0) {
            self.active();
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.check_idle(cx)?;
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
impl axum::serve::Listener for BoundedListener {
    type Io = Connection;
    type Addr = SocketAddr;
    async fn accept(&mut self) -> (Connection, SocketAddr) {
        loop {
            match self.0.accept().await {
                Ok((stream, addr)) => {
                    let Ok(permit) = CONNECTIONS.try_acquire() else {
                        continue;
                    };
                    return (
                        Connection {
                            stream,
                            _permit: permit,
                            idle: Box::pin(tokio::time::sleep(Duration::from_secs(60))),
                        },
                        addr,
                    );
                }
                Err(error) => {
                    tracing::warn!(%error, "API accept failed");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
    }
}
