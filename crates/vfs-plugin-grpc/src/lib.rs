//! Host-side gRPC transport. Plugin servers depend on vfs-plugin-grpc-server.
use ah_vfs::domain::{DriverError, StorageError, StorageErrorKind};
use std::{collections::HashMap, net::IpAddr, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tonic::{
    Request, Status,
    metadata::{Ascii, MetadataValue},
    transport::{Channel, Endpoint},
};
use vfs_plugin_codec as c;
use vfs_plugin_host::{Transport, TransportFuture};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};

pub use tonic::transport::{Certificate, ClientTlsConfig, Identity};
pub use vfs_plugin_grpc_wire as wire;
type Client = wire::plugin_client::PluginClient<Channel>;
type Incoming = Arc<Mutex<tonic::Streaming<wire::Reply>>>;
#[derive(Default)]
struct Readers {
    generation: String,
    streams: HashMap<(String, u64), Incoming>,
}

/// Bearer-authenticated transport. Plaintext is limited to literal loopback IPs;
/// custom TLS connections are constructed from inspectable Endpoint values.
pub struct GrpcTransport {
    client: Client,
    token: MetadataValue<Ascii>,
    readers: Mutex<Readers>,
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
            readers: Mutex::new(Readers::default()),
            timeout: Duration::from_secs(30),
        }
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
                    let key = (generation.to_owned(), *handle);
                    let existing = {
                        let readers = self.readers.lock().await;
                        if readers.generation != generation {
                            return Err(StorageError::new(StorageErrorKind::StaleHandle).into());
                        }
                        readers.streams.get(&key).cloned()
                    };
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
                            let mut readers = self.readers.lock().await;
                            if readers.generation != generation {
                                return Err(StorageError::new(StorageErrorKind::StaleHandle).into());
                            }
                            readers.streams.insert(key.clone(), stream.clone());
                            stream
                        }
                    };
                    let mut incoming = stream.lock().await;
                    let result = match tokio::time::timeout(self.timeout, incoming.message()).await
                    {
                        Ok(result) => result,
                        Err(_) => {
                            self.readers.lock().await.streams.remove(&key);
                            return Err(StorageError::new(StorageErrorKind::TimedOut).into());
                        }
                    };
                    match result {
                        Ok(Some(reply)) => {
                            if reply.failed {
                                self.readers.lock().await.streams.remove(&key);
                            }
                            reply
                        }
                        Ok(None) => {
                            self.readers.lock().await.streams.remove(&key);
                            return Err(c::invalid());
                        }
                        Err(status) => {
                            self.readers.lock().await.streams.remove(&key);
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
                    self.readers
                        .lock()
                        .await
                        .streams
                        .remove(&(generation.to_owned(), *handle));
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
            if let Ok(p::Reply {
                value: Value::Description { description },
                ..
            }) = &result
            {
                let mut readers = self.readers.lock().await;
                readers.generation = description.generation.clone();
                readers
                    .streams
                    .retain(|(generation, _), _| generation == &description.generation);
            }
            if let Op::ReaderNext { handle } = op
                && !matches!(
                    &result,
                    Ok(p::Reply {
                        value: Value::Chunk { eof: false },
                        ..
                    })
                )
            {
                self.readers
                    .lock()
                    .await
                    .streams
                    .remove(&(generation.to_owned(), handle));
            }
            result
        })
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
