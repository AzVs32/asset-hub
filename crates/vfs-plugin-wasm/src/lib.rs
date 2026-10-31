//! WIT resource adapter with bounded execution and explicitly granted HTTP origins.
use ah_vfs::domain::{DriverError, StorageError, StorageErrorKind, new_instance_id};
use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use vfs_plugin_host::{Transport, TransportFuture, convert as c};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};
use wasmtime::component::{Component, Linker, ResourceAny};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

wasmtime::component::bindgen!({
    path: "../vfs-plugin-protocol/wit",
    world: "plugin",
    imports: { default: async | trappable },
    exports: { default: async },
});
use assethub::vfs::http;
use assethub::vfs::{clock, random};
use exports::assethub::vfs::driver as guest;

/// Permissions for the stable service.v1 profile. HTTP, clock and bounded random
/// services are always linked; only explicitly granted HTTP origins are reachable.
/// Filesystem access, environment variables and raw sockets are not exposed.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostGrants {
    #[serde(default)]
    pub http_origins: Vec<String>,
}

fn unix_time() -> wasmtime::Result<i64> {
    Ok(i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())?)
}

struct Host {
    client: reqwest::Client,
    origins: Vec<String>,
    limits: StoreLimits,
}
impl http::Host for Host {
    async fn unix_time(&mut self) -> wasmtime::Result<i64> {
        unix_time()
    }
    async fn request_http(
        &mut self,
        request: http::Request,
    ) -> wasmtime::Result<Result<http::Response, String>> {
        let execute = async {
            let url = reqwest::Url::parse(&request.url).map_err(|_| "invalid_request")?;
            if !url.username().is_empty()
                || url.password().is_some()
                || !self.origins.contains(&url.origin().ascii_serialization())
            {
                return Err("permission_denied");
            }
            if request.body.len() > p::MAX_DATA_BYTES || request.headers.len() > 64 {
                return Err("too_large");
            }
            let method = reqwest::Method::from_bytes(request.method.as_bytes())
                .map_err(|_| "invalid_request")?;
            let is_head = method == reqwest::Method::HEAD;
            let mut builder = self.client.request(method, url);
            for header in request.headers {
                if header.name.eq_ignore_ascii_case("host")
                    || header.name.eq_ignore_ascii_case("content-length")
                    || header.name.eq_ignore_ascii_case("connection")
                {
                    return Err("invalid_request");
                }
                if header.name.len() + header.value.len() > 16 * 1024 {
                    return Err("too_large");
                }
                builder = builder.header(header.name, header.value);
            }
            let mut response = builder.body(request.body).send().await.map_err(|e| {
                if e.is_timeout() {
                    "timed_out"
                } else {
                    "unavailable"
                }
            })?;
            if !is_head
                && response
                    .content_length()
                    .is_some_and(|n| n > p::MAX_DATA_BYTES as u64)
            {
                return Err("too_large");
            }
            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    value.to_str().ok().map(|value| http::Header {
                        name: name.as_str().into(),
                        value: value.into(),
                    })
                })
                .collect();
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| "unavailable")? {
                if body.len() + chunk.len() > p::MAX_DATA_BYTES {
                    return Err("too_large");
                }
                body.extend_from_slice(&chunk);
            }
            Ok(http::Response {
                status,
                headers,
                body,
            })
        }
        .await;
        Ok(execute.map_err(str::to_owned))
    }
}
impl clock::Host for Host {
    async fn unix_time(&mut self) -> wasmtime::Result<i64> {
        unix_time()
    }
}
impl random::Host for Host {
    async fn random_bytes(&mut self, length: u32) -> wasmtime::Result<Result<Vec<u8>, String>> {
        if length > 65536 {
            return Ok(Err("too_large".into()));
        }
        let mut bytes = vec![0; length as usize];
        Ok(getrandom::fill(&mut bytes)
            .map(|()| bytes)
            .map_err(|_| "unavailable".into()))
    }
}
struct Resource {
    kind: p::ResourceKind,
    value: ResourceAny,
    touched: Instant,
}
struct State {
    store: Store<Host>,
    bindings: Plugin,
    resources: HashMap<u64, Resource>,
    next: u64,
    initialized: bool,
    poisoned: bool,
    generation: String,
}
impl State {
    fn resource(&mut self, kind: p::ResourceKind, handle: u64) -> Result<ResourceAny, DriverError> {
        let r = self
            .resources
            .get_mut(&handle)
            .ok_or_else(|| StorageError::new(StorageErrorKind::SessionClosed))?;
        if r.kind != kind {
            return Err(c::invalid());
        }
        r.touched = Instant::now();
        Ok(r.value)
    }
    async fn insert(
        &mut self,
        kind: p::ResourceKind,
        value: ResourceAny,
    ) -> Result<u64, DriverError> {
        if self.resources.len() >= p::MAX_RESOURCES {
            let _ = value.resource_drop_async(&mut self.store).await;
            return Err(StorageError::new(StorageErrorKind::CapacityExceeded).into());
        }
        self.next = self.next.checked_add(1).ok_or_else(c::invalid)?;
        self.resources.insert(
            self.next,
            Resource {
                kind,
                value,
                touched: Instant::now(),
            },
        );
        Ok(self.next)
    }
    async fn execute(&mut self, op: Op, data: Vec<u8>) -> Result<p::Reply, DriverError> {
        let expired: Vec<_> = self
            .resources
            .iter()
            .filter(|(_, r)| r.touched.elapsed() > Duration::from_secs(300))
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            if let Some(r) = self.resources.remove(&id) {
                r.value
                    .resource_drop_async(&mut self.store)
                    .await
                    .map_err(|_| c::invalid())?;
            }
        }
        let g = self.bindings.assethub_vfs_driver();
        let value = match op {
            Op::Describe => {
                let bytes = g
                    .call_describe(&mut self.store)
                    .await
                    .map_err(|_| c::invalid())?;
                let mut description: p::Description = json(&bytes)?;
                description.generation = self.generation.clone();
                Value::Description { description }
            }
            Op::Initialize { settings } => {
                if self.initialized {
                    return Err(StorageError::new(StorageErrorKind::Conflict).into());
                }
                result(
                    g.call_initialize(&mut self.store, &encode(&settings)?)
                        .await,
                    None,
                )?;
                self.initialized = true;
                Value::Unit
            }
            _ if !self.initialized => {
                return Err(StorageError::new(StorageErrorKind::Unavailable).into());
            }
            Op::Prepare { config } => {
                let bytes = result(
                    g.call_prepare(&mut self.store, &encode(&config)?).await,
                    None,
                )?;
                json(&bytes)?
            }
            Op::Migrate {
                from_version,
                config,
            } => {
                let bytes = result(
                    g.call_migrate(&mut self.store, from_version, &encode(&config)?)
                        .await,
                    None,
                )?;
                Value::Config {
                    config: json(&bytes)?,
                }
            }
            Op::TestConnection { config } => {
                result(
                    g.call_test_connection(&mut self.store, &encode(&config)?)
                        .await,
                    None,
                )?;
                Value::Unit
            }
            Op::Open { id, config } => {
                let config = config.as_ref().map(encode).transpose()?;
                let resource = result(
                    g.call_open(&mut self.store, &id, config.as_deref()).await,
                    Some(&id),
                )?;
                let bytes = g
                    .storage()
                    .call_info(&mut self.store, resource)
                    .await
                    .map_err(|_| c::invalid())?;
                let info = json(&bytes)?;
                let handle = self.insert(p::ResourceKind::Storage, resource).await?;
                Value::Storage { handle, info }
            }
            op @ (Op::GetInstance { .. }
            | Op::ListInstances { .. }
            | Op::CreateInstance { .. }
            | Op::RemoveInstance { .. }) => {
                let id = match &op {
                    Op::GetInstance { id } | Op::RemoveInstance { id } => Some(id.as_str()),
                    _ => None,
                };
                let bytes = result(g.call_instances(&mut self.store, &encode(&op)?).await, id)?;
                json(&bytes)?
            }
            Op::Storage { handle, request } => {
                let resource = self.resource(p::ResourceKind::Storage, handle)?;
                let g = self.bindings.assethub_vfs_driver();
                let reply = result(
                    g.storage()
                        .call_call(&mut self.store, resource, &encode(&request)?, &data)
                        .await,
                    None,
                )?;
                return Ok(p::Reply {
                    value: json(&reply.control)?,
                    data: reply.data,
                });
            }
            Op::Read {
                handle,
                path,
                offset,
                length,
            } => {
                let resource = self.resource(p::ResourceKind::Storage, handle)?;
                let g = self.bindings.assethub_vfs_driver();
                let reader = result(
                    g.storage()
                        .call_read(&mut self.store, resource, &path, offset, length)
                        .await,
                    None,
                )?;
                let handle = self.insert(p::ResourceKind::Reader, reader).await?;
                Value::Reader { handle }
            }
            Op::Write {
                handle,
                path,
                replace,
                atomic,
            } => {
                let resource = self.resource(p::ResourceKind::Storage, handle)?;
                let g = self.bindings.assethub_vfs_driver();
                let writer = result(
                    g.storage()
                        .call_write(&mut self.store, resource, &path, replace, atomic)
                        .await,
                    None,
                )?;
                let handle = self.insert(p::ResourceKind::Writer, writer).await?;
                Value::Writer { handle }
            }
            Op::ReaderNext { handle } => {
                let resource = self.resource(p::ResourceKind::Reader, handle)?;
                let g = self.bindings.assethub_vfs_driver();
                let chunk = result(g.reader().call_next(&mut self.store, resource).await, None)?;
                return Ok(p::Reply {
                    value: Value::Chunk {
                        eof: chunk.is_none(),
                    },
                    data: chunk.unwrap_or_default(),
                });
            }
            Op::WriterChunk { handle } => {
                let resource = self.resource(p::ResourceKind::Writer, handle)?;
                let g = self.bindings.assethub_vfs_driver();
                result(
                    g.writer()
                        .call_write(&mut self.store, resource, &data)
                        .await,
                    None,
                )?;
                Value::Unit
            }
            Op::Commit { handle } | Op::Abort { handle } => {
                let resource = self.resource(p::ResourceKind::Writer, handle)?;
                self.resources.remove(&handle);
                let g = self.bindings.assethub_vfs_driver();
                let outcome = if matches!(op, Op::Commit { .. }) {
                    result(
                        g.writer().call_commit(&mut self.store, resource).await,
                        None,
                    )
                    .and_then(|bytes| {
                        json(&bytes)
                            .map(|entry| Value::Entry { entry })
                            .map_err(|_| StorageError::new(StorageErrorKind::OutcomeUnknown).into())
                    })
                } else {
                    result(g.writer().call_abort(&mut self.store, resource).await, None)
                        .map(|()| Value::Unit)
                };
                let dropped = resource.resource_drop_async(&mut self.store).await;
                match outcome {
                    Ok(value) => {
                        dropped.map_err(|_| c::invalid())?;
                        value
                    }
                    Err(error) => return Err(error),
                }
            }
            Op::Release { kind, handle } => {
                if let Some(resource) = self.resources.remove(&handle) {
                    if resource.kind != kind {
                        return Err(c::invalid());
                    }
                    resource
                        .value
                        .resource_drop_async(&mut self.store)
                        .await
                        .map_err(|_| c::invalid())?;
                }
                Value::Unit
            }
            Op::Renew { resources } => {
                if resources.len() > p::MAX_RESOURCES {
                    return Err(c::invalid());
                }
                for r in resources {
                    if let Some(resource) = self.resources.get_mut(&r.handle)
                        && resource.kind == r.kind
                    {
                        resource.touched = Instant::now();
                    }
                }
                Value::Unit
            }
        };
        Ok(p::Reply::new(value))
    }
}
fn encode(value: &impl serde::Serialize) -> Result<Vec<u8>, DriverError> {
    let bytes = serde_json::to_vec(value).map_err(|_| c::invalid())?;
    if bytes.len() > p::MAX_CONTROL_BYTES {
        return Err(StorageError::new(StorageErrorKind::TooLarge).into());
    }
    Ok(bytes)
}
fn json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, DriverError> {
    if bytes.len() > p::MAX_CONTROL_BYTES {
        return Err(c::invalid());
    }
    serde_json::from_slice(bytes).map_err(|_| c::invalid())
}
fn result<T>(
    r: wasmtime::Result<Result<T, guest::Failure>>,
    id: Option<&str>,
) -> Result<T, DriverError> {
    r.map_err(|_| StorageError::new(StorageErrorKind::StaleHandle))?
        .map_err(|e| {
            c::driver_error(
                p::Failure {
                    code: e.code,
                    field: e.field,
                    config_kind: e.config_kind,
                },
                id,
            )
        })
}

pub struct WasmTransport {
    state: Mutex<State>,
    ticker: tokio::task::JoinHandle<()>,
}
impl Drop for WasmTransport {
    fn drop(&mut self) {
        self.ticker.abort();
    }
}
impl WasmTransport {
    /// Accept a portable component or a core module carrying WIT metadata. Only
    /// explicitly allowed origins can be contacted; no filesystem/WASI is linked.
    pub async fn load(bytes: Vec<u8>, origins: Vec<String>) -> Result<Self, DriverError> {
        Self::load_with_grants(bytes, HostGrants { http_origins: origins }).await
    }
    /// Load any plugin implementing the stable VFS ABI and service.v1 imports.
    /// Grants describe platform permissions, never a particular backend's fields.
    pub async fn load_with_grants(bytes: Vec<u8>, grants: HostGrants) -> Result<Self, DriverError> {
        let mut config = Config::new();
        config
            .wasm_component_model(true)
            .consume_fuel(true)
            .epoch_interruption(true);
        let engine = Engine::new(&config).map_err(|_| c::invalid())?;
        let compile_engine = engine.clone();
        let component = tokio::task::spawn_blocking(move || {
            if let Ok(component) = Component::new(&compile_engine, &bytes) {
                return Ok(component);
            }
            let component = wit_component::ComponentEncoder::default()
                .module(&bytes)
                .map_err(|_| c::invalid())?
                .validate(true)
                .encode()
                .map_err(|_| c::invalid())?;
            Component::new(&compile_engine, component).map_err(|_| c::invalid())
        })
        .await
        .map_err(|_| c::invalid())??;
        let origins = grants.http_origins
            .into_iter()
            .map(|origin| {
                let url = reqwest::Url::parse(&origin).map_err(|_| c::invalid())?;
                if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none()
                    || !url.username().is_empty() || url.password().is_some()
                    || url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
                    return Err(c::invalid());
                }
                Ok(url.origin().ascii_serialization())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|_| c::invalid())?;
        let host = Host {
            client,
            origins,
            limits: StoreLimitsBuilder::new()
                .memory_size(128 * 1024 * 1024)
                .instances(16)
                .memories(4)
                .build(),
        };
        let mut store = Store::new(&engine, host);
        store.limiter(|host| &mut host.limits);
        store.set_fuel(2_000_000_000).map_err(|_| c::invalid())?;
        store
            .fuel_async_yield_interval(Some(100_000))
            .map_err(|_| c::invalid())?;
        store.set_epoch_deadline(3000);
        let mut linker = Linker::new(&engine);
        Plugin::add_to_linker::<Host, wasmtime::component::HasSelf<Host>>(&mut linker, |host| host)
            .map_err(|_| c::invalid())?;
        // Resolve every import before running component initialization. New
        // backends use this fixed profile rather than backend-specific linking.
        linker.instantiate_pre(&component)
            .map_err(|_| StorageError::new(StorageErrorKind::UnsupportedOperation))?;
        let bindings = Plugin::instantiate_async(&mut store, &component, &linker)
            .await
            .map_err(|error| {
                StorageError::with_source(
                    StorageErrorKind::InvalidData,
                    std::io::Error::other(error),
                )
            })?;
        let ticker = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(10));
            loop {
                interval.tick().await;
                engine.increment_epoch();
            }
        });
        Ok(Self {
            state: Mutex::new(State {
                store,
                bindings,
                resources: HashMap::new(),
                next: 0,
                initialized: false,
                poisoned: false,
                generation: new_instance_id()?.as_str().into(),
            }),
            ticker,
        })
    }
}
impl Transport for WasmTransport {
    fn host_capabilities(&self) -> &[&str] {
        p::WASM_HOST_CAPABILITIES
    }
    fn call<'a>(&'a self, generation: &'a str, op: Op, data: Vec<u8>) -> TransportFuture<'a> {
        Box::pin(async move {
            if data.len() > p::MAX_DATA_BYTES {
                return Err(StorageError::new(StorageErrorKind::TooLarge).into());
            }
            let mut state = tokio::time::timeout(Duration::from_secs(30), self.state.lock())
                .await
                .map_err(|_| StorageError::new(StorageErrorKind::TimedOut))?;
            if state.poisoned || (!matches!(op, Op::Describe) && generation != state.generation) {
                return Err(StorageError::new(StorageErrorKind::StaleHandle).into());
            }
            state
                .store
                .set_fuel(2_000_000_000)
                .map_err(|_| c::invalid())?;
            state.store.set_epoch_deadline(3000);
            // Cancellation leaves the generation poisoned instead of reusing a store
            // whose guest-side session progress may be unknown.
            state.poisoned = true;
            let mutates = op.mutates();
            let outcome =
                tokio::time::timeout(Duration::from_secs(30), state.execute(op, data)).await;
            match outcome {
                Ok(Err(error))
                    if error
                        .storage_error()
                        .is_some_and(|e| e.kind() == StorageErrorKind::StaleHandle) =>
                {
                    if mutates {
                        Err(StorageError::new(StorageErrorKind::OutcomeUnknown).into())
                    } else {
                        Err(error)
                    }
                }
                Ok(result) => {
                    state.poisoned = false;
                    result
                }
                Err(_) => Err(StorageError::new(if mutates {
                    StorageErrorKind::OutcomeUnknown
                } else {
                    StorageErrorKind::TimedOut
                })
                .into()),
            }
        })
    }
}
