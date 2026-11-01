//! Native plugin server SDK. No dependency on the host's Driver/Storage proxies.
pub mod dispatch;
mod limits;
mod security;
pub use limits::ServerLimits;

use ah_vfs::{
    application::port::Driver,
    domain::{DriverError, StorageError, StorageErrorKind},
};
use dispatch::Dispatcher;
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::{
    net::TcpListener,
    sync::{OwnedSemaphorePermit, mpsc},
};
use tokio_stream::{Stream, wrappers::ReceiverStream};
use tonic::{Request, Response, Status, transport::Server};
use vfs_plugin_codec as c;
pub use vfs_plugin_grpc_wire as wire;
use vfs_plugin_protocol::{self as p, Operation as Op, Value};

type ReadStream = Pin<Box<dyn Stream<Item = Result<wire::Reply, Status>> + Send>>;
pub type GrpcService = tonic::service::interceptor::InterceptedService<
    wire::plugin_server::PluginServer<PluginService>,
    AdmissionInterceptor,
>;

#[derive(Clone)]
struct RequestPermit(Arc<OwnedSemaphorePermit>);

/// Admit and authenticate at the HTTP headers, before decoding a protobuf body.
#[derive(Clone)]
pub struct AdmissionInterceptor(PluginService);
impl tonic::service::Interceptor for AdmissionInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let permit = self.0.admit(&request)?;
        request.extensions_mut().insert(RequestPermit(permit));
        Ok(request)
    }
}

#[derive(Clone)]
pub struct PluginService {
    dispatcher: Arc<Dispatcher>,
    token: security::AuthToken,
    admission: Arc<limits::Admission>,
}
impl PluginService {
    pub fn new(dispatcher: Arc<Dispatcher>, token: &str) -> Result<Self, DriverError> {
        Self::with_limits(dispatcher, token, ServerLimits::default())
    }
    pub fn with_limits(
        dispatcher: Arc<Dispatcher>,
        token: &str,
        limits: ServerLimits,
    ) -> Result<Self, DriverError> {
        Ok(Self {
            dispatcher,
            token: security::AuthToken::new(token)?,
            admission: Arc::new(limits::Admission::new(limits)?),
        })
    }
    fn admit<T>(&self, r: &Request<T>) -> Result<Arc<OwnedSemaphorePermit>, Status> {
        if let Some(permit) = r.extensions().get::<RequestPermit>() {
            return Ok(permit.0.clone());
        }
        let permit = Arc::new(self.admission.acquire()?);
        if !self.token.accepts(r.metadata()) {
            return Err(Status::unauthenticated("plugin authentication failed"));
        }
        Ok(permit)
    }
    fn decode(&self, request: wire::Request) -> Result<(String, Op, Vec<u8>), Status> {
        p::from_request(request).map_err(|error| match error {
            p::CodecError::TooLarge => Status::resource_exhausted("plugin frame is too large"),
            p::CodecError::InvalidData => Status::invalid_argument("invalid plugin request"),
        })
    }
}
fn encode(reply: Result<p::Reply, DriverError>) -> Result<wire::Reply, Status> {
    p::to_reply(reply.map_err(c::failure))
        .map_err(|_| Status::resource_exhausted("invalid or oversized plugin response"))
}

#[tonic::async_trait]
impl wire::plugin_server::Plugin for PluginService {
    async fn invoke(
        &self,
        request: Request<wire::Request>,
    ) -> Result<Response<wire::Reply>, Status> {
        let _permit = self.admit(&request)?;
        let (generation, op, data) = self.decode(request.into_inner())?;
        Ok(Response::new(encode(
            tokio::time::timeout(
                self.admission.limits.request_timeout,
                self.dispatcher.call(&generation, op, data),
            )
            .await
            .map_err(|_| Status::deadline_exceeded("plugin operation timed out"))?,
        )?))
    }
    type ReadStream = ReadStream;
    async fn read(
        &self,
        request: Request<wire::Request>,
    ) -> Result<Response<Self::ReadStream>, Status> {
        let permit = self.admit(&request)?;
        let (generation, op, data) = self.decode(request.into_inner())?;
        let Op::ReaderNext(p::Handle { handle }) = op else {
            return Err(Status::invalid_argument("expected reader operation"));
        };
        if !data.is_empty() {
            return Err(Status::invalid_argument("unexpected reader data"));
        }
        let dispatcher = self.dispatcher.clone();
        let admission = self.admission.clone();
        let task_permit = permit.clone();
        let (tx, rx) = mpsc::channel(1);
        tokio::spawn(async move {
            let _permit = task_permit;
            loop {
                if tx.is_closed() {
                    break;
                }
                if let Err(error) = admission.operation() {
                    let _ = tokio::time::timeout(
                        admission.limits.stream_idle_timeout,
                        tx.send(Err(error)),
                    )
                    .await;
                    break;
                }
                let result = tokio::select! {
                    ()=tx.closed()=>break,
                    result=tokio::time::timeout(admission.limits.request_timeout, dispatcher.call(&generation,Op::ReaderNext(p::Handle {handle}),vec![]))=>
                        result.unwrap_or_else(|_| Err(StorageError::new(StorageErrorKind::TimedOut).into())),
                };
                let done = !matches!(
                    &result,
                    Ok(p::Reply {
                        result: Some(Value::Chunk(p::Chunk { eof: false })),
                        ..
                    })
                );
                if !matches!(
                    tokio::time::timeout(
                        admission.limits.stream_idle_timeout,
                        tx.send(encode(result))
                    )
                    .await,
                    Ok(Ok(()))
                ) || done
                {
                    break;
                }
            }
            let _ = tokio::time::timeout(
                admission.limits.request_timeout,
                dispatcher.call(
                    &generation,
                    Op::Release(p::ResourceRef {
                        kind: p::ResourceKind::Reader as i32,
                        handle,
                    }),
                    vec![],
                ),
            )
            .await;
        });
        Ok(Response::new(Box::pin(limits::ReadStream {
            inner: ReceiverStream::new(rx),
            permit: Some(permit),
        })))
    }
    async fn write(
        &self,
        request: Request<tonic::Streaming<wire::Request>>,
    ) -> Result<Response<wire::Reply>, Status> {
        let _permit = self.admit(&request)?;
        let mut stream = request.into_inner();
        let mut identity = None;
        while let Some(frame) =
            tokio::time::timeout(self.admission.limits.stream_idle_timeout, stream.message())
                .await
                .map_err(|_| Status::deadline_exceeded("plugin stream idle timeout"))??
        {
            self.admission.operation()?;
            let (generation, op, data) = self.decode(frame)?;
            let Op::WriterChunk(p::Handle { handle }) = op else {
                return Err(Status::invalid_argument("expected writer operation"));
            };
            if identity
                .as_ref()
                .is_some_and(|previous| previous != &(generation.clone(), handle))
            {
                return Err(Status::invalid_argument("mixed writer resources"));
            }
            identity = Some((generation.clone(), handle));
            if let Err(error) = tokio::time::timeout(
                self.admission.limits.request_timeout,
                self.dispatcher
                    .call(&generation, Op::WriterChunk(p::Handle { handle }), data),
            )
            .await
            .map_err(|_| Status::deadline_exceeded("plugin operation timed out"))?
            {
                return Ok(Response::new(encode(Err(error))?));
            }
        }
        if identity.is_none() {
            return Err(Status::invalid_argument("empty writer stream"));
        }
        Ok(Response::new(encode(Ok(p::Reply::unit()))?))
    }
}
pub fn service(dispatcher: Arc<Dispatcher>, token: &str) -> Result<GrpcService, DriverError> {
    service_with_limits(dispatcher, token, ServerLimits::default())
}
pub fn service_with_limits(
    dispatcher: Arc<Dispatcher>,
    token: &str,
    limits: ServerLimits,
) -> Result<GrpcService, DriverError> {
    let service = PluginService::with_limits(dispatcher, token, limits)?;
    let interceptor = AdmissionInterceptor(service.clone());
    let server = wire::plugin_server::PluginServer::new(service)
        .max_decoding_message_size(p::MAX_CONTROL_BYTES + p::MAX_DATA_BYTES + 1024)
        .max_encoding_message_size(p::MAX_CONTROL_BYTES + p::MAX_DATA_BYTES + 1024);
    Ok(tonic::service::interceptor::InterceptedService::new(
        server,
        interceptor,
    ))
}
pub async fn serve(
    listener: TcpListener,
    driver: Arc<dyn Driver>,
    token: &str,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    serve_configured(listener, driver, token, None, shutdown).await
}
pub use tonic::transport::{Identity, ServerTlsConfig};
pub async fn serve_configured(
    listener: TcpListener,
    driver: Arc<dyn Driver>,
    token: &str,
    tls: Option<ServerTlsConfig>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    serve_with_limits(
        listener,
        driver,
        token,
        tls,
        ServerLimits::default(),
        shutdown,
    )
    .await
}

/// Serve with explicit transport and admission limits. Plaintext is restricted
/// to loopback listeners; all other bind addresses require TLS.
pub async fn serve_with_limits(
    listener: TcpListener,
    driver: Arc<dyn Driver>,
    token: &str,
    tls: Option<ServerTlsConfig>,
    limits: ServerLimits,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    limits.validate()?;
    if tls.is_none() && !listener.local_addr()?.ip().is_loopback() {
        return Err("remote plugin listeners require TLS".into());
    }
    let dispatcher = Arc::new(Dispatcher::new(driver)?);
    serve_dispatcher(listener, dispatcher, token, tls, limits, shutdown).await
}

/// Serve a plugin whose backend can be reconstructed after a panic. The factory
/// must create an independent driver; uncertain mutations are never retried.
pub async fn serve_with_factory(
    listener: TcpListener,
    factory: Arc<dispatch::DriverFactory>,
    token: &str,
    tls: Option<ServerTlsConfig>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    serve_with_factory_and_limits(
        listener,
        factory,
        token,
        tls,
        ServerLimits::default(),
        shutdown,
    )
    .await
}

/// Combine fresh-backend recovery with explicitly bounded server resources.
pub async fn serve_with_factory_and_limits(
    listener: TcpListener,
    factory: Arc<dispatch::DriverFactory>,
    token: &str,
    tls: Option<ServerTlsConfig>,
    limits: ServerLimits,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    limits.validate()?;
    if tls.is_none() && !listener.local_addr()?.ip().is_loopback() {
        return Err("remote plugin listeners require TLS".into());
    }
    let dispatcher = Arc::new(Dispatcher::with_factory(factory).await?);
    serve_dispatcher(listener, dispatcher, token, tls, limits, shutdown).await
}

async fn serve_dispatcher(
    listener: TcpListener,
    dispatcher: Arc<Dispatcher>,
    token: &str,
    tls: Option<ServerTlsConfig>,
    limits: ServerLimits,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let svc = service_with_limits(dispatcher.clone(), token, limits.clone())?;
    let expire = AbortOnDrop(tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            dispatcher.expire().await;
        }
    }));
    let mut server = Server::builder()
        .concurrency_limit_per_connection(limits.max_streams_per_connection as usize)
        .max_concurrent_streams(limits.max_streams_per_connection)
        .http2_max_header_list_size(16 * 1024)
        .max_connection_age(limits.max_connection_age)
        .max_connection_age_grace(limits.stream_idle_timeout)
        .load_shed(true);
    if let Some(tls) = tls {
        server = server.tls_config(tls.timeout(limits.request_timeout))?;
    }
    let result = server
        .add_service(svc)
        .serve_with_incoming_shutdown(limits::incoming(listener, limits.max_connections), shutdown)
        .await;
    drop(expire);
    result?;
    Ok(())
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
