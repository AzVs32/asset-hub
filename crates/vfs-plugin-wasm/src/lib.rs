//! Protobuf-over-WIT adapter with bounded execution and explicitly granted HTTP origins.
use ah_vfs::domain::{DriverError, StorageError, StorageErrorKind, new_instance_id};
use std::{
    collections::HashMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use vfs_plugin_host::{
    ConnectFuture, OpenFuture, OpenedStorage, Transport, TransportFactory, TransportFuture,
    convert as c,
};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

wasmtime::component::bindgen!({
    path: "../vfs-plugin-protocol/wit",
    world: "plugin",
    imports: { default: async | trappable },
    exports: { default: async },
});
use assethub::vfs::http;
use assethub::vfs::{clock, random};

/// Bound concurrent guest memory independently from the 128 MiB per-Store limit.
pub const MAX_STORAGE_INSTANCES: usize = 16;
const MAX_CONTROL_INSTANCES: usize = 4;

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
    Ok(i64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    )?)
}

struct Host {
    client: reqwest::Client,
    origins: Vec<String>,
    limits: StoreLimits,
}
impl http::Host for Host {
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
}
struct State {
    store: Store<Host>,
    bindings: Plugin,
    resources: HashMap<u64, Resource>,
    next: u64,
    initialized: Option<p::Object>,
    poisoned: bool,
    faulted: bool,
    generation: String,
    _capacity: OwnedSemaphorePermit,
}
impl State {
    fn resource(&mut self, kind: p::ResourceKind, handle: u64) -> Result<(), DriverError> {
        let resource = self
            .resources
            .get_mut(&handle)
            .ok_or_else(|| StorageError::new(StorageErrorKind::SessionClosed))?;
        if resource.kind != kind {
            return Err(c::invalid());
        }
        Ok(())
    }
    fn invalid_reply(&mut self, op: &Op) -> DriverError {
        self.faulted = true;
        StorageError::new(if op.mutates() {
            StorageErrorKind::OutcomeUnknown
        } else {
            StorageErrorKind::InvalidData
        })
        .into()
    }
    async fn invoke(&mut self, op: &Op, data: Vec<u8>) -> Result<p::Reply, DriverError> {
        let bytes = p::encode_request(&self.generation, op, data).map_err(|error| {
            StorageError::new(match error {
                p::CodecError::TooLarge => StorageErrorKind::TooLarge,
                p::CodecError::InvalidData => StorageErrorKind::InvalidRequest,
            })
        })?;
        let response = self
            .bindings
            .assethub_vfs_driver()
            .call_call(&mut self.store, &bytes)
            .await
            .map_err(|_| StorageError::new(StorageErrorKind::StaleHandle))?;
        let id = match op {
            Op::GetInstance(p::InstanceId { id })
            | Op::RemoveInstance(p::InstanceId { id })
            | Op::Open(p::Open { id, .. }) => Some(id.as_str()),
            _ => None,
        };
        let reply = p::decode_reply(&response)
            .map_err(|_| self.invalid_reply(op))?
            .map_err(|failure| c::driver_error(failure, id))?;
        if !reply
            .result
            .as_ref()
            .is_some_and(|value| op.accepts_reply(value))
        {
            return Err(self.invalid_reply(op));
        }
        Ok(reply)
    }
    async fn execute(&mut self, op: Op, data: Vec<u8>) -> Result<p::Reply, DriverError> {
        if let Op::Initialize(p::Initialize {
            settings: Some(settings),
        }) = &op
            && let Some(previous) = &self.initialized
        {
            return if previous == settings {
                Ok(p::Reply::unit())
            } else {
                Err(StorageError::new(StorageErrorKind::Conflict).into())
            };
        }
        if !matches!(op, Op::Describe(p::Empty {}) | Op::Initialize(_))
            && self.initialized.is_none()
        {
            return Err(StorageError::new(StorageErrorKind::Unavailable).into());
        }
        match &op {
            Op::Storage(p::StorageCall { handle, .. })
            | Op::Read(p::Read { handle, .. })
            | Op::Write(p::Write { handle, .. }) => {
                self.resource(p::ResourceKind::Storage, *handle)?
            }
            Op::ReaderNext(p::Handle { handle }) => {
                self.resource(p::ResourceKind::Reader, *handle)?
            }
            Op::WriterChunk(p::Handle { handle }) => {
                self.resource(p::ResourceKind::Writer, *handle)?
            }
            Op::Commit(p::Handle { handle }) | Op::Abort(p::Handle { handle }) => {
                self.resource(p::ResourceKind::Writer, *handle)?;
                self.resources.remove(handle);
            }
            Op::Release(p::ResourceRef { handle, kind }) => {
                if self
                    .resources
                    .get(handle)
                    .is_some_and(|resource| resource.kind as i32 != *kind)
                {
                    return Err(c::invalid());
                }
                self.resources.remove(handle);
            }
            _ => {}
        }
        if matches!(op, Op::Open(_) | Op::Read(_) | Op::Write(_))
            && self.resources.len() >= p::MAX_RESOURCES
        {
            return Err(StorageError::new(StorageErrorKind::CapacityExceeded).into());
        }
        let result = self.invoke(&op, data).await;
        if let Op::ReaderNext(p::Handle { handle }) = &op
            && (result.is_err()
                || matches!(
                    &result,
                    Ok(p::Reply {
                        result: Some(Value::Chunk(p::Chunk { eof: true })),
                        ..
                    })
                ))
        {
            self.resources.remove(handle);
            // Guests close failed/finished readers; release is also idempotent for
            // other implementations and never reopens or advances a stream.
            if !self.faulted
                && result.as_ref().err().is_none_or(|error| {
                    error
                        .storage_error()
                        .is_none_or(|error| error.kind() != StorageErrorKind::StaleHandle)
                })
            {
                self.invoke(
                    &Op::Release(p::ResourceRef {
                        kind: p::ResourceKind::Reader as i32,
                        handle: *handle,
                    }),
                    vec![],
                )
                .await?;
            }
        }
        let mut reply = result?;
        match reply.result.as_mut().ok_or_else(c::invalid)? {
            Value::Description(description) => description.generation = self.generation.clone(),
            Value::Storage(p::Storage { handle, .. })
            | Value::Reader(p::Handle { handle })
            | Value::Writer(p::Handle { handle }) => {
                let kind = match op {
                    Op::Open(_) => p::ResourceKind::Storage,
                    Op::Read(_) => p::ResourceKind::Reader,
                    _ => p::ResourceKind::Writer,
                };
                if *handle <= self.next {
                    return Err(self.invalid_reply(&op));
                }
                self.next = *handle;
                self.resources.insert(*handle, Resource { kind });
            }
            _ => {}
        }
        if let Op::Initialize(p::Initialize {
            settings: Some(settings),
        }) = op
        {
            self.initialized = Some(settings);
        }
        Ok(reply)
    }
}

/// Compiled plugin factory. Each connection and Storage owns a separate Store.
pub struct WasmTransport {
    factory: Arc<InstanceFactory>,
}
struct InstanceFactory {
    pre: PluginPre<Host>,
    client: reqwest::Client,
    origins: Vec<String>,
    controls: Arc<Semaphore>,
    storages: Arc<Semaphore>,
    ticker: tokio::task::JoinHandle<()>,
}
impl Drop for InstanceFactory {
    fn drop(&mut self) {
        self.ticker.abort();
    }
}
impl InstanceFactory {
    async fn instantiate(&self, capacity: &Arc<Semaphore>) -> Result<Instance, DriverError> {
        let capacity = capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| StorageError::new(StorageErrorKind::CapacityExceeded))?;
        let host = Host {
            client: self.client.clone(),
            origins: self.origins.clone(),
            limits: StoreLimitsBuilder::new()
                .memory_size(128 * 1024 * 1024)
                .instances(16)
                .memories(4)
                .build(),
        };
        let mut store = Store::new(self.pre.engine(), host);
        store.limiter(|host| &mut host.limits);
        store.set_fuel(2_000_000_000).map_err(|_| c::invalid())?;
        store
            .fuel_async_yield_interval(Some(100_000))
            .map_err(|_| c::invalid())?;
        store.set_epoch_deadline(3000);
        let bindings = tokio::time::timeout(
            Duration::from_secs(30),
            self.pre.instantiate_async(&mut store),
        )
        .await
        .map_err(|_| StorageError::new(StorageErrorKind::TimedOut))?
        .map_err(|error| {
            StorageError::with_source(StorageErrorKind::InvalidData, std::io::Error::other(error))
        })?;
        Ok(Instance {
            state: Mutex::new(Some(State {
                store,
                bindings,
                resources: HashMap::new(),
                next: 0,
                initialized: None,
                poisoned: false,
                faulted: false,
                generation: new_instance_id()?.as_str().into(),
                _capacity: capacity,
            })),
            disconnected: Arc::new(AtomicBool::new(false)),
        })
    }
}
struct Instance {
    state: Mutex<Option<State>>,
    disconnected: Arc<AtomicBool>,
}
struct Execution<'a> {
    state: tokio::sync::MutexGuard<'a, Option<State>>,
    disconnected: &'a AtomicBool,
}
impl Drop for Execution<'_> {
    fn drop(&mut self) {
        if self.state.as_ref().is_some_and(|state| state.poisoned) {
            // Drop the failed Store and its capacity immediately, even if stale
            // proxies are retained. A cancelled future follows the same path.
            self.state.take();
            self.disconnected.store(true, Ordering::Release);
        }
    }
}
impl Instance {
    async fn call(&self, generation: &str, op: Op, data: Vec<u8>) -> Result<p::Reply, DriverError> {
        if data.len() > p::MAX_DATA_BYTES {
            return Err(StorageError::new(StorageErrorKind::TooLarge).into());
        }
        let state = tokio::time::timeout(Duration::from_secs(30), self.state.lock())
            .await
            .map_err(|_| StorageError::new(StorageErrorKind::TimedOut))?;
        let mut execution = Execution {
            state,
            disconnected: &self.disconnected,
        };
        let state = execution
            .state
            .as_mut()
            .ok_or_else(|| StorageError::new(StorageErrorKind::StaleHandle))?;
        if state.poisoned || (!matches!(op, Op::Describe(_)) && generation != state.generation) {
            return Err(StorageError::new(StorageErrorKind::StaleHandle).into());
        }
        state
            .store
            .set_fuel(2_000_000_000)
            .map_err(|_| c::invalid())?;
        state.store.set_epoch_deadline(3000);
        state.poisoned = true;
        let mutates = op.mutates();
        match tokio::time::timeout(Duration::from_secs(30), state.execute(op, data)).await {
            Ok(Err(error))
                if error
                    .storage_error()
                    .is_some_and(|error| error.kind() == StorageErrorKind::StaleHandle) =>
            {
                if mutates {
                    Err(StorageError::new(StorageErrorKind::OutcomeUnknown).into())
                } else {
                    Err(error)
                }
            }
            Ok(result) => {
                state.poisoned = state.faulted
                    || result
                        .as_ref()
                        .err()
                        .and_then(DriverError::storage_error)
                        .is_some_and(|error| {
                            matches!(
                                error.kind(),
                                StorageErrorKind::TimedOut
                                    | StorageErrorKind::Unavailable
                                    | StorageErrorKind::OutcomeUnknown
                            )
                        });
                result
            }
            Err(_) => Err(StorageError::new(if mutates {
                StorageErrorKind::OutcomeUnknown
            } else {
                StorageErrorKind::TimedOut
            })
            .into()),
        }
    }
}

#[derive(Default)]
struct Bootstrap {
    description: Option<p::Description>,
    settings: Option<p::Object>,
}
struct ControlTransport {
    instance: Instance,
    bootstrap: RwLock<Bootstrap>,
    factory: Arc<InstanceFactory>,
}
impl TransportFactory for WasmTransport {
    fn connect(self: Arc<Self>) -> ConnectFuture {
        Box::pin(async move {
            let instance = self.factory.instantiate(&self.factory.controls).await?;
            Ok(Arc::new(ControlTransport {
                instance,
                bootstrap: RwLock::new(Bootstrap::default()),
                factory: self.factory.clone(),
            }) as Arc<dyn Transport>)
        })
    }
}
impl Transport for ControlTransport {
    fn host_capabilities(&self) -> &[&str] {
        p::WASM_HOST_CAPABILITIES
    }
    fn disconnected(&self) -> Option<Arc<AtomicBool>> {
        Some(self.instance.disconnected.clone())
    }
    fn call<'a>(&'a self, generation: &'a str, op: Op, data: Vec<u8>) -> TransportFuture<'a> {
        Box::pin(async move {
            if op.is_resource() || matches!(op, Op::Open(_)) {
                return Err(StorageError::new(StorageErrorKind::InvalidRequest).into());
            }
            let settings = match &op {
                Op::Initialize(p::Initialize { settings }) => settings.clone(),
                _ => None,
            };
            let reply = self.instance.call(generation, op, data).await?;
            let mut bootstrap = self
                .bootstrap
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(Value::Description(description)) = &reply.result {
                bootstrap.description = Some(*description.clone());
            }
            if let Some(settings) = settings {
                bootstrap.settings = Some(settings);
            }
            Ok(reply)
        })
    }
    fn open_session<'a>(&'a self, generation: &'a str, open: p::Open) -> Option<OpenFuture<'a>> {
        Some(Box::pin(async move {
            if self.instance.disconnected.load(Ordering::Acquire) {
                return Err(StorageError::new(StorageErrorKind::StaleHandle).into());
            }
            // This snapshot never waits for execution in the control Store.
            let (mut expected, settings) = {
                let bootstrap = self
                    .bootstrap
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let description = bootstrap
                    .description
                    .clone()
                    .ok_or_else(|| StorageError::new(StorageErrorKind::Unavailable))?;
                if description.generation != generation {
                    return Err(StorageError::new(StorageErrorKind::StaleHandle).into());
                }
                (
                    description,
                    bootstrap
                        .settings
                        .clone()
                        .ok_or_else(|| StorageError::new(StorageErrorKind::Unavailable))?,
                )
            };
            let instance = self.factory.instantiate(&self.factory.storages).await?;
            let Value::Description(description) = instance
                .call("", Op::Describe(p::Empty {}), vec![])
                .await?
                .into_value()
                .map_err(|_| c::invalid())?
            else {
                return Err(c::invalid());
            };
            let mut description = *description;
            let generation = description.generation.clone();
            expected.generation.clear();
            description.generation.clear();
            if expected != description {
                return Err(c::invalid());
            }
            instance
                .call(
                    &generation,
                    Op::Initialize(p::Initialize {
                        settings: Some(settings),
                    }),
                    vec![],
                )
                .await?;
            let Value::Storage(storage) = instance
                .call(&generation, Op::Open(open), vec![])
                .await
                .map_err(|error| {
                    // A fresh storage Store can fail without invalidating the
                    // control instance or its cached bootstrap information.
                    if error
                        .storage_error()
                        .is_some_and(|error| error.kind() == StorageErrorKind::StaleHandle)
                    {
                        StorageError::new(StorageErrorKind::Unavailable).into()
                    } else {
                        error
                    }
                })?
                .into_value()
                .map_err(|_| c::invalid())?
            else {
                return Err(c::invalid());
            };
            Ok(OpenedStorage {
                storage,
                generation,
                transport: Arc::new(StorageTransport {
                    instance,
                    _factory: self.factory.clone(),
                }),
            })
        }))
    }
}
struct StorageTransport {
    instance: Instance,
    _factory: Arc<InstanceFactory>,
}
impl Transport for StorageTransport {
    fn disconnected(&self) -> Option<Arc<AtomicBool>> {
        Some(self.instance.disconnected.clone())
    }
    fn call<'a>(&'a self, generation: &'a str, op: Op, data: Vec<u8>) -> TransportFuture<'a> {
        Box::pin(async move {
            if !op.is_resource() {
                return Err(StorageError::new(StorageErrorKind::InvalidRequest).into());
            }
            self.instance.call(generation, op, data).await
        })
    }
}

impl WasmTransport {
    /// Accept a portable component or a core module carrying WIT metadata. Only
    /// explicitly allowed origins can be contacted; no filesystem/WASI is linked.
    pub async fn load(bytes: Vec<u8>, origins: Vec<String>) -> Result<Self, DriverError> {
        Self::load_with_grants(
            bytes,
            HostGrants {
                http_origins: origins,
            },
        )
        .await
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
        let origins = grants
            .http_origins
            .into_iter()
            .map(|origin| {
                let url = reqwest::Url::parse(&origin).map_err(|_| c::invalid())?;
                if !matches!(url.scheme(), "http" | "https")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.path() != "/"
                    || url.query().is_some()
                    || url.fragment().is_some()
                {
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
        let mut linker = Linker::new(&engine);
        Plugin::add_to_linker::<Host, wasmtime::component::HasSelf<Host>>(&mut linker, |host| host)
            .map_err(|_| c::invalid())?;
        let pre = PluginPre::new(
            linker
                .instantiate_pre(&component)
                .map_err(|_| StorageError::new(StorageErrorKind::UnsupportedOperation))?,
        )
        .map_err(|_| c::invalid())?;
        let ticker = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(10));
            loop {
                interval.tick().await;
                engine.increment_epoch();
            }
        });
        Ok(Self {
            factory: Arc::new(InstanceFactory {
                pre,
                client,
                origins,
                controls: Arc::new(Semaphore::new(MAX_CONTROL_INSTANCES)),
                storages: Arc::new(Semaphore::new(MAX_STORAGE_INSTANCES)),
                ticker,
            }),
        })
    }
}
