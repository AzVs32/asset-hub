//! Native plugin server SDK. No dependency on the host's Driver/Storage proxies.
use ah_vfs_storage::{BackendFailure, ContractViolation, Failure};
pub mod dispatch;
mod limits;
mod security;
#[cfg(feature = "storage-contract")]
pub mod storage_contract;
pub use limits::ServerLimits;

use ah_vfs::application::port::Driver;

use ah_vfs::application::driver_error::DriverError;
use dispatch::Dispatcher;
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::{
    net::TcpListener,
    sync::{OwnedSemaphorePermit, mpsc},
};
use tokio_stream::{Stream, wrappers::ReceiverStream};
use tonic::{Request, Response, Status, transport::Server};
use vfs_plugin_codec as c;
pub use vfs_plugin_grpc_wire as wire;
use vfs_plugin_protocol::{self as p, Operation as Op};

type SessionStream = Pin<Box<dyn Stream<Item = Result<wire::Reply, Status>> + Send>>;
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
            p::CodecError::InvalidData | p::CodecError::Decode(_) => {
                Status::invalid_argument("invalid plugin request")
            }
        })
    }
}
fn encode(reply: Result<p::Reply, DriverError>, op: &Op) -> Result<wire::Reply, Status> {
    let reply = reply.map_err(|error| match error {
        DriverError::Storage(error) if matches!(error.failure(), Failure::Contract(_)) => {
            c::response_error(error.operation(), op.may_publish(), error).into()
        }
        error => error,
    });
    Ok(
        p::to_reply(reply.map_err(c::failure)).unwrap_or_else(|source| {
            p::Reply::new(p::Value::Failure(c::failure(c::call_error(
                op,
                Failure::Contract(ContractViolation::InvalidResponse),
                true,
                Some(Box::new(source)),
            ))))
        }),
    )
}

#[tonic::async_trait]
impl wire::plugin_server::Plugin for PluginService {
    async fn invoke(
        &self,
        request: Request<wire::Request>,
    ) -> Result<Response<wire::Reply>, Status> {
        let _permit = self.admit(&request)?;
        let (generation, operation, data) = self.decode(request.into_inner())?;
        if operation.is_resource() || matches!(operation, Op::Open(_)) {
            return Err(Status::invalid_argument(
                "Storage requires a Session stream",
            ));
        }
        let request_context = operation.clone();
        Ok(Response::new(encode(
            tokio::time::timeout(
                self.admission.limits.request_timeout,
                self.dispatcher.call(&generation, operation, data),
            )
            .await
            .map_err(|_| Status::deadline_exceeded("plugin operation timed out"))?,
            &request_context,
        )?))
    }
    type SessionStream = SessionStream;
    async fn session(
        &self,
        request: Request<tonic::Streaming<wire::Request>>,
    ) -> Result<Response<Self::SessionStream>, Status> {
        let permit = self.admit(&request)?;
        let mut incoming = request.into_inner();
        let first = tokio::time::timeout(
            self.admission.limits.stream_idle_timeout,
            incoming.message(),
        )
        .await
        .map_err(|_| Status::deadline_exceeded("session opening timed out"))??
        .ok_or_else(|| Status::invalid_argument("missing Open"))?;
        let (generation, operation, data) = self.decode(first)?;
        let Op::Open(open) = operation else {
            return Err(Status::invalid_argument("expected Open"));
        };
        if !data.is_empty() {
            return Err(Status::invalid_argument("unexpected Open data"));
        }
        let opening_context = Op::Open(open.clone());
        let opened = tokio::time::timeout(
            self.admission.limits.request_timeout,
            self.dispatcher.open_session(&generation, open),
        )
        .await
        .map_err(|_| Status::deadline_exceeded("session opening timed out"))?;
        let (tx, rx) = mpsc::channel(1);
        let service = self.clone();
        let task_permit = permit.clone();
        tokio::spawn(async move {
            let _permit = task_permit;
            let session = match opened {
                Ok(session) => session,
                Err(error) => {
                    let _ = service
                        .send(&tx, encode(Err(error), &opening_context))
                        .await;
                    return;
                }
            };
            let mut retired = session.retired();
            if *retired.borrow() {
                return;
            }
            if !service
                .send(&tx, encode(Ok(session.opened()), &opening_context))
                .await
            {
                return;
            }
            let (requests, mut frames) = mpsc::channel(1);
            let (closed, mut disconnected) = tokio::sync::watch::channel(false);
            // Observe request half-close even while a backend call is pending.
            // The bounded queue applies backpressure to malicious pipelining.
            let _incoming = AbortOnDrop(tokio::spawn(async move {
                while let Ok(Some(frame)) = incoming.message().await {
                    if requests.send(frame).await.is_err() {
                        break;
                    }
                }
                closed.send_replace(true);
            }));
            loop {
                let frame = tokio::select! {
                    ()=tx.closed()=>break,
                    _=retired.changed()=>break,
                    _=disconnected.changed()=>break,
                    frame=frames.recv()=>match frame {Some(frame)=>frame,None=>break},
                };
                if let Err(error) = service.admission.operation() {
                    let _ = service.send(&tx, Err(error)).await;
                    break;
                }
                let (generation, operation, data) = match service.decode(frame) {
                    Ok(frame) => frame,
                    Err(error) => {
                        let _ = service.send(&tx, Err(error)).await;
                        break;
                    }
                };
                if !generation.is_empty() || !operation.is_resource() {
                    let _ = service
                        .send(
                            &tx,
                            Err(Status::invalid_argument(
                                "expected Storage session operation",
                            )),
                        )
                        .await;
                    break;
                }
                let request_context = operation.clone();
                let result = tokio::select! {
                    ()=tx.closed()=>break,
                    _=retired.changed()=>break,
                    _=disconnected.changed()=>break,
                    result=tokio::time::timeout(service.admission.limits.request_timeout,session.call(operation,data))=>
                        result.unwrap_or_else(|_|Err(c::call_error(&request_context, Failure::Backend(BackendFailure::TimedOut), true, None).into())),
                };
                let broken = result
                    .as_ref()
                    .err()
                    .and_then(DriverError::storage_error)
                    .is_some_and(|error| {
                        !c::is_stale_directory(error)
                            && matches!(
                                error.failure(),
                                Failure::Backend(BackendFailure::Other)
                                    | Failure::Backend(BackendFailure::Stale)
                                    | Failure::Backend(BackendFailure::Unavailable)
                                    | Failure::Backend(BackendFailure::TimedOut)
                            )
                    });
                if !service.send(&tx, encode(result, &request_context)).await || broken {
                    break;
                }
            }
            // Dropping Session drops its entire resource table. Uncommitted
            // writers are aborted by their backend RAII, never committed here.
        });
        Ok(Response::new(Box::pin(limits::SessionStream {
            inner: ReceiverStream::new(rx),
            permit: Some(permit),
        })))
    }
}
impl PluginService {
    async fn send(
        &self,
        tx: &mpsc::Sender<Result<wire::Reply, Status>>,
        reply: Result<wire::Reply, Status>,
    ) -> bool {
        let valid = reply.is_ok();
        matches!(
            tokio::time::timeout(self.admission.limits.stream_idle_timeout, tx.send(reply)).await,
            Ok(Ok(()))
        ) && valid
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
    let _sessions = CloseSessions(dispatcher.clone());
    let shutdown = async move {
        shutdown.await;
        dispatcher.shutdown();
    };
    let mut server = Server::builder()
        .concurrency_limit_per_connection(limits.max_streams_per_connection as usize)
        .max_concurrent_streams(limits.max_streams_per_connection)
        .http2_max_header_list_size(16 * 1024)
        .load_shed(true);
    if let Some(age) = limits.max_connection_age {
        server = server
            .max_connection_age(age)
            .max_connection_age_grace(limits.stream_idle_timeout);
    }
    if let Some(tls) = tls {
        server = server.tls_config(tls.timeout(limits.request_timeout))?;
    }
    let result = server
        .add_service(svc)
        .serve_with_incoming_shutdown(limits::incoming(listener, limits.max_connections), shutdown)
        .await;
    result?;
    Ok(())
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);
struct CloseSessions(Arc<Dispatcher>);
impl Drop for CloseSessions {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod response_tests {
    use super::*;
    use ah_vfs_storage::Operation;

    #[test]
    fn malformed_backend_results_report_contract_bugs_without_claiming_mutations_rolled_back()
    -> Result<(), Box<dyn std::error::Error>> {
        for mutates in [false, true] {
            let operation = if mutates {
                Operation::Write
            } else {
                Operation::Read
            };
            let request = if mutates {
                Op::Commit(p::Handle { handle: 1 })
            } else {
                Op::ReaderNext(p::Handle { handle: 1 })
            };
            for reply in [
                Ok(p::Reply {
                    result: None,
                    data: vec![],
                }),
                Ok(p::Reply {
                    result: Some(p::Value::Bytes(p::Empty {})),
                    data: vec![0; p::MAX_DATA_BYTES + 1],
                }),
                Err(c::storage_protocol_violation(operation).into()),
            ] {
                let reply = encode(reply, &request)?;
                let failure = p::from_reply(reply)?
                    .err()
                    .ok_or("invalid response accepted")?;
                let failure = p::decode_storage_error(failure.storage.ok_or("storage context")?)?;
                assert_eq!(failure.operation(), operation);
                assert_eq!(
                    failure.failure(),
                    Failure::Contract(ContractViolation::InvalidResponse)
                );
                assert_eq!(
                    failure.outcome(),
                    if mutates {
                        ah_vfs_storage::MutationOutcome::Unknown
                    } else {
                        ah_vfs_storage::MutationOutcome::Unchanged
                    }
                );
            }
        }
        Ok(())
    }
}
