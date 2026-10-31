//! Host-side gRPC transport. Plugin servers depend on vfs-plugin-grpc-server.
use ah_vfs::domain::{DriverError, StorageError, StorageErrorKind};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tonic::{
    Request, Status,
    metadata::{Ascii, MetadataValue},
    transport::{Channel, Endpoint},
};
use vfs_plugin_host::{Transport, TransportFuture};
use vfs_plugin_codec as c;
use vfs_plugin_protocol::{self as p, Operation as Op, Value};

pub use vfs_plugin_grpc_wire as wire;
pub use tonic::transport::{Certificate, ClientTlsConfig, Identity};
type Client = wire::plugin_client::PluginClient<Channel>;
type Incoming = Arc<Mutex<tonic::Streaming<wire::Reply>>>;

pub struct GrpcTransport {
    client: Client,
    token: MetadataValue<Ascii>,
    readers: Mutex<HashMap<u64, Incoming>>,
    timeout: Duration,
}
impl GrpcTransport {
    pub async fn connect(endpoint: String, token: &str) -> Result<Self, DriverError> {
        Self::connect_with_tls(endpoint, token, None).await
    }
    /// Configure transport trust/identity without teaching the loader about a
    /// concrete plugin. Business credentials remain in opaque DriverSettings.
    pub async fn connect_with_tls(
        endpoint: String,
        token: &str,
        tls: Option<ClientTlsConfig>,
    ) -> Result<Self, DriverError> {
        let mut endpoint = Endpoint::from_shared(endpoint).map_err(|_| c::invalid())?;
        let tls = tls.or_else(|| {
            (endpoint.uri().scheme_str() == Some("https"))
                .then(|| ClientTlsConfig::new().with_enabled_roots())
        });
        if let Some(tls) = tls {
            endpoint = endpoint.tls_config(tls).map_err(|_| c::invalid())?;
        }
        let channel = endpoint
            .connect_timeout(Duration::from_secs(10))
            .connect()
            .await
            .map_err(|_| StorageError::new(StorageErrorKind::Unavailable))?;
        Self::from_channel(channel, token)
    }
    /// Use a caller-configured channel for private CAs or mutual TLS.
    pub fn from_channel(channel: Channel, token: &str) -> Result<Self, DriverError> {
        if token.is_empty() {
            return Err(StorageError::new(StorageErrorKind::PermissionDenied).into());
        }
        let token = MetadataValue::try_from(format!("Bearer {token}")).map_err(|_| c::invalid())?;
        Ok(Self {
            client: Client::new(channel)
                .max_decoding_message_size(p::MAX_CONTROL_BYTES + p::MAX_DATA_BYTES + 1024)
                .max_encoding_message_size(p::MAX_CONTROL_BYTES + p::MAX_DATA_BYTES + 1024),
            token,
            readers: Mutex::new(HashMap::new()),
            timeout: Duration::from_secs(30),
        })
    }
    fn request<T>(&self, value: T) -> Request<T> {
        let mut r = Request::new(value);
        r.metadata_mut().insert("authorization", self.token.clone());
        r.set_timeout(self.timeout);
        r
    }
    fn encode(&self, generation: &str, op: &Op, data: Vec<u8>) -> Result<wire::Call, DriverError> {
        let control = serde_json::to_vec(op).map_err(|_| c::invalid())?;
        if control.len() > p::MAX_CONTROL_BYTES || data.len() > p::MAX_DATA_BYTES {
            return Err(StorageError::new(StorageErrorKind::TooLarge).into());
        }
        Ok(wire::Call {
            generation: generation.into(),
            control,
            data,
        })
    }
}
fn transport_error(status: Status, mutates: bool) -> DriverError {
    let kind = match status.code() {
        tonic::Code::Unauthenticated => StorageErrorKind::PermissionDenied,
        _ if mutates => StorageErrorKind::OutcomeUnknown,
        tonic::Code::PermissionDenied => StorageErrorKind::PermissionDenied,
        tonic::Code::InvalidArgument => StorageErrorKind::InvalidRequest,
        tonic::Code::ResourceExhausted => StorageErrorKind::TooLarge,
        tonic::Code::DeadlineExceeded => StorageErrorKind::TimedOut,
        tonic::Code::Cancelled => StorageErrorKind::Cancelled,
        _ => StorageErrorKind::Unavailable,
    };
    StorageError::new(kind).into()
}
fn decode(reply: wire::Reply, op: &Op) -> Result<p::Reply, DriverError> {
    let invalid = || {
        if op.mutates() {
            StorageError::new(StorageErrorKind::OutcomeUnknown).into()
        } else {
            c::invalid()
        }
    };
    if reply.control.len() > p::MAX_CONTROL_BYTES || reply.data.len() > p::MAX_DATA_BYTES {
        return Err(invalid());
    }
    if reply.failed {
        let failure = serde_json::from_slice(&reply.control).map_err(|_| invalid())?;
        let id = match op {
            Op::GetInstance { id } | Op::RemoveInstance { id } | Op::Open { id, .. } => {
                Some(id.as_str())
            }
            _ => None,
        };
        return Err(c::driver_error(failure, id));
    }
    Ok(p::Reply {
        value: serde_json::from_slice(&reply.control).map_err(|_| invalid())?,
        data: reply.data,
    })
}
impl Transport for GrpcTransport {
    fn call<'a>(&'a self, generation: &'a str, op: Op, data: Vec<u8>) -> TransportFuture<'a> {
        Box::pin(async move {
            let call = self.encode(generation, &op, data)?;
            let mut client = self.client.clone();
            let reply = match &op {
                Op::ReaderNext { handle } => {
                    let existing = self.readers.lock().await.get(handle).cloned();
                    let stream = match existing {
                        Some(stream) => stream,
                        None => {
                            // A file stream can outlive a single operation deadline.
                            // Bound metadata and each next-chunk wait independently.
                            let mut request = Request::new(call);
                            request
                                .metadata_mut()
                                .insert("authorization", self.token.clone());
                            let stream = tokio::time::timeout(self.timeout, client.read(request))
                                .await
                                .map_err(|_| StorageError::new(StorageErrorKind::TimedOut))?
                                .map_err(|s| transport_error(s, false))?
                                .into_inner();
                            let stream = Arc::new(Mutex::new(stream));
                            self.readers.lock().await.insert(*handle, stream.clone());
                            stream
                        }
                    };
                    let mut incoming = stream.lock().await;
                    let result = match tokio::time::timeout(self.timeout, incoming.message()).await
                    {
                        Ok(result) => result,
                        Err(_) => {
                            self.readers.lock().await.remove(handle);
                            return Err(StorageError::new(StorageErrorKind::TimedOut).into());
                        }
                    };
                    match result {
                        Ok(Some(reply)) => {
                            if reply.failed {
                                self.readers.lock().await.remove(handle);
                            }
                            reply
                        }
                        Ok(None) => {
                            self.readers.lock().await.remove(handle);
                            return Err(c::invalid());
                        }
                        Err(status) => {
                            self.readers.lock().await.remove(handle);
                            return Err(transport_error(status, false));
                        }
                    }
                }
                Op::WriterChunk { .. } => client
                    .write(self.request(tokio_stream::iter([call])))
                    .await
                    .map_err(|s| transport_error(s, true))?
                    .into_inner(),
                Op::Release {
                    kind: p::ResourceKind::Reader,
                    handle,
                } => {
                    self.readers.lock().await.remove(handle);
                    client
                        .invoke(self.request(call))
                        .await
                        .map_err(|s| transport_error(s, false))?
                        .into_inner()
                }
                _ => client
                    .invoke(self.request(call))
                    .await
                    .map_err(|s| transport_error(s, op.mutates()))?
                    .into_inner(),
            };
            let result = decode(reply, &op);
            if let Op::ReaderNext { handle } = op
                && !matches!(
                    &result,
                    Ok(p::Reply {
                        value: Value::Chunk { eof: false },
                        ..
                    })
                )
            {
                self.readers.lock().await.remove(&handle);
            }
            result
        })
    }
}

