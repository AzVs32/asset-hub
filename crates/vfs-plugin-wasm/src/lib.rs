//! Protobuf-over-WIT adapter with WASIp2 services and explicit per-plugin authority.
use ah_vfs_storage::{BackendFailure, ContractViolation, Failure, Rejection};
mod dns;
mod grants;
mod http;
use ah_vfs::domain::InstanceId;

use ah_vfs::application::driver_error::{DriverError, LifecycleError, LifecycleFailure};
use grants::NetworkPolicy;
pub use grants::{DirectoryGrant, ExecutionLimits, HostGrants, HttpGrants};
use std::{
    collections::HashMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use vfs_plugin_host::{
    ConnectFuture, OpenFuture, OpenedStorage, SessionTransport, Transport, TransportFactory,
    TransportFuture, convert as c,
};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};
use wasmtime::component::ResourceTable;
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{FsPerms, WasiCtx, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpCtxView, WasiHttpView};

wasmtime::component::bindgen!({
    path: "../vfs-plugin-protocol/wit",
    world: "plugin",
    exports: { default: async },
});
struct Host {
    wasi: WasiCtx,
    table: ResourceTable,
    http: WasiHttpCtx,
    hooks: http::HttpPolicy,
    network: NetworkPolicy,
    limits: StoreLimits,
    exhausted: bool,
    memory_limit: usize,
    used_memory: usize,
}
impl WasiView for Host {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}
impl WasiHttpView for Host {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: &mut self.hooks,
        }
    }
}
impl wasmtime::ResourceLimiter for Host {
    fn instances(&self) -> usize {
        wasmtime::ResourceLimiter::instances(&self.limits)
    }
    fn memories(&self) -> usize {
        wasmtime::ResourceLimiter::memories(&self.limits)
    }
    fn tables(&self) -> usize {
        wasmtime::ResourceLimiter::tables(&self.limits)
    }
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired.saturating_sub(current) > self.memory_limit.saturating_sub(self.used_memory) {
            self.exhausted = true;
            return Err(wasmtime::Error::msg("WASI Store memory budget exhausted"));
        }
        let result = self.limits.memory_growing(current, desired, maximum);
        if matches!(result, Ok(true)) {
            self.used_memory += desired.saturating_sub(current);
        }
        self.exhausted |= !matches!(result, Ok(true));
        result
    }
    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let result = self.limits.table_growing(current, desired, maximum);
        self.exhausted |= !matches!(result, Ok(true));
        result
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
    fn resource(&self, op: &Op, kind: p::ResourceKind, handle: u64) -> Result<(), DriverError> {
        let resource = self.resources.get(&handle).ok_or_else(|| {
            let reason = if kind == p::ResourceKind::Lookup {
                BackendFailure::Stale
            } else {
                BackendFailure::Closed
            };
            c::call_error(op, Failure::Backend(reason), false, None)
        })?;
        if resource.kind != kind {
            if kind == p::ResourceKind::Lookup {
                return Err(c::call_error(
                    op,
                    Failure::Backend(BackendFailure::Stale),
                    false,
                    None,
                ));
            }
            return Err(c::protocol_violation());
        }
        Ok(())
    }
    fn invalid_reply(&mut self, op: &Op) -> DriverError {
        self.faulted = true;
        c::call_error(
            op,
            Failure::Contract(ContractViolation::InvalidResponse),
            true,
            None,
        )
    }
    async fn invoke(&mut self, op: &Op, data: Vec<u8>) -> Result<p::Reply, DriverError> {
        let bytes = p::encode_request(&self.generation, op, data).map_err(|error| {
            c::call_error(
                op,
                match error {
                    p::CodecError::TooLarge => Failure::Rejected(Rejection::LimitExceeded),
                    p::CodecError::InvalidData | p::CodecError::Decode(_) => {
                        Failure::Contract(ContractViolation::InvalidResponse)
                    }
                },
                false,
                Some(Box::new(error)),
            )
        })?;
        let response = match self
            .bindings
            .assethub_vfs_driver()
            .call_call(&mut self.store, &bytes)
            .await
        {
            Ok(response) => response,
            Err(error) => {
                self.faulted = true;
                let kind = if self.store.data().exhausted
                    || error.downcast_ref::<wasmtime::Trap>() == Some(&wasmtime::Trap::OutOfFuel)
                {
                    Failure::Backend(BackendFailure::CapacityExceeded)
                } else if error.downcast_ref::<wasmtime::Trap>() == Some(&wasmtime::Trap::Interrupt)
                {
                    Failure::Backend(BackendFailure::TimedOut)
                } else {
                    Failure::Backend(BackendFailure::Stale)
                };
                return Err(c::call_error(
                    op,
                    kind,
                    true,
                    Some(Box::new(std::io::Error::other(error))),
                )
                .into());
            }
        };
        let id = match op {
            Op::Open(p::Open { id, .. }) => Some(id.as_str()),
            _ => None,
        };
        let reply = p::decode_reply(&response)
            .map_err(|source| {
                self.faulted = true;
                c::call_error(
                    op,
                    Failure::Contract(ContractViolation::InvalidResponse),
                    true,
                    Some(Box::new(source)),
                )
            })?
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
                Err(LifecycleError::new(LifecycleFailure::Conflict).into())
            };
        }
        if !matches!(op, Op::Describe(p::Empty {}) | Op::Initialize(_))
            && self.initialized.is_none()
        {
            return Err(c::call_error(
                &op,
                Failure::Backend(BackendFailure::Unavailable),
                false,
                None,
            )
            .into());
        }
        match &op {
            Op::Storage(p::StorageCall { handle, .. })
            | Op::Read(p::Read { handle, .. })
            | Op::Write(p::Write { handle, .. })
            | Op::BeginLookup(p::Handle { handle }) => {
                self.resource(&op, p::ResourceKind::Storage, *handle)?
            }
            Op::ReaderNext(p::Handle { handle }) => {
                self.resource(&op, p::ResourceKind::Reader, *handle)?
            }
            Op::ReadObservedLink(p::ReadObservedLink { lookup: handle, .. })
            | Op::ObserveChild(p::ObserveChild { lookup: handle, .. })
            | Op::VerifyLookup(p::Handle { handle }) => {
                self.resource(&op, p::ResourceKind::Lookup, *handle)?
            }
            Op::WriterChunk(p::Handle { handle }) => {
                self.resource(&op, p::ResourceKind::Writer, *handle)?
            }
            Op::Commit(p::Handle { handle }) | Op::Abort(p::Handle { handle }) => {
                self.resource(&op, p::ResourceKind::Writer, *handle)?;
                self.resources.remove(handle);
            }
            Op::Release(p::ResourceRef { handle, kind }) => {
                if self
                    .resources
                    .get(handle)
                    .is_some_and(|resource| resource.kind as i32 != *kind)
                {
                    return Err(c::protocol_violation());
                }
                self.resources.remove(handle);
            }
            _ => {}
        }
        if op.allocates_resource() && self.resources.len() >= p::MAX_RESOURCES
        {
            return Err(c::call_error(
                &op,
                Failure::Backend(BackendFailure::CapacityExceeded),
                false,
                None,
            )
            .into());
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
        }
        let mut reply = result?;
        match reply.result.as_mut().ok_or_else(c::protocol_violation)? {
            Value::Description(description) => description.generation = self.generation.clone(),
            Value::Storage(p::Storage { handle, .. })
            | Value::Reader(p::Handle { handle })
            | Value::Writer(p::Handle { handle })
            | Value::OpenedLookup(p::OpenedLookup { handle, .. }) => {
                let kind = match op {
                    Op::Open(_) => p::ResourceKind::Storage,
                    Op::Read(_) => p::ResourceKind::Reader,
                    Op::BeginLookup(_) => p::ResourceKind::Lookup,
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
    grants: HostGrants,
    network: NetworkPolicy,
    capabilities: Vec<&'static str>,
    controls: Arc<Semaphore>,
    storages: Arc<Semaphore>,
    ticker: tokio::task::JoinHandle<()>,
    #[cfg(test)]
    test_dns: Option<Arc<grants::TestDns>>,
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
            .map_err(|_| LifecycleError::new(LifecycleFailure::CapacityExceeded))?;
        let mut builder = WasiCtx::builder();
        let network = self.network.clone();
        #[cfg(test)]
        let network = network.with_test_dns(self.test_dns.clone());
        let check = network.clone();
        builder
            .allow_tcp(!self.grants.tcp.is_empty())
            .allow_udp(false)
            .allow_ip_name_lookup(!self.grants.tcp.is_empty())
            .socket_addr_check(move |addr, use_| {
                let check = check.clone();
                Box::pin(async move { check.allows(addr, use_).await })
            });
        for name in &self.grants.env {
            if let Some(value) = std::env::var_os(name) {
                builder.env(
                    name,
                    value
                        .to_str()
                        .ok_or_else(|| LifecycleError::new(LifecycleFailure::InvalidRequest))?,
                );
            }
        }
        for directory in &self.grants.fs {
            let perms = if directory.write {
                FsPerms::ReadWrite
            } else {
                FsPerms::ReadOnly
            };
            builder
                .preopened_dir(&directory.host, &directory.guest, perms)
                .map_err(|error| {
                    LifecycleError::with_source(
                        LifecycleFailure::InvalidRequest,
                        std::io::Error::other(error),
                    )
                })?;
        }
        let host = Host {
            wasi: builder.build(),
            table: ResourceTable::new(),
            http: WasiHttpCtx::new(),
            hooks: http::HttpPolicy {
                grants: self.grants.http.clone(),
            },
            network,
            exhausted: false,
            memory_limit: self.grants.limits.memory_bytes,
            used_memory: 0,
            limits: StoreLimitsBuilder::new()
                .memory_size(self.grants.limits.memory_bytes)
                .instances(16)
                .memories(4)
                .trap_on_grow_failure(true)
                .build(),
        };
        let mut store = Store::new(self.pre.engine(), host);
        store.limiter(|host| host);
        store.set_fuel(self.grants.limits.fuel).map_err(|source| {
            LifecycleError::with_source(LifecycleFailure::Backend, std::io::Error::other(source))
        })?;
        store
            .fuel_async_yield_interval(Some(100_000))
            .map_err(|source| {
                LifecycleError::with_source(
                    LifecycleFailure::Backend,
                    std::io::Error::other(source),
                )
            })?;
        store.set_epoch_deadline(self.grants.limits.timeout_ms.div_ceil(10));
        let bindings = tokio::time::timeout(
            self.grants.limits.timeout(),
            self.pre.instantiate_async(&mut store),
        )
        .await
        .map_err(|_| LifecycleError::new(LifecycleFailure::TimedOut))?
        .map_err(|error| {
            LifecycleError::with_source(
                LifecycleFailure::InvalidDeclaration,
                std::io::Error::other(error),
            )
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
                generation: InstanceId::generate().as_str().into(),
                _capacity: capacity,
            })),
            disconnected: Arc::new(AtomicBool::new(false)),
            limits: self.grants.limits.clone(),
        })
    }
}
struct Instance {
    state: Mutex<Option<State>>,
    disconnected: Arc<AtomicBool>,
    limits: ExecutionLimits,
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
            return Err(c::call_error(
                &op,
                Failure::Rejected(Rejection::LimitExceeded),
                false,
                None,
            ));
        }
        let state = tokio::time::timeout(self.limits.timeout(), self.state.lock())
            .await
            .map_err(|_| {
                c::call_error(&op, Failure::Backend(BackendFailure::TimedOut), false, None)
            })?;
        let mut execution = Execution {
            state,
            disconnected: &self.disconnected,
        };
        let state = execution.state.as_mut().ok_or_else(|| {
            c::call_error(&op, Failure::Backend(BackendFailure::Stale), false, None)
        })?;
        if state.poisoned || (!matches!(op, Op::Describe(_)) && generation != state.generation) {
            return Err(c::call_error(
                &op,
                Failure::Backend(BackendFailure::Stale),
                false,
                None,
            ));
        }
        state.store.set_fuel(self.limits.fuel).map_err(|source| {
            c::call_error(
                &op,
                Failure::Backend(BackendFailure::Other),
                false,
                Some(Box::new(std::io::Error::other(source))),
            )
        })?;
        state
            .store
            .set_epoch_deadline(self.limits.timeout_ms.div_ceil(10));
        state.poisoned = true;
        match tokio::time::timeout(self.limits.timeout(), state.execute(op.clone(), data)).await {
            Ok(result) => {
                state.poisoned = state.faulted
                    || result.as_ref().err().is_some_and(|error| {
                        error.storage_error().is_some_and(|error| {
                            !c::is_stale_directory(error)
                                && matches!(
                                    error.failure(),
                                    Failure::Backend(
                                        BackendFailure::TimedOut
                                            | BackendFailure::Unavailable
                                            | BackendFailure::Stale
                                    )
                                )
                        }) || error.lifecycle_error().is_some_and(|error| {
                            matches!(
                                error.failure(),
                                LifecycleFailure::TimedOut
                                    | LifecycleFailure::Unavailable
                                    | LifecycleFailure::Stale
                            )
                        })
                    });
                result
            }
            Err(_) => Err(c::call_error(
                &op,
                Failure::Backend(BackendFailure::TimedOut),
                true,
                None,
            )),
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
impl SessionTransport for ControlTransport {
    fn disconnected(&self) -> Option<Arc<AtomicBool>> {
        Some(self.instance.disconnected.clone())
    }
    fn call<'a>(&'a self, generation: &'a str, op: Op, data: Vec<u8>) -> TransportFuture<'a> {
        Box::pin(async move {
            if op.is_resource() || matches!(op, Op::Open(_)) {
                return Err(LifecycleError::new(LifecycleFailure::InvalidRequest).into());
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
}
impl Transport for ControlTransport {
    fn host_capabilities(&self) -> &[&str] {
        &self.factory.capabilities
    }
    fn open_session(self: Arc<Self>, generation: &str, open: p::Open) -> OpenFuture<'_> {
        Box::pin(async move {
            if self.instance.disconnected.load(Ordering::Acquire) {
                return Err(LifecycleError::new(LifecycleFailure::Stale).into());
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
                    .ok_or_else(|| LifecycleError::new(LifecycleFailure::Unavailable))?;
                if description.generation != generation {
                    return Err(LifecycleError::new(LifecycleFailure::Stale).into());
                }
                (
                    description,
                    bootstrap
                        .settings
                        .clone()
                        .ok_or_else(|| LifecycleError::new(LifecycleFailure::Unavailable))?,
                )
            };
            let instance = self.factory.instantiate(&self.factory.storages).await?;
            let Value::Description(description) = instance
                .call("", Op::Describe(p::Empty {}), vec![])
                .await?
                .into_value()
                .map_err(|source| {
                    c::protocol_violation_with_source(std::io::Error::other(source))
                })?
            else {
                return Err(c::protocol_violation());
            };
            let mut description = *description;
            let generation = description.generation.clone();
            expected.generation.clear();
            description.generation.clear();
            if expected != description {
                return Err(c::protocol_violation());
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
                        .lifecycle_error()
                        .is_some_and(|error| error.failure() == LifecycleFailure::Stale)
                    {
                        LifecycleError::new(LifecycleFailure::Unavailable).into()
                    } else {
                        error
                    }
                })?
                .into_value()
                .map_err(|source| {
                    c::protocol_violation_with_source(std::io::Error::other(source))
                })?
            else {
                return Err(c::protocol_violation());
            };
            Ok(OpenedStorage {
                storage,
                generation,
                transport: Arc::new(StorageTransport {
                    instance,
                    _factory: self.factory.clone(),
                }),
            })
        })
    }
}
struct StorageTransport {
    instance: Instance,
    _factory: Arc<InstanceFactory>,
}
impl SessionTransport for StorageTransport {
    fn disconnected(&self) -> Option<Arc<AtomicBool>> {
        Some(self.instance.disconnected.clone())
    }
    fn call<'a>(&'a self, generation: &'a str, op: Op, data: Vec<u8>) -> TransportFuture<'a> {
        Box::pin(async move {
            if !op.is_resource() {
                return Err(LifecycleError::new(LifecycleFailure::InvalidRequest).into());
            }
            self.instance.call(generation, op, data).await
        })
    }
}

impl WasmTransport {
    /// Accept components only. Every import must resolve to the linked WASIp2 services.
    pub async fn load(bytes: Vec<u8>, mut grants: HostGrants) -> Result<Self, DriverError> {
        let network = grants.validate()?;
        let mut config = Config::new();
        config
            .wasm_component_model(true)
            .consume_fuel(true)
            .epoch_interruption(true);
        let engine = Engine::new(&config).map_err(|source| {
            LifecycleError::with_source(LifecycleFailure::Backend, std::io::Error::other(source))
        })?;
        let compile_engine = engine.clone();
        let component = tokio::task::spawn_blocking(move || {
            Component::from_binary(&compile_engine, &bytes)
                .map_err(|source| c::protocol_violation_with_source(std::io::Error::other(source)))
        })
        .await
        .map_err(|source| LifecycleError::with_source(LifecycleFailure::Unavailable, source))??;
        let mut linker = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(|source| {
            LifecycleError::with_source(LifecycleFailure::Backend, std::io::Error::other(source))
        })?;
        wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker).map_err(|source| {
            LifecycleError::with_source(LifecycleFailure::Backend, std::io::Error::other(source))
        })?;
        linker.allow_shadowing(true);
        wasmtime_wasi::p2::bindings::sockets::ip_name_lookup::add_to_linker::<
            Host,
            wasmtime::component::HasSelf<Host>,
        >(&mut linker, |host| host)
        .map_err(|source| {
            LifecycleError::with_source(LifecycleFailure::Backend, std::io::Error::other(source))
        })?;
        linker.allow_shadowing(false);
        let pre = PluginPre::new(linker.instantiate_pre(&component).map_err(|error| {
            LifecycleError::with_source(LifecycleFailure::Unsupported, std::io::Error::other(error))
        })?)
        .map_err(|source| c::protocol_violation_with_source(std::io::Error::other(source)))?;
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
                capabilities: grants.capabilities(),
                controls: Arc::new(Semaphore::new(grants.limits.control_instances)),
                storages: Arc::new(Semaphore::new(grants.limits.storage_instances)),
                grants,
                network,
                ticker,
                #[cfg(test)]
                test_dns: None,
            }),
        })
    }
}

#[cfg(test)]
mod integration_tests;
