//! Native plugin server SDK. No dependency on the host's Driver/Storage proxies.
pub mod dispatch;

use ah_vfs::{
    application::port::Driver,
    domain::{DriverError, StorageError, StorageErrorKind},
};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::mpsc};
use tokio_stream::{Stream, wrappers::{ReceiverStream, TcpListenerStream}};
use tonic::{Request, Response, Status, metadata::{Ascii, MetadataValue}, transport::Server};
use dispatch::Dispatcher;
use vfs_plugin_codec as c;
use vfs_plugin_protocol::{self as p, Operation as Op, Value};
pub use vfs_plugin_grpc_wire as wire;

type ReadStream = Pin<Box<dyn Stream<Item=Result<wire::Reply,Status>>+Send>>;

#[derive(Clone)]
pub struct PluginService {
    dispatcher: Arc<Dispatcher>,
    token: MetadataValue<Ascii>,
}
impl PluginService {
    pub fn new(dispatcher: Arc<Dispatcher>, token: &str) -> Result<Self, DriverError> {
        if token.is_empty() {
            return Err(StorageError::new(StorageErrorKind::PermissionDenied).into());
        }
        Ok(Self {
            dispatcher,
            token: MetadataValue::try_from(format!("Bearer {token}")).map_err(|_| c::invalid())?,
        })
    }
    fn authenticate<T>(&self, r: &Request<T>) -> Result<(), Status> {
        if r.metadata().get("authorization") != Some(&self.token) {
            return Err(Status::unauthenticated("plugin authentication failed"));
        }
        Ok(())
    }
    fn decode(&self, r: wire::Call) -> Result<(String, Op, Vec<u8>), Status> {
        if r.control.len() > p::MAX_CONTROL_BYTES
            || r.data.len() > p::MAX_DATA_BYTES
            || r.generation.len() > 128
        {
            return Err(Status::invalid_argument("plugin frame is too large"));
        }
        let op = serde_json::from_slice(&r.control)
            .map_err(|_| Status::invalid_argument("invalid plugin operation"))?;
        Ok((r.generation, op, r.data))
    }
}
fn encode(reply: Result<p::Reply, DriverError>) -> Result<wire::Reply, Status> {
    let result = match reply {
        Ok(r) => wire::Reply {
            control: serde_json::to_vec(&r.value)
                .map_err(|_| Status::internal("invalid plugin response"))?,
            data: r.data,
            failed: false,
        },
        Err(e) => wire::Reply {
            control: serde_json::to_vec(&c::failure(e))
                .map_err(|_| Status::internal("invalid plugin error"))?,
            data: vec![],
            failed: true,
        },
    };
    if result.control.len() > p::MAX_CONTROL_BYTES || result.data.len() > p::MAX_DATA_BYTES {
        return Err(Status::resource_exhausted("plugin response is too large"));
    }
    Ok(result)
}
#[tonic::async_trait]
impl wire::plugin_server::Plugin for PluginService {
    async fn invoke(&self, request: Request<wire::Call>) -> Result<Response<wire::Reply>, Status> {
        self.authenticate(&request)?;
        let (generation, op, data) = self.decode(request.into_inner())?;
        Ok(Response::new(encode(
            self.dispatcher.call(&generation, op, data).await,
        )?))
    }
    type ReadStream = ReadStream;
    async fn read(
        &self,
        request: Request<wire::Call>,
    ) -> Result<Response<Self::ReadStream>, Status> {
        self.authenticate(&request)?;
        let (generation, op, data) = self.decode(request.into_inner())?;
        let Op::ReaderNext { handle } = op else {
            return Err(Status::invalid_argument("expected reader operation"));
        };
        if !data.is_empty() {
            return Err(Status::invalid_argument("unexpected reader data"));
        }
        let dispatcher = self.dispatcher.clone();
        let (tx, rx) = mpsc::channel(1);
        tokio::spawn(async move {
            loop {
                if tx.is_closed() {
                    break;
                }
                let result = tokio::select! {
                    ()=tx.closed()=>break,
                    result=dispatcher.call(&generation,Op::ReaderNext{handle},vec![])=>result,
                };
                let done = !matches!(
                    &result,
                    Ok(p::Reply {
                        value: Value::Chunk { eof: false },
                        ..
                    })
                );
                if tx.send(encode(result)).await.is_err() || done {
                    break;
                }
            }
            let _ = dispatcher
                .call(
                    &generation,
                    Op::Release {
                        kind: p::ResourceKind::Reader,
                        handle,
                    },
                    vec![],
                )
                .await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
    async fn write(
        &self,
        request: Request<tonic::Streaming<wire::Call>>,
    ) -> Result<Response<wire::Reply>, Status> {
        self.authenticate(&request)?;
        let mut stream = request.into_inner();
        let mut identity = None;
        while let Some(frame) = stream.message().await? {
            let (generation, op, data) = self.decode(frame)?;
            let Op::WriterChunk { handle } = op else {
                return Err(Status::invalid_argument("expected writer operation"));
            };
            if identity
                .as_ref()
                .is_some_and(|previous| previous != &(generation.clone(), handle))
            {
                return Err(Status::invalid_argument("mixed writer resources"));
            }
            identity = Some((generation.clone(), handle));
            if let Err(error) = self
                .dispatcher
                .call(&generation, Op::WriterChunk { handle }, data)
                .await
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
pub fn service(
    dispatcher: Arc<Dispatcher>,
    token: &str,
) -> Result<wire::plugin_server::PluginServer<PluginService>, DriverError> {
    Ok(
        wire::plugin_server::PluginServer::new(PluginService::new(dispatcher, token)?)
            .max_decoding_message_size(p::MAX_CONTROL_BYTES + p::MAX_DATA_BYTES + 1024)
            .max_encoding_message_size(p::MAX_CONTROL_BYTES + p::MAX_DATA_BYTES + 1024),
    )
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
    if tls.is_none() && !listener.local_addr()?.ip().is_loopback() {
        return Err("remote plugin listeners require TLS".into());
    }
    let dispatcher = Arc::new(Dispatcher::new(driver)?);
    let svc = service(dispatcher.clone(), token)?;
    let expire = AbortOnDrop(tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            dispatcher.expire().await;
        }
    }));
    let mut server = Server::builder();
    if let Some(tls) = tls {
        server = server.tls_config(tls)?;
    }
    let result = server
        .add_service(svc)
        .serve_with_incoming_shutdown(TcpListenerStream::new(listener), shutdown)
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

