use ah_vfs::application::driver_error::DriverError;
use ah_vfs::application::driver_error::{LifecycleError, LifecycleFailure};
use std::{
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore},
    time::Instant,
};
use tokio_stream::{Stream, wrappers::ReceiverStream};
use tonic::{
    Status,
    transport::server::{Connected, TcpConnectInfo},
};

/// Shared admission limits for one plugin server, across all connections.
/// `serve_*` also applies connection and HTTP/2 limits. Custom server builders
/// using only `service_with_limits` must bound their own incoming connections.
#[derive(Clone, Debug)]
pub struct ServerLimits {
    pub max_connections: usize,
    /// Includes protobuf decoding and the full lifetime of a streaming response.
    pub max_concurrent_requests: usize,
    pub max_streams_per_connection: u32,
    /// Counts RPC admission and every Storage session frame, including failures.
    pub operations_per_second: u32,
    pub operation_burst: u32,
    /// Maximum time for one backend operation or TLS handshake.
    pub request_timeout: Duration,
    /// Maximum wait for the opening frame or a blocked downstream consumer.
    /// Established sessions have no idle expiry.
    pub stream_idle_timeout: Duration,
    /// Optional forced connection rotation; disabled for long-lived sessions.
    pub max_connection_age: Option<Duration>,
}
impl Default for ServerLimits {
    fn default() -> Self {
        Self {
            max_connections: 64,
            max_concurrent_requests: 128,
            max_streams_per_connection: 32,
            operations_per_second: 1000,
            operation_burst: 2000,
            request_timeout: Duration::from_secs(30),
            stream_idle_timeout: Duration::from_secs(30),
            max_connection_age: None,
        }
    }
}
impl ServerLimits {
    pub fn validate(&self) -> Result<(), DriverError> {
        if self.max_connections == 0
            || self.max_connections > Semaphore::MAX_PERMITS
            || self.max_concurrent_requests == 0
            || self.max_concurrent_requests > Semaphore::MAX_PERMITS
            || self.max_streams_per_connection == 0
            || self.operations_per_second == 0
            || self.operation_burst == 0
            || self.request_timeout.is_zero()
            || self.stream_idle_timeout.is_zero()
            || self.max_connection_age.is_some_and(|age| age.is_zero())
        {
            return Err(LifecycleError::new(LifecycleFailure::InvalidRequest).into());
        }
        Ok(())
    }
}
struct Bucket {
    tokens: f64,
    updated: Instant,
}
pub(crate) struct Admission {
    slots: Arc<Semaphore>,
    bucket: Mutex<Bucket>,
    pub limits: ServerLimits,
}
impl Admission {
    pub fn new(limits: ServerLimits) -> Result<Self, DriverError> {
        limits.validate()?;
        Ok(Self {
            slots: Arc::new(Semaphore::new(limits.max_concurrent_requests)),
            bucket: Mutex::new(Bucket {
                tokens: f64::from(limits.operation_burst),
                updated: Instant::now(),
            }),
            limits,
        })
    }
    pub fn operation(&self) -> Result<(), Status> {
        let mut bucket = self
            .bucket
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        let refill = now.duration_since(bucket.updated).as_secs_f64()
            * f64::from(self.limits.operations_per_second);
        bucket.tokens = (bucket.tokens + refill).min(f64::from(self.limits.operation_burst));
        bucket.updated = now;
        if bucket.tokens < 1.0 {
            return Err(Status::resource_exhausted("plugin operation rate exceeded"));
        }
        bucket.tokens -= 1.0;
        Ok(())
    }
    pub fn acquire(&self) -> Result<OwnedSemaphorePermit, Status> {
        // Failed authentication and saturated callers also consume rate budget.
        self.operation()?;
        self.slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Status::resource_exhausted("plugin concurrency limit exceeded"))
    }
}

pub(crate) struct SessionStream<T> {
    pub inner: ReceiverStream<Result<T, Status>>,
    pub permit: Option<Arc<OwnedSemaphorePermit>>,
}
impl<T> Stream for SessionStream<T> {
    type Item = Result<T, Status>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let result = Pin::new(&mut self.inner).poll_next(cx);
        if matches!(result, Poll::Ready(None)) {
            self.permit.take();
        }
        result
    }
}

/// The permit follows the accepted IO through TLS and HTTP/2 and is released
/// only when that connection is dropped, including failed TLS handshakes.
pub(crate) struct Connection {
    stream: TcpStream,
    _permit: OwnedSemaphorePermit,
}
impl Connected for Connection {
    type ConnectInfo = TcpConnectInfo;
    fn connect_info(&self) -> Self::ConnectInfo {
        self.stream.connect_info()
    }
}
impl AsyncRead for Connection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}
impl AsyncWrite for Connection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
pub(crate) fn incoming(
    listener: TcpListener,
    max_connections: usize,
) -> impl Stream<Item = io::Result<Connection>> {
    let slots = Arc::new(Semaphore::new(max_connections));
    futures_util::stream::unfold((listener, slots), |(listener, slots)| async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    if let Ok(permit) = slots.clone().try_acquire_owned() {
                        return Some((
                            Ok(Connection {
                                stream,
                                _permit: permit,
                            }),
                            (listener, slots),
                        ));
                    }
                    drop(stream);
                    // Bound work per executor turn when rejecting a connection flood.
                    tokio::task::yield_now().await;
                }
                Err(error) => return Some((Err(error), (listener, slots))),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_stream::StreamExt;
    type Result = std::result::Result<(), Box<dyn std::error::Error>>;

    #[tokio::test(start_paused = true)]
    async fn rate_budget_refills_and_never_exceeds_the_configured_burst() -> Result {
        let admission = Admission::new(ServerLimits {
            operations_per_second: 2,
            operation_burst: 2,
            ..Default::default()
        })?;
        admission.operation()?;
        admission.operation()?;
        assert_eq!(
            admission.operation().err().ok_or("rate limit")?.code(),
            tonic::Code::ResourceExhausted
        );
        tokio::time::advance(Duration::from_millis(500)).await;
        admission.operation()?;
        assert!(admission.operation().is_err());
        tokio::time::advance(Duration::from_secs(3600)).await;
        admission.operation()?;
        admission.operation()?;
        assert!(admission.operation().is_err());
        Ok(())
    }

    #[tokio::test]
    async fn session_responses_hold_concurrency_until_dropped_or_consumed() -> Result {
        let admission = Admission::new(ServerLimits {
            max_concurrent_requests: 1,
            ..Default::default()
        })?;
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let stream: SessionStream<()> = SessionStream {
            inner: ReceiverStream::new(rx),
            permit: Some(Arc::new(admission.acquire()?)),
        };
        assert!(admission.acquire().is_err());
        drop(stream);
        let permit = admission.acquire()?;
        let (tx2, rx2) = tokio::sync::mpsc::channel(1);
        let mut stream: SessionStream<()> = SessionStream {
            inner: ReceiverStream::new(rx2),
            permit: Some(Arc::new(permit)),
        };
        drop(tx2);
        assert!(stream.next().await.is_none());
        assert!(admission.acquire().is_ok());
        drop(tx);
        Ok(())
    }

    #[test]
    fn invalid_limits_are_rejected_before_starting_the_server() {
        for limits in [
            ServerLimits {
                max_connections: 0,
                ..Default::default()
            },
            ServerLimits {
                max_connections: Semaphore::MAX_PERMITS + 1,
                ..Default::default()
            },
            ServerLimits {
                max_concurrent_requests: 0,
                ..Default::default()
            },
            ServerLimits {
                max_streams_per_connection: 0,
                ..Default::default()
            },
            ServerLimits {
                operations_per_second: 0,
                ..Default::default()
            },
            ServerLimits {
                operation_burst: 0,
                ..Default::default()
            },
            ServerLimits {
                request_timeout: Duration::ZERO,
                ..Default::default()
            },
            ServerLimits {
                stream_idle_timeout: Duration::ZERO,
                ..Default::default()
            },
            ServerLimits {
                max_connection_age: Some(Duration::ZERO),
                ..Default::default()
            },
        ] {
            assert!(limits.validate().is_err());
        }
    }

    #[tokio::test]
    async fn connection_permits_reject_excess_sockets_and_release_on_drop() -> Result {
        use tokio::io::AsyncReadExt;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let mut incoming = Box::pin(incoming(listener, 1));
        let first_peer = TcpStream::connect(address).await?;
        let first = incoming.next().await.ok_or("first connection")??;
        let mut second_peer = TcpStream::connect(address).await?;
        let mut next = Box::pin(incoming.next());
        std::future::poll_fn(|cx| {
            assert!(next.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        drop(next);
        let mut data = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), second_peer.read(&mut data)).await??,
            0
        );
        drop(first);
        let third_peer = TcpStream::connect(address).await?;
        let third = tokio::time::timeout(Duration::from_secs(1), incoming.next())
            .await?
            .ok_or("third connection")??;
        assert_eq!(
            third.connect_info().remote_addr(),
            Some(third_peer.local_addr()?)
        );
        drop((first_peer, third_peer, third));
        Ok(())
    }
}
