//! Host-side gRPC transport. Plugin servers depend on vfs-plugin-grpc-server.
use ah_vfs::domain::{DriverError, StorageError, StorageErrorKind};
use std::{
    net::IpAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};
use tonic::{
    Request, Status,
    metadata::{Ascii, MetadataValue},
    transport::{Channel, Endpoint},
};
use vfs_plugin_codec as c;
use vfs_plugin_host::{OpenFuture, OpenedStorage, Transport, TransportFuture};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};

pub use tonic::transport::{Certificate, ClientTlsConfig, Identity};
pub use vfs_plugin_grpc_wire as wire;
type Client = wire::plugin_client::PluginClient<Channel>;
#[derive(Default)]
struct ConnectionState {
    generation: String,
    disconnected: Arc<AtomicBool>,
}

/// Bearer-authenticated transport. Plaintext is limited to literal loopback IPs;
/// custom TLS connections are constructed from inspectable Endpoint values.
pub struct GrpcTransport {
    client: Client,
    token: MetadataValue<Ascii>,
    connection: Arc<Mutex<ConnectionState>>,
    timeout: Duration,
}
impl GrpcTransport {
    /// Reject credentials, resource paths and plaintext endpoints other than
    /// literal loopback IPs. DNS names require TLS, including localhost aliases.
    pub fn validate_endpoint(endpoint: &str) -> Result<(), DriverError> {
        endpoint_config(endpoint).map(|_| ())
    }
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
        let mut endpoint = endpoint_config(&endpoint)?;
        if tls.is_some() && endpoint.uri().scheme_str() != Some("https") {
            return Err(c::invalid());
        }
        let tls = tls.or_else(|| {
            (endpoint.uri().scheme_str() == Some("https"))
                .then(|| ClientTlsConfig::new().with_enabled_roots())
        });
        if let Some(tls) = tls {
            endpoint = endpoint.tls_config(tls).map_err(|_| c::invalid())?;
        }
        Self::connect_endpoint(endpoint, token).await
    }
    /// Connect a caller-configured endpoint, preserving custom trust and mutual
    /// TLS while applying the same plaintext restrictions before sending a token.
    pub async fn connect_endpoint(endpoint: Endpoint, token: &str) -> Result<Self, DriverError> {
        Self::validate_endpoint(&endpoint.uri().to_string())?;
        let token = bearer_token(token)?;
        let channel = endpoint
            .connect_timeout(Duration::from_secs(10))
            .connect()
            .await
            .map_err(|_| StorageError::new(StorageErrorKind::Unavailable))?;
        Ok(Self::from_channel(channel, token))
    }
    // An opaque Channel cannot prove which endpoint or TLS policy it uses.
    fn from_channel(channel: Channel, token: MetadataValue<Ascii>) -> Self {
        Self {
            client: Client::new(channel)
                .max_decoding_message_size(p::MAX_CONTROL_BYTES + p::MAX_DATA_BYTES + 1024)
                .max_encoding_message_size(p::MAX_CONTROL_BYTES + p::MAX_DATA_BYTES + 1024),
            token,
            connection: Arc::new(Mutex::new(ConnectionState::default())),
            timeout: Duration::from_secs(30),
        }
    }
    fn request<T>(&self, value: T) -> Request<T> {
        let mut r = Request::new(value);
        r.metadata_mut().insert("authorization", self.token.clone());
        r.set_timeout(self.timeout);
        r
    }
    fn encode(generation: &str, op: &Op, data: Vec<u8>) -> Result<wire::Request, DriverError> {
        p::to_request(generation, op, data).map_err(|error| {
            StorageError::new(match error {
                p::CodecError::TooLarge => StorageErrorKind::TooLarge,
                p::CodecError::InvalidData => StorageErrorKind::InvalidRequest,
            })
            .into()
        })
    }
}
fn endpoint_config(raw: &str) -> Result<Endpoint, DriverError> {
    let url = url::Url::parse(raw).map_err(|_| c::invalid())?;
    let endpoint = Endpoint::from_shared(raw.to_owned()).map_err(|_| c::invalid())?;
    if !matches!(endpoint.uri().scheme_str(), Some("http" | "https"))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(c::invalid());
    }
    if endpoint.uri().scheme_str() == Some("http") {
        // Check the exact URI passed to the connector, without DNS resolution or
        // URL normalization of noncanonical IPv4 spellings.
        let host = endpoint.uri().host().ok_or_else(c::invalid)?;
        let host = host
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(host);
        if !host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()) {
            return Err(StorageError::new(StorageErrorKind::PermissionDenied).into());
        }
    }
    Ok(endpoint)
}
fn bearer_token(token: &str) -> Result<MetadataValue<Ascii>, DriverError> {
    if token.is_empty() || token.len() > 4096 || !token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(StorageError::new(StorageErrorKind::PermissionDenied).into());
    }
    let mut value = MetadataValue::try_from(format!("Bearer {token}")).map_err(|_| c::invalid())?;
    value.set_sensitive(true);
    Ok(value)
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
    let id = match op {
        Op::Open(p::Open { id, .. }) => Some(id.as_str()),
        _ => None,
    };
    let reply = p::from_reply(reply)
        .map_err(|_| invalid())?
        .map_err(|error| c::driver_error(error, id))?;
    if !reply
        .result
        .as_ref()
        .is_some_and(|value| op.accepts_reply(value))
    {
        return Err(invalid());
    }
    Ok(reply)
}

impl vfs_plugin_host::TransportFactory for GrpcTransport {
    fn connect(self: Arc<Self>) -> vfs_plugin_host::ConnectFuture {
        Box::pin(async move {
            Ok(Arc::new(Self {
                client: self.client.clone(),
                token: self.token.clone(),
                connection: self.connection.clone(),
                timeout: self.timeout,
            }) as Arc<dyn Transport>)
        })
    }
}
impl Transport for GrpcTransport {
    fn disconnected(&self) -> Option<Arc<AtomicBool>> {
        Some(
            self.connection
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .disconnected
                .clone(),
        )
    }
    fn open_session<'a>(&'a self, generation: &'a str, open: p::Open) -> Option<OpenFuture<'a>> {
        Some(Box::pin(async move {
            let operation = Op::Open(open);
            let request = Self::encode(generation, &operation, vec![])?;
            let (tx, rx) = mpsc::channel(1);
            tx.send(request).await.map_err(|_| c::invalid())?;
            let mut request = Request::new(tokio_stream::wrappers::ReceiverStream::new(rx));
            request
                .metadata_mut()
                .insert("authorization", self.token.clone());
            // Only the opening handshake has a deadline; an idle Storage remains
            // valid for the lifetime of its duplex stream.
            let mut client = self.client.clone();
            let peer = Peer {
                generation: generation.into(),
                connection: self.connection.clone(),
            };
            let mut incoming = tokio::time::timeout(self.timeout, client.session(request))
                .await
                .map_err(|_| {
                    peer.disconnect();
                    StorageError::new(StorageErrorKind::TimedOut)
                })?
                .map_err(|status| peer.failed(status))?
                .into_inner();
            let reply = tokio::time::timeout(self.timeout, incoming.message())
                .await
                .map_err(|_| {
                    peer.disconnect();
                    StorageError::new(StorageErrorKind::TimedOut)
                })?
                .map_err(|status| peer.failed(status))?
                .ok_or_else(|| {
                    peer.disconnect();
                    StorageError::new(StorageErrorKind::Unavailable)
                })?;
            let Value::Storage(storage) = decode(reply, &operation)?
                .into_value()
                .map_err(|error| c::driver_error(error, None))?
            else {
                return Err(c::invalid());
            };
            if storage.handle != 1 {
                return Err(c::invalid());
            }
            let disconnected = Arc::new(AtomicBool::new(false));
            let (pending, requests) = mpsc::channel(1);
            let task = tokio::spawn(run_session(
                incoming,
                tx,
                requests,
                disconnected.clone(),
                peer,
                self.timeout,
            ));
            let transport = Arc::new(SessionTransport {
                generation: generation.into(),
                pending,
                disconnected,
                task,
            });
            Ok(OpenedStorage {
                transport,
                storage,
                generation: generation.into(),
            })
        }))
    }
    fn call<'a>(&'a self, generation: &'a str, op: Op, data: Vec<u8>) -> TransportFuture<'a> {
        Box::pin(async move {
            if op.is_resource() || matches!(op, Op::Open(_)) {
                return Err(StorageError::new(StorageErrorKind::InvalidRequest).into());
            }
            let call = Self::encode(generation, &op, data)?;
            let mut client = self.client.clone();
            let reply = client
                .invoke(self.request(call))
                .await
                .map_err(|s| transport_error(s, op.mutates()))?
                .into_inner();
            let reply = decode(reply, &op)?;
            if let Some(Value::Description(description)) = &reply.result {
                let mut connection = self
                    .connection
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if connection.generation != description.generation
                    || connection.disconnected.load(Ordering::Acquire)
                {
                    connection.disconnected.store(true, Ordering::Release);
                    *connection = ConnectionState {
                        generation: description.generation.clone(),
                        disconnected: Arc::new(AtomicBool::new(false)),
                    };
                }
            }
            Ok(reply)
        })
    }
}

struct Peer {
    generation: String,
    connection: Arc<Mutex<ConnectionState>>,
}
impl Peer {
    fn failed(&self, status: Status) -> DriverError {
        let error = transport_error(status, false);
        if error.storage_error().is_some_and(|error| {
            matches!(
                error.kind(),
                StorageErrorKind::Unavailable
                    | StorageErrorKind::TimedOut
                    | StorageErrorKind::Cancelled
            )
        }) {
            self.disconnect();
        }
        error
    }
    fn disconnect(&self) {
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // The currently described incarnation of the same backend must observe
        // failures of retained sessions too. Old backend generations cannot
        // invalidate the new generation after a reconnect.
        if connection.generation == self.generation {
            connection.disconnected.store(true, Ordering::Release);
        }
    }
}

struct Pending {
    request: p::Request,
    operation: Op,
    reply: oneshot::Sender<Result<p::Reply, DriverError>>,
}
struct SessionTransport {
    generation: String,
    pending: mpsc::Sender<Pending>,
    disconnected: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for SessionTransport {
    fn drop(&mut self) {
        self.disconnected.store(true, Ordering::Release);
        // Dropping the HTTP/2 response resets the stream immediately, including
        // when the host runtime is shutting down and cannot spawn Release calls.
        self.task.abort();
    }
}
impl Transport for SessionTransport {
    fn disconnected(&self) -> Option<Arc<AtomicBool>> {
        Some(self.disconnected.clone())
    }
    fn call<'a>(
        &'a self,
        generation: &'a str,
        operation: Op,
        data: Vec<u8>,
    ) -> TransportFuture<'a> {
        Box::pin(async move {
            if generation != self.generation || self.disconnected.load(Ordering::Acquire) {
                return Err(StorageError::new(StorageErrorKind::StaleHandle).into());
            }
            if !operation.is_resource() {
                return Err(StorageError::new(StorageErrorKind::InvalidRequest).into());
            }
            let request = GrpcTransport::encode("", &operation, data)?;
            let (reply, received) = oneshot::channel();
            self.pending
                .send(Pending {
                    request,
                    operation,
                    reply,
                })
                .await
                .map_err(|_| StorageError::new(StorageErrorKind::StaleHandle))?;
            received
                .await
                .map_err(|_| StorageError::new(StorageErrorKind::StaleHandle))?
        })
    }
}
struct Disconnected(Arc<AtomicBool>);
impl Drop for Disconnected {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

async fn run_session(
    mut incoming: tonic::Streaming<p::Reply>,
    outgoing: mpsc::Sender<p::Request>,
    mut requests: mpsc::Receiver<Pending>,
    disconnected: Arc<AtomicBool>,
    peer: Peer,
    timeout: Duration,
) {
    let _closed = Disconnected(disconnected);
    loop {
        // Poll the response even while no operation is pending: process exit or
        // backend replacement invalidates the Storage without another API call.
        let mut pending = tokio::select! {
            message=incoming.message()=> { let _=message; peer.disconnect();break; },
            pending=requests.recv()=> match pending { Some(pending)=>pending,None=>break },
        };
        if pending.reply.is_closed() {
            continue;
        } // Not dispatched, safe to skip.
        let result = tokio::select! {
            ()=pending.reply.closed()=>break,
            result=tokio::time::timeout(timeout, async {
                outgoing.send(pending.request).await.map_err(|_|transport_error(Status::unavailable("session closed"),pending.operation.mutates()))?;
                let reply=incoming.message().await.map_err(|s|transport_error(s,pending.operation.mutates()))?
                    .ok_or_else(||transport_error(Status::unavailable("session closed"),pending.operation.mutates()))?;
                decode(reply,&pending.operation)
            })=>result.unwrap_or_else(|_|Err(StorageError::new(if pending.operation.mutates() {StorageErrorKind::OutcomeUnknown}else {StorageErrorKind::TimedOut}).into())),
        };
        let broken = result
            .as_ref()
            .err()
            .and_then(DriverError::storage_error)
            .is_some_and(|error| {
                matches!(
                    error.kind(),
                    StorageErrorKind::OutcomeUnknown
                        | StorageErrorKind::StaleHandle
                        | StorageErrorKind::Unavailable
                        | StorageErrorKind::TimedOut
                        | StorageErrorKind::InvalidData
                )
            });
        let _ = pending.reply.send(result);
        if broken {
            peer.disconnect();
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    type Result = std::result::Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn only_literal_loopback_addresses_allow_plaintext() -> Result {
        for endpoint in [
            "http://127.0.0.1:7443",
            "http://127.42.0.9",
            "http://[::1]:7443",
            "https://plugin.example:7443",
            "https://192.0.2.1:7443",
        ] {
            GrpcTransport::validate_endpoint(endpoint)?;
        }
        for endpoint in [
            "http://192.0.2.1:7443",
            "http://plugin.example:7443",
            "http://localhost:7443",
            "http://127.0.0.1.example.test",
            "http://127.1",
            "http://2130706433",
            "http://[::ffff:127.0.0.1]",
        ] {
            let error = GrpcTransport::validate_endpoint(endpoint)
                .err()
                .ok_or("plaintext endpoint")?;
            assert_eq!(
                error.storage_error().ok_or("category")?.kind(),
                StorageErrorKind::PermissionDenied
            );
        }
        for endpoint in [
            "ftp://plugin.example",
            "https://user:password@plugin.example",
            "https://plugin.example/resource",
            "https://plugin.example/?secret=value",
            "https://plugin.example/#fragment",
        ] {
            assert!(GrpcTransport::validate_endpoint(endpoint).is_err());
        }
        Ok(())
    }

    #[tokio::test]
    async fn every_public_connector_rejects_remote_plaintext_before_connecting() -> Result {
        let error = GrpcTransport::connect("http://192.0.2.1:7443".into(), "test-token")
            .await
            .err()
            .ok_or("insecure connection")?;
        assert_eq!(
            error.storage_error().ok_or("category")?.kind(),
            StorageErrorKind::PermissionDenied
        );
        let endpoint = Endpoint::from_static("http://192.0.2.1:7443");
        let error = GrpcTransport::connect_endpoint(endpoint, "test-token")
            .await
            .err()
            .ok_or("insecure endpoint")?;
        assert_eq!(
            error.storage_error().ok_or("category")?.kind(),
            StorageErrorKind::PermissionDenied
        );
        assert!(
            GrpcTransport::connect_with_tls(
                "http://127.0.0.1:7443".into(),
                "test-token",
                Some(ClientTlsConfig::new())
            )
            .await
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn bearer_metadata_is_sensitive_and_tokens_are_bounded() -> Result {
        assert!(bearer_token("test-token")?.is_sensitive());
        for token in ["", "has space", "line\nbreak", "nonascii-令牌"] {
            assert!(bearer_token(token).is_err());
        }
        assert!(bearer_token(&"x".repeat(4097)).is_err());
        assert!(bearer_token(&"x".repeat(4096)).is_ok());
        Ok(())
    }
}
